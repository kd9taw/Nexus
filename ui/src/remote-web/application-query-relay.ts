import type { Peer } from '../remote-monitor/relay'
import { APPLICATION_TIMEOUT_MS, APPLICATION_REQUEST_BYTES } from './application-protocol'
import { QUERY_ERRORS, QUERY_MAX_BYTES, queryPage, queryRequest } from './application-query-protocol'
import type { Collection } from './application-query-protocol'
import { streamId } from './application-stream-protocol'

type Pending = { id: string; forwardId: string; collection: Collection; at: number; delivered: boolean }
export type QueryCheckpoint = { pending: Pending | null; windowAt: number; count: number; version?: 3 | 4 | 6 | 7 }
type Browser = QueryCheckpoint & { peer: Peer }
// Only the room's already approved/entitled peers enter here. Attachments contain
// routing credits only: no query text, contacts, history, rows or credentials.
export class ApplicationQueryRelay {
  private station: Peer | null = null
  private stationVersion = 3
  private browsers = new Map<string, Browser>()
  private peers = new Map<string, Peer>()
  constructor(private readonly id: () => string) {}
  sync(station: Peer | null, observers: { sessionId: string; peer: Peer }[], now: number, version = 3): void {
    if (this.station !== station) {
      for (const b of this.browsers.values()) if (b.pending) this.close(b, 'applicationUnavailable')
    }
    this.station = station
    this.stationVersion = version
    this.peers = new Map(observers.map(o => [o.sessionId, o.peer]))
    for (const id of this.browsers.keys()) if (!this.peers.has(id)) this.browsers.delete(id)
    this.expire(now)
  }
  add(id: string, now: number, version: 3 | 4 | 6 | 7 = 3): void {
    const peer = this.peers.get(id)
    if (peer) this.browsers.set(id, { peer, pending: null, count: 0, windowAt: now, version })
  }
  checkpoint(id: string): QueryCheckpoint | undefined {
    const b = this.browsers.get(id)
    if (b) return { pending: b.pending && { ...b.pending }, count: b.count, windowAt: b.windowAt, version: b.version ?? 3 }
  }
  restore(id: string, saved: QueryCheckpoint): void {
    const peer = this.peers.get(id)
    if (!peer || (saved.version !== undefined && ![3, 4, 6, 7].includes(saved.version)) || !Number.isSafeInteger(saved.count) || saved.count < 0 || saved.count > 16 ||
      !Number.isFinite(saved.windowAt) || (saved.pending && (!streamId(saved.pending.id) || !streamId(saved.pending.forwardId) || !Number.isFinite(saved.pending.at)))) throw new Error('invalidQueryCheckpoint')
    this.browsers.set(id, { ...saved, peer })
  }
  receiveBrowser(id: string, message: Record<string, unknown>, now: number): void {
    const b = this.browsers.get(id)
    if (!b) { try { this.peers.get(id)?.close(1008, 'applicationUnsupported') } catch { /* gone */ }; return }
    try {
      if (new TextEncoder().encode(JSON.stringify(message)).length > APPLICATION_REQUEST_BYTES) throw new Error('applicationLimit')
      if (message.type === 'applicationQueryAck' && Object.keys(message).length === 2 && streamId(message.requestId)) {
        if (b.pending?.delivered && b.pending.id === message.requestId) b.pending = null
        return
      }
      const request = queryRequest(message, Math.min(b.version ?? 3, this.stationVersion))
      if (now - b.windowAt >= 1000) { b.windowAt = now; b.count = 0 }
      if (++b.count > 16 || b.pending) throw new Error('applicationLimit')
      const pending = { id: request.requestId, forwardId: this.id(), collection: request.collection, at: now, delivered: false }
      b.pending = pending
      const count = [...this.browsers.values()].reduce((sum, peer) => sum + (now - peer.windowAt < 1000 ? peer.count : 0), 0)
      if (!this.station || count > 32) {
        pending.delivered = true
        b.peer.send(JSON.stringify({ type: 'applicationQueryError', requestId: pending.id, error: this.station ? 'applicationBusy' : 'applicationUnavailable' }))
      } else this.station.send(JSON.stringify({ ...request, requestId: pending.forwardId }))
    } catch { this.close(b, 'invalidApplicationQuery') }
  }
  receiveStation(message: Record<string, unknown>, now: number): void {
    if (!this.station) return
    try {
      if (new TextEncoder().encode(JSON.stringify(message)).length > QUERY_MAX_BYTES) throw new Error('applicationLimit')
      const b = [...this.browsers.values()].find(b => b.pending?.forwardId === message.requestId)
      if (!b?.pending || b.pending.delivered) return // revoked or timed out
      const pending = b.pending
      if (now - pending.at >= APPLICATION_TIMEOUT_MS) { this.close(b, 'applicationTimeout'); return }
      if (message.type === 'applicationQueryError') {
        if (Object.keys(message).length !== 3 || !QUERY_ERRORS.includes(message.error as never)) throw new Error('invalidApplicationPage')
      } else {
        const page = queryPage(message, Math.min(b.version ?? 3, this.stationVersion))
        if (page.collection !== pending.collection || page.ageMs + now - pending.at >= APPLICATION_TIMEOUT_MS) throw new Error('invalidApplicationPage')
      }
      try { b.peer.send(JSON.stringify({ ...message, requestId: pending.id })); pending.delivered = true }
      catch { this.close(b, 'applicationUnavailable') }
    } catch { try { this.station.close(1008, 'invalidApplicationPage') } catch { /* gone */ } }
  }
  expire(now: number): void {
    for (const b of this.browsers.values()) if (b.pending && now - b.pending.at >= APPLICATION_TIMEOUT_MS) this.close(b, 'applicationTimeout')
  }
  nextDeadline(): number | null {
    const times = [...this.browsers.values()].flatMap(b => b.pending ? [b.pending.at + APPLICATION_TIMEOUT_MS] : [])
    return times.length ? Math.min(...times) : null
  }
  private close(b: Browser, reason: string): void { b.pending = null; try { b.peer.close(1008, reason) } catch { /* gone */ } }
}
