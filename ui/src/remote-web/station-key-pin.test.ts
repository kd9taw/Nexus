// @vitest-environment jsdom
// Security review S3-L1 (2026-10-03): the page checked each stream answer against the key the Remote
// service lists for the station, afresh at every check. That holds against a relay, which never
// carries the listing. It did not hold against the service's own records: changed (a compromised
// database), they could list a key of someone else's, and an answer signed with it was taken. Now the
// page keeps the first key the service lists for each station, and while the service lists any other
// the stream is refused before its answer is looked at, so nothing reaches the browser. Taking the new
// key is the operator's own act on the page, never automatic.
//
// Everything here runs through the page's own connection and stream link, with real WebCrypto keys
// made at run time and answers signed as the contract's README describes the bytes. The kept keys are
// in memory: jsdom has no IndexedDB.
import { afterEach, expect, it, vi } from 'vitest'
import { HostedConnection, type BrowserClient } from './client'
import { deviceKey } from './device-key'
import { acceptStationKey, type StationPins } from './station-key'
import { ANSWER, LEASE, harness } from './stream-link.testkit'
import { offerFingerprint, shortFingerprint } from './stream-protocol'

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
const reset = () => { vi.useRealTimers(); vi.unstubAllGlobals(); sockets = [] }
afterEach(reset)

const STATION = crypto.randomUUID(), DEVICE = crypto.randomUUID(), SESSION = crypto.randomUUID()
const IDS = { station: STATION, device: DEVICE, session: SESSION }
const CANDIDATE = 'candidate:1 1 udp 2130706175 192.168.1.20 61000 typ host'
const hex = (bytes: ArrayBuffer) => Buffer.from(bytes).toString('hex')
const text = new TextEncoder()
const sha256 = async (bytes: Uint8Array) => new Uint8Array(await crypto.subtle.digest('SHA-256', new Uint8Array(bytes)))

/** A station's own key, made for the run. */
async function stationKey() {
  const pair = await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, false, ['sign', 'verify']) as CryptoKeyPair
  return {
    spki: hex(await crypto.subtle.exportKey('spki', pair.publicKey)),
    sign: async (bytes: Uint8Array) => hex(await crypto.subtle.sign({ name: 'ECDSA', hash: 'SHA-256' }, pair.privateKey, new Uint8Array(bytes))),
  }
}
type StationKey = Awaited<ReturnType<typeof stationKey>>
/** The answer as a station sends it, signed with `key` for `offer` in this session. */
async function signedAnswer(key: StationKey, offer: string) {
  const signed = new Uint8Array([...text.encode('nexus-stream-answer/1'), ...await sha256(offerFingerprint(ANSWER)!),
    ...await sha256(offerFingerprint(offer)!), ...text.encode(IDS.station), ...text.encode(IDS.device), ...text.encode(IDS.session)])
  return ANSWER.replace('\r\nm=', `\r\na=nexus-station-signature:${await key.sign(signed)}\r\nm=`)
}

/** Keys kept in memory as the browser keeps them: the first one kept for a station stays until it is
 *  replaced or forgotten. */
function memoryPins(initial: Record<string, string> = {}) {
  const kept = new Map(Object.entries(initial))
  const pins: StationPins = {
    keep: async (id, key) => { if (!kept.has(id)) kept.set(id, key); return kept.get(id)! },
    replace: async (id, key) => { kept.set(id, key) },
    forget: async id => { kept.delete(id) },
  }
  return { kept, pins }
}
/** A browser that cannot keep one (no IndexedDB). */
const unkept: StationPins = {
  keep: async () => { throw Error('no IndexedDB') },
  replace: async () => { throw Error('no IndexedDB') },
  forget: async () => { throw Error('no IndexedDB') },
}

/** A page streaming STATION as DEVICE in SESSION, its offer sent; `listed` is the station key the
 *  service lists for the station now, and `pins` what this browser keeps. */
