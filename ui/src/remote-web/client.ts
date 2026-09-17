import { pendingLogStorage } from './operation-storage'
import { pendingControlStorage } from './control-storage'
import { OperationClient } from './operation-client'
import { OPERATION_EXPORT_RESPONSE_BYTES, OPERATION_REQUEST_BYTES } from './operation-protocol'
import { advertisedOperationVersion, parseOperationVersion } from './operation-version'
import { Auth0Client } from '@auth0/auth0-spa-js'
import { ageFrame, MAX_FRAME_BYTES, parseFrame, STALE_MS } from '../remote-monitor/protocol'
import type { MonitorFrame } from '../remote-monitor/protocol'
import type { MonitorSource } from '../remote-monitor/session'
import { APPLICATION_VERSIONS } from './application-capabilities'
import { ApplicationClient } from './application-client'
import { APPLICATION_MAX_BYTES } from './application-protocol'
import { AudioLink, browserAudio } from './audio-listen'

export type AccountSession = {
  accountId: string
  entitlement: { accountId: string; enabled: boolean; expiresAt: number
    state: 'none' | 'active' | 'ended' | 'disabled'; startedAt: number | null; source: string | null }
  /** The service's clock. Dates are derived from this, never the browser's own, so a laptop
   *  with the wrong time still shows the right number of days remaining. */
  serverNow: number
  /** `expires_at` is when this browser's approval ends unless it is used again; `renewsUntil` is the end
   *  that use cannot move, or null for an approval that never renews. */
  stations: { id: string; name: string; device: { id: string; name: string; approved: number
    generation?: number; expires_at?: number; renewsUntil?: number | null } | null }[]
  /** A code this account has claimed that the shack has not approved yet, or null. Durable on the
   *  server, so the waiting-for-approval state survives a reload instead of living in component
   *  state that a refresh throws away. Never carries the pairing credentials. */
  pending: { id: string; name: string; expiresAt: number; confirmed: boolean } | null
}
/** The audio lane's own send budget. Small on purpose - see where it is used. */
const AUDIO_BUDGET_BYTES = 512
/** How long a connection granted a session has to LAST before its next failure is forgiven the
 *  reconnect ladder. Ten seconds: twenty of the relay's observation intervals, so an ordinary
 *  working connection is always credited, and far enough above the ladder's 1 s floor that a
 *  socket which cannot hold itself up for ten seconds is flapping and should be backed off like
 *  anything else.
 *
 *  ⚠️ DURATION ALONE CANNOT BE THE WHOLE RULE, and this is why `retry` credits a delivered frame
 *  separately. This constant sits above EVERY deadline that can close this socket - not one of
 *  them, all of them:
 *    `APPLICATION_TIMEOUT_MS`  the browser's own application credit  application-stream-client
 *    `APPLICATION_TIMEOUT_MS`  the relay's application expiry        application-stream-relay
 *    `ACK_TIMEOUT_MS`          the relay's observation ACK timeout   remote-monitor/relay
 *  A socket closed by any of those dies structurally below this window, so a duration-only rule can
 *  never forgive that whole class however healthy the station was - which is exactly what happened:
 *  a station answering the whole time bought 1, 2, 4, 8, 16, 30 s of "Station data unavailable".
 *  Raising this, or adding a deadline, does not reintroduce that on its own; deleting the delivery
 *  credit does. The values are deliberately not written here - `client.test.ts` imports those two
 *  constants and exercises the credit at each, so this list cannot quietly disagree with them. */
export const RECONNECT_CREDIT_MS = 10_000
/** What this browser is doing with the feed, for the one row that can say so. */
export type FeedView = {
  /** Asleep because this tab is in the background. */
  asleep: boolean
  /** It was asleep and is on its way back - the reason for the gap, said at the one moment the
   *  operator is there to read it. */
  resumed: boolean
  /** The operator asked for the feed to stay live in the background. */
  keepWatching: boolean
}
export type FeedControl = {
  subscribe: (f: () => void) => () => void
  getSnapshot: () => FeedView
  keepWatching: (value: boolean) => void
}
/** The "keep watching" choice: browser-local, per station, beside this browser's other per-station
 *  memory. Storage that throws - a private window, blocked site data - means "not chosen", which is
 *  the safe answer both ways: the feed sleeps, and the operator can still turn it on for the
 *  session. */
