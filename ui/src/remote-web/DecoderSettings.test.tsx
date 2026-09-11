// @vitest-environment jsdom
import { afterEach, beforeAll, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render } from '@testing-library/react'
import { Js8Cockpit } from '../components/Js8Cockpit'
import { OperateCockpit } from '../components/OperateCockpit'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { RemoteObservationContext } from './amplifier-observation'
import { OperationClient } from './operation-client'
import { pendingControlStorage } from './control-storage'
import type { AppSnapshot, Js8State } from '../types'
import type { MonitorFrame } from '../remote-monitor/protocol'
import type { ControlCapability } from './station-operation'
import frames from '../remote-monitor/fixtures.v2.json'
import js8Fixture from './__fixtures__/js8.json'

const reading = vi.hoisted(() => ({ current: null as Js8State | null }))
vi.mock('../api', async importOriginal => {
  const actual = await importOriginal<Record<string, unknown>>()
  return { ...Object.fromEntries(Object.entries(actual).map(([key, value]) => [key, typeof value === 'function' ? vi.fn(async () => ({})) : value])),
    getJs8State: vi.fn(async () => { if (!reading.current) throw Error('readingUnavailable'); return reading.current }),
    js8SetSpeed: vi.fn(async () => reading.current),
    getLog: vi.fn(async () => []), getLicensedBandPlan: vi.fn(async () => []), getSettings: vi.fn(async () => ({})) }
})
vi.mock('./useJs8Context', () => ({ useJs8Context: () => ({ remote: true, value: null, loading: false, refresh: () => {} }) }))
vi.mock('../components/Waterfall', () => ({ Waterfall: () => <div/> }))
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn(async (run: () => Promise<unknown>) => run()) }))
import { js8SetSpeed, js8Enter, setMsk144Period } from '../api'
import { pushToast } from '../toast'

const clients: OperationClient[] = []
beforeAll(() => { globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver })
afterEach(() => { cleanup(); clients.splice(0).forEach(c => c.disconnected()); vi.useRealTimers(); vi.clearAllMocks() })
const noop = () => {}
const panels = { layout: { v: 1 as const, state: {}, share: {} }, stateOf: () => 'removed' as const, setPanelState: noop,
  shareOf: () => 1, setShare: noop, setShares: noop, undo: noop, canUndo: false, undoRemoves: [], reset: noop }