async function offered(listed: string | null, pins: StationPins) {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'performance'] })
  vi.stubGlobal('WebSocket', Socket)
  const ticket = crypto.randomUUID().replace(/-/g, '') + crypto.randomUUID().replace(/-/g, '')
  const post = vi.fn(async () => ({ ticket, serverNow: 1000 }))
  const observationTicket = async () => ({ body: await post(), startedAt: performance.now() })
  const pairs = new Map<string, CryptoKeyPair>()
  const store = { get: async (id: string) => pairs.get(id),
    keep: async (id: string, pair: CryptoKeyPair) => { if (!pairs.has(id)) pairs.set(id, pair); return pairs.get(id)! } }
  // The page's own answer check, not the harness's.
  const h = harness({ sign: false, verify: false })
  const remote = new HostedConnection({ post, observationTicket, operationVersion: 4 } as unknown as BrowserClient, STATION, true, h.env,
    { id: DEVICE, key: () => deviceKey(STATION, store) }, listed, pins)
  remote.start(); await vi.advanceTimersByTimeAsync(0)
  const socket = sockets[sockets.length - 1]
  socket.receive({ type: 'session', sessionId: SESSION })
  await remote.stream.start(LEASE)
  const offer = socket.sent.map(value => JSON.parse(value)).find(m => m.type === 'streamSignal' && m.payload.kind === 'offer')
  expect(offer?.payload.sdp, 'premise: the page offered').toBeTypeOf('string')
  return { h, remote, offer: offer.payload.sdp as string, socket }
}
/** The relay hands the page an answer and the candidate behind it; then the page decides (up to five
 *  seconds of real time, for WebCrypto on a busy machine). */
async function answered(stream: Awaited<ReturnType<typeof offered>>, sdp: string) {
  stream.socket.receive({ type: 'streamSignal', payload: { kind: 'answer', sdp } })
  stream.socket.receive({ type: 'streamSignal', payload: { kind: 'candidate', candidate: CANDIDATE, sdpMid: '0' } })
  const turn = () => new Promise(resolve => setImmediate(resolve)), until = Date.now() + 5000
  while (Date.now() < until && !stream.h.peer.remote && stream.remote.stream.getSnapshot().phase !== 'ended') await turn()
  for (let i = 0; i < 20; i++) await turn()
}
const refused = (stream: Awaited<ReturnType<typeof offered>>) => stream.remote.stream.getSnapshot()

it('S3-L1: a page that kept key A refuses the stream while the service lists key B, even an answer B signed: nothing reaches the browser', async () => {
  const a = await stationKey(), b = await stationKey()
  // Positive control: the same answer, signed with B for this offer and session, is taken by a page
  // that kept B. So what refuses it below is the kept key, never its signature.
  const control = await offered(b.spki, memoryPins({ [STATION]: b.spki }).pins)
  const sdp = await signedAnswer(b, control.offer)
  await answered(control, sdp)
  expect(control.h.peer.remote, 'control: a page that kept B takes B\'s own answer').toEqual({ type: 'answer', sdp })
  control.remote.stop(); reset()

  const { kept, pins } = memoryPins({ [STATION]: a.spki })
  const stream = await offered(b.spki, pins)
  await answered(stream, await signedAnswer(b, stream.offer))
  expect(stream.h.peer.remote, 'the browser was handed an answer under a key this page never kept').toBeNull()
  expect(stream.h.peer.candidates, 'the browser probes no address it names').toEqual([])
  expect(refused(stream)).toMatchObject({ phase: 'ended', reason: 'stationKeyChanged' })
  expect(kept.get(STATION), 'what the service lists never replaces the kept key').toBe(a.spki)
  stream.remote.stop()
})

it('S3-L1: first sight pins: with no key kept for the station, the one the service lists is kept, and holds from then on', async () => {
  const a = await stationKey(), b = await stationKey()
  const { kept, pins } = memoryPins()
  const first = await offered(a.spki, pins)
  const sdp = await signedAnswer(a, first.offer)
  await answered(first, sdp)
  expect(first.h.peer.remote, 'control: the station\'s own answer is taken').toEqual({ type: 'answer', sdp })
  expect(kept.get(STATION), 'the first key listed is the one kept').toBe(a.spki)
  first.remote.stop(); reset()
  const later = await offered(b.spki, pins)
  await answered(later, await signedAnswer(b, later.offer))
  expect(later.h.peer.remote).toBeNull()
  expect(refused(later)).toMatchObject({ phase: 'ended', reason: 'stationKeyChanged' })
  later.remote.stop()
})