function keepWatchingStorage(stationId: string) {
  const key = `nexus.remote.keep-watching.${stationId}`
  return {
    read(): boolean { try { return globalThis.localStorage?.getItem(key) === '1' } catch { return false } },
    write(value: boolean): void {
      try { if (value) globalThis.localStorage?.setItem(key, '1'); else globalThis.localStorage?.removeItem(key) } catch { /* the choice still holds for this session */ }
    },
  }
}
export class RemoteError extends Error {
  // `code` is the service's own word for what it refused - trialEnded, trialDisabled,
  // stationLimit, pairingExpired. Without it every refusal arrives as a bare status number and
  // the browser can only show one vague wall for situations that need different sentences.
  constructor(readonly status: number, readonly code = 'remoteUnavailable') { super(code) }
}
// The service answers a refusal with {error: code}. A body that is missing, truncated or not
// JSON is not a failure worth reporting on its own - the status still stands - so this falls
// back to the generic code rather than throwing a second error on top of the first.
async function refusalCode(response: Response): Promise<string> {
  try {
    const body = await response.json() as { error?: unknown }
    return typeof body?.error === 'string' && body.error ? body.error : 'remoteUnavailable'
  } catch { return 'remoteUnavailable' }
}
/** Why the account service turned away the sign-in this page was just redirected back from.
 *  `emailUnverified` is a sign-up that worked but whose address is not confirmed yet - the page
 *  shows "confirm your email to finish" rather than a refusal. `signInRefused` is any other deny. */
