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
import { TuningStrip } from '../components/TuningStrip'
import { useWheelTune } from '../useWheelTune'
import type { AppSnapshot } from '../types'
import type { ApplicationClient } from './application-client'
import type { OperationState } from './operation-protocol'
import { LoggingAuthority } from './operations'
import { RemoteObservationContext } from './amplifier-observation'
import frames from '../remote-monitor/fixtures.v2.json'
import type { MonitorState } from '../remote-monitor/session'
import type { MonitorFrame } from '../remote-monitor/protocol'
import { useRemoteScopeClick } from './useRemoteScopeClick'

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
  const app = { invoke: read, age: () => age, held: () => snapshot } as unknown as ApplicationClient
  const tuning = new WheelTuning(client, app, failed); tuning.activate()
  closes.push(() => { tuning.dispose(); client.disconnected() })
  const source = () => ({ dialMhz: snapshot.radio.dialMhz, sideband: snapshot.radio.sideband, context: state.controls!.context })
  const frame = structuredClone(frames.spe) as MonitorFrame
  frame.station.radio.id = 1; frame.station.radio.readings.cat!.connectionGeneration = 7; frame.station.amplifier = null
  const observation = { status: 'current', frame } as MonitorState
  const finish = (outcome: 'applied' | 'unknown' = 'applied') => {
    const writes = sent.filter(w => w.request.type === 'stationControl')
    const request = writes[writes.length - 1].request
    reply({ operation: 'stationControl', operationId: request.requestId, outcome,
      ...(outcome === 'applied' ? { evidence: 'radioReadback' } : { reason: 'hardwareUnconfirmed' }) })
  }
  const fresh = async (dial = snapshot.radio.dialMhz, waitMs = 1000) => {
    snapshot = { ...snapshot, radio: { ...snapshot.radio, dialMhz: dial } }
    await tick(waitMs)
    act(() => reply({ ...state, revision: ++revision, commandWindowId: crypto.randomUUID(), nextSequence: ++sequence }))
    await tick()
  }
  return { tuning, client, state, reply, sent, read, failed, source, finish, fresh, observation,
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
  const current = { ...displayed, context }
  expect(h.tuning.nudge(100, current)).toBe(true)
  await tick(120)
  expect(h.writes()).toHaveLength(1); expect(h.writes()[0].request.context).toEqual(context)
  act(() => h.finish()); await tick()
})

it.each(['radio', 'connection'] as const)('rejects both an old scope press and a new press on a stale displayed %s', async changed => {
  const h = fixture(), displayed = h.source(), old = h.tuning.captureTarget(displayed)!
  const context = { ...h.state.controls!.context, ...(changed === 'radio' ? { radioId: 2 } : { radioConnection: 8 }) }
  await tick(1000)
  h.setSnapshot({ ...h.getSnapshot(), activeRadioId: context.radioId })
  act(() => h.reply({ ...h.state, revision: 2, controls: { ...h.state.controls!, context } })); await tick()
  expect(old(7_205_000)).toBe(false)
  expect(h.tuning.captureTarget(displayed)).toBeNull()
  expect(h.writes()).toHaveLength(0)
  const fresh = h.tuning.captureTarget({ ...displayed, context })!
  expect(fresh(7_205_000)).toBe(true); await tick()
  expect(h.writes()).toHaveLength(1); expect(h.writes()[0].request.context).toEqual(context)
  act(() => h.finish()); await tick()
})

