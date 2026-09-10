// The cloud retains only bounded routing metadata. Native Nexus alone owns
// permission, leases, command windows, deduplication and durable outcomes.
import type { Peer } from '../remote-monitor/relay'
import { object } from './display-validation'
import { operationId, operationRequest, operationResponse } from './operation-protocol'
import { controlVersion, parseOperationVersion, type OperationVersion } from './operation-version'
type Browser = { sessionId: string; deviceId: string; peer: Peer }
type Pending = {
  requestId: string
  sessionId: string
  deviceId: string
  at: number
  mutation: boolean
  operationVersion?: OperationVersion
}
export type OperationCheckpoint = {
  version: 1
  pending: { requestId: string; at: number; mutation: boolean; operationVersion?: OperationVersion }[]
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
  private station: { peer: Peer; supported: boolean; operationVersion?: number } | null = null
  private browsers = new Map<string, Browser>()
  private pending = new Map<string, Pending>()
  private rates = new Map<string, number[]>()
  sync(station: { peer: Peer; supported: boolean; operationVersion?: number } | null, browsers: Browser[], now: number): void {
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
      const versioned = !!raw && typeof raw === 'object' && 'operationVersion' in raw
      const wire = object(raw, ['type', 'request', ...(versioned ? ['operationVersion'] : [])])
      if (wire.type !== 'operationRequest') throw Error()
      const browserVersion = versioned ? parseOperationVersion(wire.operationVersion) : 1
      if (!browserVersion || versioned && browserVersion === 1) throw Error()
      const request = operationRequest(wire.request)
      if (request.type === 'stationControl' && !versioned) throw Error()
      const p = {
        requestId: request.requestId,
        sessionId,
        deviceId: browser.deviceId,
        at: now,
        mutation: request.type === 'logManual' || request.type === 'stationControl'
      }
      if (!this.station?.supported) {
        this.error(p, 'stationUnsupported')
        return
      }
      const stationVersion = parseOperationVersion(this.station.operationVersion) ?? 1
      const version = Math.min(browserVersion, stationVersion) as OperationVersion
      if (request.type === 'stationControl' && version < controlVersion(request.action)) {
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
      this.pending.set(p.requestId, { ...p, operationVersion: version })
      if (
        !deliver(
          this.station.peer,
          JSON.stringify({
            type: 'operationRequest',
            sessionId,
            deviceId: browser.deviceId,
            ...(version >= 2 ? { operationVersion: version } : {}),
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
      if ('value' in response && 'controls' in response.value && response.value.controls) {
        // Keep capability vocabulary compatible even with pre-v3 stations that
        // advertised extra hints under v2. This only removes hints; it cannot
        // grant a command. Historical checkpoints predate v3 and default to v2.
        const version = p.operationVersion ?? 2
        if (version === 1) delete response.value.controls
        else if (version === 2) response.value.controls.capabilities = response.value.controls.capabilities.filter(c => ['decoder', 'radio', 'amplifier'].includes(c))
      }
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
        .map(({ requestId, at, mutation, operationVersion }) => ({ requestId, at, mutation, ...(operationVersion ? { operationVersion } : {}) }))
    }
  }
  restore(sessionId: string, raw: OperationCheckpoint): void {
    const b = this.browsers.get(sessionId)
    if (!b || raw.version !== 1 || !Array.isArray(raw.pending) || raw.pending.length > 2)
      throw Error('invalidOperationCheckpoint')
    for (const p of raw.pending) {
      object(p, ['requestId', 'at', 'mutation', ...('operationVersion' in p ? ['operationVersion'] : [])])
      if (
        !operationId(p.requestId) ||
        !Number.isSafeInteger(p.at) ||
        p.at < 0 ||
        typeof p.mutation !== 'boolean' ||
        ('operationVersion' in p && !parseOperationVersion(p.operationVersion)) ||
        this.pending.has(p.requestId)
      )
        throw Error('invalidOperationCheckpoint')
      this.pending.set(p.requestId, { ...p, sessionId, deviceId: b.deviceId })
    }
  }
}
