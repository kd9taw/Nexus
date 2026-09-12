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
// The real cockpit owns the DSP and AGC controls. Only unrelated heavy children are
// substituted here; the compiled browser suite exercises the full application.
vi.mock('../components/PhoneScope', () => ({ PhoneScope: () => <div/> }))
vi.mock('../components/BandStrip', () => ({ BandStrip: () => <div/> }))
vi.mock('../components/VoiceKeyer', () => ({ VoiceKeyer: () => <div/> }))
vi.mock('../components/LogEntry', () => ({ LogEntry: () => <div/> }))
vi.mock('../components/SpotDialog', () => ({ SpotDialog: () => null }))
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn(async (run: () => Promise<unknown>) => run()) }))
import { getSettings, getCatCwUnprovenRigModels, cwDecode, setAgc, setFrequency, setRigFunc, setPtt } from '../api'

const clients: OperationClient[] = []
beforeAll(() => {
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(() => { cleanup(); clients.splice(0).forEach(c => c.disconnected()); vi.useRealTimers(); vi.clearAllMocks() })
async function tick(ms = 0) { await act(async () => { await vi.advanceTimersByTimeAsync(ms) }) }

function fixture(mode: 'cw' | 'phone' = 'cw', capabilities: ControlCapability[] = ['receiverDsp'], version: 2 | 3 = 3, local = false) {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'setTimeout', 'clearTimeout', 'performance'] })
  vi.mocked(getSettings).mockResolvedValue(structuredClone(settings) as never)
  vi.mocked(getCatCwUnprovenRigModels).mockResolvedValue([])
  vi.mocked(cwDecode).mockResolvedValue({ text: '', wpm: 22, sent: [], candidates: [], keyerError: null,
    state: 'listening', headline: '', prompt: '', recommended: null, workedCall: null, rst: null, name: null } as never)
  const snap = { activeRadioId: 1, mycall: 'N0CALL', mygrid: 'AA00', mode: 'qso', stations: [], recentDecodes: [],
    conversations: [], highlights: [], link: { tier: 'FT8', dtSec: 0 }, radio: { source: 'native', operatingMode: mode,
      dialMhz: 14.275, band: '20m', sideband: 'USB', rigMode: mode === 'cw' ? 'CW' : 'LSB', catOk: true,
      txEnabled: false, transmitting: false, rigKeyed: false, tuning: false, txAllowed: true, filterWidthHz: mode === 'cw' ? 500 : 2400,
      cwWpm: 22, cwKeyer: 'cat', nrLevel: 0.3, agc: 'fast', refusedAgc: null, nb: false, nr: false, notch: false, manualNotch: false, comp: false, vox: false,
      splitTxMhz: null, smeterDb: null } } as unknown as AppSnapshot
  const sent: any[] = [], values = new Map<string, string>()
  const storage = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const client = new OperationClient(wire => sent.push(JSON.parse(wire)), true, () => 1000 + performance.now(), undefined, version,
    pendingControlStorage(() => storage, 'receiver-dsp', async (_key, run) => run()))
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
    functions: () => [...ui.container.querySelectorAll<HTMLButtonElement>('.ph-dsp-btn')],
    agc: () => [...ui.container.querySelectorAll<HTMLButtonElement>('.ph-agc button')],
    func: (label: string) => [...ui.container.querySelectorAll<HTMLButtonElement>('.ph-dsp-btn')].find(b => b.textContent === label)! }
}