it('coalesces readout and scope input once, and a step made behind a submitted command joins the next burst', async () => {
  const h = fixture(), readout = {}, scope = {}
  expect(h.tuning.nudge(100, { ...h.source(), owner: readout })).toBe(true)
  expect(h.tuning.nudge(1000, { ...h.source(), owner: scope })).toBe(true)
  await tick(119); expect(h.writes()).toHaveLength(0)
  await tick(1); expect(h.writes()).toHaveLength(1)
  expect(h.writes()[0].request.action).toEqual({ action: 'radio.frequency', dialMhz: 7.2011, band: '40m', sideband: 'LSB' })
  // ONE IN FLIGHT, ONE QUEUED. This step used to be dropped on the floor while a command was out —
  // the operator's correction, refused with nothing said. It is kept now, built on the dial the
  // command in flight is asking for, and nothing extra goes on the wire for it.
  expect(h.tuning.nudge(1000, h.source())).toBe(true)
  expect(h.writes()).toHaveLength(1)
  act(() => h.finish()); await tick(); await h.fresh(7.2011)
  // Sent the moment the first command confirmed and control was current again: 7.2011 + 1 kHz.
  expect(h.writes()).toHaveLength(2)
  expect(h.writes()[1].request.action).toEqual({ action: 'radio.frequency', dialMhz: 7.2021, band: '40m', sideband: 'LSB' })
  act(() => h.finish()); await tick()
  expect(h.failed).not.toHaveBeenCalled(); expect(setFrequency).not.toHaveBeenCalled()
})

it('sends a queued burst from the dial the station read back, while its own sample still reads the old one', async () => {
  const h = fixture()
  h.tuning.nudge(100, h.source()); await tick(120)
  expect(h.writes()[0].request.action.dialMhz).toBe(7.2001)
  expect(h.tuning.nudge(100, h.source())).toBe(true)
  // The station's reading was taken BEFORE it told us the radio had landed, so it is older news
  // than that readback and not evidence the dial moved. The sample still says 7.2.
  h.setAge(400)
  act(() => h.finish()); await h.fresh(7.2, 300)
  expect(h.writes()).toHaveLength(2)
  expect(h.writes()[1].request.action.dialMhz).toBe(7.2002)
  expect(h.failed).not.toHaveBeenCalled()
  act(() => h.finish()); await tick()
})

it('control: a reading taken AFTER that readback, disagreeing, still refuses the queued burst', async () => {
  // The same run with one thing changed: the sample is current, so its 7.2 is the station saying
  // the dial is elsewhere. That is the case the pre-dispatch re-read exists for, and it still bites.
  const h = fixture()
  h.tuning.nudge(100, h.source()); await tick(120)
  expect(h.tuning.nudge(100, h.source())).toBe(true)
  h.setAge(0)
  act(() => h.finish()); await h.fresh(7.2, 300)
  expect(h.writes()).toHaveLength(1)
  expect(h.failed).toHaveBeenCalledOnce()
  expect(h.failed.mock.calls[0][0].message).toBe('staleContext')
})

// THE PAGE DRAWS A SAMPLE A POLL AFTER THE STREAM HOLDS IT. After a readback, a sample taken later
// can already be in the stream while the cockpit still draws the one from before the command.
// `drawnBehind` renders the real controls on that older sample (`drawn`) and never re-renders them
// with the newer one, which is exactly the poll in which an operator's next notch or press lands.
function drawnBehind(h: ReturnType<typeof fixture>, controls: (snap: AppSnapshot) => React.ReactNode) {
  const drawn = h.getSnapshot()
  return render(<StationControlContext.Provider value={false}><StationDataContext.Provider value={true}>
    <RemoteOperationsContext.Provider value={h.client}><RemoteWheelTuningContext.Provider value={h.tuning}><RemoteObservationContext.Provider value={h.observation}>
      {controls(drawn)}
    </RemoteObservationContext.Provider></RemoteWheelTuningContext.Provider></RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>)
}

