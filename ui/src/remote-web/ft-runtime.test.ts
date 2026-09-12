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
const settings = { key: key.repeat(2), txOffsetHz: 1500, rxOffsetHz: 1500, holdTxFreq: false, txEven: true, txCycleAuto: true }
const expected = { settings, skipTx1: false }
const qso = { dxcall: 'W1AW', state: 'awaitReport', txNow: 'W1AW KD9TAW R-12', cqRunning: false }
function fixture(exclusive: ControlStorage['exclusive'] = async run => run()) {
  vi.useFakeTimers()
  const sent: any[] = [], saved = vi.fn()
  const c = new OperationClient(w => sent.push(JSON.parse(w)), true, () => Date.now(), undefined, 4,
    { read: () => null, write: saved, exclusive })
  clients.push(c); c.open()
  const state: OperationState = { stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(),
    revision: 1, commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: ['log.manual'], txArmed: false, transmitEpoch: key,
    controls: { context: { radioId: 0, radioConnection: null, ampConnection: null, ampReadSequence: null }, capabilities: ['ftRuntime'] } }
  const reply = (requestId: string, value: unknown) => c.receive({ type: 'operationResponse', requestId, value })
  reply(sent[0].request.requestId, state)
  const snapshot = { link: { tier: 'FT8' }, qso, pendingLog: null, currentQsoLogKey: key, pendingQsoLogKey: null }
  let age = Infinity
  const reads = { kind: 'remote', invoke: vi.fn(async () => snapshot) } as unknown as ApplicationTransport
  return { c, sent, saved, state, reply, snapshot, reads, fresh: () => { age = 0 },
    transport: controlTransport(reads, { age: () => age } as unknown as ApplicationClient, c) }
}

it.each([
  ['set_rx_offset', { hz: 900 }, { kind: 'rxOffset', hz: 900 }, 'settingsSaved'],
  ['set_skip_tx1', { enabled: true }, { kind: 'skipTx1', on: true }, 'stationState'],
] as const)('routes %s through the FT grant and captures the displayed runtime', async (command, value, change, evidence) => {
  const h=fixture(), args={...value,expectedTier:'FT4',expected:structuredClone(expected)}
  const result=h.transport.invoke(command,args)
  args.expected.skipTx1=true
  await vi.advanceTimersByTimeAsync(0)
  const request=h.sent[h.sent.length-1].request
  expect(request.action).toEqual({action:'ft.runtime',expectedTier:'FT4',transmitEpoch:key,expected,change})
  expect(controlVersion(request.action)).toBe(4);expect(h.reads.invoke).not.toHaveBeenCalled()
  h.reply(request.requestId,{operation:'stationControl',operationId:request.requestId,outcome:'applied',evidence})
  let done=false;void result.then(()=>{done=true})
  await vi.advanceTimersByTimeAsync(100);expect(done).toBe(false)
  h.fresh();await vi.advanceTimersByTimeAsync(50);expect(await result).toBe(h.snapshot)
})
it('requires current settings, Skip state and the closed runtime vocabulary',()=>{
  const action={action:'ft.runtime',expectedTier:'FT8',transmitEpoch:key,expected,change:{kind:'skipTx1',on:true}}
  expect(stationAction(action)).toEqual(action)
  for(const patch of [{expected:settings},{expected:{...expected,skipTx1:undefined}},{expected:{...expected,skipTx1:'true'}},{change:{kind:'txOffset',hz:900}},{change:{kind:'rxOffset',hz:Infinity}},{change:{kind:'rxOffset',hz:199}}])
    expect(()=>stationAction({...action,...patch})).toThrow()
  expect(()=>stationAction({...action,action:'ft.setting'})).toThrow()
})
it('does not turn an uncertain RX outcome into a saved preference',async()=>{
  const h=fixture(),result=h.transport.invoke('set_rx_offset',{hz:900,expectedTier:'FT8',expected}),assertion=expect(result).rejects.toThrow('operationUnknown')
  await vi.advanceTimersByTimeAsync(0);const request=h.sent[h.sent.length-1].request
  h.reply(request.requestId,{operation:'stationControl',operationId:request.requestId,outcome:'applied',evidence:'stationState'});await assertion
})
