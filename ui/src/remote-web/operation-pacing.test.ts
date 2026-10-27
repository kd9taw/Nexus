import { afterEach, expect, it, vi } from 'vitest'
import { OperationClient } from './operation-client'
import { OperationRelay } from './operation-relay'
import type { OperationState } from './operation-protocol'

afterEach(() => vi.useRealTimers())

// Exercise the real browser client against the real relay admission boundary.
// Only the station engine is simulated; every operator gesture is sent once.
function connected() {
  vi.useFakeTimers({ now: 1000 })
  const relay = new OperationRelay(), sessionId = crypto.randomUUID()
  const errors: string[] = [], forwarded: { at: number; type: string }[] = []
  const state: OperationState = {
    stationBootId: crypto.randomUUID(), allowed: true, phase: 'available', leaseId: null,
    revision: 1, commandWindowId: null, nextSequence: null, leaseRemainingMs: null,
    actions: [], txArmed: false,
    controls: { context: { radioId: 1, radioConnection: 1, ampConnection: 1, ampReadSequence: 1 }, capabilities: ['ftOperate'] }
  }
  let leaseUntil = 0
  const client = new OperationClient(wire => relay.receiveBrowser(sessionId, JSON.parse(wire), Date.now()), true,
    Date.now, 4)
  relay.sync({ supported: true, operationVersion: 4, peer: {
    close: () => { throw Error('station closed') },
    send: raw => {
      const { request } = JSON.parse(raw)
      forwarded.push({ at: Date.now(), type: request.type })
      if (state.leaseId && Date.now() >= leaseUntil) Object.assign(state, { phase: 'available', leaseId: null,
        commandWindowId: null, nextSequence: null, leaseRemainingMs: null, transmitEpoch: null, txArmed: false })
      if (request.type === 'acquire') Object.assign(state, { phase: 'controlling', leaseId: crypto.randomUUID(),
        commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, transmitEpoch: 'a'.repeat(16) })
      if (request.type === 'acquire' || request.type === 'heartbeat' && request.leaseId === state.leaseId) leaseUntil = Date.now() + 5000
      if (state.leaseId) state.leaseRemainingMs = leaseUntil - Date.now()
      if (request.type === 'release') Object.assign(state, { phase: 'available', leaseId: null,
        commandWindowId: null, nextSequence: null, leaseRemainingMs: null, transmitEpoch: null, txArmed: false })
      const value = request.type === 'stopTransmit' ? { stop: 'accepted' } : structuredClone(state)
      queueMicrotask(() => relay.receiveStation({ type: 'operationResponse', sessionId, requestId: request.requestId, value }))
    }
  } }, [{ sessionId, deviceId: crypto.randomUUID(), commandUntil: Number.MAX_SAFE_INTEGER, peer: {
    close: () => { throw Error('browser closed') }, send: raw => {
      const response = JSON.parse(raw)
      if (response.error) errors.push(response.error)
      client.receive(response)
    }
  } }], Date.now())
  return { client, forwarded, errors, state }
}

it('keeps the original lease alive on heartbeats without exhausting relay capacity', async () => {
  const h = connected()
  try {
    h.client.open(); await vi.advanceTimersByTimeAsync(0)
    const acquire = h.client.acquire(); await vi.advanceTimersByTimeAsync(0); await acquire
    const originalLease = h.state.leaseId
    await vi.advanceTimersByTimeAsync(10000)
    expect(h.client.getSnapshot()).toMatchObject({ fresh: true, state: { phase: 'controlling' } })
    expect(Date.now()).toBeGreaterThanOrEqual(11000)
    expect(h.state.leaseId).toBe(originalLease)
    expect(h.forwarded.filter(entry => entry.type === 'acquire')).toHaveLength(1)
    expect(h.forwarded.filter(entry => entry.type === 'heartbeat').length).toBeGreaterThan(1)
    expect(h.errors).toEqual([])
    for (const entry of h.forwarded)
      expect(h.forwarded.filter(other => other.at <= entry.at && other.at > entry.at - 1000).length).toBeLessThanOrEqual(4)
  } finally { h.client.disconnected() }
})

it('releases the original lease and goes back to reading the state', async () => {
  const h = connected()
  try {
    h.client.open(); await vi.advanceTimersByTimeAsync(0)
    const acquire = h.client.acquire(); await vi.advanceTimersByTimeAsync(0); await acquire
    expect(h.client.getSnapshot()).toMatchObject({ state: { phase: 'controlling' } })
    const release = h.client.release(); await vi.advanceTimersByTimeAsync(0); await release
    expect(h.forwarded[h.forwarded.length - 1].type).toBe('release')
    expect(h.state.phase).toBe('available')
    const afterRelease = h.forwarded.length
    await vi.advanceTimersByTimeAsync(6000)
    expect(h.forwarded.slice(afterRelease).every(entry => entry.type === 'state')).toBe(true)
    expect(h.client.getSnapshot().state?.phase).toBe('available')
  } finally { h.client.disconnected() }
})

it('delivers Stop through a full ordinary budget', async () => {
  const h = connected()
  try {
    h.client.open(); await vi.advanceTimersByTimeAsync(0)
    for (const action of [() => h.client.acquire(), () => h.client.release(), () => h.client.acquire()]) {
      const result = action(); await vi.advanceTimersByTimeAsync(0); await result
    }
    expect(h.client.getSnapshot()).toMatchObject({ requestReady: false, stopAvailable: true })
    const stop = h.client.stopTransmit(); await vi.advanceTimersByTimeAsync(0)
    expect(await stop).toEqual({ stop: 'accepted' })
    expect(h.forwarded[h.forwarded.length - 1]?.type).toBe('stopTransmit')
    expect(h.errors).toEqual([])
  } finally { h.client.disconnected() }
})
