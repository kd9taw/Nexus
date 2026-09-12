import { pendingLogStorage } from './operation-storage'
import { pendingControlStorage } from './control-storage'
import { OperationClient } from './operation-client'
import { OPERATION_REQUEST_BYTES, OPERATION_RESPONSE_BYTES } from './operation-protocol'
import { advertisedOperationVersion, parseOperationVersion } from './operation-version'
import { Auth0Client } from '@auth0/auth0-spa-js'
import { ageFrame, MAX_FRAME_BYTES, parseFrame, STALE_MS } from '../remote-monitor/protocol'
import type { MonitorFrame } from '../remote-monitor/protocol'
import type { MonitorSource } from '../remote-monitor/session'
import { APPLICATION_VERSIONS } from './application-capabilities'
import { ApplicationClient } from './application-client'
import { APPLICATION_MAX_BYTES } from './application-protocol'

export type AccountSession = {
  accountId: string
  entitlement: { accountId: string; enabled: boolean; expiresAt: number
    state: 'none' | 'active' | 'ended' | 'disabled'; startedAt: number | null; source: string | null }
  /** The service's clock. Dates are derived from this, never the browser's own, so a laptop
   *  with the wrong time still shows the right number of days remaining. */
  serverNow: number
  stations: { id: string; name: string; device: { id: string; name: string; approved: number } | null }[]
  /** A code this account has claimed that the shack has not approved yet, or null. Durable on the
   *  server, so the waiting-for-approval state survives a reload instead of living in component
   *  state that a refresh throws away. Never carries the pairing credentials. */
  pending: { id: string; name: string; expiresAt: number } | null
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
  constructor(private readonly auth: Auth0Client, readonly applicationVersion = 1, readonly operationVersion = 0) {}
  static async load(): Promise<BrowserClient | null> {
    const config = await boundedJson<{ issuer: string; audience: string; clientId: string; ready: boolean; applicationVersion?: number; operationVersion?: number; operationMaxVersion?: number; operationFtVersion?: number }>(
      '/api/remote/config', { cache: 'no-store', credentials: 'omit' })
    if (!config.ready) return null
    const issuer = new URL(config.issuer)
    if (issuer.protocol !== 'https:' || issuer.username || issuer.password || issuer.search || issuer.hash) throw new RemoteError(503)
    const auth = new Auth0Client({ domain: issuer.host, clientId: config.clientId,
      cacheLocation: 'memory', useRefreshTokens: false, httpTimeoutInSeconds: 10,
      authorizationParams: { audience: config.audience, redirect_uri: window.location.origin, scope: 'openid profile email' } })
    const query = new URLSearchParams(window.location.search)
    if (query.has('code') || query.has('error')) {
      try { await auth.handleRedirectCallback() }
      finally { window.history.replaceState({}, '', '/') }
    } else {
      try { await auth.checkSession() } catch { /* interactive login stays available */ }
    }
    return new BrowserClient(auth, APPLICATION_VERSIONS.find(version=>version===config.applicationVersion)??1, advertisedOperationVersion(config.operationVersion, config.operationMaxVersion, config.operationFtVersion))
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
  private socket: WebSocket | null = null
  private latest: { frame: MonitorFrame; at: number } | null = null
  private disposed = false
  private attempt = 0
  private reconnectTimer: ReturnType<typeof setTimeout> | undefined
  private renewal: ReturnType<typeof setInterval> | undefined
  private abort = new AbortController()
  private renewing = false
  private sessionId: string | null = null
  private lastClockRenewal = -Infinity
  private clockFailures = 0
  private anchor: { server: number; start: number } | null = null

  constructor(private client: BrowserClient, private stationId: string, private readonly applicationMode = false) {
    this.application = new ApplicationClient(message => {
      if (!this.applicationMode || this.socket?.readyState !== WebSocket.OPEN || this.socket.bufferedAmount + new TextEncoder().encode(message).length > (client.operationVersion>=1?OPERATION_REQUEST_BYTES:2048)) throw new RemoteError(503)
      this.socket.send(message)
    }, () => this.socket?.close(1000, 'applicationUnavailable'), client.applicationVersion)
    this.operations = new OperationClient(message=>{
      if(!this.applicationMode||this.socket?.readyState!==WebSocket.OPEN||this.socket.bufferedAmount+new TextEncoder().encode(message).length>OPERATION_REQUEST_BYTES)throw new RemoteError(503)
      this.socket.send(message)
    },applicationMode&&client.operationVersion>=1,()=>performance.now(),pendingLogStorage(()=>localStorage,stationId),parseOperationVersion(client.operationVersion)??1,pendingControlStorage(()=>localStorage,stationId))
    this.source = { id: `hosted-${stationId}`, kind: 'native', read: async signal => {
      if (signal.aborted || this.disposed || this.socket?.readyState !== WebSocket.OPEN || !this.latest) throw new RemoteError(503)
      return ageFrame(this.latest.frame, performance.now() - this.latest.at)
    } }
  }
  start(): void { void this.connect() }
  stop(): void {
    this.application.disconnected(); this.operations.disconnected()
    this.disposed = true; this.abort.abort(); this.latest = null
    clearTimeout(this.reconnectTimer); clearInterval(this.renewal)
    this.socket?.close(1000, 'disconnected'); this.socket = null
  }
  private retry(): void {
    this.application.disconnected(); this.operations.disconnected()
    this.latest = null; clearInterval(this.renewal)
    if (this.disposed || this.reconnectTimer) return
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
    if (this.disposed) return
    try {
      const { body: ticket, startedAt: start } = await this.client.observationTicket(this.stationId, this.abort.signal)
      if (this.disposed) return
      if (!/^[0-9a-f]{64}$/.test(ticket.ticket) || !Number.isSafeInteger(ticket.serverNow)) throw new RemoteError(403)
      this.anchor = { server: ticket.serverNow, start };this.sessionId=null;this.clockFailures=0;this.lastClockRenewal=-Infinity
      const url = new URL(`/api/remote/stations/${this.stationId}/observe`, window.location.origin)
      url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:'
      const socket = new WebSocket(url, ['nexus-observe-v1', `ticket.${ticket.ticket}`])
      this.socket = socket
      socket.onmessage = event => {
        if (this.disposed || this.socket !== socket) return
        try {
          if (typeof event.data !== 'string' || new TextEncoder().encode(event.data).length > (this.applicationMode ? APPLICATION_MAX_BYTES : MAX_FRAME_BYTES + 256)) throw new RemoteError(403)
          const message = JSON.parse(event.data) as Record<string, unknown>
          if(this.applicationMode&&message.type==='operationResponse'){if(new TextEncoder().encode(event.data).length>OPERATION_RESPONSE_BYTES)throw new RemoteError(403);this.operations.receive(message);return}
          if (this.applicationMode && typeof message.type === 'string' && message.type.startsWith('application')) {
            this.application.receive(message); return
          }
          if (new TextEncoder().encode(event.data).length > MAX_FRAME_BYTES + 256) throw new RemoteError(403)
          if (message.type === 'session' && Object.keys(message).length === 2 && typeof message.sessionId === 'string' && /^[0-9a-f-]{36}$/.test(message.sessionId)) {
            if (this.applicationMode) { this.application.open(); this.operations.open() }
            this.sessionId = message.sessionId
            // A session proves the SERVICE is reachable, so the reconnect backoff starts over
            // here rather than only when a data frame lands. Those are different failures and
            // deserve different patience: a station that is up but briefly quiet - a busy Nexus,
            // a WAN blip - used to escalate exactly like an unreachable one, because every
            // reconnect during the quiet period incremented `attempt` and none of them carried
            // data to reset it. After a short outage the next retry was already 16 to 30 seconds
            // away, and the workspace sat on "Station data unavailable" for all of it. Genuine
            // connect failures never reach this line, so they still back off as before.
            this.attempt = 0
            clearInterval(this.renewal)
            this.renewal = setInterval(() => {void this.renew(socket)},30000)
          } else if (message.type === 'observation' && Object.keys(message).length === 3 && Number.isSafeInteger(message.sentAtMs) && this.anchor) {
            const received = performance.now()
            // HTTP request start is a conservative clock anchor. This includes
            // delivery delay without trusting the phone's wall clock.
            const transit = received - this.anchor.start - (Number(message.sentAtMs) - this.anchor.server)
            if (transit >= STALE_MS) throw new RemoteError(503)
            const parsed=parseFrame(message.frame,'native')
            // A wall-clock correction can invalidate the HTTP-to-monotonic
            // mapping without invalidating this socket's application replies.
            // Hide observation until authenticated renewal establishes a new
            // anchor. ACK only the structurally validated receipt, never data
            // shown as current. No clock tolerance weakens the freshness gate.
            if(transit<0){this.latest=null;if(this.sessionId)void this.renew(socket,true);else socket.close(1000,'clockUnavailable')}
            else {this.latest={frame:ageFrame(parsed,transit),at:received};this.attempt=0;this.clockFailures=0}
            const frame=parsed
            const ack = JSON.stringify({ type: 'ack', epoch: frame.epoch, sequence: frame.sequence })
            // The full workspace shares this socket with its bounded reads and
            // subscriptions. An observation ACK must use that same queue budget.
            // Include the pending write; neither mode may grow its queue freely.
            if (socket.bufferedAmount + new TextEncoder().encode(ack).length > (this.applicationMode ? (this.operations.enabled?OPERATION_REQUEST_BYTES:2048) : 512)) throw new RemoteError(503)
            socket.send(ack)
          } else throw new RemoteError(403)
        } catch { this.latest = null; this.application.disconnected(); this.operations.disconnected(); socket.close(1000, 'invalidObservation') }
      }
      socket.onclose = () => { if (this.socket === socket) { this.socket = null; this.retry() } }
      socket.onerror = () => { this.latest = null; this.application.disconnected(); this.operations.disconnected() }
    } catch (error) {
      if (error instanceof RemoteError && [401, 403].includes(error.status)) { this.latest = null; return }
      this.retry()
    }
  }
}
