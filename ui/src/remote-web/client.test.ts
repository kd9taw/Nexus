// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { BrowserClient, HostedConnection, RemoteError } from './client'
import type { Auth0Client } from '@auth0/auth0-spa-js'
import fixtures from '../remote-monitor/fixtures.v2.json'
import { POLL_MS } from '../remote-monitor/protocol'
import { startMonitor } from '../remote-monitor/session'

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
  closeReason: string | undefined
  close(_code?: number, reason?: string) { this.readyState = 2; this.closeReason = reason }
  receive(value: unknown) { this.onmessage?.({ data: JSON.stringify(value) }) }
  end() { this.readyState = 3; this.onclose?.() }
}
let sockets: Socket[] = []
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); sockets = [] })
async function connection(applicationMode = false,operationVersion=0) {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'performance'] })
  vi.stubGlobal('WebSocket', Socket)
  const ticket = crypto.randomUUID().replace(/-/g, '') + crypto.randomUUID().replace(/-/g, '')
  const post = vi.fn(async (_path: string, _body?: object, _signal?: AbortSignal) => ({ ticket, serverNow: 1000 }))
  const observationTicket = async (stationId: string, signal: AbortSignal) => {
    const startedAt = performance.now()
    return { body: await post(`stations/${stationId}/ticket`, {}, signal), startedAt }
  }
  const remote = new HostedConnection({ post, observationTicket, operationVersion } as unknown as BrowserClient, crypto.randomUUID(), applicationMode)
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
      // Stale, never shown as current; still acknowledged so the relay keeps delivering.
      await expect(remote.source.read(new AbortController().signal)).rejects.toThrow('remoteUnavailable')
      expect(socket.sent.map(value => JSON.parse(value))).toEqual([{ type: 'ack', epoch: fixtures.spe.epoch, sequence: 1 }])
      expect(socket.readyState).toBe(1)
    }
  } finally { remote.stop() }
})

