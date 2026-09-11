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
const qso = { dxcall: 'W1AW', state: 'awaitReport', txNow: 'W1AW KD9TAW R-12', cqRunning: false }
const record = { call: 'W1AW', grid: 'FN31', rstSent: '-12', rstRcvd: '-10', whenUnix: 1700000000, freqMhz: 14.0755, notes: 'Preserve native fields' }
function fixture(exclusive: ControlStorage['exclusive'] = async run => run()) {
  vi.useFakeTimers()
  const sent: any[] = [], saved = vi.fn()
  const c = new OperationClient(w => sent.push(JSON.parse(w)), true, () => Date.now(), undefined, 4,
    { read: () => null, write: saved, exclusive })
  clients.push(c); c.open()
  const state: OperationState = { stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(),
    revision: 1, commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: ['log.manual'], txArmed: false,
    controls: { context: { radioId: 0, radioConnection: null, ampConnection: null, ampReadSequence: null }, capabilities: ['qsoLogging'] } }
  const reply = (requestId: string, value: unknown) => c.receive({ type: 'operationResponse', requestId, value })
  reply(sent[0].request.requestId, state)
  const snapshot = { link: { tier: 'FT8' }, qso, pendingLog: null, currentQsoLogKey: key, pendingQsoLogKey: null }
  let age = Infinity
  const reads = { kind: 'remote', invoke: vi.fn(async () => snapshot) } as unknown as ApplicationTransport
  return { c, sent, saved, state, reply, snapshot, reads, fresh: () => { age = 0 },
    transport: controlTransport(reads, { age: () => age } as unknown as ApplicationClient, c) }
}

it.each([
  ['log_current_qso', { expectedKey: key, expectedTier: 'FT8', expectedQso: qso }, 'qso.logCurrent', 'fileSynced'],
  ['log_current_qso', { expectedKey: key, expectedTier: 'FT8', expectedQso: qso }, 'qso.logCurrent', 'pendingConfirmationSynced'],
  ['confirm_pending_log', { expectedKey: key, record }, 'qso.confirm', 'fileSynced'],
  ['discard_pending_log', { expectedKey: key }, 'qso.discard', 'pendingDiscarded'],
] as const)('adapts %s with logging-only permission and requires %s durability', async (command, args, action, evidence) => {
  const h = fixture(), result = h.transport.invoke(command, args)
  await vi.advanceTimersByTimeAsync(0)
  const request = h.sent[h.sent.length - 1].request
  expect(request.type).toBe('stationControl')
  expect(request.action.action).toBe(action)
  expect(request.action.expectedKey).toBe(key)
  expect(request.action.transmitEpoch).toBeUndefined()
  expect(h.reads.invoke).not.toHaveBeenCalled()
  if (action === 'qso.confirm') expect(request.action.edits).toEqual({ call: record.call, grid: record.grid, rstSent: record.rstSent, rstRcvd: record.rstRcvd })
  h.reply(request.requestId, { operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence })
  let finished = false; void result.then(() => { finished = true })
  await vi.advanceTimersByTimeAsync(100)
  expect(finished).toBe(false)
  h.fresh(); await vi.advanceTimersByTimeAsync(50)
  expect(await result).toEqual(action === 'qso.logCurrent'
    ? { logged: evidence === 'fileSynced', pending: evidence === 'pendingConfirmationSynced', snapshot: h.snapshot } : h.snapshot)
})

it.each(['stationState', 'radioReadback', 'pendingDiscarded'])('never reports a saved QSO from unrelated evidence %s', async evidence => {
  const h = fixture()
  const result = h.transport.invoke('confirm_pending_log', { expectedKey: key, record })
  const assertion = expect(result).rejects.toThrow('operationUnknown')
  await vi.advanceTimersByTimeAsync(0)
  const request = h.sent[h.sent.length - 1].request
  h.reply(request.requestId, { operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence })
  await assertion
})

it('retains the original gesture and refuses it after its storage wait outlives the command window', async () => {
  let release!: () => void
  const gate = new Promise<void>(resolve => { release = resolve })
  const h = fixture(async run => { await gate; return run() })
  const args = { expectedKey: key, record: { ...record } }
  const result = h.transport.invoke('confirm_pending_log', args)
  const assertion = expect(result).rejects.toThrow('windowExpired')
  args.expectedKey = '0000000000000002'; args.record.call = 'K2ABC'
  await vi.advanceTimersByTimeAsync(1250)
  h.reply(h.sent[h.sent.length - 1].request.requestId, h.state)
  release(); await assertion
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(0)
  expect(h.saved).not.toHaveBeenCalled()
})

it.each(['noEligibleContact', 'alreadyPresent'])('returns the native no-log result for %s without retry', async reason => {
  const h = fixture()
  const result = h.transport.invoke('log_current_qso', { expectedKey: key, expectedTier: 'FT8', expectedQso: qso })
  await vi.advanceTimersByTimeAsync(0)
  const request = h.sent[h.sent.length - 1].request
  h.reply(request.requestId, { operation: 'stationControl', operationId: request.requestId, outcome: 'rejected', reason })
  h.fresh(); await vi.advanceTimersByTimeAsync(50)
  expect(await result).toEqual({ logged: false, pending: false, snapshot: h.snapshot })
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(1)
})

it('bounds QSO keys and editable fields and requires version four', () => {
  const confirm = { action: 'qso.confirm', expectedKey: key, edits: { call: record.call, grid: record.grid, rstSent: record.rstSent, rstRcvd: record.rstRcvd } }
  expect(stationAction(confirm)).toEqual(confirm)
  expect(controlVersion(stationAction(confirm))).toBe(4)
  for (const patch of [{ expectedKey: '1' }, { expectedKey: key.toUpperCase().replace('1', 'G') }, { invoke: 'confirm_pending_log' },
    { edits: { ...confirm.edits, whenUnix: 0 } }, { edits: { ...confirm.edits, call: 'W1AW;halt_tx' } }])
    expect(() => stationAction({ ...confirm, ...patch })).toThrow()
})
