// The cloud retains only bounded routing metadata. Native Nexus alone owns
// permission, leases, command windows, deduplication and durable outcomes.
import type { Peer } from '../remote-monitor/relay'
import { object } from './display-validation'
import { operationId, operationRequest, operationResponse } from './operation-protocol'
type Browser = { sessionId: string; deviceId: string; peer: Peer }
type Pending = {
  requestId: string
  sessionId: string
  deviceId: string
  at: number
  mutation: boolean
}
export type OperationCheckpoint = {
  version: 1
  pending: { requestId: string; at: number; mutation: boolean }[]
}
// Socket close notifications can race revocation. A lost send must close its
// peer without aborting the authoritative revocation/checkpoint operation.
function deliver(peer: Peer, message: string): boolean {
  try {
    peer.send(message)
    return true
  } catch {
    try {
      peer.close(1011, 'stationUnavailable')
    } catch {}
    return false
  }
}
export class OperationRelay {
  private station: { peer: Peer; supported: boolean } | null = null
  private browsers = new Map<string, Browser>()
  private pending = new Map<string, Pending>()
  private rates = new Map<string, number[]>()
  sync(station: { peer: Peer; supported: boolean } | null, browsers: Browser[], now: number): void {
    const next = new Map(browsers.map((b) => [b.sessionId, b]))
    for (const [id] of this.browsers)
      if (!next.has(id)) {
        if (this.station?.supported && this.station.peer === station?.peer)
          deliver(this.station.peer, JSON.stringify({ type: 'operationDisconnect', sessionId: id }))
        this.rates.delete(id)
      }
    if (this.station?.peer !== station?.peer) {
      for (const p of this.pending.values())
        this.error(p, p.mutation ? 'operationUnknown' : 'stationUnavailable')
      this.pending.clear()
    }
    this.station = station
    this.browsers = next
    for (const [id, p] of this.pending) if (!next.has(p.sessionId)) this.pending.delete(id)
    this.expire(now)
  }
  private error(p: Pending, error: string) {
    const peer = this.browsers.get(p.sessionId)?.peer
    if (peer)
      deliver(peer, JSON.stringify({ type: 'operationResponse', requestId: p.requestId, error }))
  }
  receiveBrowser(sessionId: string, raw: unknown, now: number): void {
    const browser = this.browsers.get(sessionId)
    if (!browser) return
    try {
      const wire = object(raw, ['type', 'request'])
      if (wire.type !== 'operationRequest') throw Error()
      const request = operationRequest(wire.request)
      const p = {
        requestId: request.requestId,
        sessionId,
        deviceId: browser.deviceId,
        at: now,
        mutation: request.type === 'logManual'
      }
      if (!this.station?.supported) {
        this.error(p, 'stationUnsupported')
        return
      }
      const rate = (this.rates.get(sessionId) ?? []).filter((at) => now - at < 1000)
      const mine = [...this.pending.values()].filter((p) => p.sessionId === sessionId)
      if (rate.length >= 4 || mine.length >= 2 || (p.mutation && mine.some((p) => p.mutation))) {
        this.error(p, 'remoteBusy')
        return
      }
      if (this.pending.has(p.requestId)) {
        this.error(p, 'requestConflict')
        return
      }
      rate.push(now)
      this.rates.set(sessionId, rate)
      this.pending.set(p.requestId, p)
      if (
        !deliver(
          this.station.peer,
          JSON.stringify({
            type: 'operationRequest',
            sessionId,
            deviceId: browser.deviceId,
            request
          })
        )
      ) {
        this.pending.delete(p.requestId)
        this.error(p, p.mutation ? 'operationUnknown' : 'stationUnavailable')
      }
    } catch {
      browser.peer.close(1008, 'invalidOperation')
    }
  }
  receiveStation(raw: unknown): void {
    if (!this.station?.supported) {
      this.station?.peer.close(1008, 'invalidOperation')
      return
    }
    try {
      if (!raw || typeof raw !== 'object' || Array.isArray(raw)) throw Error()
      const { sessionId, ...value } = raw as Record<string, unknown>
      if (!operationId(sessionId)) throw Error()
      const response = operationResponse(value),
        p = this.pending.get(response.requestId)
      // A closed observer or expired request may leave a legitimate late reply.
      if (!p) return
      if (p.sessionId !== sessionId) throw Error()
      this.pending.delete(response.requestId)
      const peer = this.browsers.get(sessionId)?.peer
      if (peer) deliver(peer, JSON.stringify(response))
    } catch {
      this.station?.peer.close(1008, 'invalidOperation')
    }
  }
  expire(now: number) {
    for (const [id, p] of this.pending)
      if (now - p.at >= 7000) {
        this.pending.delete(id)
        this.error(p, p.mutation ? 'operationUnknown' : 'stationUnavailable')
      }
  }
  nextDeadline(): number | null {
    return this.pending.size
      ? Math.min(...[...this.pending.values()].map((p) => p.at + 7000))
      : null
  }
  checkpoint(sessionId: string): OperationCheckpoint {
    return {
      version: 1,
      pending: [...this.pending.values()]
        .filter((p) => p.sessionId === sessionId)
        .map(({ requestId, at, mutation }) => ({ requestId, at, mutation }))
    }
  }
  restore(sessionId: string, raw: OperationCheckpoint): void {
    const b = this.browsers.get(sessionId)
    if (!b || raw.version !== 1 || !Array.isArray(raw.pending) || raw.pending.length > 2)
      throw Error('invalidOperationCheckpoint')
    for (const p of raw.pending) {
      object(p, ['requestId', 'at', 'mutation'])
      if (
        !operationId(p.requestId) ||
        !Number.isSafeInteger(p.at) ||
        p.at < 0 ||
        typeof p.mutation !== 'boolean' ||
        this.pending.has(p.requestId)
      )
        throw Error('invalidOperationCheckpoint')
      this.pending.set(p.requestId, { ...p, sessionId, deviceId: b.deviceId })
    }
  }
}
