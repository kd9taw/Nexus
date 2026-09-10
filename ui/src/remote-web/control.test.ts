import { afterEach, expect, it, vi } from 'vitest'
import { OperationClient } from './operation-client'
import { OperationRelay } from './operation-relay'
import { pendingControlStorage } from './control-storage'
import { pendingLogStorage, type ReceiptLock } from './operation-storage'
import type { OperationState } from './operation-protocol'

afterEach(() => vi.useRealTimers())
const action = { action: 'amplifier.operate', expectedOperate: false, operate: true } as const
function storage() {
  const values = new Map<string, string>(), locks = new Set<string>()
  const data = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k,v) }, removeItem: (k: string) => { values.delete(k) } }
  const lock: ReceiptLock = async (k, run) => {
    if (locks.has(k)) throw Error('remoteBusy')
    locks.add(k)
    try { return await run() } finally { locks.delete(k) }
  }
  return { data, lock }
}
function setup(store = storage()) {
  vi.useFakeTimers()
  let now = 1000
  const sent: any[] = []
  const controls = pendingControlStorage(() => store.data, 'station', store.lock)
  const logs = pendingLogStorage(() => store.data, 'station', store.lock)
  const client = new OperationClient(s => sent.push(JSON.parse(s)), true, () => now, logs, 2, controls)
  const s: OperationState = {
    stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(),
    revision: 1, commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000,
    actions: [], txArmed: false, controls: { context: { radioId: 1, radioConnection: 1, ampConnection: 1, ampReadSequence: 1 }, capabilities: ['decoder','amplifier'] }
  }
  const reply = (value: unknown) => client.receive({ type: 'operationResponse', requestId: sent[sent.length - 1].request.requestId, value })
  client.open(); reply(s)
  return { client, controls, logs, sent, state: s, reply, store, advance: async (ms: number) => { now += ms; await vi.advanceTimersByTimeAsync(ms) } }
}

it('sends one explicit intent and retains it until a later native readback', async () => {
  const h = setup(), completed = h.client.control(action)
  await Promise.resolve()
  const request = h.sent[h.sent.length - 1].request
  expect(h.sent[h.sent.length - 1].operationVersion).toBe(2)
  expect(request).toMatchObject({ type: 'stationControl', action, context: h.state.controls!.context })
  const receipt = { operation: 'stationControl', operationId: request.requestId }
  h.reply({ ...receipt, outcome: 'pending' })
  await Promise.resolve(); await Promise.resolve()
  expect(h.controls.read()?.operationId).toBe(request.requestId)
  await h.advance(250)
  expect(h.sent[h.sent.length - 1].request.type).toBe('state')
  h.reply({ ...h.state, revision: 2, nextSequence: 2 })
  await h.advance(250)
  expect(h.sent[h.sent.length - 1].request).toMatchObject({ type: 'result', operationId: request.requestId })
  h.reply({ ...receipt, outcome: 'applied', evidence: 'amplifierReadback' })
  expect(await completed).toMatchObject({ outcome: 'applied', evidence: 'amplifierReadback' })
  expect(h.controls.read()).toBeNull()
  expect(h.sent.filter(w => w.request.type === 'stationControl')).toHaveLength(1)
  h.client.disconnected()
})

it('does not replay an uncertain toggle after disconnect or reload', async () => {
  const h = setup(), result = h.client.control(action).catch(e => e.message)
  await Promise.resolve()
  const operationId = h.sent[h.sent.length - 1].request.requestId
  h.client.disconnected()
  await result
  const next = setup(h.store)
  await expect(next.client.control(action)).rejects.toThrow('operationUnknown')
  expect(next.sent.map(w => w.request.type)).toEqual(['state'])
  const checked = next.client.refreshControl()
  await Promise.resolve()
  next.reply({ operation: 'stationControl', operationId, outcome: 'unknown', reason: 'hardwareUnconfirmed' })
  await checked
  expect(next.controls.read()?.operationId).toBe(operationId)
  await next.client.acknowledgeControl()
  expect(next.controls.read()).toBeNull()
  next.client.disconnected()
})

it('binds result kinds and never clears a control intent using a log receipt', async () => {
  const h = setup(), result = h.client.control(action).catch(() => {})
  await Promise.resolve()
  const id = h.sent[h.sent.length - 1].request.requestId
  expect(() => h.reply({ operationId: id, outcome: 'applied', evidence: 'fileSynced', uploads: 'stationPipeline' })).toThrow('invalidOperation')
  expect(h.controls.read()?.operationId).toBe(id)
  h.reply({ operation: 'stationControl', operationId: id, outcome: 'rejected', reason: 'stationBusy' })
  await result
  expect(h.controls.read()).toBeNull()
  h.client.disconnected()
})

it('keeps logging and operating receipts exclusive across tabs', async () => {
  const h = setup(), entry = { operationId: crypto.randomUUID(), action }
  await h.controls.exclusive(() => h.controls.write(entry))
  const another = pendingLogStorage(() => h.store.data, 'station', h.store.lock)
  await expect(another.exclusive!(() => another.write(crypto.randomUUID()))).rejects.toThrow('operationUnknown')
  const oldTab = pendingControlStorage(() => h.store.data, 'station', h.store.lock)
  oldTab.read()
  await h.controls.exclusive(() => h.controls.write(null))
  const newer = { operationId: crypto.randomUUID(), action }
  await h.controls.exclusive(() => h.controls.write(newer))
  await oldTab.exclusive(() => oldTab.write(null))
  expect(h.controls.read()).toEqual(newer)
  h.client.disconnected()
})

it('refuses station control on an old station while retaining legacy read negotiation', () => {
  const frames: any[] = [], replies: any[] = [], relay = new OperationRelay()
  const station = { send: (v: unknown) => { frames.push(v) }, close: () => {} }, browser = { send: (v: unknown) => { replies.push(v) }, close: () => {} }
  // The legacy peer shape is accepted by the relay. Control is versioned separately.
  relay.sync({ peer: station, supported: true }, [{ peer: browser, sessionId: 'browser', deviceId: crypto.randomUUID() }], 1000)
  const requestId = crypto.randomUUID()
  const request = { type: 'stationControl', requestId, stationBootId: crypto.randomUUID(), leaseId: crypto.randomUUID(), commandWindowId: crypto.randomUUID(), expectedRevision: 1, clientSequence: 1, context: { radioId: 1, radioConnection: 1, ampConnection: 1, ampReadSequence: 1 }, action }
  relay.receiveBrowser('browser', { type: 'operationRequest', operationVersion: 2, request }, 1000)
  expect(frames).toHaveLength(0)
  expect(JSON.stringify(replies)).toContain('stationUnsupported')
})
