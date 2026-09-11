// @vitest-environment jsdom
import { afterEach, beforeAll, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render } from '@testing-library/react'
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
// The real cockpit owns the Phone mode controls. Only unrelated heavy children are
// substituted here; the compiled browser suite exercises the full application.
vi.mock('../components/PhoneScope', () => ({ PhoneScope: () => <div/> }))
vi.mock('../components/BandStrip', () => ({ BandStrip: () => <div/> }))
vi.mock('../components/VoiceKeyer', () => ({ VoiceKeyer: () => <div/> }))
vi.mock('../components/LogEntry', () => ({ LogEntry: () => <div/> }))
vi.mock('../components/SpotDialog', () => ({ SpotDialog: () => null }))
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn(async (run: () => Promise<unknown>) => run()) }))
import { getSettings, getCatCwUnprovenRigModels, cwDecode, setSidebandOverride, setFrequency, setRigFunc, setPtt } from '../api'

const clients: OperationClient[] = []
beforeAll(() => {
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(() => { cleanup(); clients.splice(0).forEach(c => c.disconnected()); vi.useRealTimers(); vi.clearAllMocks() })
async function tick(ms = 0) { await act(async () => { await vi.advanceTimersByTimeAsync(ms) }) }

function fixture(mode: 'cw' | 'phone' = 'phone', capabilities: ControlCapability[] = ['phoneMode'], version: 2 | 3 = 3, local = false) {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'setTimeout', 'clearTimeout', 'performance'] })
  vi.mocked(getSettings).mockResolvedValue(structuredClone(settings) as never)
  vi.mocked(getCatCwUnprovenRigModels).mockResolvedValue([])
  vi.mocked(cwDecode).mockResolvedValue({ text: '', wpm: 22, sent: [], candidates: [], keyerError: null,
    state: 'listening', headline: '', prompt: '', recommended: null, workedCall: null, rst: null, name: null } as never)
  const snap = { activeRadioId: 1, mycall: 'N0CALL', mygrid: 'AA00', mode: 'qso', stations: [], recentDecodes: [],
    conversations: [], highlights: [], link: { tier: 'FT8', dtSec: 0 }, radio: { source: 'native', operatingMode: mode,
      dialMhz: 7.22, band: '40m', sideband: 'LSB', sidebandOverride: null, rigMode: mode === 'cw' ? 'CW' : 'LSB', catOk: true,
      txEnabled: false, transmitting: false, rigKeyed: false, tuning: false, txAllowed: true, filterWidthHz: mode === 'cw' ? 500 : 2400,
      cwWpm: 22, cwKeyer: 'cat', nrLevel: 0.3, agc: 'fast', refusedAgc: null, nb: false, nr: false, notch: false, manualNotch: false, comp: false, vox: false,
      splitTxMhz: null, smeterDb: null } } as unknown as AppSnapshot
  const sent: any[] = [], values = new Map<string, string>()
  const storage = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const client = new OperationClient(wire => sent.push(JSON.parse(wire)), true, () => 1000 + performance.now(), undefined, version,
    pendingControlStorage(() => storage, 'phone-mode', async (_key, run) => run()))
  clients.push(client)
  const state: OperationState = { stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(), revision: 1,
    commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false,
    controls: { context: { radioId: 1, radioConnection: 7, ampConnection: null, ampReadSequence: null }, capabilities } }
  const reply = (value: unknown) => client.receive({ type: 'operationResponse', requestId: sent[sent.length - 1].request.requestId, value })
  client.open(); reply(state)
  const onSnap = vi.fn(), Component = PhoneCockpit
  const frame = structuredClone(frames.spe) as MonitorFrame
  frame.station.radio.id = 1; frame.station.radio.readings.cat!.connectionGeneration = 7; frame.station.amplifier = null
  const observation = { status: 'current', frame } as MonitorState
  const view = (current = snap, available = true, shown = observation, phoneMode = 'ssb') => <StationControlContext.Provider value={local}><StationDataContext.Provider value={available}>
    <RemoteOperationsContext.Provider value={local ? null : client}>
      <RemoteObservationContext.Provider value={shown}>
      <Component snap={current} phoneMode={phoneMode} theme="dark" spots={[]} onWorkSpot={() => {}} onSnap={onSnap}/>
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
    buttons: () => [...ui.container.querySelectorAll<HTMLButtonElement>('.ph-mode-btn')],
    button: (mode: string | null) => [...ui.container.querySelectorAll<HTMLButtonElement>('.ph-mode-btn')].find(b => mode === null ? b.textContent?.startsWith('AUTO') : b.textContent === mode)! }

}

