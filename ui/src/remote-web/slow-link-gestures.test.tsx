// @vitest-environment jsdom
// Gestures across a brief station-control lapse. The operation client polls once a second on a
// 250 ms tick and treats a station reply as current for 1200 ms from the request's start, so any
// heartbeat round trip over 200 ms leaves control stale for a moment every cycle. A drag or a
// typed value must survive that moment; nothing may be sent while control is stale.
import { afterEach, beforeAll, beforeEach, expect, it, vi } from 'vitest'
import { useRef } from 'react'
import { act, cleanup, fireEvent, render } from '@testing-library/react'
import { PhoneCockpit } from '../components/PhoneCockpit'
import { OperateCockpit } from '../components/OperateCockpit'
import { CockpitHeader } from '../components/CockpitHeader'
import { useWheelTune } from '../useWheelTune'
import { WheelTuning } from './wheel-tuning'
import { RemoteWheelTuningContext } from './wheel-tuning-context'
import { controlFailureMessage } from './control-failure'
import type { ApplicationClient } from './application-client'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { RemoteObservationContext } from './amplifier-observation'
import { OperationClient } from './operation-client'
import { pendingControlStorage } from './control-storage'
import { t } from '../i18n'
import type { AppSnapshot } from '../types'
import type { OperationState } from './operation-protocol'
import type { ControlCapability } from './station-operation'
import type { MonitorFrame } from '../remote-monitor/protocol'
import type { MonitorState } from '../remote-monitor/session'
import settings from '../components/__fixtures__/defaultSettings.json'
import frames from '../remote-monitor/fixtures.v2.json'

vi.mock('../api', async original => {
  const actual = await original<Record<string, unknown>>()
  const reads: Record<string, unknown> = { getLicensedBandPlan: [], getBandPlan: [], getCatCwUnprovenRigModels: [],
    getSpectrumRow: { row: [], loHz: 200, hiHz: 4000 } }
  return Object.fromEntries(Object.entries(actual).map(([name, value]) => [name,
    typeof value === 'function' ? vi.fn(async () => structuredClone(reads[name] ?? {})) : value]))
})
vi.mock('../components/PhoneScope', () => ({ PhoneScope: () => <div/> }))
vi.mock('../components/BandStrip', () => ({ BandStrip: () => <div/> }))
vi.mock('../components/VoiceKeyer', () => ({ VoiceKeyer: () => <div/> }))
vi.mock('../components/LogEntry', () => ({ LogEntry: () => <div/> }))
vi.mock('../components/SpotDialog', () => ({ SpotDialog: () => null }))
vi.mock('./useJs8Context', () => ({ useJs8Context: () => ({ remote: true, value: null, loading: false, refresh: () => {} }) }))
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn(async (run: () => Promise<unknown>) => run()) }))
import { getSettings } from '../api'
import { pushToast } from '../toast'

const clients: OperationClient[] = []
const disposers: (() => void)[] = []
beforeAll(() => {
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  Element.prototype.scrollIntoView = vi.fn()
})
beforeEach(() => { localStorage.clear(); vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null) })
afterEach(() => { cleanup(); disposers.splice(0).forEach(f => f()); clients.splice(0).forEach(c => c.disconnected()); vi.useRealTimers(); vi.clearAllMocks(); vi.restoreAllMocks() })

async function step(ms = 10) { await act(async () => { await vi.advanceTimersByTimeAsync(ms) }) }
async function until(condition: () => boolean, limitMs = 4000) {
  for (let elapsed = 0; elapsed <= limitMs; elapsed += 10) { if (condition()) return; await step(10) }
  throw Error('condition not reached')
}

/** A station answering state reads and heartbeats after a 250 ms round trip. The extra 10 ms is
 * the page taking the reply off its socket; it also keeps the reply from landing on the same fake
 * instant as the client's own 250 ms tick, where the lapse would last no time at all. */
