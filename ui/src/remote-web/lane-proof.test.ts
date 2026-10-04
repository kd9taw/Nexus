// @vitest-environment jsdom
// Security review S1-M1 (2026-10-03), the page's half: the relay stamps the session and device on
// everything it hands the station, so the older lanes took a relay's word for which approved browser
// was asking. Now every operation request but Stop and `state`, and every Listen, carries this
// browser's proof: its device key's signature over the message itself, this station, this browser,
// this session and a number never used before in it. The station checks it against the key the
// operator pinned at the radio. Stop stays unsigned on purpose: a forged one only stops.
//
// Keys are made at run time in a memory store; the proofs are checked here against the contract's
// bytes, not against the page's own code.
import { afterEach, expect, it, vi } from 'vitest'
import { HostedConnection, type BrowserClient } from './client'
import { deviceKey } from './device-key'
import { laneProof } from './lane-proof'
import { harness } from './stream-link.testkit'

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
}
let sockets: Socket[] = []
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); sockets = [] })

const STATION = crypto.randomUUID(), DEVICE = crypto.randomUUID(), SESSION = crypto.randomUUID()
const text = new TextEncoder()
const bytes = (value: string) => Uint8Array.from(Buffer.from(value, 'hex'))
// eslint-disable-next-line @typescript-eslint/no-explicit-any
type Sent = Record<string, any>
const sent = (socket: Socket): Sent[] => socket.sent.map(value => JSON.parse(value))
const turn = () => new Promise(resolve => setImmediate(resolve))
/** Signing takes WebCrypto a moment, longer on a busy machine: up to five seconds of real time. */
async function until(what: string, found: () => Sent | undefined): Promise<Sent> {
  for (const end = Date.now() + 5000; Date.now() < end;) { const value = found(); if (value) return value; await turn() }
  throw Error(`never sent: ${what}`)
}
/** Does `proof` hold for `body` on the lane `label`: the signature over the label, SHA-256 of the
 *  body's own bytes, the three ids and the sequence as eight big-endian bytes, under its key? */
async function holds(label: string, body: string, proof: { publicKey: string; seq: number; signature: string }, session = SESSION) {
  const key = await crypto.subtle.importKey('spki', bytes(proof.publicKey), { name: 'ECDSA', namedCurve: 'P-256' }, false, ['verify'])
  const seq = new Uint8Array(8)
  new DataView(seq.buffer).setBigUint64(0, BigInt(proof.seq))
  const signed = new Uint8Array([...text.encode(label), ...new Uint8Array(await crypto.subtle.digest('SHA-256', text.encode(body))),
    ...text.encode(STATION), ...text.encode(DEVICE), ...text.encode(session), ...seq])
  return crypto.subtle.verify({ name: 'ECDSA', hash: 'SHA-256' }, key, bytes(proof.signature), signed)
}
const available = { stationBootId: crypto.randomUUID(), allowed: true, phase: 'available', leaseId: null, revision: 1,
  commandWindowId: null, nextSequence: null, leaseRemainingMs: null, actions: [], txArmed: false, transmitEpoch: null }
const controlling = { ...available, phase: 'controlling', leaseId: crypto.randomUUID(), commandWindowId: crypto.randomUUID(),
  nextSequence: 1, leaseRemainingMs: 5000, transmitEpoch: '000000000000002a' }

