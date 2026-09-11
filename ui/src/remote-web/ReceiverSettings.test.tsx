// @vitest-environment jsdom
import { afterEach, beforeAll, beforeEach, expect, it, vi } from 'vitest'
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
    getSpectrumRow: vi.fn(async () => ({ row: [], loHz: 200, hiHz: 4000 })),
    js8SetSpeed: vi.fn(async () => reading.current),
    getLog: vi.fn(async () => []), getLicensedBandPlan: vi.fn(async () => []), getSettings: vi.fn(async () => ({})) }
})
vi.mock('./useJs8Context', () => ({ useJs8Context: () => ({ remote: true, value: null, loading: false, refresh: () => {} }) }))
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn(async (run: () => Promise<unknown>) => run()) }))
import { setRxOffset, setTxOffset, setDecodeDepth } from '../api'
import { pushToast } from '../toast'

const clients: OperationClient[] = []
beforeAll(() => { globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver })
afterEach(() => { cleanup(); clients.splice(0).forEach(c => c.disconnected()); vi.useRealTimers(); vi.clearAllMocks(); vi.restoreAllMocks() })
beforeEach(() => { localStorage.clear(); vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null) })
const noop = () => {}
const panels = { layout: { v: 1 as const, state: {}, share: {} }, stateOf: (id: string) => ['waterfall', 'scope', 'offsets'].includes(id) ? 'docked' as const : 'removed' as const, setPanelState: noop,
  shareOf: () => 1, setShare: noop, setShares: noop, undo: noop, canUndo: false, undoRemoves: [], reset: noop }
async function tick(ms = 0) { await act(async () => { await vi.advanceTimersByTimeAsync(ms) }) }
function fixture(kind: 'JS8' | 'FT8', capabilities: ControlCapability[] = ['receiverSettings'], version: 2 | 3 = 3) {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'setTimeout', 'clearTimeout', 'performance'] })
  reading.current = { ...structuredClone(js8Fixture.state), stations: [], inbox: [], queue: [], pendingReply: null } as Js8State
  const snap = { activeRadioId: 3, mode: 'qso', mycall: 'N0CALL', mygrid: 'AA00', stations: [], recentDecodes: [], conversations: [], highlights: [],
    harqRescues: 0, clearTick: 0, qso: null, link: { tier: kind, periodSecs: 15 }, radio: { source: 'native', sourceLabel: 'Native',
      decodeDepth: 3, operatingMode: 'digital', dialMhz: 14.078, band: '20m', sideband: 'USB', slot: 0, catOk: true, txEnabled: false,
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
    pendingControlStorage(() => storage, 'receiver-settings-test', async (_key, run) => run()))
  clients.push(client)
  const state = { stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(), revision: 1,
    commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false,
    controls: { context: { radioId: 3, radioConnection: 7, ampConnection: null, ampReadSequence: null }, capabilities } }
  const reply = (value: unknown) => client.receive({ type: 'operationResponse', requestId: sent[sent.length - 1].request.requestId, value })
  client.open(); reply(state)
  const onSnap = vi.fn(), onTune = vi.fn()
  const view = (current = snap, observation = frame, available = true, local = false) =>
    <StationControlContext.Provider value={local}><StationDataContext.Provider value={available}>
      <RemoteOperationsContext.Provider value={client}><RemoteObservationContext.Provider value={{ status: 'current', frame: observation }}>
        {kind === 'JS8' ? <Js8Cockpit snap={current} onSnap={onSnap} panels={panels}/> :
          <OperateCockpit snap={current} tier="FT8" onSnap={onSnap} theme="dark" bandPlan={[]} onTierChange={noop} onSetFrequency={noop}
            onSourceChange={noop} onTune={onTune} onCall={noop} onSetTxLevel={noop} onSetMode={noop} onSetTxEven={noop} onSetTxCycleAuto={noop}
            onResend={noop} onFreetext={noop} onLog={noop} onOverrideTx={noop} onHaltTx={noop} roster={<div/>} needByCall={new Map()}
            selectedCall={null} onSelect={noop} layoutMode="classic" onLayoutMode={noop} panels={panels} active={false}/>
        }
      </RemoteObservationContext.Provider></RemoteOperationsContext.Provider>
    </StationDataContext.Provider></StationControlContext.Provider>
  const rendered = render(view())
  const canvas = () => {
    const e = rendered.container.querySelector<HTMLCanvasElement>('canvas.waterfall-canvas')!
    expect(e).toBeTruthy()
    e.getBoundingClientRect = () => ({ left: 0, top: 0, right: 400, bottom: 200, width: 400, height: 200, x: 0, y: 0 }) as DOMRect
    return e
  }
  return { ...rendered, view, snap, frame, state, client, sent, reply, onSnap, onTune, canvas,
    writes: () => sent.filter(w => w.request.type === 'stationControl') }

}

