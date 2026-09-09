// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { BrowserClient, HostedConnection, RemoteError } from './client'
import type { Auth0Client } from '@auth0/auth0-spa-js'
import fixtures from '../remote-monitor/fixtures.v2.json'

class Socket {
  static OPEN = 1
  readyState = 1
  bufferedAmount = 0
  sent: string[] = []
  onmessage: ((event: { data: unknown }) => void) | null = null
  onclose: (() => void) | null = null
  onerror: (() => void) | null = null
  constructor(readonly url: URL, readonly protocols: string[]) { sockets.push(this) }
  send(value: string) { this.sent.push(value) }
  close() { this.readyState = 2 }
  receive(value: unknown) { this.onmessage?.({ data: JSON.stringify(value) }) }
  end() { this.readyState = 3; this.onclose?.() }
}
let sockets: Socket[] = []
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); sockets = [] })
async function connection() {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'performance'] })
  vi.stubGlobal('WebSocket', Socket)
  const ticket = crypto.randomUUID().replace(/-/g, '') + crypto.randomUUID().replace(/-/g, '')
  const post = vi.fn(async (_path: string, _body?: object, _signal?: AbortSignal) => ({ ticket, serverNow: 1000 }))
  const observationTicket = async (stationId: string, signal: AbortSignal) => {
    const startedAt = performance.now()
    return { body: await post(`stations/${stationId}/ticket`, {}, signal), startedAt }
  }
  const remote = new HostedConnection({ post, observationTicket } as unknown as BrowserClient, crypto.randomUUID())
  remote.start(); await vi.advanceTimersByTimeAsync(0)
  return { remote, post, ticket, socket: sockets[sockets.length - 1] }
}
function publication(sequence = 1, source = 'native', sentAtMs = 1000) {
  return { type: 'observation', sentAtMs, frame: { ...fixtures.spe, source, sequence } }
}

it('uses a one-use subprotocol ticket, accounts for transport/residence age and emits only ACKs', async () => {
  const { remote, ticket, socket } = await connection()
  expect(socket.url.search).toBe('')
  expect(socket.url.toString()).not.toContain(ticket)
  expect(socket.protocols).toEqual(['nexus-observe-v1', `ticket.${ticket}`])
  await vi.advanceTimersByTimeAsync(200)
  socket.receive(publication())
  const current = await remote.source.read(new AbortController().signal) as typeof fixtures.spe
  expect(current.station.radio.readings.dial!.ageMs).toBe(fixtures.spe.station.radio.readings.dial.ageMs + 200)
  await vi.advanceTimersByTimeAsync(300)
  const later = await remote.source.read(new AbortController().signal) as typeof fixtures.spe
  expect(later.station.radio.readings.dial!.ageMs).toBe(current.station.radio.readings.dial!.ageMs + 300)
  expect(socket.sent.map(value => JSON.parse(value))).toEqual([{ type: 'ack', epoch: fixtures.spe.epoch, sequence: 1 }])
  // The close handshake may remain pending; CLOSING must already hide readings.
  socket.close()
  await expect(remote.source.read(new AbortController().signal)).rejects.toThrow('remoteUnavailable')
  remote.stop()
})

it.each([
  { authenticationMs: 0, transportMs: 100, fresh: true },
  { authenticationMs: 4000, transportMs: 100, fresh: true },
  { authenticationMs: 0, transportMs: 3001, fresh: false },
  { authenticationMs: 4000, transportMs: 3001, fresh: false },
])('measures $transportMs ms transport independently of $authenticationMs ms obtaining authority', async ({ authenticationMs, transportMs, fresh }) => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'performance'] })
  vi.stubGlobal('WebSocket', Socket)
  const ticket = crypto.randomUUID().replace(/-/g, '') + crypto.randomUUID().replace(/-/g, '')
  const auth = { getTokenSilently: async () => {
    await new Promise(resolve => setTimeout(resolve, authenticationMs))
    return 'synthetic-account-token'
  } } as unknown as Auth0Client
  vi.stubGlobal('fetch', vi.fn(async () => ({ ok: true,
    json: async () => ({ ticket, serverNow: 1000 + performance.now() }),
  })))
  const remote = new HostedConnection(new BrowserClient(auth), crypto.randomUUID())
  try {
    remote.start()
    await vi.advanceTimersByTimeAsync(authenticationMs)
    expect(sockets).toHaveLength(1)
    const socket = sockets[0]
    await vi.advanceTimersByTimeAsync(200)
    const sentAtMs = 1000 + performance.now()
    await vi.advanceTimersByTimeAsync(transportMs)
    socket.receive(publication(1, 'native', sentAtMs))
    if (fresh) {
      const current = await remote.source.read(new AbortController().signal) as typeof fixtures.spe
      expect(current.station.radio.readings.dial!.ageMs).toBe(fixtures.spe.station.radio.readings.dial.ageMs + transportMs)
      expect(socket.sent.map(value => JSON.parse(value))).toEqual([{ type: 'ack', epoch: fixtures.spe.epoch, sequence: 1 }])
    } else {
      await expect(remote.source.read(new AbortController().signal)).rejects.toThrow('remoteUnavailable')
      expect(socket.sent).toHaveLength(0)
      expect(socket.readyState).toBe(2)
    }
  } finally { remote.stop() }
})

