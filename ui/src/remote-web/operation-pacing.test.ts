import { afterEach, expect, it, vi } from 'vitest'
import { OperationClient } from './operation-client'
import { OperationRelay } from './operation-relay'
import type { OperationState } from './operation-protocol'
import type { PendingControl } from './control-storage'

afterEach(() => vi.useRealTimers())

// Exercise the real browser client against the real relay admission boundary.
// Only the station engine is simulated; every operator gesture is sent once.
function connected(pendingMutation = false) {
  vi.useFakeTimers({ now: 1000 })
  const relay = new OperationRelay(), sessionId = crypto.randomUUID()
  const errors: string[] = [], forwarded: { at: number; type: string }[] = []
  const state: OperationState = {
    stationBootId: crypto.randomUUID(), allowed: true, phase: 'available', leaseId: null,
    revision: 1, commandWindowId: null, nextSequence: null, leaseRemainingMs: null,
    actions: [], txArmed: false,
    controls: { context: { radioId: 1, radioConnection: 1, ampConnection: 1, ampReadSequence: 1 }, capabilities: ['ftOperate'] }
  }
  let stored: PendingControl | null = null
  const client = new OperationClient(wire => relay.receiveBrowser(sessionId, JSON.parse(wire), Date.now()), true,
    Date.now, undefined, 4, { read: () => stored, write: value => { stored = value }, exclusive: async run => run() })
  relay.sync({ supported: true, operationVersion: 4, peer: {
    close: () => { throw Error('station closed') },
    send: raw => {
      const { request } = JSON.parse(raw)
      forwarded.push({ at: Date.now(), type: request.type })
      if (request.type === 'acquire') Object.assign(state, { phase: 'controlling', leaseId: crypto.randomUUID(),
        commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, transmitEpoch: 'a'.repeat(16) })
      if (request.type === 'release') Object.assign(state, { phase: 'available', leaseId: null,
        commandWindowId: null, nextSequence: null, leaseRemainingMs: null, transmitEpoch: null })
      if (request.type === 'stationControl') { state.nextSequence!++; state.revision++; state.txArmed = request.action.on }
      const value = request.type === 'stationControl'
        ? pendingMutation ? { operation: 'stationControl', operationId: request.requestId, outcome: 'pending' }
          : { operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'stationState' }
        : request.type === 'stopTransmit' ? { stop: 'accepted' } : structuredClone(state)
      queueMicrotask(() => relay.receiveStation({ type: 'operationResponse', sessionId, requestId: request.requestId, value }))
    }
  } }, [{ sessionId, deviceId: crypto.randomUUID(), peer: {
    close: () => { throw Error('browser closed') }, send: raw => {
      const response = JSON.parse(raw)
      if (response.error) errors.push(response.error)
      client.receive(response)
    }
  } }], Date.now())
  return { client, forwarded, errors, state, stored: () => stored }
}

it('completes successive enabled FT gestures without status polling exhausting relay capacity', async () => {
  const h = connected()
  try {
    h.client.open(); await vi.advanceTimersByTimeAsync(0)
    const acquire = h.client.acquire(); await vi.advanceTimersByTimeAsync(0); await acquire
    for (let index = 0; index < 8; index++) {
      for (let ticks = 0; ticks < 12 && (!h.client.getSnapshot().fresh || h.client.getSnapshot().busy); ticks++)
        await vi.advanceTimersByTimeAsync(250)
      expect(h.client.getSnapshot()).toMatchObject({ fresh: true, busy: false })
      const action = h.client.control({ action: 'ft.txEnabled', expectedTier: 'FT8',
        transmitEpoch: h.state.transmitEpoch!, on: index % 2 === 0 }).catch(error => error as Error)
      await vi.advanceTimersByTimeAsync(0)
      expect(await action).toMatchObject({ outcome: 'applied' })
      expect(h.stored()).toBeNull()
    }
    expect(h.errors).toEqual([])
    expect(h.forwarded.filter(entry => entry.type === 'stationControl')).toHaveLength(8)
    for (const entry of h.forwarded)
      expect(h.forwarded.filter(other => other.at <= entry.at && other.at > entry.at - 1000).length).toBeLessThanOrEqual(4)
  } finally { h.client.disconnected() }
})

it('refuses an unsent action before writing a receipt when explicit requests fill the budget', async () => {
  const h = connected()
  try {
    h.client.open(); await vi.advanceTimersByTimeAsync(0)
    for (const action of [() => h.client.acquire(), () => h.client.release(), () => h.client.acquire()]) {
      const result = action(); await vi.advanceTimersByTimeAsync(0); await result
    }
    expect(h.client.getSnapshot()).toMatchObject({ fresh: true, requestReady: false, stopAvailable: true })
    await expect(h.client.control({ action: 'ft.txEnabled', expectedTier: 'FT8', transmitEpoch: h.state.transmitEpoch!, on: true })).rejects.toThrow('remoteBusy')
    expect(h.stored()).toBeNull()
    expect(h.client.getSnapshot().controlPending).toBeNull()
    expect(h.forwarded.filter(entry => entry.type === 'stationControl')).toHaveLength(0)
    expect(h.errors).toEqual([])
    await vi.advanceTimersByTimeAsync(1000)
    expect(h.client.getSnapshot().requestReady).toBe(true)
  } finally { h.client.disconnected() }
})

it('delivers Stop through a full ordinary budget and an unresolved native action', async () => {
  const h = connected(true)
  try {
    h.client.open(); await vi.advanceTimersByTimeAsync(0)
    const acquire = h.client.acquire(); await vi.advanceTimersByTimeAsync(0); await acquire
    const action = h.client.control({ action: 'ft.txEnabled', expectedTier: 'FT8', transmitEpoch: h.state.transmitEpoch!, on: true }).catch(error => error as Error)
    await vi.advanceTimersByTimeAsync(250)
    expect(h.forwarded).toHaveLength(4)
    expect(h.client.getSnapshot()).toMatchObject({ requestReady: false, stopAvailable: true })
    const stop = h.client.stopTransmit(); await vi.advanceTimersByTimeAsync(0)
    expect(await stop).toEqual({ stop: 'accepted' })
    expect(h.forwarded[h.forwarded.length - 1]?.type).toBe('stopTransmit')
    expect(h.errors).toEqual([])
    h.client.disconnected()
    expect(await action).toMatchObject({ message: 'operationUnknown' })
  } finally { h.client.disconnected() }
})
