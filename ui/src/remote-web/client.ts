import { Auth0Client } from '@auth0/auth0-spa-js'
import { ageFrame, MAX_FRAME_BYTES, parseFrame, STALE_MS } from '../remote-monitor/protocol'
import type { MonitorFrame } from '../remote-monitor/protocol'
import type { MonitorSource } from '../remote-monitor/session'

export type AccountSession = {
  accountId: string
  entitlement: { accountId: string; enabled: boolean; expiresAt: number }
  stations: { id: string; name: string; device: { id: string; name: string; approved: number } | null }[]
}
export class RemoteError extends Error {
  constructor(readonly status: number) { super('remoteUnavailable') }
}
async function boundedJson<T>(path: string, options: RequestInit): Promise<T> {
  const controller = new AbortController()
  const cancel = () => controller.abort()
  if (options.signal?.aborted) cancel()
  options.signal?.addEventListener('abort', cancel, { once: true })
  const timer = setTimeout(cancel, 10000)
  try {
    const response = await fetch(path, { ...options, signal: controller.signal })
    if (!response.ok) throw new RemoteError(response.status)
    // Keep both cancellation and the deadline active until the body is read.
    return await response.json() as T
  }
  finally { clearTimeout(timer); options.signal?.removeEventListener('abort', cancel) }
}
export class BrowserClient {
  constructor(private readonly auth: Auth0Client) {}
  static async load(): Promise<BrowserClient | null> {
    const config = await boundedJson<{ issuer: string; audience: string; clientId: string; ready: boolean }>(
      '/api/remote/config', { cache: 'no-store', credentials: 'omit' })
    if (!config.ready) return null
    const issuer = new URL(config.issuer)
    if (issuer.protocol !== 'https:' || issuer.username || issuer.password || issuer.search || issuer.hash) throw new RemoteError(503)
    const auth = new Auth0Client({ domain: issuer.host, clientId: config.clientId,
      cacheLocation: 'memory', useRefreshTokens: false, httpTimeoutInSeconds: 10,
      authorizationParams: { audience: config.audience, redirect_uri: window.location.origin, scope: 'openid profile' } })
    const query = new URLSearchParams(window.location.search)
    if (query.has('code') || query.has('error')) {
      try { await auth.handleRedirectCallback() }
      finally { window.history.replaceState({}, '', '/') }
    } else {
      try { await auth.checkSession() } catch { /* interactive login stays available */ }
    }
    return new BrowserClient(auth)
  }
  authenticated(): Promise<boolean> { return this.auth.isAuthenticated() }
  signIn(): Promise<void> { return this.auth.loginWithRedirect() }
  signOut(): Promise<void> { return this.auth.logout({ logoutParams: { returnTo: window.location.origin } }) }
  async post<T>(path: string, body: object = {}, signal?: AbortSignal): Promise<T> {
    let token: string
    try { token = await this.auth.getTokenSilently() } catch { throw new RemoteError(401) }
    return boundedJson<T>(`/api/remote/${path}`, {
      method: 'POST', credentials: 'same-origin', cache: 'no-store', signal,
      headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' }, body: JSON.stringify(body),
    })
  }
}

/** A single socket and a single latest frame. Tickets and JWTs never enter URLs,
 * storage or display text. Only observation ACKs can leave this capability. */
export class HostedConnection {
  readonly source: MonitorSource
  private socket: WebSocket | null = null
  private latest: { frame: MonitorFrame; at: number } | null = null
  private disposed = false
  private attempt = 0
  private reconnectTimer: ReturnType<typeof setTimeout> | undefined
  private renewal: ReturnType<typeof setInterval> | undefined
  private abort = new AbortController()
  private renewing = false
  private anchor: { server: number; start: number } | null = null

  constructor(private client: BrowserClient, private stationId: string) {
    this.source = { id: `hosted-${stationId}`, kind: 'native', read: async signal => {
      if (signal.aborted || this.disposed || this.socket?.readyState !== WebSocket.OPEN || !this.latest) throw new RemoteError(503)
      return ageFrame(this.latest.frame, performance.now() - this.latest.at)
    } }
  }
  start(): void { void this.connect() }
  stop(): void {
    this.disposed = true; this.abort.abort(); this.latest = null
    clearTimeout(this.reconnectTimer); clearInterval(this.renewal)
    this.socket?.close(1000, 'disconnected'); this.socket = null
  }
  private retry(): void {
    this.latest = null; clearInterval(this.renewal)
    if (this.disposed || this.reconnectTimer) return
    const delay = Math.min(30000, 1000 * 2 ** Math.min(this.attempt++, 5))
    this.reconnectTimer = setTimeout(() => { this.reconnectTimer = undefined; void this.connect() }, delay)
  }
  private async connect(): Promise<void> {
    if (this.disposed) return
    try {
      const start = performance.now()
      const ticket = await this.client.post<{ ticket: string; serverNow: number }>(`stations/${this.stationId}/ticket`, {}, this.abort.signal)
      if (this.disposed) return
      if (!/^[0-9a-f]{64}$/.test(ticket.ticket) || !Number.isSafeInteger(ticket.serverNow)) throw new RemoteError(403)
      this.anchor = { server: ticket.serverNow, start }
      const url = new URL(`/api/remote/stations/${this.stationId}/observe`, window.location.origin)
      url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:'
      const socket = new WebSocket(url, ['nexus-observe-v1', `ticket.${ticket.ticket}`])
      this.socket = socket
      socket.onmessage = event => {
        if (this.disposed || this.socket !== socket) return
        try {
          if (typeof event.data !== 'string' || new TextEncoder().encode(event.data).length > MAX_FRAME_BYTES + 256) throw new RemoteError(403)
          const message = JSON.parse(event.data) as Record<string, unknown>
          if (message.type === 'session' && Object.keys(message).length === 2 && typeof message.sessionId === 'string' && /^[0-9a-f-]{36}$/.test(message.sessionId)) {
            const sessionId = message.sessionId
            clearInterval(this.renewal)
            this.renewal = setInterval(() => {
              if (this.renewing || this.disposed) return
              this.renewing = true
              void this.client.post(`stations/${this.stationId}/renew`, { sessionId }, this.abort.signal)
                .catch(() => socket.close(1000, 'accessEnded')).finally(() => { this.renewing = false })
            }, 30000)
          } else if (message.type === 'observation' && Object.keys(message).length === 3 && Number.isSafeInteger(message.sentAtMs) && this.anchor) {
            const received = performance.now()
            // HTTP request start is a conservative clock anchor. This includes
            // delivery delay without trusting the phone's wall clock.
            const transit = received - this.anchor.start - (Number(message.sentAtMs) - this.anchor.server)
            if (transit < 0 || transit >= STALE_MS) throw new RemoteError(503)
            const frame = ageFrame(parseFrame(message.frame, 'native'), transit)
            this.latest = { frame, at: received }; this.attempt = 0
            if (socket.bufferedAmount > 512) throw new RemoteError(503)
            socket.send(JSON.stringify({ type: 'ack', epoch: frame.epoch, sequence: frame.sequence }))
          } else throw new RemoteError(403)
        } catch { this.latest = null; socket.close(1000, 'invalidObservation') }
      }
      socket.onclose = () => { if (this.socket === socket) { this.socket = null; this.retry() } }
      socket.onerror = () => { this.latest = null }
    } catch (error) {
      if (error instanceof RemoteError && [401, 403].includes(error.status)) { this.latest = null; return }
      this.retry()
    }
  }
}
