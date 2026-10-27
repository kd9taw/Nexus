// One hibernating room per station. Socket attachments carry admission and bounded
// ordering/ACK state; neither D1 nor Durable Object storage receives observations.
import { entitledCommand, OperationRelay, type OperationCheckpoint } from '../../ui/src/remote-web/operation-relay'
import { OPERATION_EXPORT_RESPONSE_BYTES, OPERATION_REQUEST_BYTES } from '../../ui/src/remote-web/operation-protocol'
import { OPERATION_ENTITLEMENT_CHECK_MS, OPERATION_ENTITLEMENT_MS } from '../../ui/src/remote-web/operation-limits'
import { parseOperationVersion } from '../../ui/src/remote-web/operation-version'
import { DurableObject } from 'cloudflare:workers'
import { ObservationRelay } from '../../ui/src/remote-monitor/relay'
import type { BrowserIdentity, Entitlement, ObserverCheckpoint, Peer, StationAccess, StationIdentity } from '../../ui/src/remote-monitor/relay'
import type { FrameOrderState } from '../../ui/src/remote-monitor/protocol'
import { ageFrame, MAX_FRAME_BYTES, parseFrame, STALE_MS } from '../../ui/src/remote-monitor/protocol'
import { lanEntitlement, readEntitlement, Refusal } from './authority'
import type { AccountEntitlement, RemoteEnv } from './authority'
import { AudioRelay } from '../../ui/src/remote-web/audio-relay'
import { STREAM_ENTITLEMENT_CHECK_MS, StreamRelay } from '../../ui/src/remote-web/stream-relay'
import { Pipes } from './pipe'
import type { PipeAttachment } from './pipe'

type Saved = { access: StationAccess; order: FrameOrderState }
type Sample = { requestId: string; at: number }
type StationAttachment = { version: 1; role: 'station'; identity: StationIdentity; order: FrameOrderState; sample: Sample | null; operationVersion?: number; audioVersion?: number; streamVersion?: number; streamLanVersion?: number; laneVersion?: number; pipeVersion?: number }
type BrowserAttachment = Omit<ObserverCheckpoint, 'peer'> & { version: 1; role: 'browser'; operations?: OperationCheckpoint; commandUntil?: number; lan?: boolean }
type Attachment = StationAttachment | BrowserAttachment
type Admission = { access: StationAccess; identity: StationIdentity & BrowserIdentity; entitlement: AccountEntitlement; sessionId: string; operationVersion?: number; audioVersion?: number; streamVersion?: number; streamLanVersion?: number; laneVersion?: number; pipeVersion?: number; pipeId?: string
  /** 'lan': a LAN-only session (index.ts, a ticket of that scope). Taken at admission and kept with the
   *  session; a renewal never changes it. */
  scope?: 'lan' }

/** When this browser's account may no longer command the station, from the entitlement the Worker
 *  has just read out of D1. Admission and renewal both reach here, so it is never staler than one
 *  renewal; a disabled or unparseable entitlement yields 0, which refuses every command. Fails
 *  CLOSED on purpose - the alternative to "no deadline means no commands" is "no deadline means
 *  all commands", which is the bug this exists to shut. */
function commandDeadline(entitlement: Entitlement, now: number): number {
  if (!entitlement?.enabled || !Number.isSafeInteger(entitlement.expiresAt) || entitlement.expiresAt <= now) return 0
  return Math.min(entitlement.expiresAt, now + OPERATION_ENTITLEMENT_MS)
}

/** What `ObservationRelay.connectStation` / `connectObserver` refuse admission with, by name. */
const ADMISSION_REFUSALS = new Set(['stationNotApproved', 'stationOffline', 'sessionAlreadyConnected', 'observerLimit', 'deviceNotApproved', 'serviceAccessExpired'])