it.each(['USB', 'LSB', 'AM', null] as const)('the actual Phone %s button waits for the observed override', async mode => {
  const h = fixture(); await tick()
  expect(h.button(null).getAttribute('aria-pressed')).toBe('true')
  fireEvent.click(h.button(mode)); await tick()
  expect(h.writes()).toHaveLength(1)
  expect(h.writes()[0].request.action).toEqual({ action: 'radio.phoneMode', expectedMode: 'auto', mode: mode ?? 'auto' })
  expect(h.writes()[0].request.context).toEqual(h.state.controls!.context)
  expect(h.button(null).getAttribute('aria-pressed')).toBe('true'); expect(h.onSnap).not.toHaveBeenCalled()
  fireEvent.click(h.button(mode)); await tick(); expect(h.writes()).toHaveLength(1)
  act(() => h.finish()); await tick()
  expect(h.button(null).getAttribute('aria-pressed')).toBe('true')
  h.rerender(h.view({ ...h.snap, radio: { ...h.snap.radio, sidebandOverride: mode, rigMode: mode ?? 'LSB' } })); await tick()
  expect(h.button(mode).getAttribute('aria-pressed')).toBe('true')
  for (const fn of [setSidebandOverride, setFrequency, setRigFunc, setPtt]) expect(fn).not.toHaveBeenCalled()
})

it.each(['radio', 'connection'] as const)('the Phone picker waits for its displayed %s after a handoff', async changed => {
  const h = fixture(), context = { ...h.state.controls!.context, ...(changed === 'radio' ? { radioId: 2 } : { radioConnection: 8 }) }
  await tick(1000)
  const snap = { ...h.snap, activeRadioId: context.radioId }
  act(() => h.reply({ ...h.state, revision: 2, controls: { ...h.state.controls!, context } })); await tick()
  h.rerender(h.view(snap)); await tick(); fireEvent.click(h.button('USB')); await tick()
  expect(h.writes()).toHaveLength(0)
  const fresh = structuredClone(h.observation)
  fresh.frame!.station.radio.id = context.radioId; fresh.frame!.station.radio.readings.cat!.connectionGeneration = context.radioConnection!
  h.rerender(h.view(snap, true, fresh)); await tick(); fireEvent.click(h.button('USB')); await tick()
  expect(h.writes()).toHaveLength(1); expect(h.writes()[0].request.context).toEqual(context)
  act(() => h.finish()); await tick()
})

it.each([[[],3],[['phoneMode'],2]] as const)('keeps older and ungranted Phone pickers inert', async (capabilities,version) => {
  const h = fixture('phone',[...capabilities],version); await tick()
  expect(h.buttons().every(b => b.disabled)).toBe(true)
  for (const b of h.buttons()) fireEvent.click(b)
  await tick(); expect(h.writes()).toHaveLength(0)
})