async function tick(ms = 0) { await act(async () => { await vi.advanceTimersByTimeAsync(ms) }) }
function fixture(kind: 'JS8' | 'MSK144', capabilities: ControlCapability[] = ['decoderSettings'], version: 2 | 3 = 3) {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'setTimeout', 'clearTimeout', 'performance'] })
  reading.current = { ...structuredClone(js8Fixture.state), activity: [], stations: [], inbox: [] } as Js8State
  const snap = { activeRadioId: 3, mode: 'qso', mycall: 'N0CALL', mygrid: 'AA00', stations: [], recentDecodes: [], conversations: [], highlights: [],
    harqRescues: 0, clearTick: 0, qso: null, link: { tier: kind, periodSecs: 15 }, radio: { source: 'native', sourceLabel: 'Native',
      operatingMode: 'digital', dialMhz: 14.078, band: '20m', sideband: 'USB', slot: 0, catOk: true, txEnabled: false,
      transmitting: false, rigKeyed: false, tuning: false, rxOffsetHz: 1500, txOffsetHz: 1500, txLevel: 0.5, txEven: true, txCycleAuto: true }
  } as unknown as AppSnapshot
  const frame = structuredClone(frames.spe) as MonitorFrame
  Object.assign(frame.station.radio, { id: 3, catConnected: true, rigKeyed: false, nexusBusy: false })
  frame.station.radio.readings.cat = { connectionGeneration: 7, readSequence: 10, ageMs: 0 }
  frame.station.radio.readings.ptt = { connectionGeneration: 7, readSequence: 10, ageMs: 0 }
  frame.station.amplifier = null
  const sent: any[] = [], values = new Map<string, string>()
  const storage = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const client = new OperationClient(wire => sent.push(JSON.parse(wire)), true, () => 1000, undefined, version,
    pendingControlStorage(() => storage, 'decoder-settings-test', async (_key, run) => run()))
  clients.push(client)
  const state = { stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(), revision: 1,
    commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false,
    controls: { context: { radioId: 3, radioConnection: 7, ampConnection: null, ampReadSequence: null }, capabilities } }
  const reply = (value: unknown) => client.receive({ type: 'operationResponse', requestId: sent[sent.length - 1].request.requestId, value })
  client.open(); reply(state)
  const onSnap = vi.fn()
  const view = (current = snap, observation = frame, available = true, local = false) =>
    <StationControlContext.Provider value={local}><StationDataContext.Provider value={available}>
      <RemoteOperationsContext.Provider value={client}><RemoteObservationContext.Provider value={{ status: 'current', frame: observation }}>
        {kind === 'JS8' ? <Js8Cockpit snap={current} onSnap={onSnap} panels={panels}/> :
          <OperateCockpit snap={current} tier="MSK144" onSnap={onSnap} theme="dark" bandPlan={[]} onTierChange={noop} onSetFrequency={noop}
            onSourceChange={noop} onTune={noop} onCall={noop} onSetTxLevel={noop} onSetMode={noop} onSetTxEven={noop} onSetTxCycleAuto={noop}
            onResend={noop} onFreetext={noop} onLog={noop} onOverrideTx={noop} onHaltTx={noop} roster={<div/>} needByCall={new Map()}
            selectedCall={null} onSelect={noop} layoutMode="classic" onLayoutMode={noop} panels={panels} active={false}/>
        }
      </RemoteObservationContext.Provider></RemoteOperationsContext.Provider>
    </StationDataContext.Provider></StationControlContext.Provider>
  const rendered = render(view())
  const widget = () => rendered.container.querySelector<HTMLButtonElement | HTMLSelectElement>(kind === 'JS8' ? '.js8-speed-chip:last-child' : '.cm-trperiod')!
  const gesture = () => kind === 'JS8' ? fireEvent.click(widget()) : fireEvent.change(widget(), { target: { value: '5' } })
  return { ...rendered, view, snap, frame, state, client, sent, reply, onSnap, widget, gesture,
    writes: () => sent.filter(w => w.request.type === 'stationControl') }
}

it.each(['JS8', 'MSK144'] as const)('%s uses the existing cockpit widget, one exact intent and later native samples', async kind => {
  const h = fixture(kind); await tick()
  expect(h.writes()).toHaveLength(0)
  expect(h.widget().disabled).toBe(false)
  h.gesture(); await tick()
  expect(h.writes()).toHaveLength(1)
  const request = h.writes()[0].request
  expect(request.action).toEqual(kind === 'JS8' ? { action: 'decoder.js8Speed', expectedSpeed: 1, speed: 3 }
    : { action: 'decoder.msk144Period', expectedPeriodSecs: 15, periodSecs: 5 })
  expect(request.context).toEqual(h.state.controls.context)
  expect(h.widget().disabled).toBe(true)
  h.gesture(); await tick(); expect(h.writes()).toHaveLength(1)
  act(() => h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'settingsSaved' }))
  await tick()
  if (kind === 'JS8') {
    expect(h.widget().getAttribute('aria-pressed')).toBe('false')
    reading.current = { ...reading.current!, speed: 'turbo' }; await tick(500)
    expect(h.widget().getAttribute('aria-pressed')).toBe('true')
  } else {
    expect((h.widget() as HTMLSelectElement).value).toBe('15')
    h.rerender(h.view({ ...h.snap, link: { ...h.snap.link, periodSecs: 5 } }))
    expect((h.widget() as HTMLSelectElement).value).toBe('5')
  }
  expect(h.onSnap).not.toHaveBeenCalled()
  expect(js8SetSpeed).not.toHaveBeenCalled(); expect(setMsk144Period).not.toHaveBeenCalled(); expect(js8Enter).not.toHaveBeenCalled()
})

