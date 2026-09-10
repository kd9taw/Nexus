// One hibernating room per station. Socket attachments carry admission and bounded
// ordering/ACK state; neither D1 nor Durable Object storage receives observations.
import { DurableObject } from 'cloudflare:workers'
import { ObservationRelay } from '../../ui/src/remote-monitor/relay'
import type { BrowserIdentity, Entitlement, ObserverCheckpoint, Peer, StationAccess, StationIdentity } from '../../ui/src/remote-monitor/relay'
import type { FrameOrderState } from '../../ui/src/remote-monitor/protocol'
import { ageFrame, MAX_FRAME_BYTES, parseFrame, STALE_MS } from '../../ui/src/remote-monitor/protocol'
import { Refusal } from './authority'
import type { RemoteEnv } from './authority'
import { ApplicationRelay } from '../../ui/src/remote-web/application-relay'
import type { ApplicationCheckpoint } from '../../ui/src/remote-web/application-relay'
import { APPLICATION_MAX_BYTES, APPLICATION_REQUEST_BYTES } from '../../ui/src/remote-web/application-protocol'

type Saved = { access: StationAccess; order: FrameOrderState }
type Sample = { requestId: string; at: number }
type StationAttachment = { version: 1; role: 'station'; identity: StationIdentity; order: FrameOrderState; sample: Sample | null; applicationVersion?: number }
type BrowserAttachment = Omit<ObserverCheckpoint, 'peer'> & { version: 1; role: 'browser'; application?: ApplicationCheckpoint }
type Attachment = StationAttachment | BrowserAttachment
type Admission = { access: StationAccess; identity: StationIdentity & BrowserIdentity; entitlement: Entitlement; sessionId: string; applicationVersion?: number }

export class StationRoom extends DurableObject<RemoteEnv> {
  private relay: ObservationRelay | null = null
  private peers = new Map<WebSocket, Peer>()
  private samples = new Map<WebSocket, Sample>()
  private saved: Saved | undefined
  private alarmAt: number | null = null
  private application = new ApplicationRelay()
  private applicationVersions = new Map<WebSocket, number>()
  private applicationPeers = new Map<WebSocket, Peer>()

  constructor(ctx: DurableObjectState, env: RemoteEnv) {
    super(ctx, env)
    ctx.setWebSocketAutoResponse(new WebSocketRequestResponsePair('ping', 'pong'))
    ctx.blockConcurrencyWhile(async () => {
      this.saved = await ctx.storage.get<Saved>('authority')
      this.alarmAt = await ctx.storage.getAlarm()
      const sockets = ctx.getWebSockets()
      if (!this.saved) { for (const ws of sockets) ws.close(1011, 'authorityUnavailable'); return }
      try {
        let station: { peer: Peer; identity: StationIdentity } | null = null
        let order = this.saved.order
        const observers: ObserverCheckpoint[] = []
        const applicationSaved: { sessionId: string; value: ApplicationCheckpoint }[] = []
        for (const ws of sockets) {
          const attachment = ws.deserializeAttachment() as Attachment
          if (attachment?.version !== 1) throw new Error('invalidCheckpoint')
          if (attachment.role === 'station' && attachment.sample) this.samples.set(ws, attachment.sample)
          const peer = this.peer(ws, attachment.role)
          if (attachment.role === 'station') {
            if (station) throw new Error('invalidCheckpoint')
            station = { peer, identity: attachment.identity }; order = attachment.order
            this.applicationVersions.set(ws, [1, 2, 3, 4, 5, 6, 7, 8, 9].includes(attachment.applicationVersion ?? 0) ? attachment.applicationVersion! : 0)
          } else if (attachment.role === 'browser') {
            observers.push({ ...attachment, peer })
            if (attachment.application) applicationSaved.push({ sessionId: attachment.sessionId, value: attachment.application })
          }
          else throw new Error('invalidCheckpoint')
        }
        this.relay = ObservationRelay.restore(this.saved.access, order, station, observers, Date.now())
        this.syncApplication(Date.now())
        for (const saved of applicationSaved) {
          if (this.application.checkpoint(saved.sessionId)) this.application.restore(saved.sessionId, saved.value)
        }
        await this.checkpoint()
      } catch {
        for (const ws of sockets) ws.close(1011, 'authorityUnavailable')
        this.peers.clear()
        this.samples.clear(); this.applicationPeers.clear(); this.applicationVersions.clear()
        this.application = new ApplicationRelay()
        this.relay = new ObservationRelay(this.saved.access)
      }
    })
  }

