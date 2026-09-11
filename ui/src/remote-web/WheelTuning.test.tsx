// @vitest-environment jsdom
import { useRef } from 'react'
import { afterEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render } from '@testing-library/react'
import { WheelTuning } from './wheel-tuning'
import { RemoteWheelTuningContext } from './wheel-tuning-context'
import { OperationClient } from './operation-client'
import { pendingControlStorage } from './control-storage'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { CockpitHeader } from '../components/CockpitHeader'
import { useWheelTune } from '../useWheelTune'
import type { AppSnapshot } from '../types'
import type { ApplicationClient } from './application-client'
import type { OperationState } from './operation-protocol'
import { LoggingAuthority } from './operations'

vi.mock('../api', async original => ({ ...await original<Record<string, unknown>>(), setFrequency: vi.fn(async () => null) }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))
import { setFrequency } from '../api'

const closes: (() => void)[] = []
afterEach(() => { cleanup(); closes.splice(0).forEach(f => f()); vi.useRealTimers(); vi.clearAllMocks() })
async function tick(ms = 0) { await act(async () => { await vi.advanceTimersByTimeAsync(ms) }) }
function fixture(dialMhz = 7.2) {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'setTimeout', 'clearTimeout', 'performance'] })
  const sent: any[] = [], values = new Map<string, string>()
  const storage = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const client = new OperationClient(wire => sent.push(JSON.parse(wire)), true, () => 1000 + performance.now(), undefined, 3,
    pendingControlStorage(() => storage, 'wheel-station', async (_key, run) => run()))
  const state: OperationState = { stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(), revision: 1,
    commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false,
    controls: { context: { radioId: 1, radioConnection: 7, ampConnection: null, ampReadSequence: null }, capabilities: ['frequency'] } }
  const reply = (value: unknown) => client.receive({ type: 'operationResponse', requestId: sent[sent.length - 1].request.requestId, value })
  client.open(); reply(state)
  let snapshot = { activeRadioId: 1, radio: { dialMhz, band: '40m', sideband: 'LSB', source: 'native', operatingMode: 'phone', catOk: true,
    txEnabled: false, transmitting: false, rigKeyed: false, tuning: false, txAllowed: true }, link: { tier: 'FT8' } } as unknown as AppSnapshot
  let age = 0
  let revision = state.revision, sequence = 1
  const read = vi.fn(async (): Promise<AppSnapshot> => structuredClone(snapshot)), failed = vi.fn()
  const app = { invoke: read, age: () => age } as unknown as ApplicationClient
  const tuning = new WheelTuning(client, app, failed); tuning.activate()
  closes.push(() => { tuning.dispose(); client.disconnected() })
  const source = () => ({ dialMhz: snapshot.radio.dialMhz, sideband: snapshot.radio.sideband })
  const finish = (outcome: 'applied' | 'unknown' = 'applied') => {
    const writes = sent.filter(w => w.request.type === 'stationControl')
    const request = writes[writes.length - 1].request
    reply({ operation: 'stationControl', operationId: request.requestId, outcome,
      ...(outcome === 'applied' ? { evidence: 'radioReadback' } : { reason: 'hardwareUnconfirmed' }) })
  }
  const fresh = async (dial = snapshot.radio.dialMhz) => {
    snapshot = { ...snapshot, radio: { ...snapshot.radio, dialMhz: dial } }
    await tick(1000)
    act(() => reply({ ...state, revision: ++revision, commandWindowId: crypto.randomUUID(), nextSequence: ++sequence }))
    await tick()
  }
  return { tuning, client, state, reply, sent, read, failed, source, finish, fresh,
    getSnapshot: () => snapshot, setSnapshot: (s: AppSnapshot) => { snapshot = s }, setAge: (v: number) => { age = v },
    writes: () => sent.filter(w => w.request.type === 'stationControl') }
}

