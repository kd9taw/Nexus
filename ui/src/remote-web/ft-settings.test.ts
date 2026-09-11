import { afterEach, expect, it, vi } from 'vitest'
import { OperationClient } from './operation-client'
import { controlTransport } from './control-transport'
import { stationAction } from './station-operation'
import { controlVersion } from './operation-version'
import type { OperationState } from './operation-protocol'
import type { ApplicationClient } from './application-client'
import type { ApplicationTransport } from '../applicationTransport'
import type { ControlStorage } from './control-storage'

const clients: OperationClient[] = []
afterEach(() => { clients.splice(0).forEach(c => c.disconnected()); vi.useRealTimers() })
const key = '0000000000000001'
const expected = { key: key.repeat(2), txOffsetHz: 1500, rxOffsetHz: 1500, holdTxFreq: false, txEven: true, txCycleAuto: true }
const qso = { dxcall: 'W1AW', state: 'awaitReport', txNow: 'W1AW KD9TAW R-12', cqRunning: false }
function fixture(exclusive: ControlStorage['exclusive'] = async run => run()) {
  vi.useFakeTimers()
  const sent: any[] = [], saved = vi.fn()
  const c = new OperationClient(w => sent.push(JSON.parse(w)), true, () => Date.now(), undefined, 4,
    { read: () => null, write: saved, exclusive })
  clients.push(c); c.open()
  const state: OperationState = { stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(),
    revision: 1, commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: ['log.manual'], txArmed: false, transmitEpoch: key,
    controls: { context: { radioId: 0, radioConnection: null, ampConnection: null, ampReadSequence: null }, capabilities: ['ftSettings'] } }
  const reply = (requestId: string, value: unknown) => c.receive({ type: 'operationResponse', requestId, value })
  reply(sent[0].request.requestId, state)
  const snapshot = { link: { tier: 'FT8' }, qso, pendingLog: null, currentQsoLogKey: key, pendingQsoLogKey: null }
  let age = Infinity
  const reads = { kind: 'remote', invoke: vi.fn(async () => snapshot) } as unknown as ApplicationTransport
  return { c, sent, saved, state, reply, snapshot, reads, fresh: () => { age = 0 },
    transport: controlTransport(reads, { age: () => age } as unknown as ApplicationClient, c) }
}

it.each([
  ['set_tx_offset', { hz: 1800 }, { kind: 'txOffset', hz: 1800 }],
  ['set_ft_both_offsets', { hz: 2200 }, { kind: 'bothOffsets', hz: 2200 }],
  ['set_hold_tx_freq', { on: true }, { kind: 'hold', on: true }],
  ['set_tx_even', { even: false }, { kind: 'even', even: false }],
  ['set_tx_cycle_auto', { auto: true }, { kind: 'auto', auto: true }],
] as const)('binds %s to the rendered settings and waits for a later snapshot', async (command, value, change) => {
  const h = fixture(), result = h.transport.invoke(command, { ...value, expectedTier: 'FT8', expected })
  await vi.advanceTimersByTimeAsync(0)
  const request = h.sent.at(-1).request
  expect(request.action).toEqual({ action: 'ft.setting', expectedTier: 'FT8', transmitEpoch: key, expected, change })
  expect(h.reads.invoke).not.toHaveBeenCalled()
  expect(controlVersion(request.action)).toBe(4)
  h.reply(request.requestId, { operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: change.kind === 'auto' ? 'stationState' : 'settingsSaved' })
  let done = false; void result.then(() => { done = true })
  await vi.advanceTimersByTimeAsync(100); expect(done).toBe(false)
  h.fresh(); await vi.advanceTimersByTimeAsync(50); expect(await result).toBe(h.snapshot)
})
it('refuses expired captured intent without adopting refreshed settings or sending a command', async () => {
  let release!: () => void
  const gate = new Promise<void>(resolve => { release = resolve })
  const h = fixture(async run => { await gate; return run() })
  const args = { hz: 1800, expectedTier: 'FT8', expected: { ...expected } }
  const result = h.transport.invoke('set_tx_offset', args), assertion = expect(result).rejects.toThrow('windowExpired')
  args.hz = 2500; args.expected.key = key.replace('1','2').repeat(2)
  await vi.advanceTimersByTimeAsync(1250); h.reply(h.sent.at(-1).request.requestId, h.state)
  release(); await assertion
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(0)
})
it('refuses unrelated receipt evidence and malformed settings intents', async () => {
  const h=fixture(), result=h.transport.invoke('set_tx_offset',{hz:1800,expectedTier:'FT8',expected})
  const assertion=expect(result).rejects.toThrow('operationUnknown')
  await vi.advanceTimersByTimeAsync(0);const request=h.sent.at(-1).request
  h.reply(request.requestId,{operation:'stationControl',operationId:request.requestId,outcome:'applied',evidence:'stationState'});await assertion
  const action={action:'ft.setting',expectedTier:'FT8',transmitEpoch:key,expected,change:{kind:'hold',on:true}}
  expect(stationAction(action)).toEqual(action)
  for(const patch of [{expectedTier:'WSPR'},{transmitEpoch:'0'},{expected:{...expected,key:'bad'}},{change:{kind:'txOffset',hz:NaN}},{change:{kind:'bothOffsets',hz:4001}},{change:{kind:'hold',on:'true'}},{change:{kind:'hold',on:true,invoke:'set_settings'}}])
    expect(()=>stationAction({...action,...patch})).toThrow()
})