  private peer(ws: WebSocket, role: Attachment['role'], buffer?: { pending: boolean; messages: string[] }): Peer {
    let peer = this.peers.get(ws)
    if (!peer) {
      peer = { send: value => {
        if (role === 'station') {
          const watch = JSON.parse(value) as { enabled: boolean }
          // An issued sample may already be on the wire when the last observer
          // leaves. Finish that bounded request before sending idle demand;
          // cancelling it here would reject a legitimate late publication.
          if (this.samples.has(ws)) return
          if (watch.enabled) {
            const sample = { requestId: crypto.randomUUID(), at: Date.now() }
            this.samples.set(ws, sample)
            value = JSON.stringify({ type: 'watch', enabled: true, requestId: sample.requestId })
          }
        }
        if (buffer?.pending) buffer.messages.push(value)
        else ws.send(value)
      }, close: (code, reason) => {
        if (!buffer?.pending) ws.close(code, reason)
        this.peers.delete(ws); this.samples.delete(ws)
        this.applicationVersions.delete(ws); this.applicationPeers.delete(ws)
      } }
      this.peers.set(ws, peer)
    }
    return peer
  }

  private async policy(access: StationAccess): Promise<void> {
    if (!this.relay) this.relay = new ObservationRelay(access)
    else {
      const current = this.relay.checkpoint().access
      if (current.stationId !== access.stationId || current.accountId !== access.accountId || current.policyVersion > access.policyVersion) throw new Refusal('obsoleteStationAccess')
      if (current.policyVersion < access.policyVersion) this.relay.updateAccess(access, Date.now())
    }
    if (this.saved?.access.policyVersion !== access.policyVersion) {
      this.saved = { access, order: this.relay.checkpoint().order }
      await this.ctx.storage.put('authority', this.saved)
    }
  }

  // Bounded service diagnostics through the private Durable Object binding.
  // No public route, identities, credentials or observation contents are returned.
  status(): { enabled: boolean; online: boolean; observers: number; sockets: number; runtimeSockets: number } {
    const state = this.relay?.checkpoint()
    return { enabled: state?.access.enabled ?? false, online: !!state?.station,
      observers: state?.observers.length ?? 0, sockets: this.peers.size, runtimeSockets: this.ctx.getWebSockets().length }
  }

  async fetch(request: Request): Promise<Response> {
    // This class has no public route or user-controlled proxy. Only index.ts calls it.
    try {
      const input = request.method === 'GET' ? JSON.parse(request.headers.get('x-nexus-admission') ?? '') as Admission : await request.json<Admission>()
      await this.policy(input.access)
      const relay = this.relay!, path = new URL(request.url).pathname, now = Date.now()
      if (path === '/policy') { await this.checkpoint(); return new Response(null, { status: 204 }) }
      if (path === '/renew') {
        relay.renewObserver(input.sessionId, input.identity, input.entitlement, now)
        await this.checkpoint()
        return new Response(null, { status: 204 })
      }
      if (path !== '/station' && path !== '/browser') return new Response(null, { status: 404 })
      // Reserve in the bounded core before accepting a runtime socket. Failed
      // admissions do not leave accepted but untracked connections behind.
      const pair = new WebSocketPair(), server = pair[1]
      const buffered = { pending: true, messages: [] as string[] }
      const peer = this.peer(server, path === '/station' ? 'station' : 'browser', buffered)
      if (path === '/station') this.applicationVersions.set(server, [1, 2, 3, 4, 5, 6, 7, 8, 9].includes(input.applicationVersion ?? 0) ? input.applicationVersion! : 0)
      try {
        if (path === '/station') relay.connectStation(input.identity, peer, now)
        else relay.connectObserver(input.sessionId, input.identity, input.entitlement, peer, now)
      } catch (error) { this.peers.delete(server); this.samples.delete(server); this.applicationVersions.delete(server); throw error }
      this.ctx.acceptWebSocket(server)
      buffered.pending = false
      for (const message of buffered.messages) server.send(message)
      if (path === '/browser') server.send(JSON.stringify({ type: 'session', sessionId: input.sessionId }))
      await this.checkpoint()
      return new Response(null, { status: 101, webSocket: pair[0],
        headers: path === '/browser' ? { 'sec-websocket-protocol': 'nexus-observe-v1' } : undefined })
    } catch (error) {
      await this.checkpoint()
      const code = error instanceof Refusal ? error.code : 'sessionNotApproved'
      return Response.json({ error: code }, { status: 403 })
    }
  }