export type SignInRefusal = 'emailUnverified' | 'signInRefused'
// The post-login Action is the tenant's only deny, and it denies an unconfirmed address. Auth0
// passes its reason through as `error_description`, which is not guaranteed to arrive - and
// spa-js repeats the error code when it is missing - so no usable description means that case.
// `email_unverified` is the stable token the Action can deny with, so rewording its text cannot
// change what the page shows. A description that is plainly about something else stays a refusal.
function refusalKind(description: unknown): SignInRefusal {
  const text = typeof description === 'string' ? description.trim() : ''
  if (!text || text === 'access_denied' || text === 'email_unverified') return 'emailUnverified'
  return /verif/i.test(text) && /e-?mail/i.test(text) ? 'emailUnverified' : 'signInRefused'
}
async function boundedJson<T>(path: string, options: RequestInit): Promise<T> {
  const controller = new AbortController()
  const cancel = () => controller.abort()
  if (options.signal?.aborted) cancel()
  options.signal?.addEventListener('abort', cancel, { once: true })
  const timer = setTimeout(cancel, 10000)
  try {
    const response = await fetch(path, { ...options, signal: controller.signal })
    if (!response.ok) throw new RemoteError(response.status, await refusalCode(response))
    // Keep both cancellation and the deadline active until the body is read.
    return await response.json() as T
  }
  finally { clearTimeout(timer); options.signal?.removeEventListener('abort', cancel) }
}
export class BrowserClient {
  constructor(private readonly auth: Auth0Client, readonly applicationVersion = 1, readonly operationVersion = 0,
    readonly signInRefusal: SignInRefusal | null = null) {}
  static async load(): Promise<BrowserClient | null> {
    const config = await boundedJson<{ issuer: string; audience: string; clientId: string; ready: boolean; applicationVersion?: number; operationVersion?: number; operationMaxVersion?: number; operationFtVersion?: number; operationPushVersion?: number }>(
      '/api/remote/config', { cache: 'no-store', credentials: 'omit' })
    if (!config.ready) return null
    const issuer = new URL(config.issuer)
    if (issuer.protocol !== 'https:' || issuer.username || issuer.password || issuer.search || issuer.hash) throw new RemoteError(503)
    const auth = new Auth0Client({ domain: issuer.host, clientId: config.clientId,
      cacheLocation: 'memory', useRefreshTokens: false, httpTimeoutInSeconds: 10,
      authorizationParams: { audience: config.audience, redirect_uri: window.location.origin, scope: 'openid profile email' } })
    const query = new URLSearchParams(window.location.search)
    let signInRefusal: SignInRefusal | null = null
    if (query.has('code') || query.has('error')) {
      // `access_denied` is the account service refusing the sign-in itself. It is RETURNED with a
      // working client, never thrown: a throw left the page with no client, so the sign-in buttons
      // vanished and a new ham was stranded behind "Try again". Auth0 keeps its session through a
      // deny, so the page needs that client to sign in again or sign out of the refused account.
      try { await auth.handleRedirectCallback() }
      catch (cause) {
        const failure = cause as { error?: unknown; error_description?: unknown } | null
        if (failure?.error !== 'access_denied') throw cause
        signInRefusal = refusalKind(failure.error_description)
      }
      finally { window.history.replaceState({}, '', '/') }
    } else {
      try { await auth.checkSession() } catch { /* interactive login stays available */ }
    }
    return new BrowserClient(auth, APPLICATION_VERSIONS.find(version=>version===config.applicationVersion)??1, advertisedOperationVersion(config.operationVersion, config.operationMaxVersion, config.operationFtVersion, config.operationPushVersion), signInRefusal)
  }
  authenticated(): Promise<boolean> { return this.auth.isAuthenticated() }
  // `createAccount` sends Auth0 straight to its sign-up screen. Without it a first-time operator
  // lands on the login form and has to notice a small "Sign up" link to get anywhere, which is a
  // configuration step in disguise - the thing this project treats as unfinished.
  signIn(createAccount = false): Promise<void> {
    return this.auth.loginWithRedirect(createAccount ? { authorizationParams: { screen_hint: 'signup' } } : undefined)
  }
  signOut(): Promise<void> { return this.auth.logout({ logoutParams: { returnTo: window.location.origin } }) }
  async post<T>(path: string, body: object = {}, signal?: AbortSignal): Promise<T> {
    return (await this.request<T>(path, body, signal)).body
  }
  observationTicket(stationId: string, signal: AbortSignal): Promise<{ body: { ticket: string; serverNow: number }; startedAt: number }> {
    return this.request(`stations/${stationId}/ticket`, {}, signal)
  }
  renewObservation(stationId:string,sessionId:string,signal:AbortSignal):Promise<{body:{ok:boolean;serverNow?:number};startedAt:number}> {
    return this.request(`stations/${stationId}/renew`,{sessionId},signal)
  }
  private async request<T>(path: string, body: object, signal?: AbortSignal): Promise<{ body: T; startedAt: number }> {
    let token: string
    try { token = await this.auth.getTokenSilently() } catch { throw new RemoteError(401) }
    // The service clock anchor starts at the HTTP request, after account-token
    // acquisition. Waiting for identity cannot make a later fresh frame stale.
    const startedAt = performance.now()
    const result = await boundedJson<T>(`/api/remote/${path}`, {
      method: 'POST', credentials: 'same-origin', cache: 'no-store', signal,
      headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' }, body: JSON.stringify(body),
    })
    return { body: result, startedAt }
  }
}

/** A single socket and a single latest frame. Tickets and JWTs never enter URLs,
 * storage or display text. Application mode adds only the reviewed read/ACK contract. */
export class HostedConnection {
  readonly source: MonitorSource
  readonly application: ApplicationClient
  readonly operations: OperationClient
  /** Listening. It shares this socket and nothing else: no ACK, no credit, no retry,
   *  and its own byte budget, so a bundle can never consume the allowance an operation
   *  response or an observation ACK needs. */
  readonly audio: AudioLink
  private socket: WebSocket | null = null
  private latest: { frame: MonitorFrame; at: number } | null = null
  private disposed = false
  private attempt = 0
  /** When the live connection was granted its session - or null for one that never was. A grant
   *  only proves the SERVICE is reachable, so it starts a clock rather than crediting anything;
   *  `retry` pays the credit once that clock reaches RECONNECT_CREDIT_MS. */
  private liveSince: number | null = null
  /** Whether the live connection ever carried an observation frame this page accepted. Unlike a
   *  grant this is progress in itself - the relay reached the station and the station answered -
   *  so `retry` credits it however briefly the socket that carried it survived. */
  private delivered = false
  private reconnectTimer: ReturnType<typeof setTimeout> | undefined
  private renewal: ReturnType<typeof setInterval> | undefined
  private abort = new AbortController()
  private renewing = false
  private sessionId: string | null = null
  private lastClockRenewal = -Infinity
  private clockFailures = 0
  private anchor: { server: number; start: number } | null = null
  private sleepState: FeedView
  private feedListeners = new Set<() => void>()
  /** Removes the visibilitychange listener; set for the life of the connection. */
  private watching: (() => void) | undefined
  /** Removes the operation subscription that is waiting for a live Stop to go away. */
  private deferred: (() => void) | undefined
  private readonly keep: ReturnType<typeof keepWatchingStorage>
  /** This tab's feed: whether it is asleep, and the operator's one control over that. */
  readonly feed: FeedControl
  /** Called once when the service refuses a new ticket (401/403): the trial ended, the station or
   *  browser was revoked, or the sign-in expired. The connection has already stopped retrying, and
   *  without this the page kept showing a workspace that could never come back. */
  onRefused: ((error: RemoteError) => void) | null = null