it('S3-L1: the new key is taken only by the operator\'s accept: another try changes nothing', async () => {
  const a = await stationKey(), b = await stationKey()
  const { kept, pins } = memoryPins({ [STATION]: a.spki })
  for (const attempt of [1, 2]) {
    const stream = await offered(b.spki, pins)
    await answered(stream, await signedAnswer(b, stream.offer))
    expect(refused(stream), `try ${attempt}`).toMatchObject({ phase: 'ended', reason: 'stationKeyChanged' })
    stream.remote.stop(); reset()
  }
  expect(kept.get(STATION), 'nothing but the accept replaces it').toBe(a.spki)
  await acceptStationKey(STATION, b.spki, pins)
  expect(kept.get(STATION)).toBe(b.spki)
  const stream = await offered(b.spki, pins)
  const sdp = await signedAnswer(b, stream.offer)
  await answered(stream, sdp)
  expect(stream.h.peer.remote, 'accepted: B\'s own answer is taken').toEqual({ type: 'answer', sdp })
  stream.remote.stop()
})

it('S3-L1: the kept key is a check besides the signature, never instead of it', async () => {
  const a = await stationKey(), relay = await stationKey()
  // Kept and listed alike, an answer signed with another key is still refused by its signature.
  const forged = await offered(a.spki, memoryPins({ [STATION]: a.spki }).pins)
  await answered(forged, await signedAnswer(relay, forged.offer))
  expect(forged.h.peer.remote).toBeNull()
  expect(refused(forged)).toMatchObject({ phase: 'ended', reason: 'stationKeyMismatch' })
  forged.remote.stop(); reset()
  // A browser that cannot keep a key is held to the listed key alone, as before keys were kept: the
  // station's own answer is taken (control) and a forged one is refused.
  const signed = await offered(a.spki, unkept)
  const sdp = await signedAnswer(a, signed.offer)
  await answered(signed, sdp)
  expect(signed.h.peer.remote).toEqual({ type: 'answer', sdp })
  signed.remote.stop(); reset()
  const unsigned = await offered(a.spki, unkept)
  await answered(unsigned, await signedAnswer(relay, unsigned.offer))
  expect(unsigned.h.peer.remote).toBeNull()
  expect(refused(unsigned)).toMatchObject({ phase: 'ended', reason: 'stationKeyMismatch' })
  unsigned.remote.stop()
})

it('S3-L1: no key listed, or a listing that is not a key, keeps nothing', async () => {
  const a = await stationKey()
  const { kept, pins } = memoryPins()
  const none = await offered(null, pins)
  await answered(none, await signedAnswer(a, none.offer))
  expect(refused(none)).toMatchObject({ phase: 'ended', reason: 'stationNotSigned' })
  none.remote.stop(); reset()
  const junk = await offered('not a key', pins)
  await answered(junk, await signedAnswer(a, junk.offer))
  expect(refused(junk)).toMatchObject({ phase: 'ended', reason: 'stationKeyMismatch' })
  junk.remote.stop(); reset()
  expect([...kept.keys()], 'nothing kept').toEqual([])
  // Control: the station's key, once listed, is kept.
  const listed = await offered(a.spki, pins)
  await answered(listed, await signedAnswer(a, listed.offer))
  expect(kept.get(STATION)).toBe(a.spki)
  listed.remote.stop()
})

it('S3-L1: a key is shown as the first 128 bits of its fingerprint in eight groups of four, and an older Nexus\'s four groups lead it', () => {
  // SHA-256 of a made-up key shape (the P-256 SPKI prefix, 04, then 0xab sixty-four times), not a key.
  const fingerprint = 'a8ddd2ffad4930ac6b77647b8de0d37935aaeb9fb957326da6370df67205dc77'
  const shown = shortFingerprint(fingerprint)
  expect(shown).toBe('A8DD D2FF AD49 30AC 6B77 647B 8DE0 D379')
  expect(shown.replace(/ /g, '').length * 4, 'bits shown').toBeGreaterThanOrEqual(128)
  // The LAN pairing's form: uppercase, its first 32 hex digits, groups of four.
  expect(shown).toBe((fingerprint.toUpperCase().slice(0, 32).match(/.{4}/g) ?? []).join(' '))
  // Before, both ends showed the first eight bytes: those four groups are the first four here, so an
  // older Nexus at the shack can still be compared group by group.
  const older = (fingerprint.slice(0, 16).toUpperCase().match(/.{1,4}/g) ?? []).join(' ')
  expect(older).toBe('A8DD D2FF AD49 30AC')
  expect(shown.startsWith(`${older} `)).toBe(true)
})