it('commits one absolute scope target using the original press without a wheel band-edge clamp', async () => {
  const h = fixture(), click = h.tuning.captureTarget(h.source())!
  expect(h.writes()).toHaveLength(0)
  expect(click(10_000_000)).toBe(true); await tick()
  expect(h.writes()).toHaveLength(1)
  expect(h.writes()[0].request.action).toEqual({ action: 'radio.frequency', dialMhz: 10, band: '', sideband: 'LSB' })
  expect(click(7_100_000)).toBe(false)
  act(() => h.finish()); await tick(); await h.fresh(10)
  expect(h.writes()).toHaveLength(1); expect(setFrequency).not.toHaveBeenCalled()
})

it('drops a scope press across control loss even when the same lease returns', async () => {
  const h = fixture(), old = h.tuning.captureTarget(h.source())!
  act(() => { h.client.disconnected(); h.client.open() }); await tick(1000)
  act(() => h.reply(h.state)); await tick()
  expect(old(7_205_000)).toBe(false); expect(h.writes()).toHaveLength(0)
  const fresh = h.tuning.captureTarget(h.source())!
  expect(fresh(7_201_000)).toBe(true); await tick()
  expect(h.writes()[0].request.action.dialMhz).toBe(7.201)
  act(() => h.finish()); await tick()
})

it('does not borrow a renewed command window to finish an old scope press', async () => {
  const h = fixture(), old = h.tuning.captureTarget(h.source())!
  await tick(1000); act(() => h.reply(h.state)); await tick(300)
  old(7_201_000); await tick()
  expect(h.writes()).toHaveLength(0)
  const fresh = h.tuning.captureTarget(h.source())!
  expect(fresh(7_202_000)).toBe(true); await tick()
  expect(h.writes()).toHaveLength(1)
  act(() => h.finish()); await tick()
})

it('keeps wheel input and local dial changes from becoming an old scope release', async () => {
  const h = fixture(), click = h.tuning.captureTarget(h.source())!
  expect(h.tuning.nudge(100, h.source())).toBe(true)
  expect(click(7_205_000)).toBe(false); await tick(120)
  expect(h.writes()).toHaveLength(1); expect(h.writes()[0].request.action.dialMhz).toBe(7.2001)
  act(() => h.finish()); await tick(); await h.fresh(7.2001)
  const changed = h.tuning.captureTarget(h.source())!
  h.setSnapshot({ ...h.getSnapshot(), radio: { ...h.getSnapshot().radio, dialMhz: 7.21 } })
  changed(7_201_000); await tick()
  expect(h.writes()).toHaveLength(1); expect(h.failed).toHaveBeenCalledOnce()
})

it.each(['radio', 'connection'] as const)('does not retarget an older displayed %s when the new station has the same dial', async changed => {
  const h = fixture(), displayed = { ...h.source(), context: structuredClone(h.state.controls!.context) }
  const context = { ...h.state.controls!.context, ...(changed === 'radio' ? { radioId: 2 } : { radioConnection: 8 }) }
  await tick(1000)
  h.setSnapshot({ ...h.getSnapshot(), activeRadioId: context.radioId })
  act(() => h.reply({ ...h.state, revision: 2, controls: { ...h.state.controls, context } })); await tick()
  expect(h.tuning.nudge(100, displayed)).toBe(false)
  await tick(120); expect(h.writes()).toHaveLength(0)
  expect(h.tuning.nudge(100, { ...displayed, context })).toBe(true)
  await tick(120)
  expect(h.writes()).toHaveLength(1); expect(h.writes()[0].request.context).toEqual(context)
  act(() => h.finish()); await tick()
})

it('coalesces readout and scope input once and queues nothing behind a submitted command', async () => {
  const h = fixture(), readout = {}, scope = {}
  expect(h.tuning.nudge(100, { ...h.source(), owner: readout })).toBe(true)
  expect(h.tuning.nudge(1000, { ...h.source(), owner: scope })).toBe(true)
  await tick(119); expect(h.writes()).toHaveLength(0)
  await tick(1); expect(h.writes()).toHaveLength(1)
  expect(h.writes()[0].request.action).toEqual({ action: 'radio.frequency', dialMhz: 7.2011, band: '40m', sideband: 'LSB' })
  expect(h.tuning.nudge(1000, h.source())).toBe(false)
  act(() => h.finish()); await tick(); await h.fresh(7.2011)
  expect(h.writes()).toHaveLength(1)
  expect(h.tuning.nudge(100, h.source())).toBe(true); await tick(120)
  expect(h.writes()).toHaveLength(2)
  expect(h.writes()[1].request.action.dialMhz).toBe(7.2012)
  act(() => h.finish()); await tick()
  expect(h.failed).not.toHaveBeenCalled(); expect(setFrequency).not.toHaveBeenCalled()
})