function station(capabilities: ControlCapability[], version: 3 | 4, radioId: number, extra: Partial<OperationState> = {}) {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'setTimeout', 'clearTimeout', 'performance'] })
  const state: OperationState = { stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(),
    revision: 1, commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false,
    controls: { context: { radioId, radioConnection: 7, ampConnection: null, ampReadSequence: null }, capabilities }, ...extra }
  // `applies`: also answer a station command, applied with a radio readback, after the same round trip.
  const link = { roundTripMs: 250 + 10, answering: true, applies: false }
  // The transport spy: every message that really left the browser, with whether control was current then.
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const wire: { request: any; fresh: boolean }[] = []
  const values = new Map<string, string>()
  const storage = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const client: OperationClient = new OperationClient(raw => {
    const { request } = JSON.parse(raw)
    wire.push({ request, fresh: client.getSnapshot().fresh })
    if (link.answering && (request.type === 'state' || request.type === 'heartbeat'))
      setTimeout(() => client.receive({ type: 'operationResponse', requestId: request.requestId, value: state }), link.roundTripMs)
    if (link.applies && request.type === 'stationControl')
      setTimeout(() => client.receive({ type: 'operationResponse', requestId: request.requestId,
        value: { operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'radioReadback' } }), link.roundTripMs)
  }, true, () => 1000 + performance.now(), undefined, version, pendingControlStorage(() => storage, 'slow-link', async (_key, run) => run()))
  clients.push(client)
  let lapses = 0, wasFresh = false
  client.subscribe(() => {
    const view = client.getSnapshot()
    if (wasFresh && !view.fresh && view.state?.phase === 'controlling') lapses++
    wasFresh = view.fresh
  })
  client.open()
  return { client, state, link, wire, lapses: () => lapses,
    heartbeats: () => wire.filter(w => w.request.type === 'heartbeat').length,
    writes: () => wire.filter(w => w.request.type === 'stationControl') }
}

function phone() {
  const link = station(['radioLevels'], 3, 1)
  vi.mocked(getSettings).mockResolvedValue(structuredClone(settings) as never)
  const snap = { activeRadioId: 1, mycall: 'N0CALL', mygrid: 'AA00', mode: 'qso', stations: [], recentDecodes: [],
    conversations: [], highlights: [], link: { tier: 'FT8', dtSec: 0 }, radio: { source: 'native', operatingMode: 'phone',
      dialMhz: 14.275, band: '20m', sideband: 'USB', rigMode: 'LSB', catOk: true, txEnabled: false, transmitting: false, rigKeyed: false,
      tuning: false, txAllowed: true, filterWidthHz: 2400, rfPower: 0.5, micGain: 0.5, compLevel: 0.5, notchFreqHz: 600, cwWpm: 22,
      cwKeyer: 'cat', nrLevel: 0.3, agc: 'fast', refusedAgc: null, nb: false, nr: false, notch: false, manualNotch: false, comp: false,
      vox: false, splitTxMhz: null, smeterDb: null } } as unknown as AppSnapshot
  const frame = structuredClone(frames.spe) as MonitorFrame
  frame.station.radio.id = 1; frame.station.radio.readings.cat!.connectionGeneration = 7; frame.station.amplifier = null
  const observation = { status: 'current', frame } as MonitorState
  const ui = render(<StationControlContext.Provider value={false}><StationDataContext.Provider value>
    <RemoteOperationsContext.Provider value={link.client}><RemoteObservationContext.Provider value={observation}>
      <PhoneCockpit snap={snap} theme="dark" spots={[]} onWorkSpot={() => {}} onSnap={() => {}}/>
    </RemoteObservationContext.Provider></RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>)
  const slider = () => ui.getByRole('slider', { name: t('phone.mic.aria') }) as HTMLInputElement
  return { ...link, ...ui, slider }
}

