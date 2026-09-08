// Transport-independent relay core. Only the service's authenticated adapter may
// supply identities/access records; these are never trusted WebSocket payloads.
// No credentials, hardware commands, QSO writes or persistent telemetry live here.
import { ageFrame, FrameOrder, MAX_FRAME_BYTES, parseFrame, STALE_MS } from './protocol'
import type { FrameOrderState, MonitorFrame } from './protocol'

export const MAX_OBSERVERS = 4
export const MAX_TRUSTED_DEVICES = 8
export const OBSERVER_LEASE_MS = 60000
export const ACK_TIMEOUT_MS = 5000

export type StationAccess = {
  stationId: string; accountId: string; enabled: boolean
  policyVersion: number; stationGeneration: number
  devices: ReadonlyArray<{ id: string; generation: number; approved: boolean }>
}
export type Identity = { accountId: string; expiresAt: number }
export type Entitlement = { accountId: string; expiresAt: number; enabled: boolean }
export type StationIdentity = Identity & { stationId: string; generation: number }
export type BrowserIdentity = Identity & { deviceId: string; deviceGeneration: number }
export type Peer = { send: (message: string) => void; close: (code: number, reason: string) => void }
type Observer = { peer: Peer; identity: BrowserIdentity; until: number
  sent: { epoch: string; sequence: number; at: number } | null; awaiting: boolean }
export type StationCheckpoint = { peer: Peer; identity: StationIdentity }
export type ObserverCheckpoint = Observer & { sessionId: string }

function validTime(value: number): boolean { return Number.isSafeInteger(value) && value >= 0 }
function browserAllowed(access: StationAccess, identity: BrowserIdentity, now: number): boolean {
  return access.enabled && identity.accountId === access.accountId && validTime(identity.expiresAt) && identity.expiresAt > now &&
    access.devices.some(d => d.id === identity.deviceId && d.approved && d.generation === identity.deviceGeneration)
}

export function observerDeadline(access: StationAccess, identity: BrowserIdentity, entitlement: Entitlement, now: number): number {
  if (!browserAllowed(access, identity, now)) throw new Error('deviceNotApproved')
  if (!entitlement.enabled || entitlement.accountId !== identity.accountId || !validTime(entitlement.expiresAt) || entitlement.expiresAt <= now) {
    throw new Error('serviceAccessExpired')
  }
  return Math.min(identity.expiresAt, entitlement.expiresAt, now + OBSERVER_LEASE_MS)
}

function copyAccess(access: StationAccess): StationAccess {
  if (access.devices.length > MAX_TRUSTED_DEVICES || new Set(access.devices.map(d => d.id)).size !== access.devices.length ||
    !Number.isSafeInteger(access.policyVersion) || access.policyVersion < 1 ||
    !Number.isSafeInteger(access.stationGeneration) || access.stationGeneration < 1) throw new Error('invalidStationAccess')
  return { ...access, devices: access.devices.map(d => ({ ...d })) }
}

/** One station, one latest publication, and at most one unacknowledged frame per
 * observer. A slow browser cannot accumulate a queue or slow the radio producer.
 * A Durable Object adapter must serialize attachments and restore this state on
 * hibernation, and schedule expire() for nextDeadline(); this core owns no timer. */
export class ObservationRelay {
  private access: StationAccess
  private station: { peer: Peer; identity: StationIdentity } | null = null
  private observers = new Map<string, Observer>()
  private order = new FrameOrder()
  private latest: { frame: MonitorFrame; at: number } | null = null

  constructor(access: StationAccess) { this.access = copyAccess(access) }

