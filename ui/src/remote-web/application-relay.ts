// The authenticated station room supplies peers. This broker multiplexes only the
// closed application data contract, independently of observation freshness/ACKs.
// One unacknowledged result per browser bounds slow consumers; checkpoints retain
// routing/limits, never application data, credentials or operator history.
import { APPLICATION_COMMANDS, APPLICATION_ERRORS, APPLICATION_MAX_BYTES, APPLICATION_REQUEST_BYTES,
  APPLICATION_TIMEOUT_MS, applicationReply, applicationRequest } from './application-protocol'
import type { ApplicationCommand } from './application-protocol'
import type { Peer } from '../remote-monitor/relay'
import { ApplicationStreamRelay } from './application-stream-relay'
import type { StreamCheckpoint } from './application-stream-relay'
import { STREAM_TOPICS } from './application-stream-protocol'

type Pending = { requestId: string; forwardId: string; command: ApplicationCommand; at: number; delivered: boolean }
type LegacyCheckpoint = { version: 1; ready: boolean; windowAt: number; count: number; pending: Pending | null }
export type ApplicationCheckpoint = LegacyCheckpoint | StreamCheckpoint
type Browser = LegacyCheckpoint & { peer: Peer }
export class ApplicationRelay {
  private station: { peer: Peer; version: number } | null = null
  private browsers = new Map<string, Browser>()
  private stream: ApplicationStreamRelay
  constructor(private readonly id = () => crypto.randomUUID()) { this.stream = new ApplicationStreamRelay(id) }
  sync(station: { peer: Peer; version: number } | null, observers: { sessionId: string; peer: Peer }[], now: number): void {
    if (this.station?.peer !== station?.peer) {
      for (const browser of this.browsers.values()) {
        if (browser.pending) this.error(browser.peer, browser.pending.requestId, 'applicationUnavailable')
        browser.pending = null; browser.ready = false
      }
    }
    this.station = station
    for (const id of this.browsers.keys()) if (!observers.some(o => o.sessionId === id)) this.browsers.delete(id)
    for (const { sessionId, peer } of observers) if (!this.browsers.has(sessionId)) {
      this.browsers.set(sessionId, { peer, version: 1, ready: false, windowAt: now, count: 0, pending: null })
    }
    this.expire(now)
    this.stream.sync(station?.version === 2 ? station.peer : null, observers, now)
  }
  restore(sessionId: string, saved: ApplicationCheckpoint): void {
    if (saved.version === 2) { this.stream.restore(sessionId, saved); return }
    const browser = this.browsers.get(sessionId)
    if (!browser || saved.version !== 1) throw new Error('invalidApplicationCheckpoint')
    this.browsers.set(sessionId, { ...saved, peer: browser.peer })
  }
  checkpoint(sessionId: string): ApplicationCheckpoint | undefined {
    const stream = this.stream.checkpoint(sessionId)
    if (stream) return stream
    const browser = this.browsers.get(sessionId)
    if (!browser) return
    const { peer: _peer, ...saved } = browser
    return saved
  }
  receiveBrowser(sessionId: string, message: Record<string, unknown>, now: number): void {
    if (this.stream.has(sessionId)) { this.stream.receiveBrowser(sessionId, message, now); return }
    const browser = this.browsers.get(sessionId)
    if (!browser) return
    try {
      if (new TextEncoder().encode(JSON.stringify(message)).length > APPLICATION_REQUEST_BYTES) throw new Error('invalidApplicationRequest')
      if (message.type === 'applicationHello' && (Object.keys(message).length === 1 || (Object.keys(message).length === 2 && message.version === 2))) {
        if (browser.ready) throw new Error('applicationAlreadyNegotiated')
        browser.ready = true
        const version = message.version === 2 && this.station?.version === 2 ? 2 : this.station && this.station.version >= 1 ? 1 : 0
        if (version === 2) this.stream.add(sessionId, now)
        browser.peer.send(JSON.stringify({ type: 'applicationCapabilities', version,
          commands: version === 2 ? STREAM_TOPICS : version === 1 ? APPLICATION_COMMANDS : [] }))
        return
      }
      if (message.type === 'applicationAck' && Object.keys(message).length === 2 && typeof message.requestId === 'string') {
        if (browser.pending?.delivered && browser.pending.requestId === message.requestId) browser.pending = null
        return
      }
      const request = applicationRequest(message)
      if (!browser.ready || !this.station || this.station.version < 1) { this.error(browser.peer, request.requestId, 'stationUpdateRequired'); return }
      if (now - browser.windowAt >= 1000) { browser.windowAt = now; browser.count = 0 }
      if (++browser.count > 24 || browser.pending) throw new Error('applicationLimit')
      const forwardId = this.id()
      browser.pending = { requestId: request.requestId, forwardId, command: request.command, at: now, delivered: false }
      this.station.peer.send(JSON.stringify({ ...request, requestId: forwardId }))
    } catch { this.closeBrowser(browser, 1008, 'invalidApplicationRequest') }
  }
  receiveStation(message: Record<string, unknown>, now: number): void {
    if (message.type === 'applicationBatch' && this.station?.version === 2) { this.stream.receiveStation(message, now); return }
    if (!this.station) return
    try {
      if (new TextEncoder().encode(JSON.stringify(message)).length > APPLICATION_MAX_BYTES) throw new Error('applicationLimit')
      const browser = [...this.browsers.values()].find(b => b.pending?.forwardId === message.requestId)
      // A response can legitimately race revocation/timeout. It has no new owner.
      if (!browser?.pending || browser.pending.delivered) return
      const pending = browser.pending
      if (now - pending.at >= APPLICATION_TIMEOUT_MS) { this.expire(now); return }
      if (message.type === 'applicationError' && Object.keys(message).length === 3 && APPLICATION_ERRORS.includes(message.error as never)) {
        this.error(browser.peer, pending.requestId, message.error as typeof APPLICATION_ERRORS[number])
      } else {
        const reply = applicationReply(message)
        if (reply.command !== pending.command || reply.ageMs + now - pending.at >= APPLICATION_TIMEOUT_MS) throw new Error('invalidApplicationResult')
        // A consumer disappearing between authorization and delivery is not a
        // station protocol fault. Keep every other approved observer connected.
        try { browser.peer.send(JSON.stringify({ ...reply, requestId: pending.requestId })) }
        catch { this.closeBrowser(browser, 1001, 'applicationUnavailable'); return }
      }
      pending.delivered = true
    } catch { try { this.station.peer.close(1008, 'invalidApplicationResult') } catch { /* already closed */ } }
  }
  expire(now: number): void {
    this.stream.expire(now)
    for (const browser of this.browsers.values()) {
      if (browser.pending && now - browser.pending.at >= APPLICATION_TIMEOUT_MS) {
        this.closeBrowser(browser, 1008, 'applicationTimeout')
      }
    }
  }
  nextDeadline(): number | null {
    const times = [...this.browsers.values()].flatMap(b => b.pending ? [b.pending.at + APPLICATION_TIMEOUT_MS] : [])
    const streamDeadline = this.stream.nextDeadline()
    if (streamDeadline !== null) times.push(streamDeadline)
    return times.length ? Math.min(...times) : null
  }
  private closeBrowser(browser: Browser, code: number, reason: string): void {
    browser.pending = null; browser.ready = false
    try { browser.peer.close(code, reason) } catch { /* already closed */ }
  }
  private error(peer: Peer, requestId: string, error: typeof APPLICATION_ERRORS[number]): void {
    try { peer.send(JSON.stringify({ type: 'applicationError', requestId, error })) }
    catch { try { peer.close(1001, 'applicationUnavailable') } catch { /* already closed */ } }
  }
}