it('drops a removed control and permits only a new explicit burst', async () => {
  const h = fixture(), owner = {}
  h.tuning.nudge(100, { ...h.source(), owner }); h.tuning.cancel(owner)
  await tick(120); expect(h.writes()).toHaveLength(0)
  h.tuning.nudge(200, { ...h.source(), owner: {} }); await tick(120)
  expect(h.writes()[0].request.action.dialMhz).toBe(7.2002)
  act(() => h.finish()); await tick()
})

it('does not carry a read across loss even when the same lease later returns', async () => {
  const h = fixture(); let resolve!: (s: AppSnapshot) => void
  h.read.mockImplementationOnce(() => new Promise(r => { resolve = r }))
  h.tuning.nudge(100, h.source()); await tick(120)
  expect(h.tuning.getPending()).toBe(true)
  act(() => { h.client.disconnected(); h.client.open() })
  await tick(1000) // Reopen preserves the client's existing poll cadence.
  act(() => h.reply(h.state)); await tick()
  await act(async () => resolve(h.getSnapshot())); await tick()
  expect(h.writes()).toHaveLength(0)
  expect(h.tuning.nudge(200, h.source())).toBe(true); await tick(120)
  expect(h.writes()[0].request.action.dialMhz).toBe(7.2002)
  act(() => h.finish()); await tick()
})

it.each(['dial', 'radio', 'keyed', 'keying unavailable', 'armed', 'busy', 'stale'] as const)('refuses %s readings before dispatch', async change => {
  const h = fixture()
  h.tuning.nudge(100, h.source())
  const s = structuredClone(h.getSnapshot())
  if (change === 'dial') s.radio.dialMhz += 0.01
  if (change === 'radio') s.activeRadioId = 2
  if (change === 'keyed') s.radio.rigKeyed = true
  if (change === 'keying unavailable') s.radio.rigKeyed = undefined
  if (change === 'armed') s.radio.txEnabled = true
  if (change === 'busy') s.radio.txBusyReason = 'manualPtt'
  if (change === 'stale') h.setAge(1200)
  h.setSnapshot(s); await tick(120)
  expect(h.writes()).toHaveLength(0); expect(h.failed).toHaveBeenCalledOnce()
})

it('retains the native band-edge stop across shared input and can leave a confirmed edge', async () => {
  const h = fixture(), edge = vi.fn()
  for (let i = 0; i < 7; i++) h.tuning.nudge(1e6, h.source(), edge)
  await tick(120)
  expect(h.writes()).toHaveLength(1)
  expect(h.writes()[0].request.action.dialMhz).toBe(7.3)
  expect(edge).toHaveBeenCalledExactlyOnceWith(7.3)
  act(() => h.finish()); await tick(); await h.fresh(7.3)
  h.tuning.nudge(1000, h.source(), edge); await tick(120)
  expect(h.writes()[1].request.action).toEqual({ action: 'radio.frequency', dialMhz: 7.301, band: '', sideband: 'LSB' })
  act(() => h.finish()); await tick()
})

it('retains uncertain command recovery without replaying a wheel target', async () => {
  const h = fixture(); h.tuning.nudge(100, h.source()); await tick(120)
  act(() => h.finish('unknown')); await tick(); await h.fresh()
  expect(h.tuning.ready()).toBe(false); expect(h.tuning.nudge(100, h.source())).toBe(false)
  expect(h.writes()).toHaveLength(1); expect(h.failed).toHaveBeenCalledOnce()
})

