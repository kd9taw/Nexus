// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { CockpitHeader } from '../components/CockpitHeader'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { RemoteObservationContext } from './amplifier-observation'
import { OperationClient } from './operation-client'
import { pendingControlStorage } from './control-storage'
import frames from '../remote-monitor/fixtures.v2.json'
import type { MonitorFrame } from '../remote-monitor/protocol'
import type { OperationState } from './operation-protocol'
import type { AppSnapshot } from '../types'
import type { OperatingWorkspace } from './ModeEntry'

vi.mock('../api', () => ({ setFrequency: vi.fn(), setOperatingMode: vi.fn() }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))
import { setOperatingMode } from '../api'

const clients: OperationClient[] = []
afterEach(() => { cleanup(); clients.splice(0).forEach(c => c.disconnected()); vi.useRealTimers(); vi.clearAllMocks() })
async function tick() { await act(async () => { await vi.advanceTimersByTimeAsync(0) }) }
const snapshot = (tier = 'FT8', mode = 'qso', operatingMode = 'digital') => ({
  activeRadioId: 3, mode, link: { tier }, radio: { operatingMode, dialMhz: 14.074,
    sideband: 'USB', catOk: true, txEnabled: false, transmitting: false, rigKeyed: false, tuning: false }
}) as AppSnapshot

function fixture(workspace: OperatingWorkspace, snap = snapshot(), capability = true) {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'setTimeout', 'clearTimeout', 'performance'] })
  const frame = structuredClone(frames.spe) as MonitorFrame
  Object.assign(frame.station.radio, { id: 3, catConnected: true, rigKeyed: false, nexusBusy: false })
  frame.station.radio.readings.cat = { connectionGeneration: 7, readSequence: 10, ageMs: 0 }
  frame.station.radio.readings.ptt = { connectionGeneration: 7, readSequence: 10, ageMs: 0 }
  frame.station.amplifier = null
  const sent: any[] = [], values = new Map<string, string>()
  const storage = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const client = new OperationClient(wire => sent.push(JSON.parse(wire)), true, () => 1000, undefined, 3,
    pendingControlStorage(() => storage, 'workspace-test', async (_key, run) => run()))
  clients.push(client)
  const state: OperationState = { stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(),
    revision: 1, commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false,
    controls: { context: { radioId: 3, radioConnection: 7, ampConnection: null, ampReadSequence: null }, capabilities: capability ? ['mode', 'tier', 'workspace'] : ['mode', 'tier'] } }
  const reply = (value: unknown) => client.receive({ type: 'operationResponse', requestId: sent[sent.length - 1].request.requestId, value })
  client.open(); reply(state)
  const onSnap = vi.fn()
  const view = (current = snap, observation = frame, available = true, local = false) =>
    <StationControlContext.Provider value={local}><StationDataContext.Provider value={available}>
      <RemoteOperationsContext.Provider value={client}><RemoteObservationContext.Provider value={{ status: 'current', frame: observation }}>
        <CockpitHeader snap={current} remoteWorkspace={workspace} modeIndicator={<span>{workspace}</span>} bandControl={<span>20m</span>} onSnap={onSnap} />
      </RemoteObservationContext.Provider></RemoteOperationsContext.Provider>
    </StationDataContext.Provider></StationControlContext.Provider>
  return { ...render(view()), view, frame, state, client, sent, reply, onSnap,
    writes: () => sent.filter(w => w.request.type === 'stationControl') }
}
function button() { return screen.getByRole<HTMLButtonElement>('button', { name: 'Use this mode' }) }

it.each([
  ['ft', snapshot('JS8'), snapshot('FT8')],
  ['tempo', snapshot(), snapshot('TempoDeep', 'chat')],
  ['js8', snapshot('TempoFast', 'chat'), snapshot('JS8', 'chat')]
] as const)('%s entry is passive until one explicit gesture and waits for the native snapshot', async (workspace, before, after) => {
  const h = fixture(workspace, before)
  h.rerender(h.view()); await tick()
  expect(h.writes()).toHaveLength(0)
  expect(button().disabled).toBe(false)
  fireEvent.click(button()); await tick()
  expect(h.writes()).toHaveLength(1)
  const request = h.writes()[0].request
  expect(request.action).toEqual({ action: 'radio.workspace', workspace })
  expect(request.context).toEqual({ radioId: 3, radioConnection: 7, ampConnection: null, ampReadSequence: null })
  expect(button().disabled).toBe(true)
  fireEvent.click(button()); await tick()
  expect(h.writes()).toHaveLength(1)
  act(() => h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'radioReadback' }))
  await tick()
  expect(button()).toBeTruthy() // The receipt cannot invent a decoder snapshot.
  expect(h.onSnap).not.toHaveBeenCalled()
  h.rerender(h.view(after))
  expect(screen.queryByRole('button', { name: 'Use this mode' })).toBeNull()
  expect(setOperatingMode).not.toHaveBeenCalled()
  expect(h.writes()).toHaveLength(1)
})

it('requires the complete native workspace, including operating area, not merely a digital CAT mode', () => {
  const h = fixture('tempo', snapshot('TempoFast', 'qso'))
  expect(button().disabled).toBe(false)
  h.rerender(h.view(snapshot('TempoFast', 'chat')))
  expect(screen.queryByRole('button', { name: 'Use this mode' })).toBeNull()
  h.rerender(h.view(snapshot('TempoFast', 'chat', 'cw')))
  expect(button().disabled).toBe(false)
  expect(h.writes()).toHaveLength(0)
})

it('keeps older stations passive when they support mode and tier but not workspace entry', async () => {
  const h = fixture('js8', snapshot(), false)
  expect(button().disabled).toBe(true)
  fireEvent.click(button()); await tick()
  expect(h.writes()).toHaveLength(0)
  expect(setOperatingMode).not.toHaveBeenCalled()
})

it('refuses missing, stale, keyed or mismatched station evidence and loss of control', async () => {
  const h = fixture('js8')
  for (const change of [
    { id: 4 }, { catConnected: false }, { rigKeyed: true }, { rigKeyed: null }, { nexusBusy: true },
    { readings: { ...h.frame.station.radio.readings, ptt: null } },
    { readings: { ...h.frame.station.radio.readings, ptt: { connectionGeneration: 7, readSequence: 10, ageMs: 1000 } } }
  ]) {
    const frame = structuredClone(h.frame); Object.assign(frame.station.radio, change)
    h.rerender(h.view(snapshot(), frame))
    expect(button().disabled).toBe(true); fireEvent.click(button()); await tick()
  }
  h.rerender(h.view(snapshot(), h.frame, false))
  expect(button().disabled).toBe(true)
  h.rerender(h.view())
  expect(button().disabled).toBe(false)
  act(() => h.client.disconnected())
  expect(button().disabled).toBe(true)
  fireEvent.click(button()); await tick()
  expect(h.writes()).toHaveLength(0)
})

it('leaves desktop entry to the existing native handlers', () => {
  const h = fixture('tempo')
  h.rerender(h.view(snapshot(), h.frame, true, true))
  expect(screen.queryByRole('button', { name: 'Use this mode' })).toBeNull()
  expect(h.writes()).toHaveLength(0)
})