it('a drag keeps its intermediate and final values across control lapses and is sent once, on current control', async () => {
  const h = phone()
  await until(() => h.client.getSnapshot().fresh && !h.slider().disabled)
  fireEvent.pointerDown(h.slider())
  fireEvent.change(h.slider(), { target: { value: '40' } })
  expect(Number(h.slider().value)).toBe(40)
  const lapses = h.lapses(), heartbeats = h.heartbeats()
  for (let elapsed = 0; elapsed < 2600; elapsed += 10) {
    await step(10)
    expect(h.slider().disabled).toBe(false)
    expect(Number(h.slider().value)).toBe(40)
  }
  // Positive controls: control really lapsed during the drag, and the spy records real sends.
  expect(h.lapses()).toBeGreaterThan(lapses)
  expect(h.heartbeats()).toBeGreaterThan(heartbeats)
  expect(h.writes()).toHaveLength(0)
  fireEvent.change(h.slider(), { target: { value: '35' } })
  expect(Number(h.slider().value)).toBe(35)
  // Release during a lapse: nothing leaves until control is current again.
  await until(() => !h.client.getSnapshot().fresh)
  fireEvent.pointerUp(h.slider())
  expect(h.writes()).toHaveLength(0)
  await until(() => h.writes().length === 1)
  expect(h.writes()[0].request.action).toEqual({ action: 'radio.level', mode: 'phone', level: 'micGain', expected: 0.5, value: 0.35 })
  expect(h.wire.every(w => w.request.type !== 'stationControl' || w.fresh)).toBe(true)
  expect(pushToast).not.toHaveBeenCalled()
})

it('a drag released while control stays stale is refused as not sent, and nothing is sent', async () => {
  const h = phone()
  await until(() => h.client.getSnapshot().fresh && !h.slider().disabled)
  fireEvent.pointerDown(h.slider())
  fireEvent.change(h.slider(), { target: { value: '35' } })
  h.link.answering = false
  await until(() => !h.client.getSnapshot().fresh)
  fireEvent.pointerUp(h.slider())
  for (let elapsed = 0; elapsed < 1700; elapsed += 10) {
    await step(10)
    expect(h.writes()).toHaveLength(0)
  }
  expect(h.heartbeats()).toBeGreaterThan(0)
  expect(pushToast).toHaveBeenCalledWith(t('remote.controlNotSent'), 'error')
})

it('a typed TX offset survives a control lapse to Enter', async () => {
  const h = station(['ftSettings', 'ftRuntime'], 4, 3, { transmitEpoch: '000000000000002a' })
  const settingsContext = { key: '1'.padStart(32, '0'), txOffsetHz: 1500, rxOffsetHz: 1500, holdTxFreq: false, txEven: true, txCycleAuto: true }
  const snap = { activeRadioId: 3, mode: 'qso', mycall: 'N0CALL', mygrid: 'AA00', stations: [], recentDecodes: [], conversations: [], highlights: [],
    harqRescues: 0, clearTick: 0, qso: null, link: { tier: 'FT8', periodSecs: 15 }, remoteFtSettings: settingsContext,
    remoteFtRuntime: { settings: settingsContext, skipTx1: false },
    radio: { source: 'native', sourceLabel: 'Native', decodeDepth: 3, operatingMode: 'digital', dialMhz: 14.074, band: '20m', sideband: 'USB',
      slot: 0, catOk: true, txEnabled: false, transmitting: false, rigKeyed: false, tuning: false, rxOffsetHz: 1500, txOffsetHz: 1500,
      txLevel: 0.5, txEven: true, txCycleAuto: true } } as unknown as AppSnapshot
  const frame = structuredClone(frames.spe) as MonitorFrame
  Object.assign(frame.station.radio, { id: 3, catConnected: true, rigKeyed: false, nexusBusy: false })
  frame.station.radio.readings.cat = { connectionGeneration: 7, readSequence: 10, ageMs: 0 }
  frame.station.radio.readings.ptt = { connectionGeneration: 7, readSequence: 10, ageMs: 0 }
  frame.station.amplifier = null
  const noop = () => {}
  const panels = { layout: { v: 1 as const, state: {}, share: {} }, stateOf: (id: string) => ['waterfall', 'scope', 'offsets'].includes(id) ? 'docked' as const : 'removed' as const,
    setPanelState: noop, shareOf: () => 1, setShare: noop, setShares: noop, undo: noop, canUndo: false, undoRemoves: [], reset: noop }
  const onTune = vi.fn()
  const ui = render(<StationControlContext.Provider value={false}><StationDataContext.Provider value>
    <RemoteOperationsContext.Provider value={h.client}><RemoteObservationContext.Provider value={{ status: 'current', frame }}>
      <OperateCockpit snap={snap} tier="FT8" onSnap={noop} theme="dark" bandPlan={[]} onTierChange={noop} onSetFrequency={noop}
        onSourceChange={noop} onTune={onTune} onCall={noop} onSetTxLevel={noop} onSetMode={noop} onSetTxEven={noop} onSetTxCycleAuto={noop}
        onResend={noop} onFreetext={noop} onLog={noop} onOverrideTx={noop} onHaltTx={noop} roster={<div/>} needByCall={new Map()}
        selectedCall={null} onSelect={noop} layoutMode="classic" onLayoutMode={noop} panels={panels} active={false}/>
    </RemoteObservationContext.Provider></RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>)
  const tx = () => ui.container.querySelectorAll<HTMLInputElement>('.df-field input')[1]
  await until(() => h.client.getSnapshot().fresh && !tx().disabled)
  act(() => tx().focus())
  fireEvent.change(tx(), { target: { value: '1800' } })
  const lapses = h.lapses()
  for (let elapsed = 0; elapsed < 2600; elapsed += 10) {
    await step(10)
    expect(tx().disabled).toBe(false)
    expect(tx().value).toBe('1800')
  }
  expect(h.lapses()).toBeGreaterThan(lapses)
  // Enter during a lapse still commits the typed value; the transport decides when it may send.
  await until(() => !h.client.getSnapshot().fresh)
  fireEvent.keyDown(tx(), { key: 'Enter' })
  expect(onTune).toHaveBeenCalledTimes(1)
  expect(onTune).toHaveBeenCalledWith(1800, 'tx')
})