  /** Attachments are written by the trusted adapter. Dropping the latest frame on
   * wake is deliberate: a new publication must arrive before any display resumes. */
  static restore(access: StationAccess, order: FrameOrderState, station: StationCheckpoint | null,
    observers: ObserverCheckpoint[], now: number): ObservationRelay {
    const relay = new ObservationRelay(access)
    relay.order = new FrameOrder(order)
    if (station) {
      if (station.identity.accountId !== access.accountId || station.identity.stationId !== access.stationId) throw new Error('invalidCheckpoint')
      relay.station = station
    }
    if (observers.length > MAX_OBSERVERS || new Set(observers.map(o => o.sessionId)).size !== observers.length) throw new Error('invalidCheckpoint')
    for (const observer of observers) {
      if (!station || !validTime(observer.until) || observer.until > observer.identity.expiresAt) throw new Error('invalidCheckpoint')
      relay.observers.set(observer.sessionId, observer)
    }
    relay.expire(now)
    return relay
  }

  checkpoint(): { access: StationAccess; order: FrameOrderState; station: StationCheckpoint | null; observers: ObserverCheckpoint[] } {
    return { access: copyAccess(this.access), order: this.order.checkpoint(),
      station: this.station ? { ...this.station, identity: { ...this.station.identity } } : null,
      observers: [...this.observers].map(([sessionId, o]) => ({ ...o, sessionId, identity: { ...o.identity }, sent: o.sent ? { ...o.sent } : null })) }
  }

  connectStation(identity: StationIdentity, peer: Peer, now: number): void {
    if (!this.access.enabled || identity.accountId !== this.access.accountId || identity.stationId !== this.access.stationId ||
      identity.generation !== this.access.stationGeneration || !validTime(identity.expiresAt) || identity.expiresAt <= now) throw new Error('stationNotApproved')
    if (this.station) this.disconnectStation(this.station.peer)
    // A newly authenticated producer may represent a new app process. Keep the
    // last sequence to reject same-process reconnect replay, but scope retired
    // epochs to that producer connection. Otherwise ordinary app restarts would
    // permanently exhaust the bounded replay history after sixteen launches.
    this.order = new FrameOrder({ last: this.order.checkpoint().last, retired: [] })
    this.station = { peer, identity: { ...identity } }
    this.latest = null
    this.demand()
  }

  connectObserver(sessionId: string, identity: BrowserIdentity, entitlement: Entitlement, peer: Peer, now: number): void {
    this.expire(now)
    const until = observerDeadline(this.access, identity, entitlement, now)
    if (!this.station) throw new Error('stationOffline')
    if (this.observers.has(sessionId)) throw new Error('sessionAlreadyConnected')
    if (this.observers.size >= MAX_OBSERVERS) throw new Error('observerLimit')
    const observer: Observer = { peer, identity: { ...identity }, until, sent: null, awaiting: false }
    this.observers.set(sessionId, observer)
    this.demand()
    this.deliver(sessionId, observer, now)
  }

  // Called only after the service revalidates identity, device approval AND
  // entitlement. A ping/ack is never a renewal of authority or a hardware read.
  renewObserver(sessionId: string, identity: BrowserIdentity, entitlement: Entitlement, now: number): void {
    this.expire(now)
    const observer = this.observers.get(sessionId)
    if (!observer || observer.identity.accountId !== identity.accountId || observer.identity.deviceId !== identity.deviceId ||
      observer.identity.deviceGeneration !== identity.deviceGeneration) throw new Error('sessionNotApproved')
    observer.until = observerDeadline(this.access, identity, entitlement, now)
    observer.identity = { ...identity }
  }

  updateAccess(next: StationAccess, now: number): void {
    if (next.stationId !== this.access.stationId || next.accountId !== this.access.accountId || next.policyVersion <= this.access.policyVersion) {
      throw new Error('obsoleteStationAccess')
    }
    this.access = copyAccess(next)
    this.expire(now)
  }

  receiveStation(peer: Peer, data: string, now: number): void {
    this.expire(now)
    if (!this.station || this.station.peer !== peer) throw new Error('stationNotConnected')
    try {
      if (typeof data !== 'string' || new TextEncoder().encode(data).length > MAX_FRAME_BYTES) throw new Error('invalidMonitorFrame')
      const frame = parseFrame(JSON.parse(data), 'native')
      if (!this.order.accept(frame)) return
      // An idle station must stop producing; there is no observer queue to fill.
      if (!this.observers.size) { this.latest = null; return }
      this.latest = { frame, at: now }
      for (const [id, observer] of this.observers) this.deliver(id, observer, now)
    } catch {
      this.disconnectStation(peer, 1008, 'invalidObservation')
    }
  }

