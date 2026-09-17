// One hibernating room per station. Socket attachments carry admission and bounded
// ordering/ACK state; neither D1 nor Durable Object storage receives observations.
import { entitledCommand, OperationRelay, type OperationCheckpoint } from '../../ui/src/remote-web/operation-relay'
import { OPERATION_REQUEST_BYTES } from '../../ui/src/remote-web/operation-protocol'
import { OPERATION_ENTITLEMENT_CHECK_MS, OPERATION_ENTITLEMENT_MS } from '../../ui/src/remote-web/operation-limits'
import { parseOperationVersion } from '../../ui/src/remote-web/operation-version'
import { DurableObject } from 'cloudflare:workers'
import { ObservationRelay } from '../../ui/src/remote-monitor/relay'
import type { BrowserIdentity, Entitlement, ObserverCheckpoint, Peer, StationAccess, StationIdentity } from '../../ui/src/remote-monitor/relay'
import type { FrameOrderState } from '../../ui/src/remote-monitor/protocol'
import { ageFrame, MAX_FRAME_BYTES, parseFrame, STALE_MS } from '../../ui/src/remote-monitor/protocol'
import { knownApplicationVersion } from './application-version'
import { Refusal, trial } from './authority'
import type { RemoteEnv } from './authority'
import { ApplicationRelay } from '../../ui/src/remote-web/application-relay'
import type { ApplicationCheckpoint } from '../../ui/src/remote-web/application-relay'
import { APPLICATION_MAX_BYTES, APPLICATION_REQUEST_BYTES } from '../../ui/src/remote-web/application-protocol'
import { AudioRelay } from '../../ui/src/remote-web/audio-relay'