it.each([['cw','nb','NB'],['cw','nr','NR'],['cw','notch','Notch'],['phone','nb','NB'],['phone','nr','NR'],['phone','notch','Notch'],['phone','manualNotch','MN']] as const)
('the actual %s %s toggle waits for radio readback and a new station sample', async (mode, func, label) => {
  const h = fixture(mode); await tick()
  const button = () => h.func(label)
  expect(button().disabled).toBe(false); expect(button().getAttribute('aria-pressed')).toBe('false')
  fireEvent.click(button()); await tick()
  expect(h.writes()).toHaveLength(1)
  expect(h.writes()[0].request.action).toEqual({ action: 'radio.function', mode, func, expectedOn: false, on: true })
  expect(h.writes()[0].request.context).toEqual(h.state.controls!.context)
  fireEvent.click(button()); await tick(); expect(h.writes()).toHaveLength(1)
  act(() => h.finish()); await tick()
  expect(button().getAttribute('aria-pressed')).toBe('false'); expect(h.onSnap).not.toHaveBeenCalled()
  h.rerender(h.view({ ...h.snap, radio: { ...h.snap.radio, [func]: true } })); await tick(1000)
  act(() => h.reply({ ...h.state, revision: 2, commandWindowId: crypto.randomUUID(), nextSequence: 2 })); await tick()
  expect(button().getAttribute('aria-pressed')).toBe('true')
  fireEvent.click(button()); await tick()
  expect(h.writes()).toHaveLength(2)
  expect(h.writes()[1].request.action).toEqual({ action: 'radio.function', mode, func, expectedOn: true, on: false })
  act(() => h.finish()); await tick()
  for (const fn of [setAgc, setRigFunc, setFrequency, setPtt]) expect(fn).not.toHaveBeenCalled()
})

it.each((['cw','phone'] as const).flatMap(mode => (['auto','fast','mid','slow','off'] as const).map((speed,index) => [mode,speed,index] as const)))
('the actual %s AGC chip sends one %s pick without optimistic selection', async (mode,speed,index) => {
  const h = fixture(mode); await tick()
  expect(h.agc()).toHaveLength(5)
  expect(h.agc()[1].getAttribute('aria-pressed')).toBe('true')
  fireEvent.click(h.agc()[index]); await tick()
  expect(h.writes()).toHaveLength(1)
  expect(h.writes()[0].request.action).toEqual({ action: 'radio.agc', mode, expectedSpeed: 'fast', speed })
  expect(h.agc()[1].getAttribute('aria-pressed')).toBe('true'); expect(h.onSnap).not.toHaveBeenCalled()
  act(() => h.finish()); await tick()
  expect(h.agc()[1].getAttribute('aria-pressed')).toBe('true')
  h.rerender(h.view({ ...h.snap, radio: { ...h.snap.radio, agc: speed } })); await tick()
  expect(h.agc()[index].getAttribute('aria-pressed')).toBe('true')
  expect(setAgc).not.toHaveBeenCalled(); expect(setPtt).not.toHaveBeenCalled()
})

it.each(['radio','connection'] as const)('a same-value %s handoff waits for the displayed station binding', async changed => {
  const h = fixture(), context = { ...h.state.controls!.context, ...(changed === 'radio' ? { radioId: 2 } : { radioConnection: 8 }) }
  await tick(1000)
  const snap = { ...h.snap, activeRadioId: context.radioId }
  act(() => h.reply({ ...h.state, revision: 2, controls: { ...h.state.controls!, context } })); await tick()
  h.rerender(h.view(snap)); await tick()
  fireEvent.click(h.func('NB')); fireEvent.click(h.agc()[3]); await tick()
  expect(h.writes()).toHaveLength(0)
  const fresh = structuredClone(h.observation)
  fresh.frame!.station.radio.id = context.radioId
  fresh.frame!.station.radio.readings.cat!.connectionGeneration = context.radioConnection!
  h.rerender(h.view(snap, true, fresh)); await tick()
  fireEvent.click(h.agc()[3]); await tick()
  expect(h.writes()).toHaveLength(1); expect(h.writes()[0].request.context).toEqual(context)
  act(() => h.finish()); await tick()
})

it.each([[[],3],[['receiverDsp'],2]] as const)('keeps older and ungranted clients inert', async (capabilities,version) => {
  const h = fixture('phone',[...capabilities],version); await tick()
  expect([...h.functions(),...h.agc()].every(b => b.disabled)).toBe(true)
  for (const b of [...h.functions(),...h.agc()]) fireEvent.click(b)
  await tick(); expect(h.writes()).toHaveLength(0)
})