  receiveObserver(sessionId: string, data: string, now: number): void {
    this.expire(now)
    const observer = this.observers.get(sessionId)
    if (!observer) return
    try {
      if (typeof data !== 'string' || new TextEncoder().encode(data).length > 256) throw new Error('invalidAcknowledgement')
      const ack = JSON.parse(data) as Record<string, unknown>
      if (!ack || Object.keys(ack).length !== 3 || ack.type !== 'ack' || typeof ack.epoch !== 'string' || ack.epoch.length > 64 ||
        !Number.isSafeInteger(ack.sequence) || Number(ack.sequence) < 1) throw new Error('invalidAcknowledgement')
      if (observer.sent?.epoch !== ack.epoch || observer.sent.sequence !== ack.sequence || !observer.awaiting) return
      observer.awaiting = false
      this.deliver(sessionId, observer, now)
    } catch { this.disconnectObserver(sessionId, 1008, 'invalidAcknowledgement') }
  }

  disconnectObserver(sessionId: string, code = 1000, reason = 'disconnected'): void {
    const observer = this.observers.get(sessionId)
    if (!observer) return
    this.observers.delete(sessionId)
    this.close(observer.peer, code, reason)
    if (!this.observers.size) this.latest = null
    this.demand()
  }

  disconnectStation(peer: Peer, code = 1001, reason = 'stationOffline'): void {
    if (this.station?.peer !== peer) return
    this.station = null
    this.latest = null
    this.close(peer, code, reason)
    for (const id of [...this.observers.keys()]) this.disconnectObserver(id, 1001, 'stationOffline')
  }

  expire(now: number): void {
    if (this.station && (!this.access.enabled || this.station.identity.generation !== this.access.stationGeneration || this.station.identity.expiresAt <= now)) {
      this.disconnectStation(this.station.peer, 1008, 'stationAccessEnded')
    }
    for (const [id, observer] of this.observers) {
      if (!browserAllowed(this.access, observer.identity, now) || observer.until <= now) this.disconnectObserver(id, 1008, 'browserAccessEnded')
      else if (observer.awaiting && observer.sent && now - observer.sent.at >= ACK_TIMEOUT_MS) this.disconnectObserver(id, 1008, 'observerTooSlow')
    }
  }

  nextDeadline(): number | null {
    const deadlines = [...this.observers.values()].flatMap(o => [o.until, ...(o.awaiting && o.sent ? [o.sent.at + ACK_TIMEOUT_MS] : [])])
    if (this.station) deadlines.push(this.station.identity.expiresAt)
    return deadlines.length ? Math.min(...deadlines) : null
  }

  private deliver(id: string, observer: Observer, now: number): void {
    const latest = this.latest
    if (observer.awaiting || !latest || now - latest.at >= STALE_MS ||
      (observer.sent?.epoch === latest.frame.epoch && observer.sent.sequence === latest.frame.sequence)) return
    observer.sent = { epoch: latest.frame.epoch, sequence: latest.frame.sequence, at: now }
    observer.awaiting = true
    try { observer.peer.send(JSON.stringify({ type: 'observation', sentAtMs: now, frame: ageFrame(latest.frame, now - latest.at) })) }
    catch { this.disconnectObserver(id, 1011, 'observerUnavailable') }
  }

  private demand(): void {
    if (!this.station) return
    try { this.station.peer.send(JSON.stringify({ type: 'watch', enabled: this.observers.size > 0 })) }
    catch { this.disconnectStation(this.station.peer) }
  }

  private close(peer: Peer, code: number, reason: string): void {
    try { peer.close(code, reason) } catch { /* the peer is already gone */ }
  }
}