  constructor(private client: BrowserClient, private stationId: string, private readonly applicationMode = false) {
    this.application = new ApplicationClient(message => {
      if (!this.applicationMode || this.socket?.readyState !== WebSocket.OPEN || this.socket.bufferedAmount + new TextEncoder().encode(message).length > (client.operationVersion>=1?OPERATION_REQUEST_BYTES:2048)) throw new RemoteError(503)
      this.socket.send(message)
    }, () => this.socket?.close(1000, 'applicationUnavailable'), client.applicationVersion)
    this.operations = new OperationClient(message=>{
      if(!this.applicationMode||this.socket?.readyState!==WebSocket.OPEN||this.socket.bufferedAmount+new TextEncoder().encode(message).length>OPERATION_REQUEST_BYTES)throw new RemoteError(503)
      this.socket.send(message)
    },applicationMode&&client.operationVersion>=1,()=>performance.now(),pendingLogStorage(()=>localStorage,stationId),parseOperationVersion(client.operationVersion)??1,pendingControlStorage(()=>localStorage,stationId))
    this.audio = new AudioLink(message => {
      const text = JSON.stringify(message)
      // Its own budget, deliberately small: a listen request is ~130 bytes and this is
      // room for it and nothing more. Audio must never be able to spend the queue an
      // acknowledgement needs, because that queue is what carries Stop.
      if (!this.applicationMode || this.socket?.readyState !== WebSocket.OPEN
        || this.socket.bufferedAmount + new TextEncoder().encode(text).length > AUDIO_BUDGET_BYTES) throw new RemoteError(503)
      this.socket.send(text)
    }, browserAudio())
    this.source = { id: `hosted-${stationId}`, kind: 'native', read: async signal => {
      if (signal.aborted || this.disposed || this.socket?.readyState !== WebSocket.OPEN || !this.latest) throw new RemoteError(503)
      return ageFrame(this.latest.frame, performance.now() - this.latest.at)
    } }
    this.keep = keepWatchingStorage(stationId)
    this.sleepState = { asleep: false, resumed: false, keepWatching: this.keep.read() }
    this.feed = {
      subscribe: f => { this.feedListeners.add(f); return () => { this.feedListeners.delete(f) } },
      getSnapshot: () => this.sleepState,
      keepWatching: value => this.setKeepWatching(value),
    }
  }
  start(): void {
    // A backgrounded tab is the one thing that keeps a room fully awake with nobody looking at
    // it, and the room bills for the wall-clock it is kept awake. Control and listening already
    // let go when the tab hides; this is the same release for the feed itself.
    const doc = typeof document === 'undefined' ? null : document
    if (doc) {
      const changed = () => { if (doc.hidden) this.considerSleep(); else this.shown() }
      doc.addEventListener('visibilitychange', changed)
      this.watching = () => doc.removeEventListener('visibilitychange', changed)
    }
    void this.connect()
  }
  stop(): void {
    this.application.disconnected(); this.operations.disconnected(); this.audio.close()
    this.disposed = true; this.abort.abort(); this.latest = null
    this.watching?.(); this.watching = undefined
    this.deferred?.(); this.deferred = undefined
    clearTimeout(this.reconnectTimer); clearInterval(this.renewal)
    this.socket?.close(1000, 'disconnected'); this.socket = null
  }
  private publishFeed(value: Partial<FeedView>): void {
    this.sleepState = { ...this.sleepState, ...value }
    for (const f of this.feedListeners) f()
  }
  private setKeepWatching(value: boolean): void {
    this.keep.write(value)
    this.publishFeed({ keepWatching: value })
    if (value) { if (this.sleepState.asleep) this.shown() }
    else if (typeof document !== 'undefined' && document.hidden) this.considerSleep()
  }
  /** Hidden, and deciding whether to let the room go back to sleep.
   *
   *  ⚠️ NEVER WHILE THIS BROWSER CAN STILL STOP A TRANSMISSION. Stop deliberately outlives the
   *  control lease (operator ruling, 2026-09-15), and closing the socket is what takes it away:
   *  `OperationClient.disconnected` drops the stop token, and a reconnect cannot get it back
   *  without acquiring control again. So a tab that hides with a live stop token stays awake and
   *  billing until that token goes. It goes on its own, within a poll: hiding already releases the
   *  lease, and an explicit release clears the station's stop owner, after which the state replies
   *  carry a null `transmitEpoch`. That is why this waits on the operation client's own updates
   *  rather than a clock - there is nothing to time, only something to be told. */
  private considerSleep(): void {
    if (this.disposed || this.sleepState.keepWatching || (typeof document !== 'undefined' && !document.hidden)) {
      this.deferred?.(); this.deferred = undefined
      return
    }
    if (this.operations.getSnapshot().stopAvailable) {
      this.deferred ??= this.operations.subscribe(() => this.considerSleep())
      return
    }
    this.deferred?.(); this.deferred = undefined
    this.sleep()
  }
  /** Let the room go back to sleep. Every line here already runs when the connection is lost -
   *  this is that same release, asked for, with no reconnect scheduled behind it. The workspace
   *  therefore goes stale exactly as it does for any other loss, which is what stops a slept tab
   *  from showing readings that look live. */
  private sleep(): void {
    if (this.sleepState.asleep) return
    this.audio.release(); this.application.disconnected(); this.operations.disconnected()
    this.latest = null
    clearInterval(this.renewal); clearTimeout(this.reconnectTimer); this.reconnectTimer = undefined
    // The in-flight ticket request goes with it; the next connect needs a fresh controller.
    this.abort.abort(); this.abort = new AbortController()
    const socket = this.socket
    this.socket = null
    this.publishFeed({ asleep: true, resumed: false })
    // Nulled and marked asleep first, so this close is not read as a drop worth reconnecting from.
    socket?.close(1000, 'backgrounded')
  }
  /** Back on screen. */
  private shown(): void {
    this.deferred?.(); this.deferred = undefined
    if (this.disposed) return
    // Someone is looking now, which is the strongest "try now" there is: a tab returning to a
    // station that recovered while nobody was watching must not sit out the rest of a 30 s step
    // it earned in the meantime.
    this.attempt = 0
    if (!this.sleepState.asleep) {
      // Awake all along - keep-watching, or a live Stop held it up. Still cut a pending retry short.
      if (this.reconnectTimer) { clearTimeout(this.reconnectTimer); this.reconnectTimer = undefined; void this.connect() }
      return
    }
    this.publishFeed({ asleep: false, resumed: true })
    void this.connect()
  }
  /** This connection has proved itself useful: it may now earn back the reconnect ladder, and a
   *  feed that was asleep has visibly come back. `delivered` marks the stronger of the two proofs -
   *  an observation frame this page accepted, which `retry` credits outright - as against a session
   *  grant, which only starts the clock the credit is paid against. */
  private proved(delivered = false): void {
    this.liveSince ??= performance.now()
    if (delivered) this.delivered = true
    if (this.sleepState.resumed) this.publishFeed({ resumed: false })
  }
  private retry(): void {
    this.application.disconnected(); this.operations.disconnected(); this.audio.disconnected()
    this.latest = null; clearInterval(this.renewal)
    if (this.disposed || this.sleepState.asleep || this.reconnectTimer) return
    // What forgives the ladder is a connection that made PROGRESS, read here and cleared as it is
    // read. It used to be the session message itself, and that made the backoff unreachable for
    // every failure that happens AFTER the relay accepts the ticket - a station offline behind a
    // reachable relay, a frame this socket rejects, a station-side close - because each round was
    // granted its own session and re-credited itself. Measured: 1000 ms, every round, no ceiling.
    // Progress has two forms and they are not the same evidence. A DELIVERED frame is progress in
    // itself: the relay reached the station and the station answered, so it is credited however
    // briefly the socket that carried it survived. A session GRANT proves only that the service is
    // reachable, so it earns nothing on its own and must hold the connection up for
    // RECONNECT_CREDIT_MS to be forgiven - that is the case above, and the one the ladder is for.
    // Requiring the connection to have LASTED in both cases was too strong: this page can close its
    // own socket (an application read missing its 3 s credit closes the shared socket - see the
    // ApplicationClient construction above), and that happens structurally below the ten-second
    // window, so a station that was answering the whole time escalated to 1, 2, 4, 8, 16, 30 s of
    // "Station data unavailable" it had done nothing to earn.
    if (this.delivered || (this.liveSince !== null && performance.now() - this.liveSince >= RECONNECT_CREDIT_MS)) this.attempt = 0
    this.liveSince = null; this.delivered = false
    const delay = Math.min(30000, 1000 * 2 ** Math.min(this.attempt++, 5))
    this.reconnectTimer = setTimeout(() => { this.reconnectTimer = undefined; void this.connect() }, delay)
  }
  private async renew(socket:WebSocket,clock=false):Promise<void> {
    if(this.renewing||this.disposed||this.socket!==socket||!this.sessionId)return
    const now=performance.now()
    if(clock&&now-this.lastClockRenewal<1000)return
    if(clock){this.lastClockRenewal=now;if(++this.clockFailures>3){socket.close(1000,'clockUnavailable');return}}
    this.renewing=true
    try {
      // Tests and old adapters can supply only post; normal BrowserClient
      // supplies the request-start clock after obtaining its account token.
      const startedAt=performance.now()
      const result=typeof this.client.renewObservation==='function'
        ? await this.client.renewObservation(this.stationId,this.sessionId,this.abort.signal)
        : {body:await this.client.post<{ok:boolean;serverNow?:number}>(`stations/${this.stationId}/renew`,{sessionId:this.sessionId},this.abort.signal),startedAt}
      if(this.socket!==socket||this.disposed)return
      if(result.body.serverNow!==undefined){
        if(!Number.isSafeInteger(result.body.serverNow)||result.body.serverNow<0)throw new RemoteError(503)
        this.anchor={server:result.body.serverNow,start:result.startedAt}
      }else if(clock)socket.close(1000,'clockUnavailable')
    }catch{if(this.socket===socket)socket.close(1000,'accessEnded')}
    finally{this.renewing=false}
  }
  private async connect(): Promise<void> {
    if (this.disposed || this.sleepState.asleep) return
    try {
      const { body: ticket, startedAt: start } = await this.client.observationTicket(this.stationId, this.abort.signal)
      if (this.disposed || this.sleepState.asleep) return
      if (!/^[0-9a-f]{64}$/.test(ticket.ticket) || !Number.isSafeInteger(ticket.serverNow)) throw new RemoteError(403)
      this.anchor = { server: ticket.serverNow, start };this.sessionId=null;this.clockFailures=0;this.lastClockRenewal=-Infinity
      const url = new URL(`/api/remote/stations/${this.stationId}/observe`, window.location.origin)
      url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:'
      const socket = new WebSocket(url, ['nexus-observe-v1', `ticket.${ticket.ticket}`])
      this.socket = socket
      socket.onmessage = event => {
        if (this.disposed || this.socket !== socket) return
        // A protocol error closes the socket, named for the kind of message that broke it. Lateness
        // is not one: late data is shown stale and the session, with its lease, carries on.
        let reason = 'invalidMessage'
        try {
          if (typeof event.data !== 'string' || new TextEncoder().encode(event.data).length > (this.applicationMode ? APPLICATION_MAX_BYTES : MAX_FRAME_BYTES + 256)) throw new RemoteError(403)
          const message = JSON.parse(event.data) as Record<string, unknown>
          // Only a chunk of the activation file this page asked for may pass the ordinary operation
          // bound; the operation client refuses any other reply over it.
          // Before the operation and application branches, and it never throws: an audio
          // message that is wrong costs the audio, never the session this socket carries.
          if (this.applicationMode && typeof message.type === 'string' && message.type.startsWith('audio')) {
            this.audio.receive(message); return
          }
          if(this.applicationMode&&message.type==='operationResponse'){reason='invalidOperation';const bytes=new TextEncoder().encode(event.data).length;if(bytes>OPERATION_EXPORT_RESPONSE_BYTES)throw new RemoteError(403);this.operations.receive(message,bytes);return}
          // Operation v5: a settled control, pushed by the station. Same lane, same strictness.
          if (this.applicationMode && message.type === 'operationEvent') { reason = 'invalidOperation'; this.operations.receiveEvent(message, new TextEncoder().encode(event.data).length); return }
          if (this.applicationMode && typeof message.type === 'string' && message.type.startsWith('application')) {
            reason = 'invalidApplication'; this.application.receive(message); return
          }
          if (new TextEncoder().encode(event.data).length > MAX_FRAME_BYTES + 256) throw new RemoteError(403)
          if (message.type === 'session' && Object.keys(message).length === 2 && typeof message.sessionId === 'string' && /^[0-9a-f-]{36}$/.test(message.sessionId)) {
            if (this.applicationMode) { this.application.open(); this.operations.open() }
            this.sessionId = message.sessionId
            // A session proves the SERVICE is reachable, so this connection starts counting
            // towards its reconnect credit here rather than only when a data frame lands. Those
            // are different failures and deserve different patience: a station that is up but
            // briefly quiet - a busy Nexus, a WAN blip - used to escalate exactly like an
            // unreachable one, because every reconnect during the quiet period incremented
            // `attempt` and none of them carried data to reset it. After a short outage the next
            // retry was already 16 to 30 seconds away, and the workspace sat on "Station data
            // unavailable" for all of it. What this must NOT do is forgive the ladder on the
            // strength of the grant alone - see `retry`.
            this.proved()
            clearInterval(this.renewal)
            this.renewal = setInterval(() => {void this.renew(socket)},30000)
          } else if (message.type === 'observation' && Object.keys(message).length === 3 && Number.isSafeInteger(message.sentAtMs) && this.anchor) {
            const received = performance.now()
            // HTTP request start is a conservative clock anchor. This includes
            // delivery delay without trusting the phone's wall clock.
            const transit = received - this.anchor.start - (Number(message.sentAtMs) - this.anchor.server)
            reason = 'invalidObservation'
            const parsed=parseFrame(message.frame,'native')
            // A wall-clock correction can invalidate the HTTP-to-monotonic
            // mapping without invalidating this socket's application replies.
            // Hide observation until authenticated renewal establishes a new
            // anchor. ACK only the structurally validated receipt, never data
            // shown as current. No clock tolerance weakens the freshness gate.
            // Delivered at or past the freshness limit (a slow link or a stalled page): hide it, which
            // the stale display handles, and still ACK the valid receipt so the relay keeps delivering.
            if(transit>=STALE_MS)this.latest=null
            else if(transit<0){this.latest=null;if(this.sessionId)void this.renew(socket,true);else socket.close(1000,'clockUnavailable')}
            else {this.latest={frame:ageFrame(parsed,transit),at:received};this.proved(true);this.clockFailures=0}
            const frame=parsed
            const ack = JSON.stringify({ type: 'ack', epoch: frame.epoch, sequence: frame.sequence })
            // The full workspace shares this socket with its bounded reads and
            // subscriptions. An observation ACK must use that same queue budget.
            // Include the pending write; neither mode may grow its queue freely.
            reason = 'acknowledgementBacklog'
            if (socket.bufferedAmount + new TextEncoder().encode(ack).length > (this.applicationMode ? (this.operations.enabled?OPERATION_REQUEST_BYTES:2048) : 512)) throw new RemoteError(503)
            socket.send(ack)
          } else throw new RemoteError(403)
        } catch { this.latest = null; this.application.disconnected(); this.operations.disconnected(); this.audio.disconnected(); socket.close(1000, reason) }
      }
      socket.onclose = () => { if (this.socket === socket) { this.socket = null; this.retry() } }
      socket.onerror = () => { this.latest = null; this.application.disconnected(); this.operations.disconnected() }
    } catch (error) {
      if (error instanceof RemoteError && [401, 403].includes(error.status)) {
        this.latest = null
        if (!this.disposed) this.onRefused?.(error)
        return
      }
      this.retry()
    }
  }
}
