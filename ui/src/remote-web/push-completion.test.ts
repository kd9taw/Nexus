import { expect, it, vi } from 'vitest'
import { OperationRelay } from './operation-relay'
import type { OperationState } from './operation-protocol'

// Operation v5: a station may PUSH a settled control's outcome (`operationEvent`) with a fresh
// state. The page no longer sends station controls, so it never takes one, but the relay still
// routes what a station sends and must still refuse what the station had no right to send.

it('the relay refuses an event from a station that did not advertise v5, and a malformed one from any station', () => {
  const state: OperationState = {
    stationBootId: crypto.randomUUID(), allowed: true, phase: 'available', leaseId: null,
    revision: 1, commandWindowId: null, nextSequence: null, leaseRemainingMs: null, actions: [], txArmed: false
  }
  const operationId = crypto.randomUUID()
  const event = { type: 'operationEvent', operationId, value: { operation: 'stationControl', operationId, outcome: 'applied', evidence: 'radioReadback' }, state }
  const peer = () => ({ frames: [] as unknown[], send(s: string) { this.frames.push(JSON.parse(s)) }, close: vi.fn() })
  for (const stationVersion of [4, 5] as const) {
    const relay = new OperationRelay(), station = peer(), browser = peer(), sessionId = crypto.randomUUID()
    relay.sync({ peer: station, supported: true, operationVersion: stationVersion }, [{ peer: browser, sessionId, deviceId: crypto.randomUUID(), commandUntil: Number.MAX_SAFE_INTEGER }], 1000)
    relay.receiveStation({ sessionId, ...event })
    if (stationVersion === 5) {
      expect(station.close).not.toHaveBeenCalled()
      expect(browser.frames).toEqual([event])
    } else {
      expect(station.close).toHaveBeenCalledWith(1008, 'invalidOperation')
      expect(browser.frames).toEqual([])
    }
  }
  // Malformed - a pending "event", or an outcome for a different operation - closes the station.
  for (const bad of [
    { ...event, value: { operation: 'stationControl', operationId, outcome: 'pending' } },
    { ...event, value: { ...event.value, operationId: crypto.randomUUID() } },
    { ...event, state: { ...state, revision: -1 } },
    { ...event, extra: true }
  ]) {
    const relay = new OperationRelay(), station = peer(), browser = peer(), sessionId = crypto.randomUUID()
    relay.sync({ peer: station, supported: true, operationVersion: 5 }, [{ peer: browser, sessionId, deviceId: crypto.randomUUID(), commandUntil: Number.MAX_SAFE_INTEGER }], 1000)
    relay.receiveStation({ sessionId, ...bad })
    expect(station.close).toHaveBeenCalledWith(1008, 'invalidOperation')
    expect(browser.frames).toEqual([])
  }
})