it.each(['unavailable','keyed','ptt unknown','armed','busy','mode','radio','stale','disconnected'] as const)('refuses %s at actual receiver controls', async change => {
  const h = fixture(), next = structuredClone(h.snap)
  if (change === 'unavailable') { next.radio.nb = null; next.radio.nr = null; next.radio.notch = null; next.radio.agc = null }
  if (change === 'keyed') next.radio.rigKeyed = true
  if (change === 'ptt unknown') next.radio.rigKeyed = undefined
  if (change === 'armed') next.radio.txEnabled = true
  if (change === 'busy') next.radio.txBusyReason = 'manualPtt'
  if (change === 'mode') next.radio.operatingMode = 'phone'
  if (change === 'radio') next.activeRadioId = 2
  if (change === 'disconnected') act(() => h.client.disconnected())
  h.rerender(h.view(next,change !== 'stale')); await tick()
  expect([...h.functions(),...h.agc()].every(b => b.disabled)).toBe(true)
  for (const b of [...h.functions(),...h.agc()]) fireEvent.click(b)
  await tick(); expect(h.writes()).toHaveLength(0)
})

it('leaves Phone COMP and VOX inert while receiver controls are available', async () => {
  const h = fixture('phone'); await tick()
  expect(h.func('NB').disabled).toBe(false)
  for (const label of ['COMP','VOX']) { expect(h.func(label).disabled).toBe(true); fireEvent.click(h.func(label)) }
  await tick(); expect(h.writes()).toHaveLength(0)
})

it('keeps uncertainty recoverable without replaying or displaying the requested AGC speed', async () => {
  const h = fixture(); await tick(); fireEvent.click(h.agc()[3]); await tick()
  act(() => h.finish('unknown')); await tick(1000)
  act(() => h.reply({ ...h.state, revision: 2, commandWindowId: crypto.randomUUID(), nextSequence: 2 })); await tick()
  expect(h.agc().every(b => b.disabled)).toBe(true)
  fireEvent.click(h.agc()[3]); await tick()
  expect(h.writes()).toHaveLength(1); expect(h.agc()[1].getAttribute('aria-pressed')).toBe('true')
  expect(h.onSnap).not.toHaveBeenCalled()
})

it.each(['cw','phone'] as const)('preserves native %s APIs, AGC repicks and returned snapshots', async mode => {
  const h = fixture(mode,[],3,true); await tick()
  const later = { ...h.snap,radio: { ...h.snap.radio, nb: true, agc: 'slow' } }
  vi.mocked(setRigFunc).mockResolvedValue(later); vi.mocked(setAgc).mockResolvedValue(later)
  fireEvent.click(h.func('NB')); await tick()
  fireEvent.click(h.agc()[1]); await tick()
  expect(setRigFunc).toHaveBeenCalledExactlyOnceWith('nb',true)
  expect(setAgc).toHaveBeenCalledExactlyOnceWith('fast')
  expect(h.onSnap).toHaveBeenCalledWith(later); expect(h.writes()).toHaveLength(0)
})


it.each(['FM', 'PKTFM'])('FM receiver controls require the newer station capability in %s', async rigMode => {
  const h = fixture('phone', ['receiverDsp'])
  h.rerender(h.view({ ...h.snap, radio: { ...h.snap.radio, rigMode } })); await tick()
  const control = h.func('NB')
  expect(control.disabled).toBe(true)
  fireEvent.click(control); await tick()
  expect(h.writes()).toHaveLength(0)
})


it.each(['FM', 'PKTFM'])('the existing Phone control sends a confirmed FM receiver command in %s', async rigMode => {
  const h = fixture('phone', ['receiverDsp', 'fmReceiver'])
  h.rerender(h.view({ ...h.snap, radio: { ...h.snap.radio, rigMode } })); await tick()
  const control = h.func('NB')
  expect(control.disabled).toBe(false)
  fireEvent.click(control); await tick()
  expect(h.writes()).toHaveLength(1)
  expect(h.writes()[0].request.action.action).toBe('radio.function')
  act(() => h.finish()); await tick()
  expect(h.onSnap).not.toHaveBeenCalled()
})