it('saves depth once from the existing chip and waits for the station sample', async () => {
  const h = fixture('FT8'); await tick()
  const chip = h.container.querySelector<HTMLButtonElement>('.cockpit-depth-chip:first-child')!
  expect(chip.disabled).toBe(false); expect(h.writes()).toHaveLength(0)
  fireEvent.click(chip); await tick()
  expect(h.writes()).toHaveLength(1)
  const request = h.writes()[0].request
  expect(request.action).toEqual({ action: 'decoder.depth', expectedTier: 'FT8', expectedDepth: 3, depth: 1 })
  expect(request.context).toEqual(h.state.controls.context)
  expect(chip.disabled).toBe(true); fireEvent.click(chip); await tick(); expect(h.writes()).toHaveLength(1)
  act(() => h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'settingsSaved' })); await tick()
  expect(chip.getAttribute('aria-pressed')).toBe('false')
  h.rerender(h.view({ ...h.snap, radio: { ...h.snap.radio, decodeDepth: 1 } })); await tick()
  expect(chip.getAttribute('aria-pressed')).toBe('true')
  expect(setDecodeDepth).not.toHaveBeenCalled(); expect(h.onSnap).not.toHaveBeenCalled()
})

it.each(['FT8', 'JS8'] as const)('%s uses the real waterfall for RX only and refuses TX/combined gestures', async kind => {
  const h = fixture(kind); await tick()
  for (const gesture of [{ button: 2 }, { button: 0, shiftKey: true }, { button: 0, ctrlKey: true }, { button: 0, metaKey: true }, { button: 1 }]) {
    fireEvent.mouseDown(h.canvas(), { clientX: 100, ...gesture }); await tick()
  }
  expect(h.writes()).toHaveLength(0); expect(h.onTune).not.toHaveBeenCalled()
  fireEvent.mouseDown(h.canvas(), { clientX: 100, button: 0 }); await tick()
  expect(h.writes()).toHaveLength(1)
  expect(h.writes()[0].request.action).toEqual({ action: 'receiver.rxOffset', expectedTier: kind, expectedHz: 1500, hz: 900 })
  expect(h.writes()[0].request.context).toEqual(h.state.controls.context)
  fireEvent.mouseDown(h.canvas(), { clientX: 200, button: 0 }); await tick(); expect(h.writes()).toHaveLength(1)
  act(() => h.reply({ operation: 'stationControl', operationId: h.writes()[0].request.requestId, outcome: 'applied', evidence: 'settingsSaved' })); await tick()
  expect(h.onSnap).not.toHaveBeenCalled(); expect(setRxOffset).not.toHaveBeenCalled(); expect(setTxOffset).not.toHaveBeenCalled()
})