type Saved = { access: StationAccess; order: FrameOrderState }
type Sample = { requestId: string; at: number }
type StationAttachment = { version: 1; role: 'station'; identity: StationIdentity; order: FrameOrderState; sample: Sample | null; applicationVersion?: number; operationVersion?: number; audioVersion?: number }
type BrowserAttachment = Omit<ObserverCheckpoint, 'peer'> & { version: 1; role: 'browser'; application?: ApplicationCheckpoint; operations?: OperationCheckpoint; commandUntil?: number }
type Attachment = StationAttachment | BrowserAttachment
type Admission = { access: StationAccess; identity: StationIdentity & BrowserIdentity; entitlement: Entitlement; sessionId: string; applicationVersion?: number; operationVersion?: number; audioVersion?: number }

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
  private application = new ApplicationRelay()
  private operations = new OperationRelay()
  // Receive audio. Runtime only: no checkpoint, no storage, no alarm. A bundle that
  // does not survive a hibernation is a gap the player conceals, which is the right
  // answer - keeping one would only deliver stale band noise late.
  private audio = new AudioRelay()
  private audioVersions = new Map<WebSocket, number>()
  private operationVersions = new Map<WebSocket,number>()
  // Per observer session, the command deadline from commandDeadline(). Checkpointed with the
  // browser's attachment so a hibernation does not hand a woken room an empty one.
  private commandUntil = new Map<string, number>()
  // When that deadline was last taken from the database, for OPERATION_ENTITLEMENT_CHECK_MS.
  // Deliberately NOT checkpointed: a woken room re-reads before the first command it forwards,
  // which is the safe direction and costs one read.
  private commandChecked = new Map<string, number>()
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
          if(attachment.role==='station')this.operationVersions.set(ws,parseOperationVersion(attachment.operationVersion)??0)
          if (attachment.role === 'station') this.audioVersions.set(ws, attachment.audioVersion === 1 ? 1 : 0)
          if (attachment.role === 'station' && attachment.sample) this.samples.set(ws, attachment.sample)
          const peer = this.peer(ws, attachment.role)
          if (attachment.role === 'station') {
            if (station) throw new Error('invalidCheckpoint')
            station = { peer, identity: attachment.identity }; order = attachment.order
            this.applicationVersions.set(ws, knownApplicationVersion(attachment.applicationVersion ?? 0) ? attachment.applicationVersion! : 0)
          } else if (attachment.role === 'browser') {
            observers.push({ ...attachment, peer })
            // An attachment written before this field existed carries none, and gets 0: that
            // session may watch and read but not command until its next renewal, which is at
            // most thirty seconds away. Fail closed across a deploy, and heal by itself.
            this.commandUntil.set(attachment.sessionId, Number.isSafeInteger(attachment.commandUntil) ? attachment.commandUntil! : 0)
            if (attachment.application) applicationSaved.push({ sessionId: attachment.sessionId, value: attachment.application })
          }
          else throw new Error('invalidCheckpoint')
        }
        this.relay = ObservationRelay.restore(this.saved.access, order, station, observers, Date.now())
        this.syncApplication(Date.now())
        for (const saved of applicationSaved) {
          if (this.application.checkpoint(saved.sessionId)) this.application.restore(saved.sessionId, saved.value)
        }
        for(const ws of sockets){const a=ws.deserializeAttachment() as Attachment;if(a.role==='browser'&&a.operations)this.operations.restore(a.sessionId,a.operations)}
        await this.checkpoint()
      } catch {
        for (const ws of sockets) ws.close(1011, 'authorityUnavailable')
        this.peers.clear()
        this.samples.clear(); this.applicationPeers.clear(); this.applicationVersions.clear()
        this.application = new ApplicationRelay()
        this.operations = new OperationRelay();this.operationVersions.clear();this.commandUntil.clear();this.commandChecked.clear()
        this.audio = new AudioRelay(); this.audioVersions.clear()
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
        this.applicationVersions.delete(ws); this.applicationPeers.delete(ws);this.operationVersions.delete(ws);this.audioVersions.delete(ws)
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
        // Only after renewObserver has accepted it: a renewal the relay refuses must not
        // extend the right to command either.
        this.commandUntil.set(input.sessionId, commandDeadline(input.entitlement, now))
        await this.checkpoint()
        return new Response(null, { status: 204 })
      }
      if (path !== '/station' && path !== '/browser') return new Response(null, { status: 404 })
      // Reserve in the bounded core before accepting a runtime socket. Failed
      // admissions do not leave accepted but untracked connections behind.
      const pair = new WebSocketPair(), server = pair[1]
      const buffered = { pending: true, messages: [] as string[] }
      const peer = this.peer(server, path === '/station' ? 'station' : 'browser', buffered)
      if(path==='/station')this.operationVersions.set(server,parseOperationVersion(input.operationVersion)??0)
      if (path === '/station') this.audioVersions.set(server, input.audioVersion === 1 ? 1 : 0)
      if (path === '/station') this.applicationVersions.set(server, knownApplicationVersion(input.applicationVersion ?? 0) ? input.applicationVersion! : 0)
      try {
        if (path === '/station') relay.connectStation(input.identity, peer, now)
        else {
          relay.connectObserver(input.sessionId, input.identity, input.entitlement, peer, now)
          this.commandUntil.set(input.sessionId, commandDeadline(input.entitlement, now))
        }
      } catch (error) {
        this.peers.delete(server); this.samples.delete(server); this.applicationVersions.delete(server)
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
    const peer = this.peers.get(ws), attachment = ws.deserializeAttachment() as Attachment
    if (!peer || !this.relay || typeof message !== 'string') {
      ws.close(1008, 'invalidMessage'); await this.disconnected(ws); return
    }
    this.syncApplication(Date.now())
    // Bounds apply before parsing. The larger envelope is available only to an
    // authenticated station that advertised this application protocol version.
    const limit = attachment.role === 'station' && knownApplicationVersion(this.applicationVersions.get(ws) ?? 0)
      ? APPLICATION_MAX_BYTES : attachment.role === 'station' ? MAX_FRAME_BYTES + 256 : Math.max(APPLICATION_REQUEST_BYTES,OPERATION_REQUEST_BYTES)
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
    // A pushed control outcome (operation v5) leaves no relay state behind it, so like audio it
    // returns WITHOUT a checkpoint: the O(peers) attachment sweep would be paid to serialise
    // nothing, on the one message whose whole point is to arrive sooner.
    if (parsed && typeof parsed === 'object' && parsed.type === 'operationEvent' && attachment.role === 'station') {
      this.operations.receiveStation(parsed)
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
        if (refreshed) this.syncApplication(Date.now())
        this.operations.receiveBrowser(attachment.sessionId,parsed,Date.now())
      }
      await this.checkpoint();return
    }
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
    if (now - (this.commandChecked.get(sessionId) ?? 0) < OPERATION_ENTITLEMENT_CHECK_MS) return false
    const observer = this.relay?.checkpoint().observers.find(o => o.sessionId === sessionId)
    if (!observer) return false
    try {
      const entitlement = await trial(this.env, observer.identity.accountId, Date.now())
      const at = Date.now()
      this.commandUntil.set(sessionId, commandDeadline(entitlement, at))
      this.commandChecked.set(sessionId, at)
    } catch { /* keep the last good reading; it expires on its own */ }
    return true
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
    for (const id of this.commandUntil.keys()) if (!state.observers.some(o => o.sessionId === id)) { this.commandUntil.delete(id); this.commandChecked.delete(id) }
    for (const [ws, peer] of this.peers) {
      let attachment: Attachment | undefined
      if (state.station?.peer === peer) attachment = { version: 1, role: 'station', identity: state.station.identity, order: state.order, sample: this.samples.get(ws) ?? null, applicationVersion: this.applicationVersions.get(ws) ?? 0, operationVersion:this.operationVersions.get(ws)??0, audioVersion: this.audioVersions.get(ws) ?? 0 }
      else {
        const observer = state.observers.find(o => o.peer === peer)
        if (observer) { const { peer: _peer, ...saved } = observer; attachment = { version: 1, role: 'browser', ...saved, application: this.application.checkpoint(observer.sessionId),operations:this.operations.checkpoint(observer.sessionId), commandUntil: this.commandUntil.get(observer.sessionId) ?? 0 } }
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
      return [{ sessionId: observer.sessionId, deviceId:observer.identity.deviceId, peer,
        commandUntil: this.commandUntil.get(observer.sessionId) ?? 0 }]
    })
    this.application.sync(station, observers, now)
    this.operations.sync(station?{peer:station.peer,supported:!!stationSocket&&parseOperationVersion(this.operationVersions.get(stationSocket))!==null,operationVersion:stationSocket?this.operationVersions.get(stationSocket):0}:null,observers,now)
    this.audio.sync(station ? { peer: station.peer, supported: this.audioVersions.get(stationSocket!) === 1 } : null, observers)
  }
}