it.each(['keyed','ptt unknown','armed','busy','mode','radio','stale','disconnected','cat mode unknown','fm'] as const)('refuses %s at the existing Phone picker', async change => {
  const h = fixture(), next = structuredClone(h.snap)
  if (change === 'keyed') next.radio.rigKeyed = true
  if (change === 'ptt unknown') next.radio.rigKeyed = undefined
  if (change === 'armed') next.radio.txEnabled = true
  if (change === 'busy') next.radio.txBusyReason = 'manualPtt'
  if (change === 'mode') next.radio.operatingMode = 'cw'
  if (change === 'radio') next.activeRadioId = 2
  if (change === 'cat mode unknown') next.radio.rigMode = undefined
  if (change === 'fm') next.radio.rigMode = 'FM'
  if (change === 'disconnected') act(() => h.client.disconnected())
  h.rerender(h.view(next, change !== 'stale')); await tick()
  expect(h.buttons().every(b => b.disabled)).toBe(true)
  for (const b of h.buttons()) fireEvent.click(b)
  await tick(); expect(h.writes()).toHaveLength(0)
})

it('keeps FM unavailable and preserves the native AM visibility rule', async () => {
  const h = fixture(); await tick()
  expect(h.button('FM').disabled).toBe(true); expect(h.button('AM').disabled).toBe(false)
  h.rerender(h.view({ ...h.snap, radio: { ...h.snap.radio, dialMhz: 14.275, band: '20m' } })); await tick()
  expect(h.button('AM')).toBeUndefined(); expect(h.button('USB').disabled).toBe(false)
  h.rerender(h.view({ ...h.snap, radio: { ...h.snap.radio, dialMhz: 146.52, band: '2m', sidebandOverride: 'USB' } }, true, h.observation, 'fm')); await tick()
  expect(h.button(null).disabled).toBe(true); expect(h.button('LSB').disabled).toBe(false)
  fireEvent.click(h.button(null)); await tick(); expect(h.writes()).toHaveLength(0)
})

it('does not replay an uncertain mode selection or display it as applied', async () => {
  const h = fixture(); await tick(); fireEvent.click(h.button('USB')); await tick()
  act(() => h.finish('unknown')); await tick(1000)
  act(() => h.reply({ ...h.state, revision: 2, commandWindowId: crypto.randomUUID(), nextSequence: 2 })); await tick()
  expect(h.buttons().every(b => b.disabled)).toBe(true)
  fireEvent.click(h.button('USB')); await tick(); expect(h.writes()).toHaveLength(1)
  expect(h.button(null).getAttribute('aria-pressed')).toBe('true'); expect(h.onSnap).not.toHaveBeenCalled()
})

it('preserves native mode API calls and returned snapshots', async () => {
  const h = fixture('phone', [], 3, true); await tick()
  const later = { ...h.snap, radio: { ...h.snap.radio, sidebandOverride: 'USB' } }
  vi.mocked(setSidebandOverride).mockResolvedValue(later)
  fireEvent.click(h.button('USB')); await tick()
  expect(setSidebandOverride).toHaveBeenCalledExactlyOnceWith('USB')
  expect(h.onSnap).toHaveBeenCalledWith(later); expect(h.writes()).toHaveLength(0)
})


it.each(['FM', 'auto', 'leave'] as const)('uses the existing Phone picker for FM %s only with station support', async pick => {
  const h = fixture('phone', ['phoneMode', 'fmTuning']);
  const snap = { ...h.snap, radio: { ...h.snap.radio, dialMhz: 145.5, band: '2m', rigMode: pick === 'leave' ? 'FM' : 'USB', sidebandOverride: pick === 'leave' ? 'FM' : null } } as AppSnapshot
  h.rerender(h.view(snap, true, h.observation, 'fm')); await tick()
  const mode = pick === 'auto' ? null : pick === 'leave' ? 'USB' : 'FM'
  expect(h.button(mode).disabled).toBe(false)
  fireEvent.click(h.button(mode)); await tick()
  expect(h.writes()).toHaveLength(1)
  expect(h.writes()[0].request.action).toEqual({ action: 'radio.phoneMode', expectedMode: pick === 'leave' ? 'FM' : 'auto', mode: mode ?? 'auto' })
  expect(h.button(pick === 'leave' ? 'FM' : null).getAttribute('aria-pressed')).toBe('true')
  act(() => h.finish()); await tick()
  expect(h.onSnap).not.toHaveBeenCalled()
  expect(setSidebandOverride).not.toHaveBeenCalled()
})