it('commits the existing RX field once, keeps TX disabled and displays only saved station samples', async () => {
  const h = fixture('FT8'); await tick()
  const fields = h.container.querySelectorAll<HTMLInputElement>('.df-field input'), rx = fields[0], tx = fields[1]
  expect(rx.disabled).toBe(false); expect(tx.disabled).toBe(true)
  expect(rx.value).toBe('1500'); expect(tx.value).toBe('1500')
  fireEvent.focus(rx); fireEvent.change(rx, { target: { value: '725.25' } }); fireEvent.blur(rx); await tick()
  expect(h.writes()).toHaveLength(1)
  expect(h.writes()[0].request.action).toEqual({ action: 'receiver.rxOffset', expectedTier: 'FT8', expectedHz: 1500, hz: 725 })
  expect(rx.value).toBe('1500'); expect(tx.value).toBe('1500')
  act(() => h.reply({ operation: 'stationControl', operationId: h.writes()[0].request.requestId, outcome: 'applied', evidence: 'settingsSaved' })); await tick()
  expect(rx.value).toBe('1500')
  h.rerender(h.view({ ...h.snap, radio: { ...h.snap.radio, rxOffsetHz: 725 } })); await tick()
  expect(rx.value).toBe('725'); expect(tx.value).toBe('1500')
  expect(h.onTune).not.toHaveBeenCalled(); expect(h.onSnap).not.toHaveBeenCalled()
})

it('discards an RX draft if a local operator changes the value or authority disappears while editing', async () => {
  const h = fixture('FT8'); await tick()
  const rx = h.container.querySelector<HTMLInputElement>('.df-field input')!
  fireEvent.focus(rx); fireEvent.change(rx, { target: { value: '725' } })
  h.rerender(h.view({ ...h.snap, radio: { ...h.snap.radio, rxOffsetHz: 1200 } })); await tick()
  fireEvent.blur(rx); await tick(); expect(h.writes()).toHaveLength(0); expect(rx.value).toBe('1200')
  h.rerender(h.view()); await tick(); fireEvent.focus(rx); fireEvent.change(rx, { target: { value: '725' } })
  h.rerender(h.view(h.snap, h.frame, false)); await tick(); fireEvent.blur(rx); await tick()
  expect(h.writes()).toHaveLength(0); expect(rx.disabled).toBe(true); expect(rx.value).toBe('1500')
})

it('double-clicks the existing JS8 offset row into an RX-only intent', async () => {
  const h = fixture('JS8'); await tick()
  const row = h.container.querySelector('.js8-offset-row')!
  expect(row.textContent).toContain('1000')
  fireEvent.doubleClick(row); await tick()
  expect(h.writes()).toHaveLength(1)
  expect(h.writes()[0].request.action).toEqual({ action: 'receiver.rxOffset', expectedTier: 'JS8', expectedHz: 1500, hz: 1000 })
  expect(setRxOffset).not.toHaveBeenCalled(); expect(setTxOffset).not.toHaveBeenCalled()
})