  async webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): Promise<void> {
    const peer = this.peers.get(ws), attachment = ws.deserializeAttachment() as Attachment
    if (!peer || !this.relay || typeof message !== 'string') {
      ws.close(1008, 'invalidMessage'); await this.disconnected(ws); return
    }
    this.syncApplication(Date.now())
    // Bounds apply before parsing. The larger envelope is available only to an
    // authenticated station that advertised this application protocol version.
    const limit = attachment.role === 'station' && [1, 2, 3, 4, 5, 6, 7, 8, 9].includes(this.applicationVersions.get(ws) ?? 0)
      ? APPLICATION_MAX_BYTES : attachment.role === 'station' ? MAX_FRAME_BYTES + 256 : APPLICATION_REQUEST_BYTES
    if (new TextEncoder().encode(message).length > limit) {
      ws.close(1008, 'invalidMessage'); await this.disconnected(ws); return
    }
    let parsed: Record<string, unknown>
    try { parsed = JSON.parse(message) as Record<string, unknown> }
    catch { ws.close(1008, 'invalidMessage'); await this.disconnected(ws); return }
    if (parsed && typeof parsed === 'object' && typeof parsed.type === 'string' && parsed.type.startsWith('application')) {
      if (attachment.role === 'station') this.application.receiveStation(parsed, Date.now())
      else this.application.receiveBrowser(attachment.sessionId, parsed, Date.now())
      await this.checkpoint(); return
    }
    if (attachment.role === 'station') {
      const sample = this.samples.get(ws), now = Date.now()
      try {
        if (new TextEncoder().encode(message).length > MAX_FRAME_BYTES + 256) throw new Error('invalidPublication')
        const publication = JSON.parse(message) as Record<string, unknown>
        if (!sample || Object.keys(publication).length !== 3 || publication.type !== 'publication' ||
          publication.requestId !== sample.requestId || now - sample.at >= STALE_MS) throw new Error('invalidPublication')
        // The complete request/response duration bounds transport residence even
        // when the shack clock is wrong. Never use a wall-clock timestamp from it.
        const frame = ageFrame(parseFrame(publication.frame, 'native'), Math.max(0, now - sample.at))
        this.samples.delete(ws)
        this.relay.receiveStation(peer, JSON.stringify(frame), now)
        const state = this.relay.checkpoint()
        if (state.station?.peer === peer) {
          peer.send(JSON.stringify({ type: 'watch', enabled: state.observers.length > 0 }))
        }
      } catch { this.relay.disconnectStation(peer, 1008, 'invalidPublication') }
    } else this.relay.receiveObserver(attachment.sessionId, message, Date.now())
    await this.checkpoint()
  }

  async webSocketClose(ws: WebSocket): Promise<void> { await this.disconnected(ws) }
  async webSocketError(ws: WebSocket): Promise<void> { await this.disconnected(ws) }
  private async disconnected(ws: WebSocket): Promise<void> {
    const peer = this.peers.get(ws), attachment = ws.deserializeAttachment() as Attachment
    if (peer && this.relay) {
      if (attachment.role === 'station') this.relay.disconnectStation(peer)
      else this.relay.disconnectObserver(attachment.sessionId)
    }
    this.peers.delete(ws)
    this.samples.delete(ws)
    this.applicationPeers.delete(ws); this.applicationVersions.delete(ws)
    await this.checkpoint()
  }
  async alarm(): Promise<void> {
    this.alarmAt = null
    this.relay?.expire(Date.now())
    this.application.expire(Date.now())
    for (const [ws, sample] of this.samples) {
      if (Date.now() - sample.at >= STALE_MS) {
        const peer = this.peers.get(ws)
        if (peer) this.relay?.disconnectStation(peer, 1008, 'stationTooSlow')
      }
    }
    await this.checkpoint()
  }

  private async checkpoint(): Promise<void> {
    if (!this.relay) return
    this.syncApplication(Date.now())
    const state = this.relay.checkpoint()
    for (const [ws, peer] of this.peers) {
      let attachment: Attachment | undefined
      if (state.station?.peer === peer) attachment = { version: 1, role: 'station', identity: state.station.identity, order: state.order, sample: this.samples.get(ws) ?? null, applicationVersion: this.applicationVersions.get(ws) ?? 0 }
      else {
        const observer = state.observers.find(o => o.peer === peer)
        if (observer) { const { peer: _peer, ...saved } = observer; attachment = { version: 1, role: 'browser', ...saved, application: this.application.checkpoint(observer.sessionId) } }
      }
      if (!attachment) { ws.close(1001, 'disconnected'); this.peers.delete(ws); continue }
      if (new TextEncoder().encode(JSON.stringify(attachment)).length > 2048) {
        ws.close(1011, 'checkpointLimit'); this.peers.delete(ws); continue
      }
      ws.serializeAttachment(attachment)
    }
    // On station teardown retain ordering for a reconnect. While streaming, the
    // runtime attachment carries it, with zero per-publication database writes.
    if (!state.station && this.saved && JSON.stringify(this.saved.order) !== JSON.stringify(state.order)) {
      this.saved = { access: state.access, order: state.order }
      await this.ctx.storage.put('authority', this.saved)
    }
    const deadlines = [...this.samples.values()].map(sample => sample.at + STALE_MS)
    const coreDeadline = this.relay.nextDeadline()
    if (coreDeadline !== null) deadlines.push(coreDeadline)
    const applicationDeadline = this.application.nextDeadline()
    if (applicationDeadline !== null) deadlines.push(applicationDeadline)
    const next = deadlines.length ? Math.min(...deadlines) : null
    // An earlier alarm can simply reschedule itself; never rewrite an alarm for
    // each 500 ms publication/ACK. Idle rooms have no periodic timer.
    if (next === null && this.alarmAt !== null) { await this.ctx.storage.deleteAlarm(); this.alarmAt = null }
    else if (next !== null && (this.alarmAt === null || next < this.alarmAt)) {
      this.alarmAt = Math.max(Date.now() + 1, next)
      await this.ctx.storage.setAlarm(this.alarmAt)
    }
  }
  private syncApplication(now: number): void {
    this.relay?.expire(now)
    const state = this.relay?.checkpoint()
    const stationSocket = [...this.peers].find(([, peer]) => peer === state?.station?.peer)?.[0]
    let station: { peer: Peer; version: number } | null = null
    if (stationSocket && state?.station) {
      let peer = this.applicationPeers.get(stationSocket)
      if (!peer) {
        const original = state.station.peer
        peer = { send: message => stationSocket.send(message), close: (code, reason) => this.relay?.disconnectStation(original, code, reason) }
        this.applicationPeers.set(stationSocket, peer)
      }
      station = { peer, version: this.applicationVersions.get(stationSocket) ?? 0 }
    }
    // Application refusals/timeouts must leave the authoritative observer set
    // immediately, rather than waiting for the runtime's socket-close callback.
    const observers = (state?.observers ?? []).flatMap(observer => {
      const socket = [...this.peers].find(([, peer]) => peer === observer.peer)?.[0]
      if (!socket) return []
      let peer = this.applicationPeers.get(socket)
      if (!peer) {
        peer = { send: message => observer.peer.send(message),
          close: (code, reason) => this.relay?.disconnectObserver(observer.sessionId, code, reason) }
        this.applicationPeers.set(socket, peer)
      }
      return [{ sessionId: observer.sessionId, peer }]
    })
    this.application.sync(station, observers, now)
  }
}
