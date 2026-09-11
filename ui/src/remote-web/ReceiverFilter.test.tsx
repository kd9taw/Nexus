// @vitest-environment jsdom
import { afterEach, beforeAll, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render } from '@testing-library/react'
import { CwCockpit } from '../components/CwCockpit'
import { PhoneCockpit } from '../components/PhoneCockpit'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { OperationClient } from './operation-client'
import { pendingControlStorage } from './control-storage'
import type { AppSnapshot } from '../types'
import type { OperationState } from './operation-protocol'
import type { ControlCapability } from './station-operation'
import settings from '../components/__fixtures__/defaultSettings.json'
import { RemoteObservationContext } from './amplifier-observation'
import frames from '../remote-monitor/fixtures.v2.json'
import type { MonitorState } from '../remote-monitor/session'
import type { MonitorFrame } from '../remote-monitor/protocol'

vi.mock('../api', async original => {
  const actual = await original<Record<string, unknown>>()
  const reads: Record<string, unknown> = { getLicensedBandPlan: [], getBandPlan: [] }
  return Object.fromEntries(Object.entries(actual).map(([name, value]) => [name,
    typeof value === 'function' ? vi.fn(async () => structuredClone(reads[name] ?? {})) : value]))
})
// The real cockpit owns the BW controls. Only unrelated heavy children are
// substituted here; the compiled browser suite exercises the full application.
vi.mock('../components/PhoneScope', () => ({ PhoneScope: () => <div/> }))
vi.mock('../components/BandStrip', () => ({ BandStrip: () => <div/> }))
vi.mock('../components/VoiceKeyer', () => ({ VoiceKeyer: () => <div/> }))
vi.mock('../components/LogEntry', () => ({ LogEntry: () => <div/> }))
vi.mock('../components/SpotDialog', () => ({ SpotDialog: () => null }))
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn(async (run: () => Promise<unknown>) => run()) }))
import { getSettings, getCatCwUnprovenRigModels, cwDecode, setFilterWidth, setFrequency, setRigFunc, setPtt } from '../api'

const clients: OperationClient[] = []
beforeAll(() => {
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(() => { cleanup(); clients.splice(0).forEach(c => c.disconnected()); vi.useRealTimers(); vi.clearAllMocks() })
async function tick(ms = 0) { await act(async () => { await vi.advanceTimersByTimeAsync(ms) }) }

function fixture(mode: 'cw' | 'phone' = 'cw', capabilities: ControlCapability[] = ['receiverFilter'], version: 2 | 3 = 3, local = false) {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'setTimeout', 'clearTimeout', 'performance'] })
  vi.mocked(getSettings).mockResolvedValue(structuredClone(settings) as never)
  vi.mocked(getCatCwUnprovenRigModels).mockResolvedValue([])
  vi.mocked(cwDecode).mockResolvedValue({ text: '', wpm: 22, sent: [], candidates: [], keyerError: null,
    state: 'listening', headline: '', prompt: '', recommended: null, workedCall: null, rst: null, name: null } as never)
  const snap = { activeRadioId: 1, mycall: 'N0CALL', mygrid: 'AA00', mode: 'qso', stations: [], recentDecodes: [],
    conversations: [], highlights: [], link: { tier: 'FT8', dtSec: 0 }, radio: { source: 'native', operatingMode: mode,
      dialMhz: 14.275, band: '20m', sideband: 'USB', rigMode: mode === 'cw' ? 'CW' : 'LSB', catOk: true,
      txEnabled: false, transmitting: false, rigKeyed: false, tuning: false, txAllowed: true, filterWidthHz: mode === 'cw' ? 500 : 2400,
      cwWpm: 22, cwKeyer: 'cat', nrLevel: 0.3, agc: 'fast', refusedAgc: null, nb: false, nr: false, notch: null,
      splitTxMhz: null, smeterDb: null } } as unknown as AppSnapshot
  const sent: any[] = [], values = new Map<string, string>()
  const storage = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const client = new OperationClient(wire => sent.push(JSON.parse(wire)), true, () => 1000 + performance.now(), undefined, version,
    pendingControlStorage(() => storage, 'receiver-filter', async (_key, run) => run()))
  clients.push(client)
  const state: OperationState = { stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(), revision: 1,
    commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false,
    controls: { context: { radioId: 1, radioConnection: 7, ampConnection: null, ampReadSequence: null }, capabilities } }
  const reply = (value: unknown) => client.receive({ type: 'operationResponse', requestId: sent[sent.length - 1].request.requestId, value })
  client.open(); reply(state)
  const onSnap = vi.fn(), Component = mode === 'cw' ? CwCockpit : PhoneCockpit
  const frame = structuredClone(frames.spe) as MonitorFrame
  frame.station.radio.id = 1; frame.station.radio.readings.cat!.connectionGeneration = 7; frame.station.amplifier = null
  const observation = { status: 'current', frame } as MonitorState
  const view = (current = snap, available = true, shown = observation) => <StationControlContext.Provider value={local}><StationDataContext.Provider value={available}>
    <RemoteOperationsContext.Provider value={local ? null : client}>
      <RemoteObservationContext.Provider value={shown}>
      <Component snap={current} theme="dark" spots={[]} onWorkSpot={() => {}} onSnap={onSnap}/>
      </RemoteObservationContext.Provider>
    </RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>
  const ui = render(view())
  const writes = () => sent.filter(w => w.request.type === 'stationControl')
  const finish = (outcome: 'applied' | 'unknown' = 'applied') => {
    const ws = writes(), request = ws[ws.length - 1].request
    reply({ operation: 'stationControl', operationId: request.requestId, outcome,
      ...(outcome === 'applied' ? { evidence: 'radioReadback' } : { reason: 'hardwareUnconfirmed' }) })
  }
  return { ...ui, snap, state, client, reply, onSnap, view, writes, finish, observation,
    buttons: () => [...ui.container.querySelectorAll<HTMLButtonElement>('.ph-filter-step')],
    value: () => ui.container.querySelector('.ph-filter-val')!.textContent }
}