it.each(['FT8', 'JS8'] as const)('%s receive gestures require their capability and fresh matching station context', async kind => {
  for (const [capabilities, version] of [[['decoderSettings'], 3], [['receiverSettings'], 2]] as [ControlCapability[], 2 | 3][]) {
    const h = fixture(kind, capabilities, version); await tick()
    fireEvent.mouseDown(h.canvas(), { clientX: 100, button: 0 }); await tick(); expect(h.writes()).toHaveLength(0); h.unmount()
  }
  const h = fixture(kind); await tick()
  for (const patch of [{ source: 'companion' as const }, { operatingMode: 'cw' }, { catOk: false }, { txEnabled: true }, { transmitting: true }, { tuning: true }, { rigKeyed: true }]) {
    h.rerender(h.view({ ...h.snap, radio: { ...h.snap.radio, ...patch } })); await tick()
    fireEvent.mouseDown(h.canvas(), { clientX: 100, button: 0 }); await tick(); expect(h.writes()).toHaveLength(0)
  }
  // Deliberately incomplete older reading; valid RadioStatus requires a source.
  const incomplete = { ...h.snap, radio: { ...h.snap.radio, source: undefined } } as unknown as AppSnapshot
  h.rerender(h.view(incomplete)); await tick()
  fireEvent.mouseDown(h.canvas(), { clientX: 100, button: 0 }); await tick(); expect(h.writes()).toHaveLength(0)
  h.rerender(h.view({ ...h.snap, link: { ...h.snap.link, tier: 'Q65' } })); await tick()
  fireEvent.mouseDown(h.canvas(), { clientX: 100, button: 0 }); await tick(); expect(h.writes()).toHaveLength(0)
  h.rerender(h.view({ ...h.snap, radio: { ...h.snap.radio, rxOffsetHz: NaN } })); await tick()
  fireEvent.mouseDown(h.canvas(), { clientX: 100, button: 0 }); await tick(); expect(h.writes()).toHaveLength(0)
  for (const patch of [{ id: 4 }, { rigKeyed: null }, { rigKeyed: true }, { catConnected: false }]) {
    const frame = structuredClone(h.frame); Object.assign(frame.station.radio, patch)
    h.rerender(h.view(h.snap, frame)); await tick(); fireEvent.mouseDown(h.canvas(), { clientX: 100, button: 0 }); await tick()
    expect(h.writes()).toHaveLength(0)
  }
  const stale = structuredClone(h.frame); stale.station.radio.readings.ptt!.ageMs = 1000
  h.rerender(h.view(h.snap, stale)); await tick(); fireEvent.mouseDown(h.canvas(), { clientX: 100, button: 0 }); await tick()
  expect(h.writes()).toHaveLength(0)
  h.rerender(h.view(h.snap, h.frame, false)); await tick(); fireEvent.mouseDown(h.canvas(), { clientX: 100, button: 0 }); await tick()
  expect(h.writes()).toHaveLength(0)
  h.rerender(h.view()); await tick(); fireEvent.mouseDown(h.canvas(), { clientX: 100, button: 0 }); await tick()
  expect(h.writes()).toHaveLength(1)
  act(() => h.client.disconnected()); await tick(); fireEvent.mouseDown(h.canvas(), { clientX: 200, button: 0 }); await tick()
  expect(h.writes()).toHaveLength(1)
})

it('reports a refused save and keeps the depth and RX readings unchanged', async () => {
  const h = fixture('FT8'); await tick()
  fireEvent.click(h.container.querySelector('.cockpit-depth-chip:first-child')!); await tick()
  act(() => h.reply({ operation: 'stationControl', operationId: h.writes()[0].request.requestId, outcome: 'rejected', reason: 'persistenceFailed' })); await tick()
  expect(pushToast).toHaveBeenCalled()
  expect(h.container.querySelector('.cockpit-depth-chip:last-child')?.getAttribute('aria-pressed')).toBe('true')
  expect(h.container.querySelector<HTMLInputElement>('.df-field input')?.value).toBe('1500')
})

it.each(['FT8', 'JS8'] as const)('%s local waterfall keeps native RX, TX and combined marker gestures', async kind => {
  const h = fixture(kind); h.rerender(h.view(h.snap, h.frame, true, true)); await tick()
  for (const [gesture, target] of [[{ button: 0 }, 'rx'], [{ button: 2 }, 'tx'], [{ button: 0, ctrlKey: true }, 'both']] as const) {
    fireEvent.mouseDown(h.canvas(), { clientX: 100, ...gesture }); await tick()
    if (kind === 'FT8') expect(h.onTune).toHaveBeenLastCalledWith(900, target)
  }
  if (kind === 'JS8') {
    expect(setRxOffset).toHaveBeenCalledTimes(2); expect(setTxOffset).toHaveBeenCalledTimes(2)
    expect(setRxOffset).toHaveBeenLastCalledWith(900); expect(setTxOffset).toHaveBeenLastCalledWith(900)
  }
  expect(h.writes()).toHaveLength(0)
})