it('a keyboard notch made while the page still draws the sample from before a readback builds on the readback', async () => {
  const h = fixture()
  const ui = drawnBehind(h, snap => <CockpitHeader snap={snap} modeIndicator="Phone" bandControl={null} onCommitDial={vi.fn()} wheelTune digitTune remoteFrequency/>)
  await tick()
  fireEvent.wheel(ui.container.querySelector('[data-decade="3"]')!, { deltaY: -100, deltaMode: 0 }); await tick(120)
  expect(h.writes()[0].request.action.dialMhz).toBe(7.201)
  // The station read the radio back at 7.201 and its next sample says so; the page still draws 7.2.
  act(() => h.finish()); await h.fresh(7.201)
  fireEvent.keyDown(ui.container.querySelector('.readout[role="button"]')!, { key: 'ArrowUp' }); await tick(120)
  expect(h.failed).not.toHaveBeenCalled()
  expect(h.writes()).toHaveLength(2); expect(h.writes()[1].request.action.dialMhz).toBe(7.2011)
  act(() => h.finish()); await tick()
})

it('a nudge pressed while the page still draws the sample from before a readback steps from the readback', async () => {
  const h = fixture()
  const ui = drawnBehind(h, snap => <TuningStrip snap={snap} step={100} showReadout={false}/>)
  await tick()
  expect(h.tuning.nudge(1000, h.source())).toBe(true); await tick(120)
  act(() => h.finish()); await h.fresh(7.201)
  fireEvent.click(ui.getByRole('button', { name: 'Tune up 100 Hz' })); await tick(120)
  expect(setFrequency, 'a browser nudge goes out through the one tuning pipeline').not.toHaveBeenCalled()
  expect(h.writes()).toHaveLength(2)
  expect(h.writes()[1].request.action.dialMhz).toBe(7.2011)
  act(() => h.finish()); await tick()
  expect(h.failed).not.toHaveBeenCalled()
})

// ONE WRITER ON ONE DIAL. The strip's arrows used to command an absolute dial of their own, built
// from the sample the strip draws. While the wheel had a command out, that dial was a step old: the
// press either walked back what the wheel had just asked for or was refused as a second command,
// and the digits — already showing where the wheel was going — said neither. The arrows now step
// the same burst, so they queue behind the command in flight exactly as a wheel notch does.
// (Through the UI this is reachable only once a pending command stops disabling the strip —
// `useStationHeld` still gates on `controlPending`, which is batch 1's blanking work — so the press
// is made on the controller here, where the two writers actually met.)
it('a nudge made while a wheel command is in flight joins it, and steps from the dial that command asked for', async () => {
  const h = fixture()
  expect(h.tuning.nudge(1000, h.source())).toBe(true); await tick(120)
  expect(h.writes()).toHaveLength(1); expect(h.writes()[0].request.action.dialMhz).toBe(7.201)
  expect(h.tuning.nudgeSteps(1, 100, h.source())).toBe(true)
  expect(setFrequency, 'nothing of its own goes out while a command is in flight').not.toHaveBeenCalled()
  expect(h.writes()).toHaveLength(1)
  expect(h.tuning.getProvisionalHz(), 'the digits already show where the press is going').toBe(7_201_100)
  act(() => h.finish()); await tick(); await h.fresh(7.201)
  expect(h.writes()).toHaveLength(2)
  expect(h.writes()[1].request.action.dialMhz).toBe(7.2011)
  act(() => h.finish()); await tick()
  expect(h.failed).not.toHaveBeenCalled()
})

// #273's rounding is the arrows' own rule and it survives the shared burst: a dial off the grid
// lands ON the grid on the first press, from wherever the burst is going.
it('a nudge rounds to the step grid first, from the burst\'s own dial', async () => {
  const h = fixture(7.20025)
  expect(h.tuning.nudgeSteps(1, 100, h.source())).toBe(true); await tick(120)
  expect(h.writes()).toHaveLength(1); expect(h.writes()[0].request.action.dialMhz).toBe(7.2003)
  act(() => h.finish()); await tick()
})