it.each(['radio', 'connection'] as const)('the native BW control waits for the displayed %s after a same-width handoff', async changed => {
  const h = fixture(), context = { ...h.state.controls!.context, ...(changed === 'radio' ? { radioId: 2 } : { radioConnection: 8 }) }
  await tick(1000)
  const snap = { ...h.snap, activeRadioId: context.radioId }
  act(() => h.reply({ ...h.state, revision: 2, controls: { ...h.state.controls!, context } })); await tick()
  h.rerender(h.view(snap)); await tick()
  fireEvent.click(h.buttons()[1]); await tick()
  expect(h.writes()).toHaveLength(0)
  const fresh = structuredClone(h.observation)
  fresh.frame!.station.radio.id = context.radioId
  fresh.frame!.station.radio.readings.cat!.connectionGeneration = context.radioConnection!
  h.rerender(h.view(snap, true, fresh)); await tick()
  expect(h.buttons().every(b => !b.disabled)).toBe(true)
  fireEvent.click(h.buttons()[1]); await tick()
  expect(h.writes()).toHaveLength(1); expect(h.writes()[0].request.context).toEqual(context)
  act(() => h.finish()); await tick()
})

it.each(['cw', 'phone'] as const)('the actual %s BW steppers require readback and a fresh station sample', async mode => {
  const h = fixture(mode), before = h.snap.radio.filterWidthHz!, step = mode === 'cw' ? 50 : 100
  const label = (hz: number) => mode === 'cw' ? String(hz) : `${(hz / 1000).toFixed(1)}k`
  await tick()
  expect(h.buttons()).toHaveLength(2); expect(h.buttons().every(b => !b.disabled)).toBe(true)
  fireEvent.click(h.buttons()[1]); await tick()
  expect(h.writes()).toHaveLength(1)
  expect(h.writes()[0].request.action).toEqual({ action: 'radio.filterWidth', mode, expectedHz: before, hz: before + step })
  expect(h.writes()[0].request.context).toEqual(h.state.controls!.context)
  expect(h.value()).toBe(label(before)); expect(h.onSnap).not.toHaveBeenCalled()
  fireEvent.click(h.buttons()[0]); await tick(); expect(h.writes()).toHaveLength(1)
  act(() => h.finish()); await tick()
  expect(h.value()).toBe(label(before)); expect(h.onSnap).not.toHaveBeenCalled()
  const later = { ...h.snap, radio: { ...h.snap.radio, filterWidthHz: before + step } }
  h.rerender(h.view(later)); await tick(1000)
  act(() => h.reply({ ...h.state, revision: 2, commandWindowId: crypto.randomUUID(), nextSequence: 2 })); await tick()
  expect(h.value()).toBe(label(before + step))
  fireEvent.click(h.buttons()[0]); await tick()
  expect(h.writes()).toHaveLength(2)
  expect(h.writes()[1].request.action).toEqual({ action: 'radio.filterWidth', mode, expectedHz: before + step, hz: before })
  act(() => h.finish()); await tick()
  for (const fn of [setFilterWidth, setFrequency, setRigFunc, setPtt]) expect(fn).not.toHaveBeenCalled()
})