function Scope({ snap }: { snap: AppSnapshot }) {
  const ref = useRef<HTMLDivElement>(null)
  useWheelTune(ref, { ...snap.radio, enabled: true, stepHz: 100, remoteFrequency: true })
  return <div ref={ref} data-testid="scope"/>
}
it('the actual digit readout and scope listener share one target and keep native calls inert', async () => {
  const h = fixture()
  const view = (show = true) => <StationControlContext.Provider value={false}><StationDataContext.Provider value={true}>
    <RemoteOperationsContext.Provider value={h.client}><RemoteWheelTuningContext.Provider value={h.tuning}>
      <LoggingAuthority client={h.client}/>
      {show && <><CockpitHeader snap={h.getSnapshot()} modeIndicator="Phone" bandControl={null} onCommitDial={vi.fn()} wheelTune digitTune remoteFrequency/><Scope snap={h.getSnapshot()}/></>}
    </RemoteWheelTuningContext.Provider></RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>
  const ui = render(view()); await tick()
  const digit = ui.container.querySelector('[data-decade="3"]')!
  expect(digit).toBeTruthy()
  fireEvent.wheel(digit, { deltaY: -100, deltaMode: 0 })
  fireEvent.wheel(ui.getByTestId('scope'), { deltaY: -100, deltaMode: 0 })
  await tick(120)
  expect(h.writes()).toHaveLength(1); expect(h.writes()[0].request.action.dialMhz).toBe(7.2011)
  act(() => h.finish()); await tick()
  expect(ui.container.textContent).not.toContain('Logging control status unavailable')
  await h.fresh(7.2011)
  ui.rerender(view()); await tick()
  const readout = ui.container.querySelector('.readout[role="button"]')!
  fireEvent.keyDown(readout, { key: 'ArrowUp' }); await tick(120)
  expect(h.writes()).toHaveLength(2); expect(h.writes()[1].request.action.dialMhz).toBe(7.2012)
  act(() => h.finish()); await tick(); await h.fresh(7.2012)
  ui.rerender(view()); await tick()
  fireEvent.wheel(ui.getByTestId('scope'), { deltaY: -100, deltaMode: 0 })
  ui.rerender(view(false)); await tick(120)
  expect(h.writes()).toHaveLength(2); expect(setFrequency).not.toHaveBeenCalled()
})

it('a real scope listener discards sub-notch residue across a loss and same-lease return', async () => {
  const h = fixture()
  const ui = render(<StationControlContext.Provider value={false}><StationDataContext.Provider value={true}>
    <RemoteOperationsContext.Provider value={h.client}><RemoteWheelTuningContext.Provider value={h.tuning}>
      <Scope snap={h.getSnapshot()}/>
    </RemoteWheelTuningContext.Provider></RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>)
  await tick(950)
  fireEvent.wheel(ui.getByTestId('scope'), { deltaY: -60, deltaMode: 0 })
  act(() => h.client.disconnected()); await tick(100)
  act(() => { h.client.open(); h.reply(h.state) }); await tick()
  fireEvent.wheel(ui.getByTestId('scope'), { deltaY: -60, deltaMode: 0 }); await tick(120)
  expect(h.writes()).toHaveLength(0)
  fireEvent.wheel(ui.getByTestId('scope'), { deltaY: -40, deltaMode: 0 }); await tick(120)
  expect(h.writes()).toHaveLength(1); expect(h.writes()[0].request.action.dialMhz).toBe(7.2001)
  act(() => h.finish()); await tick()
})

it('a confirmed command shows refresh progress, but a failed refresh and disconnect stay explicit', async () => {
  const h = fixture(), ui = render(<LoggingAuthority client={h.client}/>)
  h.tuning.nudge(100, h.source()); await tick(120)
  act(() => h.finish()); await tick()
  expect(ui.container.textContent).toContain('Updating station controls')
  await tick(1500)
  expect(ui.container.textContent).toContain('Logging control status unavailable')
  await tick(9000)
  expect(ui.container.textContent).toContain('Logging control status unavailable')
  expect(ui.container.textContent).not.toContain('Updating station controls')
  act(() => h.client.disconnected()); await tick()
  expect(ui.container.textContent).toContain('Station control disconnected')
})