export class StationRoom extends DurableObject<RemoteEnv> {
  private relay: ObservationRelay | null = null
  private peers = new Map<WebSocket, Peer>()
  private samples = new Map<WebSocket, Sample>()
  private saved: Saved | undefined
  private alarmAt: number | null = null
  private operations = new OperationRelay()
  // Receive audio. Runtime only: no checkpoint, no storage, no alarm. A bundle that
  // does not survive a hibernation is a gap the player conceals, which is the right
  // answer - keeping one would only deliver stale band noise late.
  private audio = new AudioRelay()
  private audioVersions = new Map<WebSocket, number>()
  // The stream's signalling lane: the audio lane's shape - runtime only, no checkpoint, no alarm.
  // A negotiation cut by a hibernation is one the page retries; only the station's advertisement
  // must survive, and it rides the station's attachment.
  private stream = new StreamRelay()
  private streamVersions = new Map<WebSocket, number>()
  // Whether the station keeps a LAN-only session on its own network (`x-nexus-stream-lan-version`). It
  // rides the station's attachment, like the versions above.
  private streamLanVersions = new Map<WebSocket, number>()
  // The LAN-only sessions, each with its stream deadline from lanEntitlement(): a session is LAN-only
  // exactly when it is a key here. The membership rides the browser's attachment; the deadline does not,
  // because a woken room re-reads before the first offer it forwards, as it does for commandUntil's
  // freshness. Such a session's commandUntil stays the account's own, so its commands stay refused.
  private lanUntil = new Map<string, number>()
  // Whether the station takes the browser's proof on the operation and audio lanes (S1-M1). It rides the
  // station's attachment, like the versions above: a station that never advertised it is never handed
  // a proof, which its parser would refuse by closing its socket.
  private laneVersions = new Map<WebSocket, number>()
  // The native client's pipes (pipe.ts), and whether the station asked for them. The advertisement
  // rides the station's attachment, like the versions above; the pipes' sockets carry their own.
  private pipes: Pipes
  private pipeVersions = new Map<WebSocket, number>()
  // Per browser session, the stream signal still on its way: an offer waits on a D1 read, and the
  // room takes the socket's next message meanwhile, so what the page sent after it waits behind it.
  // The station gets one session's signals in the order they were sent, never a candidate before
  // the offer it belongs to. Nothing to checkpoint: a hibernation drops the wait with the rest.
  private streamOrder = new Map<string, Promise<void>>()
  private operationVersions = new Map<WebSocket,number>()
  // Per observer session, the command deadline from commandDeadline(). Checkpointed with the
  // browser's attachment so a hibernation does not hand a woken room an empty one.
  private commandUntil = new Map<string, number>()
  // Whether the last reading behind that deadline found Remote access switched off by hand: what a
  // stream the relay ends for it is told. Not checkpointed; a woken room re-reads before it acts.
  private accessOff = new Map<string, boolean>()
  // When that deadline was last taken from the database, for OPERATION_ENTITLEMENT_CHECK_MS.
  // Deliberately NOT checkpointed: a woken room re-reads before the first command it forwards,
  // which is the safe direction and costs one read.
  private commandChecked = new Map<string, number>()
  // Each socket's peer as the lanes see it: the station's closes it through the relay, an
  // observer's retires its session there.
  private lanePeers = new Map<WebSocket, Peer>()