it.each(['cw', 'phone'] as const)('retains the native %s range and never reverses the requested direction', async mode => {
  const h = fixture(mode), max = mode === 'cw' ? 2000 : 4000
  h.rerender(h.view({ ...h.snap, radio: { ...h.snap.radio, filterWidthHz: max + 100 } })); await tick()
  fireEvent.click(h.buttons()[1]); await tick(); expect(h.writes()).toHaveLength(0)
  fireEvent.click(h.buttons()[0]); await tick()
  expect(h.writes()[0].request.action).toEqual({ action: 'radio.filterWidth', mode, expectedHz: max + 100, hz: max })
  act(() => h.finish()); await tick()
})

it.each([[[], 3], [['receiverFilter'], 2]] as const)('keeps ungranted and older station clients inert', async (capabilities, version) => {
  const h = fixture('cw', [...capabilities], version); await tick()
  expect(h.buttons().every(b => b.disabled)).toBe(true)
  fireEvent.click(h.buttons()[1]); await tick(); expect(h.writes()).toHaveLength(0)
})

it.each(['unavailable', 'keyed', 'armed', 'busy', 'mode', 'radio', 'stale', 'disconnected'] as const)('refuses %s context at the real controls', async change => {
  const h = fixture(), next = structuredClone(h.snap)
  if (change === 'unavailable') next.radio.filterWidthHz = undefined
  if (change === 'keyed') next.radio.rigKeyed = true
  if (change === 'armed') next.radio.txEnabled = true
  if (change === 'busy') next.radio.txBusyReason = 'manualPtt'
  if (change === 'mode') next.radio.operatingMode = 'phone'
  if (change === 'radio') next.activeRadioId = 2
  if (change === 'disconnected') act(() => h.client.disconnected())
  h.rerender(h.view(next, change !== 'stale')); await tick()
  expect(h.buttons().every(b => b.disabled)).toBe(true)
  fireEvent.click(h.buttons()[1]); await tick(); expect(h.writes()).toHaveLength(0)
  expect(setFilterWidth).not.toHaveBeenCalled()
})

it('keeps an uncertain result recoverable without replaying or displaying the requested width', async () => {
  const h = fixture(); await tick()
  fireEvent.click(h.buttons()[1]); await tick()
  act(() => h.finish('unknown')); await tick(1000)
  act(() => h.reply({ ...h.state, revision: 2, commandWindowId: crypto.randomUUID(), nextSequence: 2 })); await tick()
  expect(h.buttons().every(b => b.disabled)).toBe(true)
  fireEvent.click(h.buttons()[1]); await tick()
  expect(h.writes()).toHaveLength(1); expect(h.value()).toBe('500'); expect(h.onSnap).not.toHaveBeenCalled()
})

it.each(['cw', 'phone'] as const)('preserves the native %s filter API and returned snapshot', async mode => {
  const h = fixture(mode, [], 3, true); await tick()
  const hz = h.snap.radio.filterWidthHz! + (mode === 'cw' ? 50 : 100)
  const later = { ...h.snap, radio: { ...h.snap.radio, filterWidthHz: hz } }
  vi.mocked(setFilterWidth).mockResolvedValue(later)
  fireEvent.click(h.buttons()[1]); await tick()
  expect(setFilterWidth).toHaveBeenCalledExactlyOnceWith(hz)
  expect(h.onSnap).toHaveBeenCalledWith(later); expect(h.writes()).toHaveLength(0)
})
