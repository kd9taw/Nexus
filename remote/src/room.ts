// One hibernating room per station. Socket attachments carry admission and bounded
// ordering/ACK state; neither D1 nor Durable Object storage receives observations.
import { DurableObject } from 'cloudflare:workers'
import { ObservationRelay } from '../../ui/src/remote-monitor/relay'
import type { BrowserIdentity, Entitlement, ObserverCheckpoint, Peer, StationAccess, StationIdentity } from '../../ui/src/remote-monitor/relay'
import type { FrameOrderState } from '../../ui/src/remote-monitor/protocol'
import { ageFrame, MAX_FRAME_BYTES, parseFrame, STALE_MS } from '../../ui/src/remote-monitor/protocol'
import { Refusal } from './authority'
import type { RemoteEnv } from './authority'

type Saved = { access: StationAccess; order: FrameOrderState }
type Sample = { requestId: string; at: number }
type StationAttachment = { version: 1; role: 'station'; identity: StationIdentity; order: FrameOrderState; sample: Sample | null }
type BrowserAttachment = Omit<ObserverCheckpoint, 'peer'> & { version: 1; role: 'browser' }
type Attachment = StationAttachment | BrowserAttachment
type Admission = { access: StationAccess; identity: StationIdentity & BrowserIdentity; entitlement: Entitlement; sessionId: string }

export class StationRoom extends DurableObject<RemoteEnv> {
  private relay: ObservationRelay | null = null
  private peers = new Map<WebSocket, Peer>()
  private samples = new Map<WebSocket, Sample>()
  private saved: Saved | undefined
  private alarmAt: number | null = null

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
        for (const ws of sockets) {
          const attachment = ws.deserializeAttachment() as Attachment
          if (attachment?.version !== 1) throw new Error('invalidCheckpoint')
          if (attachment.role === 'station' && attachment.sample) this.samples.set(ws, attachment.sample)
          const peer = this.peer(ws, attachment.role)
          if (attachment.role === 'station') {
            if (station) throw new Error('invalidCheckpoint')
            station = { peer, identity: attachment.identity }; order = attachment.order
          } else if (attachment.role === 'browser') observers.push({ ...attachment, peer })
          else throw new Error('invalidCheckpoint')
        }
        this.relay = ObservationRelay.restore(this.saved.access, order, station, observers, Date.now())
        await this.checkpoint()
      } catch {
        for (const ws of sockets) ws.close(1011, 'authorityUnavailable')
        this.peers.clear()
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
      try {
        if (path === '/station') relay.connectStation(input.identity, peer, now)
        else relay.connectObserver(input.sessionId, input.identity, input.entitlement, peer, now)
      } catch (error) { this.peers.delete(server); this.samples.delete(server); throw error }
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
    await this.checkpoint()
  }
  async alarm(): Promise<void> {
    this.alarmAt = null
    this.relay?.expire(Date.now())
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
    const state = this.relay.checkpoint()
    for (const [ws, peer] of this.peers) {
      let attachment: Attachment | undefined
      if (state.station?.peer === peer) attachment = { version: 1, role: 'station', identity: state.station.identity, order: state.order, sample: this.samples.get(ws) ?? null }
      else {
        const observer = state.observers.find(o => o.peer === peer)
        if (observer) { const { peer: _peer, ...saved } = observer; attachment = { version: 1, role: 'browser', ...saved } }
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
    const next = deadlines.length ? Math.min(...deadlines) : null
    // An earlier alarm can simply reschedule itself; never rewrite an alarm for
    // each 500 ms publication/ACK. Idle rooms have no periodic timer.
    if (next === null && this.alarmAt !== null) { await this.ctx.storage.deleteAlarm(); this.alarmAt = null }
    else if (next !== null && (this.alarmAt === null || next < this.alarmAt)) {
      this.alarmAt = Math.max(Date.now() + 1, next)
      await this.ctx.storage.setAlarm(this.alarmAt)
    }
  }
}
