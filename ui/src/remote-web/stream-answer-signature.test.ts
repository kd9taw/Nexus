// @vitest-environment jsdom
// Security review S3-M1 (2026-10-03): the page never checked that the stream's answer came from the
// operator's station. A5 binds the browser to the station; nothing bound the station to the browser,
// so a relay that answered the page's offer itself became "the station" for this browser: every key
// typed on the picture, every paste and the microphone went to it, and the browser's ICE checks went
// wherever it said. Now the station signs its answer with its own key, the one its pairing record at
// the service holds, over both DTLS fingerprints and the three ids, and the page checks it before the
// browser is handed anything.
//
// Everything here runs through the page's own connection and stream link, with real WebCrypto keys
// made at run time. The answers are signed in this file, as the contract's README describes the
// bytes, so the page is held to the contract and not to a copy of its own code.
import { afterEach, expect, it, vi } from 'vitest'
import { HostedConnection, type BrowserClient } from './client'
import { deviceKey } from './device-key'
import { ANSWER, LEASE, OFFER, harness } from './stream-link.testkit'
import { offerFingerprint } from './stream-protocol'

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
const IDS = { station: STATION, device: DEVICE, session: SESSION }
/** The station's own host candidate, straight behind its answer: an address on the operator's LAN. */
const CANDIDATE = 'candidate:1 1 udp 2130706175 192.168.1.20 61000 typ host'
const hex = (bytes: ArrayBuffer) => Buffer.from(bytes).toString('hex')
const text = new TextEncoder()
const sha256 = async (bytes: Uint8Array) => new Uint8Array(await crypto.subtle.digest('SHA-256', new Uint8Array(bytes)))

/** A station's own key, made for the run: what Nexus at the shack keeps in its keychain. */
async function stationKey() {
  const pair = await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, false, ['sign', 'verify']) as CryptoKeyPair
  return {
    spki: hex(await crypto.subtle.exportKey('spki', pair.publicKey)),
    sign: async (bytes: Uint8Array) => hex(await crypto.subtle.sign({ name: 'ECDSA', hash: 'SHA-256' }, pair.privateKey, new Uint8Array(bytes))),
  }
}
type StationKey = Awaited<ReturnType<typeof stationKey>>
/** The answer as the station sends it: str0m's SDP with one session-level line carrying the station's
 *  signature over "nexus-stream-answer/1", SHA-256 of the answer's DTLS fingerprint, SHA-256 of the
 *  offer's, and the station, device and session ids. */
async function signedAnswer(key: StationKey, offer: string, ids: typeof IDS, sdp = ANSWER) {
  const signed = new Uint8Array([...text.encode('nexus-stream-answer/1'), ...await sha256(offerFingerprint(sdp)!),
    ...await sha256(offerFingerprint(offer)!), ...text.encode(ids.station), ...text.encode(ids.device), ...text.encode(ids.session)])
  return sdp.replace('\r\nm=', `\r\na=nexus-station-signature:${await key.sign(signed)}\r\nm=`)
}
/** An earlier offer: the page's own, with another DTLS certificate in it. */
const EARLIER_OFFER = OFFER.replace(/a=fingerprint:sha-256 \S+/g, `a=fingerprint:sha-256 ${Array.from({ length: 32 }, (_, i) => (0x10 + i).toString(16).toUpperCase()).join(':')}`)

/** A page streaming STATION as DEVICE in SESSION, its offer sent and signed; `listed` is the station
 *  key the service's pairing record holds for the station, or null. */