/** The scope wheel as PhoneScope attaches it: the same useWheelTune listener, remote surface. */
function Scope({ snap }: { snap: AppSnapshot }) {
  const ref = useRef<HTMLDivElement>(null)
  useWheelTune(ref, { ...snap.radio, radioId: snap.activeRadioId, enabled: true, stepHz: 100, remoteFrequency: true })
  return <div ref={ref} data-testid="scope"/>
}

/** The real readout digits and a scope wheel sharing the browser's WheelTuning, over the same
 * 250 ms station, which here also applies a station command after the round trip. */
function tuner() {
  const h = station(['frequency'], 3, 1)
  h.link.applies = true
  let snap = { activeRadioId: 1, link: { tier: 'FT8' }, radio: { source: 'native', operatingMode: 'phone', dialMhz: 7.2, band: '40m',
    sideband: 'LSB', catOk: true, txEnabled: false, transmitting: false, rigKeyed: false, tuning: false, txAllowed: true } } as unknown as AppSnapshot
  const application = { invoke: vi.fn(async () => structuredClone(snap)), age: () => 0, held: () => snap } as unknown as ApplicationClient
  // Exactly as BrowserApplication builds it, refusals included.
  const tuning = new WheelTuning(h.client, application, error => pushToast(controlFailureMessage(error), 'error'))
  tuning.activate()
  disposers.push(() => tuning.dispose())
  const frame = structuredClone(frames.spe) as MonitorFrame
  frame.station.radio.id = 1; frame.station.radio.readings.cat!.connectionGeneration = 7; frame.station.amplifier = null
  const page = () => <StationControlContext.Provider value={false}><StationDataContext.Provider value>
    <RemoteOperationsContext.Provider value={h.client}><RemoteWheelTuningContext.Provider value={tuning}>
      <RemoteObservationContext.Provider value={{ status: 'current', frame } as MonitorState}>
        <CockpitHeader snap={snap} modeIndicator="Phone" bandControl={null} onCommitDial={vi.fn()} wheelTune digitTune remoteFrequency/>
        <Scope snap={snap}/>
      </RemoteObservationContext.Provider>
    </RemoteWheelTuningContext.Provider></RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>
  const ui = render(page())
  const digit = () => ui.container.querySelector('[data-decade="3"]')!
  const readout = () => ui.container.querySelector<HTMLElement>('.readout')!
  const notch = (kind: 'digit wheel' | 'digit keyboard' | 'scope wheel') => kind === 'digit wheel' ? fireEvent.wheel(digit(), { deltaY: -100, deltaMode: 0 })
    : kind === 'scope wheel' ? fireEvent.wheel(ui.getByTestId('scope'), { deltaY: -100, deltaMode: 0 }) : fireEvent.keyDown(readout(), { key: 'ArrowUp' })
  const tunable = () => h.client.getSnapshot().fresh && digit()?.closest('.readout-val')?.getAttribute('data-digit-tune') === 'on'
  const moveDial = (dialMhz: number) => { snap = { ...snap, radio: { ...snap.radio, dialMhz } }; ui.rerender(page()) }
  return { ...h, ...ui, notch, tunable, moveDial }
}

