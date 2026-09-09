import { APPLICATION_MAX_BYTES, APPLICATION_REQUEST_BYTES, APPLICATION_TIMEOUT_MS, applyApplicationReply } from './application-protocol'
import type { ApplicationValue } from './application-protocol'
import { STREAM_TOPICS, streamExact, streamId, streamTopics, streamUpdates } from './application-stream-protocol'
import type { StreamTopic, StreamUpdate, StreamError } from './application-stream-protocol'
import type { Peer } from '../remote-monitor/relay'

type Credit = { id: string; at: number }
type Offered = { serial: number; revision?: number }
export type StreamCheckpoint = { version: 2; topics: StreamTopic[]; credit: Credit | null; pending: Credit | null; windowAt: number; count: number }
type Browser = StreamCheckpoint & { peer: Peer; revisions: Map<StreamTopic, number>; seen: Map<StreamTopic, number>; offered: Map<StreamTopic, Offered> }
type Cached = { value?: ApplicationValue; update: StreamUpdate; at: number; serial: number }
type Watch = { id: string; topics: StreamTopic[]; credit: Credit }

export class ApplicationStreamRelay {
  private station: Peer | null = null
  private peers = new Map<string, Peer>()
  private browsers = new Map<string, Browser>()
  private watch: Watch | null = null
  private cache = new Map<StreamTopic, Cached>()
  private serial = 0
  constructor(private readonly id = () => crypto.randomUUID()) {}
  has(sessionId: string): boolean { return this.browsers.has(sessionId) }
  sync(station: Peer | null, peers: { sessionId: string; peer: Peer }[], now: number): void {
    if (this.station !== station) {
      for (const browser of [...this.browsers.values()]) this.closeBrowser(browser, 'applicationUnavailable')
      this.watch = null; this.cache.clear()
    }
    this.station = station
    this.peers = new Map(peers.map(p => [p.sessionId, p.peer]))
    for (const key of this.browsers.keys()) if (!this.peers.has(key)) this.browsers.delete(key)
    this.expire(now)
    this.refresh(now)
  }
  add(sessionId: string, now: number): void {
    const peer = this.peers.get(sessionId)
    if (!peer || !this.station || this.has(sessionId)) throw new Error('invalidApplicationSession')
    this.browsers.set(sessionId, { version: 2, peer, topics: [], credit: null, pending: null, windowAt: now, count: 0,
      revisions: new Map(), seen: new Map(), offered: new Map() })
  }
  restore(sessionId: string, saved: StreamCheckpoint): void {
    const peer = this.peers.get(sessionId)
    if (!peer || !this.station || saved.version !== 2) throw new Error('invalidApplicationCheckpoint')
    streamTopics(saved.topics)
    for (const credit of [saved.credit, saved.pending]) if (credit && (!streamId(credit.id) || !Number.isFinite(credit.at))) throw new Error('invalidApplicationCheckpoint')
    if (saved.credit && saved.pending || !Number.isFinite(saved.windowAt) || !Number.isInteger(saved.count) || saved.count < 0 || saved.count > 32) throw new Error('invalidApplicationCheckpoint')
    // No sampled values or delta bases survive hibernation. The next watch asks
    // for full samples; an in-flight browser ACK may finish but grants no base.
    this.browsers.set(sessionId, { ...saved, peer, revisions: new Map(), seen: new Map(), offered: new Map() })
  }
  checkpoint(sessionId: string): StreamCheckpoint | undefined {
    const b = this.browsers.get(sessionId)
    if (b) return { version: 2, topics: b.topics, credit: b.credit, pending: b.pending, windowAt: b.windowAt, count: b.count }
  }
  receiveBrowser(sessionId: string, message: Record<string, unknown>, now: number): void {
    const b = this.browsers.get(sessionId)
    if (!b) return
    try {
      if (new TextEncoder().encode(JSON.stringify(message)).length > APPLICATION_REQUEST_BYTES) throw new Error('applicationLimit')
      if (now - b.windowAt >= 1000) { b.windowAt = now; b.count = 0 }
      if (++b.count > 32) throw new Error('applicationLimit')
      if (message.type === 'applicationSubscribe') {
        streamExact(message, ['type', 'topics', 'requestId'])
        const topics = streamTopics(message.topics)
        if (topics.length ? !streamId(message.requestId) : message.requestId !== null) throw new Error('invalidApplicationCredit')
        if (topics.length) {
          const existing = b.credit ?? b.pending
          if (existing && message.requestId !== existing.id) throw new Error('invalidApplicationCredit')
          if (!existing) b.credit = { id: message.requestId as string, at: now }
        } else { b.credit = null; b.pending = null; b.offered.clear() }
        b.topics = topics
        for (const topic of b.seen.keys()) if (!topics.includes(topic)) { b.seen.delete(topic); b.revisions.delete(topic) }
        this.refresh(now)
      } else {
        streamExact(message, ['type', 'requestId', 'nextRequestId'])
        if (message.type !== 'applicationFrameAck' || !streamId(message.requestId) || !streamId(message.nextRequestId) || message.requestId === message.nextRequestId) throw new Error('invalidApplicationAck')
        // An unsubscribe can race an already sent frame ACK. It cannot restart demand.
        if (!b.topics.length && !b.pending) return
        if (b.pending?.id !== message.requestId || now - b.pending.at >= APPLICATION_TIMEOUT_MS) throw new Error('invalidApplicationAck')
        for (const [topic, offered] of b.offered) {
          b.seen.set(topic, offered.serial)
          if (offered.revision !== undefined) b.revisions.set(topic, offered.revision)
          else b.revisions.delete(topic)
        }
        b.offered.clear(); b.pending = null
        b.credit = { id: message.nextRequestId, at: now }
      }
      this.deliver(b, now)
    } catch { this.closeBrowser(b, 'invalidApplicationRequest'); this.refresh(now) }
  }
  receiveStation(message: Record<string, unknown>, now: number): void {
    if (!this.station) return
    try {
      streamExact(message, ['type', 'watchId', 'requestId', 'updates'])
      if (message.type !== 'applicationBatch' || !streamId(message.watchId) || !streamId(message.requestId) ||
        new TextEncoder().encode(JSON.stringify(message)).length > APPLICATION_MAX_BYTES) throw new Error('invalidApplicationBatch')
      // Revocation, a topic change or hibernation retires the old watch epoch.
      if (!this.watch || message.watchId !== this.watch.id) return
      if (message.requestId !== this.watch.credit.id) throw new Error('invalidApplicationCredit')
      const elapsed = now - this.watch.credit.at
      if (elapsed < 0 || elapsed >= APPLICATION_TIMEOUT_MS) throw new Error('expiredApplicationBatch')
      const updates = streamUpdates(message.updates, message.requestId)
      const next = updates.map(update => {
        if (!this.watch!.topics.includes(update.command)) throw new Error('unrequestedApplicationTopic')
        const old = this.cache.get(update.command)
        if (update.type === 'applicationError') return { update, value: old?.value, at: now }
        const age = elapsed + update.ageMs
        if (age >= APPLICATION_TIMEOUT_MS) throw new Error('expiredApplicationSample')
        return { update, value: applyApplicationReply(old?.value ?? null, update), at: now - age }
      })
      for (const entry of next) this.cache.set(entry.update.command, { ...entry, serial: ++this.serial })
      const previousRequestId = this.watch.credit.id
      this.watch.credit = { id: this.id(), at: now }
      // Isolate failed consumers. Only invalid native data closes the station.
      for (const b of [...this.browsers.values()]) this.deliver(b, now)
      this.station.send(JSON.stringify({ type: 'applicationCredit', watchId: this.watch.id, previousRequestId, requestId: this.watch.credit.id }))
    } catch { try { this.station.close(1008, 'invalidApplicationResult') } catch { /* already closed */ } }
  }
  expire(now: number): void {
    for (const b of [...this.browsers.values()]) {
      const pending = b.credit ?? b.pending
      if (pending && now - pending.at >= APPLICATION_TIMEOUT_MS) this.closeBrowser(b, 'applicationTimeout')
    }
    if (this.watch && now - this.watch.credit.at >= APPLICATION_TIMEOUT_MS) {
      try { this.station?.close(1008, 'applicationTimeout') } catch { /* already closed */ }
      this.watch = null; this.cache.clear()
    }
  }
  nextDeadline(): number | null {
    const times = [...this.browsers.values()].flatMap(b => (b.credit ?? b.pending) ? [(b.credit ?? b.pending)!.at + APPLICATION_TIMEOUT_MS] : [])
    if (this.watch) times.push(this.watch.credit.at + APPLICATION_TIMEOUT_MS)
    return times.length ? Math.min(...times) : null
  }
  private refresh(now: number): void {
    if (!this.station) return
    const topics = STREAM_TOPICS.filter(topic => [...this.browsers.values()].some(b => b.topics.includes(topic)))
    if (JSON.stringify(topics) === JSON.stringify(this.watch?.topics ?? [])) return
    const id = this.id(), credit = { id: this.id(), at: now }
    this.watch = topics.length ? { id, topics, credit } : null
    this.cache.clear()
    for (const b of this.browsers.values()) { b.seen.clear(); b.revisions.clear() }
    try { this.station.send(JSON.stringify({ type: 'applicationWatch', watchId: id, topics, requestId: topics.length ? credit.id : null })) }
    catch { try { this.station.close(1001, 'applicationUnavailable') } catch { /* already closed */ } }
  }
  private deliver(b: Browser, now: number): void {
    if (!b.credit || b.pending) return
    const updates: StreamUpdate[] = []
    let bytes = 256
    for (const topic of b.topics) {
      const entry = this.cache.get(topic)
      if (!entry || entry.serial === b.seen.get(topic)) continue
      let update: StreamUpdate
      if (entry.update.type === 'applicationError') update = { ...entry.update, requestId: b.credit.id }
      else {
        const age = now - entry.at
        if (age < 0 || age >= APPLICATION_TIMEOUT_MS) continue
        const sample = entry.update
        update = sample.baseRevision !== null && b.revisions.get(topic) === sample.baseRevision
          ? { ...sample, requestId: b.credit.id, ageMs: Math.ceil(age) }
          : { ...sample, requestId: b.credit.id, baseRevision: null, data: entry.value!.value, removed: [], ageMs: Math.ceil(age) }
      }
      if (bytes + new TextEncoder().encode(JSON.stringify(update)).length > APPLICATION_MAX_BYTES - 1024) {
        update = { type: 'applicationError', requestId: b.credit.id, command: topic, error: 'applicationTooLarge' } satisfies StreamError
      }
      bytes += new TextEncoder().encode(JSON.stringify(update)).length
      updates.push(update)
      b.offered.set(topic, { serial: entry.serial, revision: update.type === 'applicationResult' ? update.revision : undefined })
    }
    if (!updates.length) return
    const requestId = b.credit.id
    b.pending = b.credit; b.credit = null
    try { b.peer.send(JSON.stringify({ type: 'applicationFrame', requestId, updates })) }
    catch { this.closeBrowser(b, 'applicationUnavailable') }
  }
  private closeBrowser(b: Browser, reason: string): void {
    for (const [id, item] of this.browsers) if (item === b) this.browsers.delete(id)
    try { b.peer.close(reason === 'applicationUnavailable' ? 1001 : 1008, reason) } catch { /* already closed */ }
  }
}