  constructor(ctx: DurableObjectState, env: RemoteEnv) {
    super(ctx, env)
    this.pipes = new Pipes(ctx, env)
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
          const attachment = ws.deserializeAttachment() as Attachment | PipeAttachment
          if (attachment?.version !== 1) throw new Error('invalidCheckpoint')
          // A pipe's end is not the relay's: it carries its own pipe (pipe.ts).
          if (attachment.role === 'pipe') { this.pipes.restore(ws, attachment); continue }
          if(attachment.role==='station')this.operationVersions.set(ws,parseOperationVersion(attachment.operationVersion)??0)
          if (attachment.role === 'station') this.audioVersions.set(ws, attachment.audioVersion === 1 ? 1 : 0)
          if (attachment.role === 'station') this.streamVersions.set(ws, attachment.streamVersion === 1 ? 1 : 0)
          if (attachment.role === 'station') this.streamLanVersions.set(ws, attachment.streamLanVersion === 1 ? 1 : 0)
          if (attachment.role === 'station') this.laneVersions.set(ws, attachment.laneVersion === 1 ? 1 : 0)
          if (attachment.role === 'station') this.pipeVersions.set(ws, attachment.pipeVersion === 1 ? 1 : 0)
          if (attachment.role === 'station' && attachment.sample) this.samples.set(ws, attachment.sample)
          const peer = this.peer(ws, attachment.role)
          if (attachment.role === 'station') {
            if (station) throw new Error('invalidCheckpoint')
            station = { peer, identity: attachment.identity }; order = attachment.order
          } else if (attachment.role === 'browser') {
            const { lan, ...observer } = attachment
            observers.push({ ...observer, peer })
            if (lan === true) this.lanUntil.set(attachment.sessionId, 0)
            // An attachment written before this field existed carries none, and gets 0: that
            // session may watch and read but not command until its next renewal, which is at
            // most thirty seconds away. Fail closed across a deploy, and heal by itself.
            this.commandUntil.set(attachment.sessionId, Number.isSafeInteger(attachment.commandUntil) ? attachment.commandUntil! : 0)
          }
          else throw new Error('invalidCheckpoint')
        }
        this.relay = ObservationRelay.restore(this.saved.access, order, station, observers, Date.now())
        this.pipes.settle()
        this.pipes.enforce(this.relay.checkpoint().access, Date.now())
        this.syncLanes(Date.now())
        for(const ws of sockets){const a=ws.deserializeAttachment() as Attachment;if(a.role==='browser'&&a.operations)this.operations.restore(a.sessionId,a.operations)}
        await this.checkpoint()
      } catch {
        for (const ws of sockets) ws.close(1011, 'authorityUnavailable')
        this.peers.clear()
        this.samples.clear(); this.lanePeers.clear()
        this.operations = new OperationRelay();this.operationVersions.clear();this.commandUntil.clear();this.commandChecked.clear();this.accessOff.clear();this.lanUntil.clear()
        this.audio = new AudioRelay(); this.audioVersions.clear()
        this.stream = new StreamRelay(); this.streamVersions.clear(); this.streamLanVersions.clear(); this.laneVersions.clear()
        this.pipes = new Pipes(ctx, env); this.pipeVersions.clear()
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
        this.lanePeers.delete(ws);this.operationVersions.delete(ws);this.audioVersions.delete(ws);this.streamVersions.delete(ws);this.streamLanVersions.delete(ws);this.laneVersions.delete(ws);this.pipeVersions.delete(ws)
      } }
      this.peers.set(ws, peer)
    }
    return peer
  }

  private async policy(access: StationAccess): Promise<void> {
    if (!this.relay) this.relay = new ObservationRelay(access)
    else {
      const current = this.relay.checkpoint().access
      // 409, not 403: the record the Worker just read lost a race with a policy bump. The next
      // attempt reads a fresh one. A 403 on the station socket turns Remote off at the shack, for
      // good, and that is reserved for a station that is actually no longer approved.
      if (current.stationId !== access.stationId || current.accountId !== access.accountId || current.policyVersion > access.policyVersion) throw new Refusal('obsoleteStationAccess', 409)
      if (current.policyVersion < access.policyVersion) this.relay.updateAccess(access, Date.now())
    }
    if (this.saved?.access.policyVersion !== access.policyVersion) {
      this.saved = { access, order: this.relay.checkpoint().order }
      await this.ctx.storage.put('authority', this.saved)
    }
    // A device revoked or approved again, or the station revoked, ends its pipes here and now.
    this.pipes.enforce(this.relay.checkpoint().access, Date.now())
  }

  // Bounded service diagnostics through the private Durable Object binding.
  // No public route, identities, credentials or observation contents are returned.
  status(): { enabled: boolean; online: boolean; observers: number; sockets: number; runtimeSockets: number; pipes: number } {
    const state = this.relay?.checkpoint()
    return { enabled: state?.access.enabled ?? false, online: !!state?.station,
      observers: state?.observers.length ?? 0, sockets: this.peers.size, runtimeSockets: this.ctx.getWebSockets().length,
      pipes: this.pipes.count() }
  }

  async fetch(request: Request): Promise<Response> {
    // This class has no public route or user-controlled proxy. Only index.ts calls it.
    try {
      const input = request.method === 'GET' ? JSON.parse(request.headers.get('x-nexus-admission') ?? '') as Admission : await request.json<Admission>()
      await this.policy(input.access)
      const relay = this.relay!, path = new URL(request.url).pathname, now = Date.now()
      if (path === '/policy') { await this.checkpoint(); return new Response(null, { status: 204 }) }
      if (path === '/renew') {
        const lan = this.lanUntil.has(input.sessionId)
        relay.renewObserver(input.sessionId, input.identity, lan ? lanEntitlement(input.entitlement, now) : input.entitlement, now)
        if (lan) this.lanUntil.set(input.sessionId, commandDeadline(lanEntitlement(input.entitlement, now), now))
        // Only after renewObserver has accepted it: a renewal the relay refuses must not
        // extend the right to command either.
        this.commandUntil.set(input.sessionId, commandDeadline(input.entitlement, now))
        this.accessOff.set(input.sessionId, input.entitlement?.state === 'disabled')
        await this.checkpoint()
        return new Response(null, { status: 204 })
      }
      if (path === '/client-pipe' || path === '/station-pipe' || path === '/pipe-renew') {
        const response = await this.pipes.fetch(path, input, relay.checkpoint().access, this.mainStation(), now)
        await this.checkpoint()
        return response
      }
      if (path !== '/station' && path !== '/browser') return new Response(null, { status: 404 })
      // Reserve in the bounded core before accepting a runtime socket. Failed
      // admissions do not leave accepted but untracked connections behind.
      const pair = new WebSocketPair(), server = pair[1]
      const buffered = { pending: true, messages: [] as string[] }
      const peer = this.peer(server, path === '/station' ? 'station' : 'browser', buffered)
      if(path==='/station')this.operationVersions.set(server,parseOperationVersion(input.operationVersion)??0)
      if (path === '/station') this.audioVersions.set(server, input.audioVersion === 1 ? 1 : 0)
      if (path === '/station') this.streamVersions.set(server, input.streamVersion === 1 ? 1 : 0)
      if (path === '/station') this.streamLanVersions.set(server, input.streamLanVersion === 1 ? 1 : 0)
      if (path === '/station') this.laneVersions.set(server, input.laneVersion === 1 ? 1 : 0)
      if (path === '/station') this.pipeVersions.set(server, input.pipeVersion === 1 ? 1 : 0)
      try {
        if (path === '/station') relay.connectStation(input.identity, peer, now)
        else {
          const lan = input.scope === 'lan'
          relay.connectObserver(input.sessionId, input.identity, lan ? lanEntitlement(input.entitlement, now) : input.entitlement, peer, now)
          if (lan) this.lanUntil.set(input.sessionId, commandDeadline(lanEntitlement(input.entitlement, now), now))
          this.commandUntil.set(input.sessionId, commandDeadline(input.entitlement, now))
          this.accessOff.set(input.sessionId, input.entitlement?.state === 'disabled')
        }
      } catch (error) {
        this.peers.delete(server); this.samples.delete(server)
        // The relay's admission refusals are decisions about this peer, and the only thing this
        // room may answer with a 403: the station turns Remote off on one, permanently, so nothing
        // that is not an approval decision - a socket the runtime would not close, say - may reach
        // it as one. Named, not "any Error": the relay refuses by code, a failure by sentence.
        throw error instanceof Error && ADMISSION_REFUSALS.has(error.message) ? new Refusal(error.message) : error
      }
      this.ctx.acceptWebSocket(server)
      buffered.pending = false
      for (const message of buffered.messages) server.send(message)
      if (path === '/browser') server.send(JSON.stringify({ type: 'session', sessionId: input.sessionId }))
      await this.checkpoint()
      return new Response(null, { status: 101, webSocket: pair[0],
        headers: path === '/browser' ? { 'sec-websocket-protocol': 'nexus-observe-v1' } : undefined })
    } catch (error) {
      await this.checkpoint()
      // The same rule as index.ts: a refusal answers with its own code and status; anything else -
      // storage, the runtime, an admission this room cannot read - is the service's failure, and
      // a 503 the station retries. It used to be a bare 403 for everything, which the station
      // reads as "no longer approved" and remembers as Remote OFF across restarts.
      if (error instanceof Refusal) return Response.json({ error: error.code }, { status: error.status })
      return Response.json({ error: 'serviceUnavailable' }, { status: 503 })
    }
  }

  async webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): Promise<void> {
    // A pipe's message is passed on unread, before anything here reads it, and with no checkpoint.
    if (this.pipes.holds(ws)) { await this.pipes.message(ws, message, Date.now()); return }
    const peer = this.peers.get(ws), attachment = ws.deserializeAttachment() as Attachment
    if (!peer || !this.relay || typeof message !== 'string') {
      ws.close(1008, 'invalidMessage'); await this.disconnected(ws); return
    }
    this.syncLanes(Date.now())
    // Bounds apply before parsing. A station's largest message is an observation frame, or an
    // export's answer from a station old enough to still serve one.
    const limit = attachment.role === 'station'
      ? Math.max(MAX_FRAME_BYTES, OPERATION_EXPORT_RESPONSE_BYTES) + 256 : OPERATION_REQUEST_BYTES
    if (new TextEncoder().encode(message).length > limit) {
      ws.close(1008, 'invalidMessage'); await this.disconnected(ws); return
    }
    let parsed: Record<string, unknown>
    try { parsed = JSON.parse(message) as Record<string, unknown> }
    catch { ws.close(1008, 'invalidMessage'); await this.disconnected(ws); return }
    // Audio first, and it returns WITHOUT a checkpoint. Everything else in this method
    // ends in `checkpoint()`, which serializes an attachment per socket and may write
    // storage; paying that every 60 ms for a throw-away bundle would turn the audio
    // lane into the room's dominant cost and would make audio able to delay a control
    // message through shared work. The lane holds no state worth checkpointing.
    if (parsed && typeof parsed === 'object' && typeof parsed.type === 'string' && parsed.type.startsWith('audio')) {
      if (attachment.role === 'station') this.audio.receiveStation(parsed)
      else this.audio.receiveBrowser(attachment.sessionId, parsed)
      return
    }
    // The stream's signalling, on the same terms and for the same reasons: no checkpoint, nothing
    // stored. An OFFER starts commanding the station, so it is held to the command lane's
    // entitlement freshness first - the one await on this lane, taken only for an offer.
    if (parsed && typeof parsed === 'object' && typeof parsed.type === 'string' && parsed.type.startsWith('stream')) {
      if (attachment.role === 'station') { this.stream.receiveStation(parsed, Date.now()); return }
      const sessionId = attachment.sessionId, before = this.streamOrder.get(sessionId)
      const handled = (async () => {
        if (before) await before
        const offer = (parsed as { payload?: { kind?: unknown } }).payload?.kind === 'offer'
        if (offer && await this.refreshEntitlement(sessionId, Date.now())) {
          // The await yielded, and the socket may have gone meanwhile - as in the operation lane.
          if (!this.peers.has(ws)) return
          this.syncLanes(Date.now())
        }
        this.stream.receiveBrowser(sessionId, parsed, Date.now())
      })()
      const settled = handled.catch(() => {})
      this.streamOrder.set(sessionId, settled)
      try { await handled }
      finally { if (this.streamOrder.get(sessionId) === settled) this.streamOrder.delete(sessionId) }
      return
    }
    if(parsed&&typeof parsed==='object'&&typeof parsed.type==='string'&&parsed.type.startsWith('operation')){
      if(attachment.role==='station')this.operations.receiveStation(parsed)
      else {
        // The entitlement is re-read HERE, before the relay is asked, so that the relay's gate
        // stays synchronous: every decision it makes about rate, conflict and pending state runs
        // to completion with no await inside it, exactly as it did before.
        const refreshed = await this.refreshCommandEntitlement(attachment.sessionId, parsed, Date.now())
        // That await is the one place this handler yields to another message, and the socket can
        // be gone by the time it returns - expired, revoked, or closed by its own peer.
        if (refreshed && !this.peers.has(ws)) return
        // Publish the refreshed deadline into the relay before it reads it. Only when there is
        // one: a heartbeat spends the relay's whole budget four times a second and must not pay
        // for a second sweep of every peer to learn nothing changed.
        if (refreshed) this.syncLanes(Date.now())
        this.operations.receiveBrowser(attachment.sessionId,parsed,Date.now())
        // A running stream commands the station peer to peer, where no relay can gate it. So while
        // one runs, its session's entitlement is re-read as the page's socket heartbeats arrive (after
        // the request is on its way, which never waits on it), within STREAM_ENTITLEMENT_CHECK_MS, and
        // a lapse ends the stream at the station: Remote switched off by hand mid-stream ends within
        // about 2 s (operator decision, 2026-09-27).
        if (this.stream.streaming(attachment.sessionId)) {
          if (await this.refreshEntitlement(attachment.sessionId, Date.now(), STREAM_ENTITLEMENT_CHECK_MS)) this.syncLanes(Date.now())
          this.stream.enforce(Date.now())
        }
      }
      await this.checkpoint();return
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

  /** Re-read the account's entitlement before a command that would change the station.
   *
   *  The whole point of the per-command gate: a session admitted while the account was entitled
   *  used to keep commanding a real radio for the rest of its observer lease, because the lease
   *  was the only thing that ever ended a session and nothing looked at the database again. Now
   *  a gated command past OPERATION_ENTITLEMENT_CHECK_MS pays for one indexed single-row read,
   *  and the relay refuses on what that read said.
   *
   *  Only gated commands, so observation, reads and - above all - Stop never wait on D1 and can
   *  never be refused because D1 was slow. A radio that is keyed must always be stoppable.
   *
   *  A read that FAILS changes nothing: the last good reading stands, carrying its own
   *  OPERATION_ENTITLEMENT_MS expiry, so an outage degrades to a bounded window rather than to
   *  "refuse every paying operator" or "allow everybody".
   *
   *  Returns whether it awaited anything, which is what tells the caller both that the deadline
   *  may have moved and that this handler yielded. */
  private async refreshCommandEntitlement(sessionId: string, raw: Record<string, unknown>, now: number): Promise<boolean> {
    const request = (raw as { request?: { type?: unknown } }).request
    if (!request || typeof request !== 'object' || !entitledCommand(request.type)) return false
    return this.refreshEntitlement(sessionId, now)
  }
  /** The freshness half of `refreshCommandEntitlement`, for any gesture that starts commanding the
   *  station: an entitled command, or a stream offer. Same window, same fallback, same answer. */
  private async refreshEntitlement(sessionId: string, now: number, budget = OPERATION_ENTITLEMENT_CHECK_MS): Promise<boolean> {
    if (now - (this.commandChecked.get(sessionId) ?? 0) < budget) return false
    const observer = this.relay?.checkpoint().observers.find(o => o.sessionId === sessionId)
    if (!observer) return false
    try {
      const entitlement = await readEntitlement(this.env, observer.identity.accountId, Date.now())
      const at = Date.now()
      this.commandUntil.set(sessionId, commandDeadline(entitlement, at))
      if (this.lanUntil.has(sessionId)) this.lanUntil.set(sessionId, commandDeadline(lanEntitlement(entitlement, at), at))
      this.accessOff.set(sessionId, entitlement.state === 'disabled')
      this.commandChecked.set(sessionId, at)
    } catch { /* keep the last good reading; it expires on its own */ }
    return true
  }

  async webSocketClose(ws: WebSocket): Promise<void> { await this.disconnected(ws) }
  async webSocketError(ws: WebSocket): Promise<void> { await this.disconnected(ws) }
  private async disconnected(ws: WebSocket): Promise<void> {
    if (this.pipes.holds(ws)) { this.pipes.closed(ws); await this.checkpoint(); return }
    const peer = this.peers.get(ws), attachment = ws.deserializeAttachment() as Attachment
    if (peer && this.relay) {
      if (attachment.role === 'station') this.relay.disconnectStation(peer)
      else this.relay.disconnectObserver(attachment.sessionId)
    }
    this.peers.delete(ws)
    this.samples.delete(ws)
    this.lanePeers.delete(ws)
    await this.checkpoint()
  }
  async alarm(): Promise<void> {
    this.alarmAt = null
    this.relay?.expire(Date.now())
    if (this.relay) this.pipes.enforce(this.relay.checkpoint().access, Date.now())
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
    this.syncLanes(Date.now())
    const state = this.relay.checkpoint()
    for (const id of this.commandUntil.keys()) if (!state.observers.some(o => o.sessionId === id)) { this.commandUntil.delete(id); this.commandChecked.delete(id); this.accessOff.delete(id); this.lanUntil.delete(id) }
    for (const [ws, peer] of this.peers) {
      let attachment: Attachment | undefined
      if (state.station?.peer === peer) attachment = { version: 1, role: 'station', identity: state.station.identity, order: state.order, sample: this.samples.get(ws) ?? null, operationVersion:this.operationVersions.get(ws)??0, audioVersion: this.audioVersions.get(ws) ?? 0, streamVersion: this.streamVersions.get(ws) ?? 0, streamLanVersion: this.streamLanVersions.get(ws) ?? 0, laneVersion: this.laneVersions.get(ws) ?? 0, pipeVersion: this.pipeVersions.get(ws) ?? 0 }
      else {
        const observer = state.observers.find(o => o.peer === peer)
        if (observer) { const { peer: _peer, ...saved } = observer; attachment = { version: 1, role: 'browser', ...saved, operations:this.operations.checkpoint(observer.sessionId), commandUntil: this.commandUntil.get(observer.sessionId) ?? 0, ...(this.lanUntil.has(observer.sessionId) ? { lan: true } : {}) } }
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
    const operationDeadline=this.operations.nextDeadline()
    if(operationDeadline!==null)deadlines.push(operationDeadline)
    const pipeDeadline = this.pipes.nextDeadline()
    if (pipeDeadline !== null) deadlines.push(pipeDeadline)
    const next = deadlines.length ? Math.min(...deadlines) : null
    // An earlier alarm can simply reschedule itself; never rewrite an alarm for
    // each 500 ms publication/ACK. Idle rooms have no periodic timer.
    if (next === null && this.alarmAt !== null) { await this.ctx.storage.deleteAlarm(); this.alarmAt = null }
    else if (next !== null && (this.alarmAt === null || next < this.alarmAt)) {
      this.alarmAt = Math.max(Date.now() + 1, next)
      await this.ctx.storage.setAlarm(this.alarmAt)
    }
  }
  /** The station's main socket while it is connected, and whether it advertised the pipe. */
  private mainStation(): { socket: WebSocket; pipe: boolean } | null {
    const station = this.relay?.checkpoint().station
    const socket = station ? [...this.peers].find(([, peer]) => peer === station.peer)?.[0] : undefined
    return socket ? { socket, pipe: this.pipeVersions.get(socket) === 1 } : null
  }
  /** Hands the relay's current station and observers to the lanes (operation, audio, stream), each
   *  with the command deadline and access its session holds now. */
  private syncLanes(now: number): void {
    this.relay?.expire(now)
    const state = this.relay?.checkpoint()
    const stationSocket = [...this.peers].find(([, peer]) => peer === state?.station?.peer)?.[0]
    let station: { peer: Peer } | null = null
    if (stationSocket && state?.station) {
      let peer = this.lanePeers.get(stationSocket)
      if (!peer) {
        const original = state.station.peer
        peer = { send: message => stationSocket.send(message), close: (code, reason) => this.relay?.disconnectStation(original, code, reason) }
        this.lanePeers.set(stationSocket, peer)
      }
      station = { peer }
    }
    // A lane's refusal must leave the authoritative observer set immediately, rather than
    // waiting for the runtime's socket-close callback.
    const observers = (state?.observers ?? []).flatMap(observer => {
      const socket = [...this.peers].find(([, peer]) => peer === observer.peer)?.[0]
      if (!socket) return []
      let peer = this.lanePeers.get(socket)
      if (!peer) {
        peer = { send: message => observer.peer.send(message),
          close: (code, reason) => this.relay?.disconnectObserver(observer.sessionId, code, reason) }
        this.lanePeers.set(socket, peer)
      }
      return [{ sessionId: observer.sessionId, deviceId:observer.identity.deviceId, peer,
        commandUntil: this.commandUntil.get(observer.sessionId) ?? 0, accessOff: this.accessOff.get(observer.sessionId) === true,
        ...(this.lanUntil.has(observer.sessionId) ? { lanUntil: this.lanUntil.get(observer.sessionId) } : {}) }]
    })
    // A LAN-only session gets no lane that would carry the station across the internet: the
    // relayed receive audio. Its lease and Stop (operations) and its stream, held to the LAN, stay.
    const internet = observers.filter(observer => observer.lanUntil === undefined)
    this.operations.sync(station?{peer:station.peer,supported:!!stationSocket&&parseOperationVersion(this.operationVersions.get(stationSocket))!==null,operationVersion:stationSocket?this.operationVersions.get(stationSocket):0,laneVersion:stationSocket?this.laneVersions.get(stationSocket):0}:null,observers,now)
    this.audio.sync(station ? { peer: station.peer, supported: this.audioVersions.get(stationSocket!) === 1, laneVersion: this.laneVersions.get(stationSocket!) } : null, internet)
    this.stream.sync(station ? { peer: station.peer, supported: this.streamVersions.get(stationSocket!) === 1, lanVersion: this.streamLanVersions.get(stationSocket!) } : null, observers)
  }
}
