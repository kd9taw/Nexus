import { afterEach, expect, it, vi } from 'vitest'
import { OperationClient } from './operation-client'
import { OperationRelay } from './operation-relay'
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