it('S1-M1: acquire, heartbeat, release and Listen carry this browser\'s proof; state and Stop never do', async () => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'performance'] })
  vi.stubGlobal('WebSocket', Socket)
  // A browser that can decode audio, so Listen asks the station at all.
  vi.stubGlobal('AudioDecoder', class {})
  const ticket = crypto.randomUUID().replace(/-/g, '') + crypto.randomUUID().replace(/-/g, '')
  const post = vi.fn(async () => ({ ticket, serverNow: 1000 }))
  const observationTicket = async () => ({ body: await post(), startedAt: performance.now() })
  const pairs = new Map<string, CryptoKeyPair>()
  const store = { get: async (id: string) => pairs.get(id),
    keep: async (id: string, pair: CryptoKeyPair) => { if (!pairs.has(id)) pairs.set(id, pair); return pairs.get(id)! } }
  const key = await deviceKey(STATION, store)
  const remote = new HostedConnection({ post, observationTicket, operationVersion: 4 } as unknown as BrowserClient, STATION, true,
    harness().env, { id: DEVICE, key: () => deviceKey(STATION, store) })
  remote.start(); await vi.advanceTimersByTimeAsync(0)
  const socket = sockets[sockets.length - 1]
  socket.receive({ type: 'session', sessionId: SESSION })
  const operation = (type: string) => () => sent(socket).find(m => m.type === 'operationRequest' && m.request.type === type)

  // `state` is how a page learns what Stop is composed from, and it grants nothing: no proof.
  await vi.advanceTimersByTimeAsync(1000)
  const state = await until('state', operation('state'))
  expect(state.proof, 'state carries no proof').toBeUndefined()
  socket.receive({ type: 'operationResponse', requestId: state.request.requestId, value: available })

  // Acquire carries one, over the request exactly as it went on the socket.
  const acquiring = remote.operations.acquire()
  const acquire = await until('acquire', operation('acquire'))
  expect(acquire.proof, 'acquire carries this browser\'s proof').toMatchObject({ publicKey: key!.publicKey })
  expect(await holds('nexus-operation/1', JSON.stringify(acquire.request), acquire.proof)).toBe(true)
  // Controls: the same proof does not hold for another request, another session or another lane.
  expect(await holds('nexus-operation/1', JSON.stringify({ ...acquire.request, stationBootId: crypto.randomUUID() }), acquire.proof)).toBe(false)
  expect(await holds('nexus-operation/1', JSON.stringify(acquire.request), acquire.proof, crypto.randomUUID())).toBe(false)
  expect(await holds('nexus-listen/1', JSON.stringify(acquire.request), acquire.proof)).toBe(false)
  socket.receive({ type: 'operationResponse', requestId: acquire.request.requestId, value: controlling })
  await acquiring

  // The heartbeat that keeps the lease: signed, under a number above the acquire's.
  await vi.advanceTimersByTimeAsync(1000)
  const heartbeat = await until('heartbeat', operation('heartbeat'))
  expect(await holds('nexus-operation/1', JSON.stringify(heartbeat.request), heartbeat.proof)).toBe(true)
  expect(heartbeat.proof.seq).toBeGreaterThan(acquire.proof.seq)
  socket.receive({ type: 'operationResponse', requestId: heartbeat.request.requestId, value: controlling })

  // Listen: signed over what it asks and under which lease.
  remote.audio.listen(controlling.leaseId)
  const listen = await until('audioListen', () => sent(socket).find(m => m.type === 'audioListen' && m.listening))
  expect(await holds('nexus-listen/1', JSON.stringify({ listening: true, leaseId: controlling.leaseId }), listen.proof)).toBe(true)

  // Stop goes at once and unsigned: a forged one only stops.
  void remote.operations.stopTransmit().catch(() => {})
  const stop = operation('stopTransmit')()
  expect(stop, 'Stop leaves on the press, with nothing to wait for').toBeDefined()
  expect(stop!.proof, 'Stop carries no proof').toBeUndefined()
  socket.receive({ type: 'operationResponse', requestId: stop!.request.requestId, value: { stop: 'accepted' } })

  // Release: signed.
  void remote.operations.release()
  const release = await until('release', operation('release'))
  expect(await holds('nexus-operation/1', JSON.stringify(release.request), release.proof)).toBe(true)
  const numbers = [acquire, heartbeat, listen, release].map(m => m.proof.seq)
  expect(new Set(numbers).size, 'no number is used twice in a session').toBe(numbers.length)
  remote.stop()
})

it('S1-M1: the relay hands a proof on only in its exact shape', () => {
  const proof = { publicKey: `3059301306072a8648ce3d020106082a8648ce3d03010703420004${'1'.repeat(128)}`, seq: 1, signature: 'a'.repeat(128) }
  expect(laneProof(proof)).toEqual(proof)
  for (const bad of [{ ...proof, seq: 0 }, { ...proof, seq: 1.5 }, { ...proof, seq: Number.MAX_SAFE_INTEGER + 1 }, { ...proof, seq: '1' },
    { ...proof, signature: 'A'.repeat(128) }, { ...proof, signature: 'a'.repeat(126) }, { ...proof, publicKey: '04' },
    { ...proof, extra: 1 }, { publicKey: proof.publicKey, seq: 1 }, null, [proof]])
    expect(() => laneProof(bad), JSON.stringify(bad)).toThrow()
})