it('control: once the station moved the dial after that readback, a nudge steps from the dial the page draws', async () => {
  // The readback is only newer news while the station's newest sample still agrees with it. Here the
  // radio moved on (the knob at the station) and the page draws that move: the press steps from it,
  // never from the readback it replaced.
  const h = fixture()
  expect(h.tuning.nudge(1000, h.source())).toBe(true); await tick(120)
  act(() => h.finish()); await h.fresh(7.25)
  const ui = drawnBehind(h, snap => <TuningStrip snap={snap} step={100} showReadout={false}/>)
  await tick()
  fireEvent.click(ui.getByRole('button', { name: 'Tune up 100 Hz' })); await tick(120)
  expect(h.writes()).toHaveLength(2)
  expect(h.writes()[1].request.action.dialMhz).toBe(7.2501)
  act(() => h.finish()); await tick()
})

it('refuses a queued burst whole when the LEASE it was made under is replaced, and says so', async () => {
  // The command window (the revision) is deliberately not part of a burst's authority any more —
  // a queued burst exists to be sent on the window AFTER the command it waits behind spent its
  // own, so folding the revision in would discard every one of them. Everything else about
  // authority is untouched, and this is the half that does the work: a new lease is a different
  // browser's grant, and input made under the old one is refused rather than replayed onto it.
  const h = fixture()
  h.tuning.nudge(100, h.source()); await tick(120)
  expect(h.tuning.nudge(100, h.source())).toBe(true)
  act(() => h.finish()); await tick(1000)
  act(() => h.reply({ ...h.state, leaseId: crypto.randomUUID(), revision: 2, commandWindowId: crypto.randomUUID(), nextSequence: 2 }))
  await tick(1600)
  expect(h.writes()).toHaveLength(1)
  expect(h.failed.mock.calls.map(c => c[0].message)).toEqual(['notController'])
})