async function offered(listed: string | null) {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'performance'] })
  vi.stubGlobal('WebSocket', Socket)
  const ticket = crypto.randomUUID().replace(/-/g, '') + crypto.randomUUID().replace(/-/g, '')
  const post = vi.fn(async () => ({ ticket, serverNow: 1000 }))
  const observationTicket = async () => ({ body: await post(), startedAt: performance.now() })
  const pairs = new Map<string, CryptoKeyPair>()
  const store = { get: async (id: string) => pairs.get(id),
    keep: async (id: string, pair: CryptoKeyPair) => { if (!pairs.has(id)) pairs.set(id, pair); return pairs.get(id)! } }
  // The page's own answer check, not the harness's: the connection builds it from the station key.
  const h = harness({ sign: false, verify: false })
  const remote = new HostedConnection({ post, observationTicket, operationVersion: 4 } as unknown as BrowserClient, STATION, true, h.env,
    { id: DEVICE, key: () => deviceKey(STATION, store) }, listed)
  remote.start(); await vi.advanceTimersByTimeAsync(0)
  const socket = sockets[sockets.length - 1]
  socket.receive({ type: 'session', sessionId: SESSION })
  await remote.stream.start(LEASE)
  const offer = socket.sent.map(value => JSON.parse(value)).find(m => m.type === 'streamSignal' && m.payload.kind === 'offer')
  expect(offer?.payload.sdp, 'premise: the page offered').toBeTypeOf('string')
  return { h, remote, socket, offer: offer.payload.sdp as string }
}
/** The relay hands the page an answer and the candidate behind it; then the page decides. Checking a
 *  signature takes WebCrypto a moment, longer on a busy machine, so this waits up to five seconds of
 *  real time for the decision either way. */
async function answered(stream: Awaited<ReturnType<typeof offered>>, sdp: string) {
  stream.socket.receive({ type: 'streamSignal', payload: { kind: 'answer', sdp } })
  stream.socket.receive({ type: 'streamSignal', payload: { kind: 'candidate', candidate: CANDIDATE, sdpMid: '0' } })
  const turn = () => new Promise(resolve => setImmediate(resolve)), until = Date.now() + 5000
  while (Date.now() < until && !stream.h.peer.remote && stream.remote.stream.getSnapshot().phase !== 'ended') await turn()
  for (let i = 0; i < 20; i++) await turn()
}

it('S3-M1: an answer the station did not sign for this offer and session never reaches the browser', async () => {
  const station = await stationKey(), relay = await stationKey(), neighbour = await stationKey()
  const cases: [string, (offer: string) => Promise<string>, string][] = [
    ['forged: the relay answers with its own certificate and signs it with a key of its own',
      offer => signedAnswer(relay, offer, IDS), 'stationKeyMismatch'],
    ['replayed: the station\'s own signed answer from an earlier session',
      offer => signedAnswer(station, offer, { ...IDS, session: crypto.randomUUID() }), 'stationKeyMismatch'],
    ['replayed: the station\'s own signed answer to an earlier offer',
      () => signedAnswer(station, EARLIER_OFFER, IDS), 'stationKeyMismatch'],
    ['another station\'s: the operator\'s second station answering for itself',
      offer => signedAnswer(neighbour, offer, { ...IDS, station: crypto.randomUUID() }), 'stationKeyMismatch'],
    ['unsigned: the answer of a station from before its key',
      async () => ANSWER, 'stationNotSigned'],
  ]
  for (const [what, make, reason] of cases) {
    const stream = await offered(station.spki)
    await answered(stream, await make(stream.offer))
    expect(stream.h.peer.remote, `${what}: the browser was handed it`).toBeNull()
    expect(stream.h.peer.candidates, `${what}: the browser probes no address it names`).toEqual([])
    expect(stream.remote.stream.getSnapshot(), what).toMatchObject({ phase: 'ended', reason })
    stream.remote.stop()
    vi.useRealTimers(); vi.unstubAllGlobals(); sockets = []
  }
})

it('S3-M1: with no station key from the service, even a signed answer is refused, as one to update', async () => {
  const station = await stationKey()
  const stream = await offered(null)
  await answered(stream, await signedAnswer(station, stream.offer, IDS))
  expect(stream.h.peer.remote).toBeNull()
  expect(stream.remote.stream.getSnapshot()).toMatchObject({ phase: 'ended', reason: 'stationNotSigned' })
  stream.remote.stop()
})

it('S3-M1 control: the station\'s own answer, signed for this offer and session, is taken, and its candidate follows', async () => {
  const station = await stationKey()
  const stream = await offered(station.spki)
  const sdp = await signedAnswer(station, stream.offer, IDS)
  await answered(stream, sdp)
  expect(stream.h.peer.remote).toEqual({ type: 'answer', sdp })
  expect(stream.h.peer.candidates).toEqual([{ candidate: CANDIDATE, sdpMid: '0' }])
  expect(stream.remote.stream.getSnapshot().phase).toBe('connecting')
  stream.remote.stop()
})
