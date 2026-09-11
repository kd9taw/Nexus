import { afterEach, expect, it, vi } from 'vitest'
import { OperationClient } from './operation-client'
import { controlTransport } from './control-transport'
import { stationAction } from './station-operation'
import { operationValue } from './operation-protocol'
import type { OperationState } from './operation-protocol'
import type { ApplicationClient } from './application-client'
import type { ApplicationTransport } from '../applicationTransport'
import type { ControlStorage } from './control-storage'
const clients: OperationClient[] = []
afterEach(() => { clients.splice(0).forEach(c => c.disconnected()); vi.useRealTimers() })
function fixture(exclusive: ControlStorage['exclusive'] = async run => run()) {
  vi.useFakeTimers()
  const sent: any[] = []
  const saved = vi.fn()
  const c = new OperationClient(w => sent.push(JSON.parse(w)), true, () => Date.now(), undefined, 4,
    { read: () => null, write: saved, exclusive })
  clients.push(c); c.open()
  const state: OperationState = { stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(),
    revision: 1, commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false,
    transmitEpoch: '0000000000000001', controls: { context: { radioId: 0, radioConnection: 1, ampConnection: null, ampReadSequence: null }, capabilities: ['ftOperate', 'ftCall'] } }
  const reply = (requestId: string, value: unknown) => c.receive({ type: 'operationResponse', requestId, value })
  reply(sent[0].request.requestId, state)
  const snapshot = { link: { tier: 'FT8' }, radio: { txEnabled: false } }
  let age = Infinity
  const reads = { kind: 'remote', invoke: vi.fn(async () => snapshot) } as unknown as ApplicationTransport
  const transport = controlTransport(reads, { age: () => age } as unknown as ApplicationClient, c)
  return { c, sent, saved, state, reply, transport, snapshot, reads, fresh: () => { age = 0 } }
}

it('validates directed CQ tokens without widening into a native invoke tunnel', () => {
  const a = { action: 'ft.cq', expectedTier: 'FT8', transmitEpoch: '0000000000000001', direction: null }
  for (const direction of [null, 'DX', 'EU', '123']) expect(stationAction({ ...a, direction })).toEqual({ ...a, direction })
  for (const patch of [{ direction: 'dx' }, { direction: '12' }, { direction: 'CQ DX W1AW' }, { expectedTier: 'WSPR' }, { transmitEpoch: '1' }, { invoke: 'start_cq' }]) expect(() => stationAction({ ...a, ...patch })).toThrow()
  expect(operationValue({ stop: 'accepted' })).toEqual({ stop: 'accepted' })
})

it.each([['call_station', { call: 'W1AW', grid: null, message: 'CQ W1AW FN31', snr: -10, freq: 1250 }, 'ft.call'], ['start_cq', { dir: 'DX' }, 'ft.cq'], ['set_tx_enabled', { enabled: true }, 'ft.txEnabled']] as const)(
  'adapts %s to its FT authority and waits for a later station sample', async (command, args, action) => {
    const h = fixture(), result = h.transport.invoke(command, args)
    await Promise.resolve(); await Promise.resolve(); await Promise.resolve()
    const request = h.sent[h.sent.length - 1].request
    expect(request.type).toBe('stationControl'); expect(request.action.action).toBe(action)
    expect(request.action.transmitEpoch).toBe(h.state.transmitEpoch)
    expect(request.action.expectedTier).toBe('FT8')
    h.reply(request.requestId, { operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'stationState' })
    let finished = false; void result.then(() => { finished = true })
    await vi.advanceTimersByTimeAsync(100); expect(finished).toBe(false)
    h.fresh(); await vi.advanceTimersByTimeAsync(50)
    expect(await result).toEqual(h.snapshot)
  }
)

it('Stop bypasses the browser storage lock and prevents a captured CQ from later leaving it', async () => {
  let release!: () => void
  const gate = new Promise<void>(resolve => { release = resolve })
  const h = fixture(async run => { await gate; return run() })
  const cq = h.c.control({ action: 'ft.cq', expectedTier: 'FT8', transmitEpoch: h.state.transmitEpoch!, direction: null })
  const rejected = expect(cq).rejects.toThrow('staleContext')
  const stopped = h.transport.invoke('halt_tx')
  const request = h.sent[h.sent.length - 1].request
  expect(request.type).toBe('stopTransmit')
  h.reply(request.requestId, { stop: 'accepted' })
  release(); await rejected
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(0)
  expect(h.saved).not.toHaveBeenCalled()
  h.fresh(); await vi.advanceTimersByTimeAsync(50); await stopped
})

it('keeps native-selected call context bounded and rejects mixed roster/decode arguments', () => {
  const action = { action: 'ft.call', expectedTier: 'FT8', transmitEpoch: '0000000000000001',
    selection: { call: 'W1AW', grid: null, message: 'CQ W1AW FN31', snr: -10, freq: 1250 } }
  expect(stationAction(action)).toEqual(action)
  for (const patch of [{ message: null }, { grid: 'FN31' }, { snr: null }, { freq: null }, { freq: NaN }, { call: 'W1AW;halt_tx' }, { invoke: 'call_station' }])
    expect(() => stationAction({ ...action, selection: { ...action.selection, ...patch } })).toThrow()
  const roster = { ...action, selection: { call: 'W1AW', grid: 'FN31', message: null, snr: null, freq: 1250 } }
  expect(stationAction(roster)).toEqual(roster)
})