it('refuses fixture data and delayed transport rather than displaying it as native', async () => {
  const { remote, socket } = await connection()
  socket.receive(publication())
  await expect(remote.source.read(new AbortController().signal)).resolves.toBeTruthy()
  socket.receive(publication(2, 'fixture'))
  expect(socket.readyState).toBe(2)
  await expect(remote.source.read(new AbortController().signal)).rejects.toThrow()
  remote.stop()
  const next = await connection()
  await vi.advanceTimersByTimeAsync(3001)
  next.socket.receive(publication())
  expect(next.socket.readyState).toBe(2)
  expect(next.socket.sent).toHaveLength(0)
  next.remote.stop()
})

it('renews account authority over HTTP and aborts renewal/retry on disconnect', async () => {
  const { remote, post, socket } = await connection()
  const sessionId = crypto.randomUUID()
  socket.receive({ type: 'session', sessionId })
  await vi.advanceTimersByTimeAsync(30000)
  expect(post.mock.calls[post.mock.calls.length - 1]?.[0]).toMatch(/\/renew$/)
  expect(post.mock.calls[post.mock.calls.length - 1]?.[1]).toEqual({ sessionId })
  const renewalSignal = post.mock.calls[post.mock.calls.length - 1]?.[2] as AbortSignal
  remote.stop(); socket.end()
  expect(renewalSignal.aborted).toBe(true)
  const calls = post.mock.calls.length
  await vi.advanceTimersByTimeAsync(120000)
  expect(post).toHaveBeenCalledTimes(calls)
  expect(sockets).toHaveLength(1)
})

it('backs off reconnects and stops retries when account/device authority is refused', async () => {
  const { remote, post, socket } = await connection()
  post.mockRejectedValue(new RemoteError(403))
  socket.end()
  await vi.advanceTimersByTimeAsync(999)
  expect(post).toHaveBeenCalledTimes(1)
  await vi.advanceTimersByTimeAsync(1)
  expect(post).toHaveBeenCalledTimes(2)
  await vi.advanceTimersByTimeAsync(120000)
  expect(post).toHaveBeenCalledTimes(2)
  remote.stop()
})

it.each(['deadline', 'disconnect'])('cancels an HTTP body stalled after headers on %s', async reason => {
  vi.useFakeTimers()
  const token = crypto.randomUUID(), cancel = new AbortController()
  let requestSignal: AbortSignal | undefined, readingBody = false
  vi.stubGlobal('fetch', vi.fn(async (_path: string, options: RequestInit) => {
    requestSignal = options.signal as AbortSignal
    return { ok: true, json: () => {
      readingBody = true
      return new Promise((_resolve, reject) => requestSignal!.addEventListener('abort', () => reject(new Error('aborted')), { once: true }))
    } }
  }))
  const client = new BrowserClient({ getTokenSilently: async () => token } as unknown as Auth0Client)
  const request = client.post('session', {}, cancel.signal).catch(() => 'aborted')
  await vi.advanceTimersByTimeAsync(0)
  expect(readingBody).toBe(true)
  expect(requestSignal?.aborted).toBe(false)
  if (reason === 'disconnect') cancel.abort()
  else await vi.advanceTimersByTimeAsync(10000)
  expect(requestSignal?.aborted).toBe(true)
  await expect(request).resolves.toBe('aborted')
})
