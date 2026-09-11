// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render } from '@testing-library/react'
import { BandPicker } from '../components/BandPicker'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { installApplicationTransport, type ApplicationTransport } from '../applicationTransport'
import { controlTransport } from './control-transport'
import { OperationClient } from './operation-client'
import type { ApplicationClient } from './application-client'
import { pendingControlStorage } from './control-storage'
import type { AppSnapshot } from '../types'
import type { ControlCapability } from './station-operation'
import { readBandChoices } from './band-choices'

vi.mock('../toast', () => ({ pushToast: vi.fn() }))
const invoke = vi.fn(async (_command: string, _args?: Record<string, unknown>): Promise<unknown> => null)

const cleanupClients: (() => void)[] = []
afterEach(() => { cleanup(); cleanupClients.splice(0).forEach(f => f()); vi.useRealTimers(); vi.clearAllMocks(); delete window.__TAURI_INTERNALS__ })
async function tick(ms = 0) { await act(async () => { await vi.advanceTimersByTimeAsync(ms) }) }
const channel = (band = '40m', dialMhz = 7.2) => ({ band, dialMhz, group: 'HF', mode: 'LSB', label: band, note: '', tx: true })
function fixture(mode: 'cw' | 'phone' = 'phone', capabilities: ControlCapability[] = ['bandSelection'], version: 2 | 3 = 3, local = false) {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'setTimeout', 'clearTimeout', 'performance'] })
  const snap = { activeRadioId: 1, radio: { source: 'native', operatingMode: mode, band: '20m', dialMhz: 14.275,
    catOk: true, txEnabled: false, transmitting: false, rigKeyed: false, tuning: false, txAllowed: true } } as AppSnapshot
  const sent: any[] = [], values = new Map<string, string>()
  const storage = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const client = new OperationClient(wire => sent.push(JSON.parse(wire)), true, () => 1000, undefined, version,
    pendingControlStorage(() => storage, 'band-selection', async (_key, run) => run()))
  const state = { stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(), revision: 1,
    commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false,
    controls: { context: { radioId: 1, radioConnection: 7, ampConnection: null, ampReadSequence: null }, capabilities } }
  const reply = (value: unknown) => client.receive({ type: 'operationResponse', requestId: sent[sent.length - 1].request.requestId, value })
  client.open(); reply(state)
  let settings: unknown = { bandChoices: { cw: [channel()], phone: [channel()] } }, age = Infinity
  const later = { ...snap, radio: { ...snap.radio, band: '40m', dialMhz: 7.255 } }
  const read = vi.fn(async (command: string) => command === 'get_settings' ? settings : later)
  if (!local) cleanupClients.push(installApplicationTransport(controlTransport({ kind: 'remote', invoke: read } as ApplicationTransport,
    { age: () => age } as unknown as ApplicationClient, client)))
  else {
    invoke.mockImplementation(async command => command === 'get_licensed_band_plan' ? [channel()] : later)
    window.__TAURI_INTERNALS__ = { invoke: invoke as NonNullable<Window['__TAURI_INTERNALS__']>['invoke'] }
  }
  cleanupClients.push(() => client.disconnected())
  const onSnap = vi.fn()
  const view = (available = true, current = snap) => <StationControlContext.Provider value={local}><StationDataContext.Provider value={available}>
    <RemoteOperationsContext.Provider value={client}><BandPicker snap={current} mode={mode} onSnap={onSnap}/></RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>
  const rendered = render(view())
  return { ...rendered, view, snap, later, sent, state, client, reply, read, onSnap, setSettings: (s: unknown) => { settings = s }, setAge: (v: number) => { age = v },
    select: () => rendered.container.querySelector<HTMLSelectElement>('select')!, writes: () => sent.filter(f => f.request.type === 'stationControl') }
}

it.each(['cw', 'phone'] as const)('uses the actual %s picker and waits for station-selected memory/readback', async mode => {
  const h = fixture(mode); await tick()
  expect(h.select().disabled).toBe(false)
  expect(h.read).toHaveBeenCalledWith('get_settings')
  fireEvent.change(h.select(), { target: { value: '40m' } }); await tick()
  const request = h.writes()[0].request
  expect(request.action).toEqual({ action: 'radio.band', band: '40m', mode })
  expect(request.context).toEqual(h.state.controls.context)
  expect(h.writes()).toHaveLength(1)
  act(() => h.reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'radioReadback' })); await tick(100)
  expect(h.onSnap).not.toHaveBeenCalled()
  h.setAge(0); await tick(50)
  expect(h.onSnap).toHaveBeenCalledExactlyOnceWith(h.later)
  expect(h.later.radio.dialMhz).not.toBe(7.2)
  expect(invoke).not.toHaveBeenCalled()
})

it.each([[[], 3], [['bandSelection'], 2]] as const)('keeps older or ungranted pickers unavailable', async (capabilities, version) => {
  const h = fixture('phone', [...capabilities], version); await tick()
  expect(h.select().disabled).toBe(true); expect(h.read).not.toHaveBeenCalled()
  fireEvent.change(h.select(), { target: { value: '40m' } }); await tick()
  expect(h.writes()).toHaveLength(0); expect(invoke).not.toHaveBeenCalled()
})

it('expires remote choices on loss and refreshes actual station license choices', async () => {
  const h = fixture(); await tick()
  h.setSettings({ bandChoices: { cw: [], phone: [channel('10m', 28.4)] } }); await tick(1000)
  expect([...h.select().options].map(o => o.value)).toEqual(['20m', '10m'])
  let resolve!: (v: unknown) => void
  h.read.mockImplementationOnce(() => new Promise(r => { resolve = r }))
  await tick(1000); h.rerender(h.view(false)); await tick()
  await act(async () => { resolve({ bandChoices: { cw: [], phone: [channel()] } }) }); await tick()
  expect(h.select().disabled).toBe(true); expect([...h.select().options].map(o => o.value)).toEqual(['20m'])
  expect(h.writes()).toHaveLength(0)
  h.rerender(h.view()); await tick()
  expect(h.select().disabled).toBe(false); expect([...h.select().options].map(o => o.value)).toEqual(['20m', '10m'])
})

it('preserves the local band-pick API and snapshot behavior', async () => {
  const h = fixture('phone', [], 3, true); await tick()
  fireEvent.change(h.select(), { target: { value: '40m' } }); await tick()
  expect(invoke).toHaveBeenCalledWith('get_licensed_band_plan', { mode: 'phone' })
  expect(invoke).toHaveBeenCalledWith('pick_band', { band: '40m', mode: 'phone' })
  expect(h.onSnap).toHaveBeenCalledWith(h.later); expect(h.writes()).toHaveLength(0)
})

it('does not invent defaults for missing or malformed station band choices', () => {
  expect(readBandChoices({ bandChoices: { cw: [channel()], phone: [] } }, 'cw')).toEqual([channel()])
  for (const value of [{}, { bandChoices: null }, { bandChoices: { cw: [channel()], phone: [], command: 'tune' } },
    { bandChoices: { cw: [channel(), channel()], phone: [] } }, { bandChoices: { cw: [{ ...channel(), mode: ['USB'] }], phone: [] } },
    { bandChoices: { cw: [{ ...channel(), tx: 'yes' }], phone: [] } }, { bandChoices: { cw: Array(33).fill(channel()), phone: [] } }])
    expect(() => readBandChoices(value, 'cw')).toThrow()
})