it('refuses fixture data and delayed transport rather than displaying it as native', async () => {
  const { remote, socket } = await connection()
  socket.receive(publication())
  await expect(remote.source.read(new AbortController().signal)).resolves.toBeTruthy()
  socket.receive(publication(2, 'fixture'))
  expect(socket.readyState).toBe(2)
  expect(socket.closeReason).toBe('invalidObservation')
  await expect(remote.source.read(new AbortController().signal)).rejects.toThrow()
  remote.stop()
  const next = await connection()
  await vi.advanceTimersByTimeAsync(3001)
  next.socket.receive(publication())
  await expect(next.remote.source.read(new AbortController().signal)).rejects.toThrow()
  expect(next.socket.readyState).toBe(1)
  expect(next.socket.sent.map(value => JSON.parse(value).type)).toEqual(['ack'])
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


it.each([
  {applicationMode:false,buffered:300,accepted:true},
  {applicationMode:false,buffered:513,accepted:false},
  {applicationMode:true,buffered:700,accepted:true},
  {applicationMode:true,buffered:1900,accepted:true},
  {applicationMode:true,buffered:2049,accepted:false},
])('uses the multiplexed socket budget for observation ACKs: %j',async({applicationMode,buffered,accepted})=>{
  const {remote,socket}=await connection(applicationMode)
  socket.bufferedAmount=buffered
  socket.receive(publication())
  if(accepted){
    expect(socket.readyState).toBe(Socket.OPEN)
    await expect(remote.source.read(new AbortController().signal)).resolves.toBeTruthy()
    expect(JSON.parse(socket.sent[socket.sent.length - 1]!)).toMatchObject({type:'ack',sequence:1})
  }else{
    expect(socket.readyState).toBe(2)
    await expect(remote.source.read(new AbortController().signal)).rejects.toThrow()
    expect(socket.sent).toHaveLength(0)
  }
  remote.stop()
})

it('recalibrates negative clock drift without displaying it or dropping independent application data',async()=>{
 const {remote,post,socket}=await connection(true)
 socket.receive({type:'session',sessionId:crypto.randomUUID()})
 const disconnected=vi.spyOn(remote.application,'disconnected')
 await vi.advanceTimersByTimeAsync(100)
 socket.receive(publication(1,'native',1000))
 expect(await remote.source.read(new AbortController().signal)).toBeTruthy()
 post.mockResolvedValueOnce({ticket:'unused',serverNow:1120})
 socket.receive(publication(2,'native',1120))
 await expect(remote.source.read(new AbortController().signal)).rejects.toThrow()
 await vi.advanceTimersByTimeAsync(0)
 expect(socket.readyState).toBe(1);expect(disconnected).not.toHaveBeenCalled()
 expect(post.mock.calls[post.mock.calls.length-1][0]).toMatch(/renew$/)
 socket.receive(publication(3,'native',1120))
 const frame=await remote.source.read(new AbortController().signal) as typeof fixtures.spe
 expect(frame.station.radio.readings.dial.ageMs).toBe(fixtures.spe.station.radio.readings.dial.ageMs)
 expect(disconnected).not.toHaveBeenCalled();remote.stop()
})
it('bounds unsuccessful clock recalibration and never exposes future readings',async()=>{
 const {remote,socket}=await connection()
 socket.receive({type:'session',sessionId:crypto.randomUUID()})
 for(let i=1;i<=4;i++){
  await vi.advanceTimersByTimeAsync(1000)
  socket.receive(publication(i,'native',1000000+i))
  await expect(remote.source.read(new AbortController().signal)).rejects.toThrow()
  await vi.advanceTimersByTimeAsync(0)
 }
 expect(socket.readyState).toBe(2);remote.stop()
})


it('shares the bounded operation envelope with ACKs without expanding old observer queues',async()=>{
 for(const [version,buffered,accepted]of [[0,3000,false],[1,5000,true],[1,6140,false]] as const){
  const {remote,socket}=await connection(true,version);socket.bufferedAmount=buffered;socket.receive(publication())
  expect(socket.readyState).toBe(accepted?1:2)
  if(accepted)expect(JSON.parse(socket.sent[socket.sent.length-1]).type).toBe('ack')
  else await expect(remote.source.read(new AbortController().signal)).rejects.toThrow()
  remote.stop()
 }
})

const lastOf = <T,>(items: T[]): T => items[items.length - 1]
const controllingState = () => ({ stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(), revision: 1,
  commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false, transmitEpoch: '000000000000002a' })
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const operationRequests = (socket: Socket): any[] => socket.sent.map(value => JSON.parse(value)).filter(m => m.type === 'operationRequest').map(m => m.request)
async function controlled() {
  const { remote, socket } = await connection(true, 4)
  socket.receive({ type: 'session', sessionId: crypto.randomUUID() })
  // The operation client polls once a second from its first tick.
  await vi.advanceTimersByTimeAsync(1000)
  const opened = lastOf(operationRequests(socket))
  expect(opened?.type).toBe('state')
  const state = controllingState()
  socket.receive({ type: 'operationResponse', requestId: opened.requestId, value: state })
  expect(remote.operations.getSnapshot().state?.phase).toBe('controlling')
  const answered = new Set<string>([opened.requestId])
  // Reply to every outstanding heartbeat with the same controlling state.
  const answer = () => {
    for (const request of operationRequests(socket)) if (request.type === 'heartbeat' && !answered.has(request.requestId)) {
      answered.add(request.requestId)
      socket.receive({ type: 'operationResponse', requestId: request.requestId, value: state })
    }
  }
  return { remote, socket, state, answer }
}

it('shows a late observation as stale but keeps the session, the control lease and Stop', async () => {
  const { remote, socket, state, answer } = await controlled()
  const statuses: string[] = []
  const stopMonitor = startMonitor(remote.source, next => statuses.push(next.status))
  try {
    // Positive control: an observation sent now is current.
    socket.receive(publication(1, 'native', 1000 + performance.now()))
    await vi.advanceTimersByTimeAsync(POLL_MS)
    expect(lastOf(statuses)).toBe('current')
    answer()
    // Processed 3001 ms after it was sent: a stalled page or a slow link.
    socket.receive(publication(2, 'native', 1000 + performance.now() - 3001))
    expect(socket.readyState).toBe(1)
    expect(socket.closeReason).toBeUndefined()
    // Acknowledged (the relay holds one unacknowledged frame) but never shown as current.
    expect(JSON.parse(lastOf(socket.sent))).toEqual({ type: 'ack', epoch: fixtures.spe.epoch, sequence: 2 })
    await expect(remote.source.read(new AbortController().signal)).rejects.toThrow('remoteUnavailable')
    const before = operationRequests(socket).length
    await vi.advanceTimersByTimeAsync(1250)
    // The display is stale, so gestures bound to the displayed station refuse.
    expect(lastOf(statuses)).toBe('unavailable')
    // The session and its lease continue: the next poll renews that same lease.
    expect(remote.operations.getSnapshot().connected).toBe(true)
    expect(operationRequests(socket).slice(before)).toContainEqual(expect.objectContaining({ type: 'heartbeat', leaseId: state.leaseId }))
    const stopped = remote.operations.stopTransmit()
    const stop = lastOf(operationRequests(socket))
    expect(stop).toMatchObject({ type: 'stopTransmit', stationBootId: state.stationBootId, leaseId: state.leaseId, transmitEpoch: state.transmitEpoch })
    socket.receive({ type: 'operationResponse', requestId: stop.requestId, value: { stop: 'accepted' } })
    await expect(stopped).resolves.toEqual({ stop: 'accepted' })
    // A following on-time observation is current again.
    socket.receive(publication(3, 'native', 1000 + performance.now()))
    await vi.advanceTimersByTimeAsync(POLL_MS)
    expect(lastOf(statuses)).toBe('current')
  } finally { stopMonitor(); remote.stop() }
})

it('still ends station control when the socket really closes, and Stop cannot reuse the old owner token', async () => {
  const { remote, socket } = await controlled()
  socket.end()
  expect(remote.operations.getSnapshot()).toMatchObject({ connected: false, state: null })
  const count = operationRequests(socket).length
  await expect(remote.operations.stopTransmit()).rejects.toThrow()
  expect(operationRequests(socket)).toHaveLength(count)
  remote.stop()
})

it.each([
  ['a fixture observation', { type: 'observation', sentAtMs: 1000, frame: { ...fixtures.spe, source: 'fixture', sequence: 1 } }, 'invalidObservation'],
  ['an invalid operation response', { type: 'operationResponse', requestId: '00000000-0000-4000-8000-000000000001', value: { stop: 'stopped' } }, 'invalidOperation'],
  ['an application frame nobody requested', { type: 'applicationFrame', requestId: '00000000-0000-4000-8000-000000000001', updates: [] }, 'invalidApplication'],
  ['an unknown message', { type: 'surprise' }, 'invalidMessage'],
])('closes with a named reason on a protocol error: %s', async (_name, message, reason) => {
  const { remote, socket } = await controlled()
  socket.receive(message)
  expect(socket.readyState).toBe(2)
  expect(socket.closeReason).toBe(reason)
  expect(remote.operations.getSnapshot().connected).toBe(false)
  remote.stop()
})

it('tells the page once when a ticket is refused, and not for a failure it will retry', async () => {
  const { remote, post, socket } = await connection()
  const refused = vi.fn()
  remote.onRefused = refused
  post.mockRejectedValueOnce(new RemoteError(503))
  socket.end()
  await vi.advanceTimersByTimeAsync(1000)
  expect(post).toHaveBeenCalledTimes(2)
  // Control: an unavailable service is retried, not reported as the end of access.
  expect(refused).not.toHaveBeenCalled()
  post.mockRejectedValue(new RemoteError(403, 'trialRequired'))
  await vi.advanceTimersByTimeAsync(2000)
  expect(post).toHaveBeenCalledTimes(3)
  expect(refused).toHaveBeenCalledTimes(1)
  expect(refused.mock.calls[0]![0].code).toBe('trialRequired')
  await vi.advanceTimersByTimeAsync(120000)
  expect(refused).toHaveBeenCalledTimes(1)
  remote.stop()
})
