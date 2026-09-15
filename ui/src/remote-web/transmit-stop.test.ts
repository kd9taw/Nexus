import { afterEach, expect, it, vi } from 'vitest'
import { OperationClient } from './operation-client'
import { OperationRelay } from './operation-relay'
import { pendingControlStorage } from './control-storage'
import { operationRequest, operationValue, type OperationState } from './operation-protocol'

const id = () => crypto.randomUUID()
const state = (): OperationState => ({ stationBootId: id(), allowed: true, phase: 'controlling', leaseId: id(), revision: 1,
  commandWindowId: id(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false, transmitEpoch: '000000000000002a' })
const peer = () => ({ frames: [] as any[], send(s: string) { this.frames.push(JSON.parse(s)) }, close: vi.fn() })
const stop = (s = state()) => ({ type: 'stopTransmit' as const, requestId: id(), stationBootId: s.stationBootId, leaseId: s.leaseId!, transmitEpoch: s.transmitEpoch! })
afterEach(() => vi.useRealTimers())

it('accepts only a bounded stop token and reports acceptance without asserting RF state', () => {
  const r = stop()
  expect(operationRequest(r)).toEqual(r)
  for (const transmitEpoch of ['', '0', 'A'.repeat(16), '0'.repeat(17), 42]) expect(() => operationRequest({ ...r, transmitEpoch })).toThrow()
  expect(() => operationRequest({ ...r, action: 'halt_tx' })).toThrow()
  expect(operationValue({ stop: 'accepted' })).toEqual({ stop: 'accepted' })
  for (const bad of [{ stop: 'stopped' }, { stop: 'accepted', rf: false }, { ...state(), phase: 'available' }]) expect(() => operationValue(bad)).toThrow()
})

it('routes Stop despite ordinary rate exhaustion and pending requests, retaining all routes across hibernation', () => {
  const relay = new OperationRelay(), station = peer(), browser = peer(), sessionId = id(), deviceId = id()
  const peers = [{ sessionId, deviceId, peer: browser }]
  relay.sync({ peer: station, supported: true, operationVersion: 4 }, peers, 1000)
  const send = (request: unknown) => relay.receiveBrowser(sessionId, { type: 'operationRequest', operationVersion: 4, request }, 1000)
  for (let n = 0; n < 2; n++) {
    const requestId = id(); send({ type: 'state', requestId })
    relay.receiveStation({ type: 'operationResponse', sessionId, requestId, value: state() })
  }
  const s = state()
  const command = { type: 'stationControl', requestId: id(), stationBootId: s.stationBootId, leaseId: s.leaseId,
    expectedRevision: 1, commandWindowId: s.commandWindowId, clientSequence: 1,
    context: { radioId: 1, radioConnection: 1, ampConnection: null, ampReadSequence: null }, action: { action: 'decoder.clear', receiver: 'cw' } }
  send(command); send({ type: 'heartbeat', requestId: id(), leaseId: s.leaseId })
  const request = stop(s); send(request)
  expect(station.frames).toHaveLength(5)
  expect(station.frames[4].request).toEqual(request)
  send(stop(s)); expect(browser.frames[browser.frames.length - 1].error).toBe('remoteBusy')
  const restored = new OperationRelay()
  restored.sync({ peer: station, supported: true, operationVersion: 4 }, peers, 1001)
  restored.restore(sessionId, relay.checkpoint(sessionId))
  expect(restored.checkpoint(sessionId).pending).toHaveLength(3)
  restored.receiveStation({ type: 'operationResponse', sessionId, requestId: request.requestId, value: { stop: 'accepted' } })
  expect(browser.frames[browser.frames.length - 1]).toEqual({ type: 'operationResponse', requestId: request.requestId, value: { stop: 'accepted' } })
  expect(restored.checkpoint(sessionId).pending).toHaveLength(2)
  expect(station.close).not.toHaveBeenCalled()
  expect(() => new OperationRelay().restore(sessionId, relay.checkpoint(sessionId))).toThrow()
})

it.each([1, 2, 3])('refuses Stop through an older peer and projects v4 owner tokens away (v%s)', version => {
  const relay = new OperationRelay(), station = peer(), browser = peer(), sessionId = id()
  relay.sync({ peer: station, supported: true, operationVersion: version }, [{ sessionId, deviceId: id(), peer: browser }], 1000)
  relay.receiveBrowser(sessionId, { type: 'operationRequest', operationVersion: 4, request: stop() }, 1000)
  expect(station.frames).toHaveLength(0)
  expect(browser.frames[0].error).toBe('stationUnsupported')
  const requestId = id()
  relay.receiveBrowser(sessionId, { type: 'operationRequest', operationVersion: 4, request: { type: 'state', requestId } }, 1000)
  relay.receiveStation({ type: 'operationResponse', sessionId, requestId, value: state() })
  expect(browser.frames[1].value.transmitEpoch).toBeUndefined()
})

function client() {
  vi.useFakeTimers()
  const sent: any[] = [], storage = { read: () => { throw Error('unavailable') }, write: vi.fn(() => { throw Error('unavailable') }) }
  const c = new OperationClient(wire => sent.push(JSON.parse(wire)), true, () => Date.now(), storage, 4)
  c.open()
  const s = state()
  c.receive({ type: 'operationResponse', requestId: sent[0].request.requestId, value: s })
  return { c, sent, s, storage }
}

it('sends Stop while a heartbeat is outstanding despite unavailable receipt storage', async () => {
  const h = client()
  await vi.advanceTimersByTimeAsync(1000)
  expect(h.sent[h.sent.length - 1].request.type).toBe('heartbeat')
  const heartbeat = h.sent[h.sent.length - 1].request.requestId, stopped = h.c.stopTransmit()
  expect(h.sent[h.sent.length - 1].request.type).toBe('stopTransmit')
  expect(h.sent[h.sent.length - 1].request.transmitEpoch).toBe(h.s.transmitEpoch)
  expect(h.c.getSnapshot().busy).toBe(true)
  await expect(h.c.stopTransmit()).rejects.toThrow('remoteBusy')
  h.c.receive({ type: 'operationResponse', requestId: h.sent[h.sent.length - 1].request.requestId, value: { stop: 'accepted' } })
  expect(await stopped).toEqual({ stop: 'accepted' })
  expect(h.c.getSnapshot().busy).toBe(true)
  expect(h.c.getSnapshot().state).toEqual(h.s)
  expect(h.storage.write).not.toHaveBeenCalled()
  h.c.receive({ type: 'operationResponse', requestId: heartbeat, value: { ...h.s, transmitEpoch: '000000000000002b' } })
  expect(h.c.getSnapshot().busy).toBe(false)
  h.c.disconnected()
})

// Stop outlives the lease (operator ruling, 2026-09-15). The STATION decides: it keeps issuing a
// current stop token to a browser that held station control after the lease runs out, and stops
// issuing one when the grant goes, another browser takes over or the station reboots. The browser
// carries the lease id the token was first issued under and sends whatever token it was last given.
it('keeps Stop available after the lease runs out, and ends it when the station stops issuing a token', async () => {
  const h = client()
  const lapsed = (transmitEpoch: string | null) => ({ ...h.s, phase: 'available' as const, leaseId: null,
    commandWindowId: null, nextSequence: null, leaseRemainingMs: null, transmitEpoch })
  const answer = async (value: object) => {
    await vi.advanceTimersByTimeAsync(1000)
    const request = h.sent[h.sent.length - 1].request
    // A heartbeat while the lease is live; a plain state read once it has lapsed.
    expect(['heartbeat', 'state']).toContain(request.type)
    h.c.receive({ type: 'operationResponse', requestId: request.requestId, value })
  }
  await answer(lapsed('000000000000002b'))
  expect(h.c.getSnapshot().stopAvailable).toBe(true)
  const stopped = h.c.stopTransmit()
  const request = h.sent[h.sent.length - 1].request
  expect(request.type).toBe('stopTransmit')
  // Only the token moves; the lease it was first issued under is the one the station matches.
  expect(request.leaseId).toBe(h.s.leaseId)
  expect(request.transmitEpoch).toBe('000000000000002b')
  h.c.receive({ type: 'operationResponse', requestId: request.requestId, value: { stop: 'accepted' } })
  await expect(stopped).resolves.toEqual({ stop: 'accepted' })
  // A successful Stop retires the token it used, so Stop waits for the next one the station issues
  // — and it is issued to this browser with its lease still gone.
  await answer(lapsed('000000000000002c'))
  expect(h.c.getSnapshot().stopAvailable).toBe(true)
  // POSITIVE CONTROL: the availability really can end. The station stops issuing a token — the
  // grant revoked, or another browser in control — and the Stop is refused without leaving here.
  await answer(lapsed(null))
  expect(h.c.getSnapshot().stopAvailable).toBe(false)
  const before = h.sent.length
  await expect(h.c.stopTransmit()).rejects.toThrow('localPermissionRequired')
  expect(h.sent).toHaveLength(before)
  h.c.disconnected()
})

it.each(['timeout', 'disconnect', 'refused'])('does not retry or claim RF stopped after %s', async cause => {
  const h = client(), stopped = h.c.stopTransmit(), requestId = h.sent[h.sent.length - 1].request.requestId
  const rejected = expect(stopped).rejects.toThrow(cause === 'refused' ? 'staleContext' : 'operationUnknown')
  if (cause === 'timeout') await vi.advanceTimersByTimeAsync(7500)
  else if (cause === 'disconnect') h.c.disconnected()
  else h.c.receive({ type: 'operationResponse', requestId, error: 'staleContext' })
  await rejected
  expect(h.sent.filter(w => w.request.type === 'stopTransmit')).toHaveLength(1)
  expect(h.c.getSnapshot().stopSending).toBe(false)
  h.c.disconnected()
  await expect(h.c.stopTransmit()).rejects.toThrow('stationUnavailable')
})

it('sends Stop while a heartbeat is in flight and the shown state is stale, where a control is refused unsent', async () => {
  vi.useFakeTimers()
  let now = 1000
  const sent: any[] = [], values = new Map<string, string>()
  const data = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const c = new OperationClient(wire => sent.push(JSON.parse(wire)), true, () => now, undefined, 4,
    pendingControlStorage(() => data, 'station', async (_key, run) => run()))
  c.open()
  c.receive({ type: 'operationResponse', requestId: sent[0].request.requestId, value: state() })
  now += 1000
  await vi.advanceTimersByTimeAsync(250)
  expect(sent[sent.length - 1].request.type).toBe('heartbeat')
  now += 300
  await vi.advanceTimersByTimeAsync(250)
  expect(c.getSnapshot().fresh).toBe(false)
  // The same moment holds an ordinary control: it waits for current control and sends nothing meanwhile...
  const control = c.control({ action: 'decoder.clear', receiver: 'cw' }).catch(e => e)
  await vi.advanceTimersByTimeAsync(0)
  expect(sent.some(w => w.request.type === 'stationControl')).toBe(false)
  // ...while Stop leaves immediately, even with that control still waiting: it checks no freshness
  // window and waits for no heartbeat.
  const stopping = c.stopTransmit()
  expect(sent[sent.length - 1].request.type).toBe('stopTransmit')
  c.receive({ type: 'operationResponse', requestId: sent[sent.length - 1].request.requestId, value: { stop: 'accepted' } })
  await expect(stopping).resolves.toEqual({ stop: 'accepted' })
  // Control never became current: the control is refused unsent, and nothing reached the station.
  await vi.advanceTimersByTimeAsync(1500)
  expect(await control).toMatchObject({ message: 'notController', sent: false })
  expect(sent.some(w => w.request.type === 'stationControl')).toBe(false)
  c.disconnected()
})