it.each(['JS8', 'MSK144'] as const)('%s refuses old capability/version, lost observation and a different or busy station', async kind => {
  for (const [capabilities, version] of [[['decoder'], 3], [['decoderSettings'], 2]] as [ControlCapability[], 2 | 3][]) {
    const h = fixture(kind, capabilities, version); await tick()
    expect(h.widget().disabled).toBe(true); h.gesture(); await tick(); expect(h.writes()).toHaveLength(0); h.unmount()
  }
  const h = fixture(kind); await tick()
  for (const patch of [{ source: 'companion' as const }, { operatingMode: 'cw' }, { txEnabled: true }, { rigKeyed: true }, { transmitting: true }, { tuning: true }, { catOk: false }]) {
    h.rerender(h.view({ ...h.snap, radio: { ...h.snap.radio, ...patch } })); await tick()
    expect(h.widget().disabled).toBe(true); h.gesture(); await tick(); expect(h.writes()).toHaveLength(0)
  }
  for (const patch of [{ rigKeyed: null }, { rigKeyed: true }, { id: 8 }, { catConnected: false }]) {
    const frame = structuredClone(h.frame); Object.assign(frame.station.radio, patch)
    h.rerender(h.view(h.snap, frame)); await tick(); expect(h.widget().disabled).toBe(true)
  }
  const frame = structuredClone(h.frame); frame.station.radio.readings.ptt!.ageMs = 1000
  h.rerender(h.view(h.snap, frame)); await tick(); expect(h.widget().disabled).toBe(true)
  h.rerender(h.view(h.snap, h.frame, false)); await tick(); expect(h.widget().disabled).toBe(true)
  h.rerender(h.view()); await tick(); expect(h.widget().disabled).toBe(false)
  act(() => h.client.disconnected()); await tick(); expect(h.widget().disabled).toBe(true)
  expect(h.writes()).toHaveLength(0)
})

it('does not offer a JS8 speed change when its own decoder reading becomes unavailable', async () => {
  const h = fixture('JS8'); await tick(); expect(h.widget().disabled).toBe(false)
  reading.current = null; await tick(500)
  expect(h.widget().disabled).toBe(true); h.gesture(); await tick(); expect(h.writes()).toHaveLength(0)
  reading.current = { ...structuredClone(js8Fixture.state), activity: [], stations: [], inbox: [] } as Js8State
  await tick(500); expect(h.widget().disabled).toBe(false)
})

it.each(['JS8', 'MSK144'] as const)('%s persistence refusal leaves the displayed preference unchanged and reports failure', async kind => {
  const h = fixture(kind); await tick(); h.gesture(); await tick()
  act(() => h.reply({ operation: 'stationControl', operationId: h.writes()[0].request.requestId, outcome: 'rejected', reason: 'persistenceFailed' }))
  await tick()
  expect(pushToast).toHaveBeenCalled()
  if (kind === 'JS8') expect(h.widget().getAttribute('aria-pressed')).toBe('false')
  else expect((h.widget() as HTMLSelectElement).value).toBe('15')
  expect(h.writes()).toHaveLength(1)
})

it.each(['JS8', 'MSK144'] as const)('%s keeps the local cockpit on the existing native command', async kind => {
  const h = fixture(kind); h.rerender(h.view(h.snap, h.frame, true, true)); await tick()
  h.gesture(); await tick()
  expect(kind === 'JS8' ? js8SetSpeed : setMsk144Period).toHaveBeenCalledWith(kind === 'JS8' ? 3 : 5)
  expect(h.writes()).toHaveLength(0)
})