it('refuses a queued burst whole, and says so, when the command it waited behind never confirmed', async () => {
  const h = fixture()
  h.tuning.nudge(100, h.source()); await tick(120)
  expect(h.tuning.nudge(100, h.source())).toBe(true)
  act(() => h.finish('unknown')); await tick()
  // Two failures: the command's own unknown outcome, and the queued burst that has no dial left to
  // build on — said as "Not sent", never silently dropped.
  expect(h.writes()).toHaveLength(1)
  expect(h.failed.mock.calls.map(c => [c[0].message, c[0].sent])).toEqual([['operationUnconfirmed', true], ['notController', false]])
  expect(h.tuning.getProvisionalHz()).toBeNull()
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
  useWheelTune(ref, { ...snap.radio, radioId: snap.activeRadioId, enabled: true, stepHz: 100, remoteFrequency: true })
  return <div ref={ref} data-testid="scope"/>
}
function ScopeClick({ snap }: { snap: AppSnapshot }) {
  const scope = useRemoteScopeClick(snap), held = useRef<((hz: number) => boolean) | null>(null)
  return <button disabled={!scope.allowed} onMouseDown={() => { held.current = scope.begin() }}
    onMouseUp={() => { held.current?.(7_201_000); held.current = null }}>Signal</button>
}
it('the scope hook uses the displayed observation when a connection is replaced at the same dial', async () => {
  const h = fixture()
  const page = (observation = h.observation) => <StationControlContext.Provider value={false}><StationDataContext.Provider value={true}>
    <RemoteOperationsContext.Provider value={h.client}><RemoteWheelTuningContext.Provider value={h.tuning}>
      <RemoteObservationContext.Provider value={observation}><ScopeClick snap={h.getSnapshot()}/></RemoteObservationContext.Provider>
    </RemoteWheelTuningContext.Provider></RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>
  const ui = render(page()); await tick(1000)
  act(() => h.reply({ ...h.state, revision: 2, controls: { ...h.state.controls!, context: { ...h.state.controls!.context, radioConnection: 8 } } })); await tick()
  fireEvent.mouseDown(ui.getByRole('button')); fireEvent.mouseUp(ui.getByRole('button')); await tick()
  expect(h.writes()).toHaveLength(0)
  const fresh = structuredClone(h.observation); fresh.frame!.station.radio.readings.cat!.connectionGeneration = 8
  ui.rerender(page(fresh)); await tick()
  fireEvent.mouseDown(ui.getByRole('button')); fireEvent.mouseUp(ui.getByRole('button')); await tick()
  expect(h.writes()).toHaveLength(1); expect(h.writes()[0].request.context.radioConnection).toBe(8)
  act(() => h.finish()); await tick()
})

it('the mounted native readout waits for its displayed radio and observation to catch up after handoff', async () => {
  const h = fixture(), displayed = h.getSnapshot()
  const page = (snap = displayed, observation = h.observation) => <StationControlContext.Provider value={false}><StationDataContext.Provider value={true}>
    <RemoteOperationsContext.Provider value={h.client}><RemoteWheelTuningContext.Provider value={h.tuning}>
      <RemoteObservationContext.Provider value={observation}>
        <CockpitHeader snap={snap} modeIndicator="Phone" bandControl={null} onCommitDial={vi.fn()} wheelTune digitTune remoteFrequency/>
      </RemoteObservationContext.Provider>
    </RemoteWheelTuningContext.Provider></RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>
  const ui = render(page()); await tick(1000)
  h.setSnapshot({ ...displayed, activeRadioId: 2 })
  act(() => h.reply({ ...h.state, revision: 2, controls: { ...h.state.controls, context: { ...h.state.controls!.context, radioId: 2 } } })); await tick()
  const digit = () => ui.container.querySelector('[data-decade="3"]')!
  fireEvent.wheel(digit(), { deltaY: -100, deltaMode: 0 }); await tick(120)
  expect(h.writes()).toHaveLength(0)
  ui.rerender(page(h.getSnapshot())); await tick()
  fireEvent.wheel(digit(), { deltaY: -100, deltaMode: 0 }); await tick(120)
  expect(h.writes()).toHaveLength(0)
  const fresh = structuredClone(h.observation); fresh.frame!.station.radio.id = 2
  ui.rerender(page(h.getSnapshot(), fresh)); await tick()
  fireEvent.wheel(digit(), { deltaY: -100, deltaMode: 0 }); await tick(120)
  expect(h.writes()).toHaveLength(1); expect(h.writes()[0].request.context.radioId).toBe(2)
  act(() => h.finish()); await tick()
})

it('the actual digit readout and scope listener share one target and keep native calls inert', async () => {
  const h = fixture()
  const view = (show = true) => <StationControlContext.Provider value={false}><StationDataContext.Provider value={true}>
    <RemoteOperationsContext.Provider value={h.client}><RemoteWheelTuningContext.Provider value={h.tuning}><RemoteObservationContext.Provider value={h.observation}>
      <LoggingAuthority client={h.client}/>
      {show && <><CockpitHeader snap={h.getSnapshot()} modeIndicator="Phone" bandControl={null} onCommitDial={vi.fn()} wheelTune digitTune remoteFrequency/><Scope snap={h.getSnapshot()}/></>}
    </RemoteObservationContext.Provider></RemoteWheelTuningContext.Provider></RemoteOperationsContext.Provider>
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
    <RemoteOperationsContext.Provider value={h.client}><RemoteWheelTuningContext.Provider value={h.tuning}><RemoteObservationContext.Provider value={h.observation}>
      <Scope snap={h.getSnapshot()}/>
    </RemoteObservationContext.Provider></RemoteWheelTuningContext.Provider></RemoteOperationsContext.Provider>
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
  // The re-read has not answered, but the state is held through a command now and the lease the
  // station reported still runs: the steady label, as in any gap between heartbeats. It goes
  // unavailable once that lease has run out with nothing heard.
  expect(ui.container.textContent).toContain('Station control active')
  await tick(9000)
  expect(ui.container.textContent).toContain('Logging control status unavailable')
  expect(ui.container.textContent).not.toContain('Updating station controls')
  act(() => h.client.disconnected()); await tick()
  expect(ui.container.textContent).toContain('Station control disconnected')
})