it.each([['digit wheel', 1000], ['digit keyboard', 100], ['scope wheel', 100]] as const)('a %s burst begun in a 250 ms heartbeat lapse lands its full step count, sent once control is current', async (kind, stepHz) => {
  const h = tuner()
  await until(h.tunable)
  const lapses = h.lapses()
  await until(() => !h.client.getSnapshot().fresh)
  const staleAtNotch: boolean[] = []
  for (let n = 0; n < 3; n++) {
    staleAtNotch.push(!h.client.getSnapshot().fresh)
    h.notch(kind)
    await step(20)
  }
  // Positive controls: the burst really began while control was stale, the lease still held.
  expect(staleAtNotch[0]).toBe(true)
  expect(h.lapses()).toBeGreaterThan(lapses)
  await until(() => h.writes().length === 1)
  expect(h.writes()[0].request.action).toEqual({ action: 'radio.frequency', dialMhz: Math.round(7.2e6 + 3 * stepHz) / 1e6, band: '40m', sideband: 'LSB' })
  await step(500)
  expect(h.writes()).toHaveLength(1)
  expect(h.wire.every(w => w.request.type !== 'stationControl' || w.fresh)).toBe(true)
  expect(pushToast).not.toHaveBeenCalled()
})

it('a burst held through a longer lapse sends nothing while stale, then lands whole once control is current', async () => {
  const h = tuner()
  await until(h.tunable)
  // The next heartbeat reply takes 900 ms: control stays stale for about 650 ms, still a brief lapse.
  h.link.roundTripMs = 900
  await until(() => !h.client.getSnapshot().fresh)
  const heartbeats = h.heartbeats()
  for (let n = 0; n < 4; n++) {
    expect(h.client.getSnapshot()).toMatchObject({ fresh: false, state: { phase: 'controlling' } })
    h.notch('digit wheel')
    for (let elapsed = 0; elapsed < 100; elapsed += 10) { await step(10); expect(h.writes()).toHaveLength(0) }
  }
  // The spy is live: it recorded the heartbeat still waiting on its reply.
  expect(h.heartbeats()).toBe(heartbeats)
  expect(h.heartbeats()).toBeGreaterThan(0)
  await until(() => h.writes().length === 1)
  expect(h.writes()[0].request.action).toEqual({ action: 'radio.frequency', dialMhz: 7.204, band: '40m', sideband: 'LSB' })
  expect(h.wire.every(w => w.request.type !== 'stationControl' || w.fresh)).toBe(true)
  expect(pushToast).not.toHaveBeenCalled()
})

it('a burst that ends while control is still stale is refused as a whole with "Not sent", and nothing is sent', async () => {
  const h = tuner()
  await until(h.tunable)
  h.link.answering = false
  await until(() => !h.client.getSnapshot().fresh)
  for (let n = 0; n < 3; n++) { h.notch('digit wheel'); await step(20) }
  for (let elapsed = 0; elapsed < 1700; elapsed += 10) { await step(10); expect(h.writes()).toHaveLength(0) }
  expect(h.heartbeats()).toBeGreaterThan(0)
  expect(pushToast).toHaveBeenCalledTimes(1)
  expect(pushToast).toHaveBeenCalledWith(t('remote.controlNotSent'), 'error')
})

it('a held burst never replays its target over a dial that moved during the lapse', async () => {
  const h = tuner()
  await until(h.tunable)
  h.link.roundTripMs = 900
  await until(() => !h.client.getSnapshot().fresh)
  for (let n = 0; n < 2; n++) { h.notch('digit wheel'); await step(50) }
  // The station's dial moves (a knob at the shack) before control is current again.
  h.moveDial(7.21)
  await until(() => h.client.getSnapshot().fresh)
  await step(500)
  expect(h.writes()).toHaveLength(0)
  expect(pushToast).toHaveBeenCalledWith(t('remote.controlNotSent'), 'error')
})
