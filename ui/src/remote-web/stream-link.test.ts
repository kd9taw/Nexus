import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { STREAM_ICE_SERVERS, STREAM_RELAY_WAIT_MS, StreamLink, browserStream } from './stream-link'
import type { AudioEnvironment } from './audio-listen'
import {
  MIC_CONSTRAINTS, MIC_ENDED, STREAM_UPLINK_BUDGET_BYTES, parseHeld, parseMicState, parseReceivedMessage, parseStreamInput,
  secureAnswer,
} from './stream-protocol'
import { ANSWER, CHANNEL, FINGERPRINT, LEASE, OFFER, OFFER_SIGNATURE, RELAY, SIGNAL, byName, harness, last } from './stream-link.testkit'

const stopCase = byName(CHANNEL.controlBrowserToStation, 'stopTransmit')
const TARGET = { stationBootId: stopCase.stationBootId as string, leaseId: LEASE, transmitEpoch: stopCase.transmitEpoch as string }
const keysOf = (value: Record<string, unknown>) => Object.keys(value).sort()

beforeEach(() => { vi.useFakeTimers() })
afterEach(() => { vi.useRealTimers() })

it('A3: without a lease it creates nothing and sends nothing; with one it offers under that lease', async () => {
  const h = harness()
  for (const lease of [null, '', 'not-a-lease', `${LEASE}x`]) await h.link.start(lease)
  expect(h.peers).toHaveLength(0)
  expect(h.signals).toHaveLength(0)
  expect(h.link.getSnapshot().phase).toBe('idle')
  // Positive control: the same link, given the lease this session holds, makes its offer - and the
  // message it would put on the socket is exactly the contract's offer.
  await h.link.start(LEASE)
  expect(h.peers).toHaveLength(1)
  expect(h.signals).toEqual([{ payload: { kind: 'offer', sdp: OFFER, ...OFFER_SIGNATURE }, leaseId: LEASE }])
  expect({ type: 'streamSignal', leaseId: LEASE, payload: h.signals[0].payload }).toEqual(byName(SIGNAL.browserToRoom, 'offer'))
  expect(h.link.getSnapshot().phase).toBe('connecting')
})

it('A5: signs its offer with the device key when it can, and offers unsigned when it cannot - the station says why', async () => {
  const signed = byName(SIGNAL.browserToRoom, 'offer'), unsigned = byName(SIGNAL.browserToRoom, 'offer from a browser without a device key (the station refuses it at admission)')
  for (const [what, sign] of [['no device key at all', false], ['a key that cannot sign', async () => null], ['a signer that fails', async () => { throw Error('no key') }]] as const) {
    const h = harness({ sign })
    await h.link.start(LEASE)
    expect({ type: 'streamSignal', leaseId: LEASE, payload: h.signals[0].payload }, what).toEqual(unsigned)
    expect(h.link.getSnapshot().phase, what).toBe('connecting')
  }
  // What it signs is the offer it sends.
  const seen: string[] = []
  const h = harness({ sign: async () => { seen.push('signed'); return OFFER_SIGNATURE } })
  await h.link.start(LEASE)
  expect(seen).toEqual(['signed'])
  expect({ type: 'streamSignal', leaseId: LEASE, payload: h.signals[0].payload }).toEqual(signed)
})

it('A5: a candidate found while the offer is being signed waits for it, so the station never hears one first', async () => {
  let release: (value: { publicKey: string; signature: string }) => void = () => {}
  const h = harness({ sign: () => new Promise(resolve => { release = resolve }) })
  const starting = h.link.start(LEASE)
  await Promise.resolve(); await Promise.resolve(); await Promise.resolve()
  const reflexive = byName(SIGNAL.browserToRoom, 'candidate (reflexive)').payload as { candidate: string; sdpMid: string }
  h.peer.onicecandidate?.({ candidate: { candidate: reflexive.candidate, sdpMid: reflexive.sdpMid } })
  expect(h.signals, 'nothing before the offer').toEqual([])
  release(OFFER_SIGNATURE)
  await starting
  expect(h.signals.map(s => s.payload.kind)).toEqual(['offer', 'candidate'])
  // And once the offer is out, a candidate goes at once.
  h.peer.onicecandidate?.({ candidate: { candidate: reflexive.candidate, sdpMid: reflexive.sdpMid } })
  expect(h.signals.map(s => s.payload.kind)).toEqual(['offer', 'candidate', 'candidate'])
})

it('offers VP8-only receive video, a microphone line it only sends, and the contract\'s three channels', async () => {
  const h = harness()
  await h.link.start(LEASE)
  expect(h.peer.transceivers).toEqual([{ kind: 'video', direction: 'recvonly' }, { kind: 'audio', direction: 'sendonly' }])
  expect(h.peer.preferences).toEqual([{ mimeType: 'video/VP8', clockRate: 90000 }])
  // S6: Opus only - the station decodes nothing else - and nothing on it until the operator asks.
  expect(h.peer.micPreferences).toEqual([expect.objectContaining({ mimeType: 'audio/opus', clockRate: 48000 })])
  expect(h.peer.mic.replaced, 'the microphone line carried something before the operator asked').toEqual([])
  expect(h.micAsks, 'the browser was asked for the microphone at the start').toEqual([])
  expect([...h.peer.channels.keys()]).toEqual(['control', 'ptt', 'audio'])
  expect(h.peer.channel('control').init).toEqual({ ordered: true })
  expect(h.peer.channel('ptt').init).toEqual({ ordered: false, maxRetransmits: 0 })
  expect(h.peer.channel('audio').init).toEqual({ ordered: false, maxRetransmits: 0 })
  // A browser that cannot list its codecs still offers; the station picks VP8 from the defaults.
  const bare = harness({ codecs: false })
  await bare.link.start(LEASE)
  expect(bare.peer.preferences).toBeNull()
  expect(bare.signals[0].payload.kind).toBe('offer')
})

it('says so when the browser has no WebRTC, rather than failing silently', async () => {
  const h = harness({ peerThrows: true })
  await h.link.start(LEASE)
  expect(h.link.getSnapshot()).toMatchObject({ phase: 'ended', reason: 'streamUnsupported' })
})

it('A4: refuses an answer without a DTLS fingerprint, or with a plain-RTP line, before the browser sees it', async () => {
  // The positive control is str0m's own answer, from the contract.
  expect(secureAnswer(ANSWER)).toBe(true)
  for (const [what, sdp] of [
    ['no fingerprint (a stripped SDP)', ANSWER.split(`${FINGERPRINT}\r\n`).join('')],
    // str0m writes one per media section: the picture's stripped, the data channels' kept.
    ['one section stripped of its own fingerprint', ANSWER.replace(`${FINGERPRINT}\r\n`, '')],
    ['plain RTP', ANSWER.replace('m=video 9 UDP/TLS/RTP/SAVPF 96 97', 'm=video 9 RTP/AVP 96 97')],
    ['SDES-keyed SRTP', ANSWER.replace('m=video 9 UDP/TLS/RTP/SAVPF 96 97', 'm=video 9 RTP/SAVPF 96 97')],
    ['a truncated fingerprint', ANSWER.replace(FINGERPRINT, 'a=fingerprint:sha-256 AB:CD')],
    ['a SHA-1 fingerprint', ANSWER.replace(FINGERPRINT, 'a=fingerprint:sha-1 ' + FINGERPRINT.split(' ')[1].split(':').slice(0, 20).join(':'))],
  ] as const) {
    expect(sdp, what).not.toBe(ANSWER)
    const h = harness()
    await h.link.start(LEASE)
    h.link.receive({ type: 'streamSignal', payload: { kind: 'answer', sdp } })
    await Promise.resolve()
    expect(h.peer.remote, what).toBeNull()
    expect(h.link.getSnapshot(), what).toMatchObject({ phase: 'ended', reason: 'insecureAnswer' })
    expect(h.peer.closed, what).toBe(true)
    expect(last(h.signals)?.payload, what).toEqual({ kind: 'close' })
  }
  // The fingerprint may cover every section from session level instead.
  const sessionLevel = ANSWER.split(`${FINGERPRINT}\r\n`).join('').replace('t=0 0\r\n', `t=0 0\r\n${FINGERPRINT}\r\n`)
  expect(sessionLevel).not.toBe(ANSWER)
  expect(secureAnswer(sessionLevel)).toBe(true)
  // Positive control on the same path: the contract's answer is applied, unchanged.
  const h = harness()
  await h.link.start(LEASE)
  h.link.receive(byName(SIGNAL.roomToBrowser, 'answer'))
  expect(h.peer.remote).toEqual({ type: 'answer', sdp: ANSWER })
  expect(h.link.getSnapshot().phase).toBe('connecting')
})

it('trickles candidates in the contract\'s shape, and holds one that overtakes its answer until the answer lands', async () => {
  const h = harness()
  await h.link.start(LEASE)
  const reflexive = byName(SIGNAL.browserToRoom, 'candidate (reflexive)').payload as { candidate: string; sdpMid: string }
  h.peer.onicecandidate?.({ candidate: { candidate: reflexive.candidate, sdpMid: reflexive.sdpMid } })
  // The contract has no end-of-candidates marker, and a candidate with no media id is refused there.
  h.peer.onicecandidate?.({ candidate: null })
  h.peer.onicecandidate?.({ candidate: { candidate: 'candidate:1 1 udp 1 192.0.2.1 5000 typ host', sdpMid: null } })
  expect(h.signals.slice(1).map(s => ({ type: 'streamSignal', leaseId: s.leaseId, payload: s.payload })))
    .toEqual([byName(SIGNAL.browserToRoom, 'candidate (reflexive)')])
  const early = byName(SIGNAL.roomToBrowser, 'candidate (reflexive; never host)')
  h.link.receive(early)
  expect(h.peer.candidates).toHaveLength(0)
  h.link.receive(byName(SIGNAL.roomToBrowser, 'answer'))
  await Promise.resolve(); await Promise.resolve()
  const payload = early.payload as { candidate: string; sdpMid: string }
  expect(h.peer.candidates).toEqual([{ candidate: payload.candidate, sdpMid: payload.sdpMid }])
})

it('reads every station message the contract names, and refuses the ones it says a page must refuse', async () => {
  for (const received of SIGNAL.roomToBrowser) expect(() => parseReceivedMessage(received.message), received.name).not.toThrow()
  for (const refused of SIGNAL.pageRefused) expect(() => parseReceivedMessage(refused.message), refused.name).toThrow()
  // `streaming: true` is admission, never an end; each refusal ends the stream in the station's word,
  // and is not answered back.
  const admitted = harness()
  await admitted.link.start(LEASE)
  admitted.link.receive(byName(SIGNAL.roomToBrowser, 'streaming'))
  expect(admitted.link.getSnapshot().phase).toBe('connecting')
  for (const refusal of SIGNAL.roomToBrowser.filter(c => c.name.startsWith('refused: '))) {
    const h = harness()
    await h.link.start(LEASE)
    const sent = h.signals.length
    h.link.receive(structuredClone(refusal.message))
    expect(h.link.getSnapshot(), refusal.name).toMatchObject({ phase: 'ended', reason: refusal.message.reason })
    expect(h.signals, refusal.name).toHaveLength(sent)
  }
  // A malformed message ends a running stream (and is ignored otherwise).
  for (const refused of SIGNAL.pageRefused) {
    const h = harness()
    h.link.receive(structuredClone(refused.message))
    expect(h.link.getSnapshot().phase, refused.name).toBe('idle')
    await h.link.start(LEASE)
    h.link.receive(structuredClone(refused.message))
    expect(h.link.getSnapshot(), refused.name).toMatchObject({ phase: 'ended', reason: 'streamFailed' })
  }
})

it('S9: heartbeats are the contract\'s shape and carry the RTP timestamp of the last presented frame', async () => {
  const h = harness()
  await h.link.start(LEASE)
  h.link.receive(byName(SIGNAL.roomToBrowser, 'answer'))
  await Promise.resolve()
  const control = h.peer.channel('control')
  control.open()
  expect(keysOf(control.sent[0])).toEqual(keysOf(byName(CHANNEL.controlBrowserToStation, 'heartbeat before the first decoded frame')))
  expect(control.sent[0]).toMatchObject({ type: 'heartbeat', leaseId: LEASE, decodedFrameAt: null })
  h.peer.ontrack?.({ track: 'track', streams: [] })
  expect(h.video.srcObject).toEqual({ wrapped: 'track' })
  const decoded = byName(CHANNEL.controlBrowserToStation, 'heartbeat with a decoded frame').decodedFrameAt as number
  h.video.present(decoded)
  h.advance(500)
  expect(last(control.sent)).toMatchObject({ type: 'heartbeat', leaseId: LEASE, decodedFrameAt: decoded })
  // The timestamp is reported as it is, stale or not: the station measures its age, not the page.
  h.advance(3000)
  expect(last(control.sent)).toMatchObject({ type: 'heartbeat', decodedFrameAt: decoded })
  expect(control.sent.filter(m => m.type === 'heartbeat')).toHaveLength(8)
})

it('S9: without a frame timestamp it falls back to the receiver\'s, and only while frames are shown', async () => {
  const h = harness()
  await h.link.start(LEASE)
  h.link.receive(byName(SIGNAL.roomToBrowser, 'answer'))
  await Promise.resolve()
  h.peer.ontrack?.({ track: 'track', streams: ['media'], receiver: { getSynchronizationSources: () => [{ rtpTimestamp: 4242 }] } })
  h.peer.channel('control').open()
  h.video.present()
  h.advance(500)
  expect(last(h.peer.channel('control').sent)).toMatchObject({ decodedFrameAt: 4242 })
  // A browser that can say neither sends null, never an estimate.
  const blind = harness()
  await blind.live()
  blind.video.present()
  blind.advance(500)
  expect(last(blind.peer.channel('control').sent)).toMatchObject({ type: 'heartbeat', decodedFrameAt: null })
})

it('shows the station\'s word on transmit presence from its heartbeat replies', async () => {
  const h = harness()
  await h.live()
  const control = h.peer.channel('control')
  const answering = (name: string) => ({ ...byName(CHANNEL.controlStationToBrowser, name), requestId: last(control.sent)!.requestId })
  expect(h.link.getSnapshot().presence).toBeNull()
  control.deliver(answering('heartbeat reply, presence held'))
  expect(h.link.getSnapshot().presence).toBe(true)
  h.advance(500)
  control.deliver(answering('heartbeat reply, picture stale'))
  expect(h.link.getSnapshot().presence).toBe(false)
  // A reply to something this page never asked is not read as an answer.
  control.deliver(byName(CHANNEL.controlStationToBrowser, 'heartbeat reply, presence held'))
  expect(h.link.getSnapshot().presence).toBe(false)
})

it('shows a stalled picture after two seconds without a frame, and live again when one arrives', async () => {
  const h = harness()
  await h.live()
  expect(h.link.getSnapshot().phase).toBe('live')
  h.advance(1900)
  expect(h.link.getSnapshot().phase).toBe('live')
  h.advance(250)
  expect(h.link.getSnapshot()).toMatchObject({ phase: 'stalled', reason: 'pictureStalled' })
  h.video.present(2000)
  expect(h.link.getSnapshot()).toMatchObject({ phase: 'live', reason: null })
})

it('S7: a held PTT is re-asserted every 100 ms under one hold id, and stops the moment it is released', async () => {
  const h = harness()
  await h.live()
  const ptt = h.peer.channel('ptt')
  const holds = () => ptt.sent.filter(m => m.type === 'pttHold')
  h.link.holdPtt()
  expect(h.link.getSnapshot().ptt).toBe(true)
  expect(keysOf(holds()[0])).toEqual(keysOf(byName(CHANNEL.pttBrowserToStation, 'first hold of a press')))
  for (let i = 0; i < 5; i++) { h.video.present(3000 + i); h.advance(100) }
  expect(holds().map(m => m.seq)).toEqual([0, 1, 2, 3, 4, 5])
  const holdId = holds()[0].holdId
  expect(new Set(holds().map(m => m.holdId))).toEqual(new Set([holdId]))
  h.link.releasePtt()
  expect(last(ptt.sent)).toEqual({ type: 'pttRelease', holdId, seq: 6 })
  expect(keysOf(last(ptt.sent)!)).toEqual(keysOf(byName(CHANNEL.pttBrowserToStation, 'release')))
  h.advance(1000)
  expect(holds()).toHaveLength(6)
  expect(h.link.getSnapshot().ptt).toBe(false)
  // A new press is a new hold id: a press the station has ended never keys again.
  h.link.holdPtt()
  expect(last(holds())!.holdId).not.toBe(holdId)
  expect(last(holds())!.seq).toBe(0)
})

it('S7: shows what the station did with the press, and only for this press', async () => {
  const h = harness()
  await h.live()
  const control = h.peer.channel('control')
  h.link.holdPtt()
  const holdId = h.peer.channel('ptt').sent[0].holdId as string
  // Another press's news does not key this one: the contract's case names a different hold id.
  const other = byName(CHANNEL.controlStationToBrowser, 'ptt keyed')
  expect(other.holdId).not.toBe(holdId)
  control.deliver(other)
  expect(h.link.getSnapshot().keyed).toBe(false)
  control.deliver({ ...other, holdId })
  expect(h.link.getSnapshot().keyed).toBe(true)
  control.deliver({ ...byName(CHANNEL.controlStationToBrowser, 'ptt ended: lapsed'), holdId })
  expect(h.link.getSnapshot().keyed).toBe(false)
})

it('S7: a blind picture lets go of PTT, and no hold is possible without an open ptt channel', async () => {
  const h = harness()
  await h.link.start(LEASE)
  h.link.holdPtt()
  expect(h.link.getSnapshot().ptt).toBe(false)
  const live = harness()
  await live.live()
  live.link.holdPtt()
  live.advance(2200)
  expect(live.link.getSnapshot()).toMatchObject({ phase: 'stalled', ptt: false })
  const count = live.peer.channel('ptt').sent.filter(m => m.type === 'pttHold').length
  live.advance(500)
  expect(live.peer.channel('ptt').sent.filter(m => m.type === 'pttHold')).toHaveLength(count)
})

it('THE DEAD-MAN: re-asserts what the picture holds every 100 ms on ptt, from 0 each stream, and nothing once let go', async () => {
  const h = harness()
  await h.live()
  const ptt = h.peer.channel('ptt'), control = h.peer.channel('control')
  const helds = () => ptt.sent.filter(m => m.type === 'held')
  h.link.holdInput(['Space'], 0)
  expect(helds()).toEqual([{ type: 'held', keys: ['Space'], buttons: 0, seq: 0 }])
  for (let i = 0; i < 5; i++) { h.video.present(3000 + i); h.advance(100) }
  expect(helds().map(m => m.seq)).toEqual([0, 1, 2, 3, 4, 5])
  // A change goes at once; the same set again does not.
  h.link.holdInput(['Space'], 1)
  h.link.holdInput(['Space'], 1)
  expect(helds().slice(6)).toEqual([{ type: 'held', keys: ['Space'], buttons: 1, seq: 6 }])
  // Never past the contract's 16 keys, and never on `control`.
  h.link.holdInput(Array.from({ length: 20 }, (_, i) => `Key${String.fromCharCode(65 + i)}`), 0)
  expect(last(helds())!.keys).toHaveLength(16)
  expect(control.sent.filter(m => m.type === 'held')).toEqual([])
  // Nothing held: nothing sent.
  h.link.holdInput([], 0)
  const count = helds().length
  h.advance(1000)
  expect(helds()).toHaveLength(count)
  // A new stream counts from 0 again, and an ended one stops re-asserting.
  h.link.holdInput(['KeyA'], 0)
  h.link.close()
  const ended = ptt.sent.length
  h.advance(500)
  expect(ptt.sent).toHaveLength(ended)
  await h.live()
  h.link.holdInput(['KeyA'], 0)
  expect(h.peer.channel('ptt').sent).toEqual([{ type: 'held', keys: ['KeyA'], buttons: 0, seq: 0 }])
})

it('re-asserts in the contract\'s own shape: every held case it names parses, and every one it refuses is refused', async () => {
  const helds = CHANNEL.pttBrowserToStation.filter(c => c.message.type === 'held')
  expect(helds.length, 'the contract names re-assertions').toBeGreaterThan(0)
  for (const accepted of helds) expect(() => parseHeld(accepted.message), accepted.name).not.toThrow()
  const refused = CHANNEL.pttRefused.filter(c => c.message.type === 'held')
  expect(refused.length).toBeGreaterThan(0)
  for (const bad of refused) expect(() => parseHeld(bad.message), bad.name).toThrow()
  // And what this page sends has exactly the contract's fields.
  const h = harness()
  await h.live()
  h.link.holdInput(['Space'], 0)
  expect(keysOf(last(h.peer.channel('ptt').sent)!)).toEqual(keysOf(byName(CHANNEL.pttBrowserToStation, 'held: a key')))
})

it('sends Stop in the contract\'s shape on the control channel, past every budget', async () => {
  const h = harness()
  expect(h.link.stopTransmit(TARGET)).toBe(false)
  await h.live()
  const control = h.peer.channel('control')
  control.bufferedAmount = 10 * 1024 * 1024
  expect(h.link.stopTransmit(TARGET)).toBe(true)
  expect(last(control.sent)).toMatchObject({ type: 'stopTransmit', ...TARGET })
  expect(keysOf(last(control.sent)!)).toEqual(keysOf(stopCase))
  expect(h.link.stopTransmit(null)).toBe(false)
})

it('sends input only in shapes the contract accepts, and the contract\'s refused input never passes', () => {
  const inputs = CHANNEL.controlBrowserToStation.filter(c => ['pointer', 'wheel', 'key', 'text'].includes(c.message.type as string))
  expect(inputs.length).toBeGreaterThanOrEqual(10)
  for (const accepted of inputs) expect(parseStreamInput(accepted.message), accepted.name).toEqual(accepted.message)
  const refused = CHANNEL.controlRefused.filter(c => ['pointer', 'wheel', 'key', 'text'].includes(c.message.type as string))
  expect(refused.length).toBeGreaterThanOrEqual(10)
  for (const bad of refused) expect(() => parseStreamInput(bad.message), bad.name).toThrow()
})

it('coalesces moves and wheel deltas to one message a frame, and never reorders a press behind a move', async () => {
  const h = harness()
  await h.live()
  const control = h.peer.channel('control')
  const inputs = () => control.sent.filter(m => m.type !== 'heartbeat')
  const move = (x: number) => ({ ...byName(CHANNEL.controlBrowserToStation, 'pointer move (no button change)'), x }) as never
  h.link.input(move(0.1)); h.link.input(move(0.2)); h.link.input(move(0.3))
  h.advance(16)
  expect(inputs()).toEqual([move(0.3)])
  const wheel = (deltaY: number, modifiers = 0) => ({ ...byName(CHANNEL.controlBrowserToStation, 'wheel'), deltaY, modifiers }) as never
  h.link.input(wheel(40)); h.link.input(wheel(60)); h.link.input(wheel(10, 2))
  h.advance(16)
  expect(inputs().slice(1)).toEqual([wheel(100), wheel(10, 2)])
  h.link.input(move(0.4))
  const down = byName(CHANNEL.controlBrowserToStation, 'pointer down') as never
  h.link.input(down)
  expect(inputs().slice(3)).toEqual([move(0.4), down])
})

it('drops moves, never presses, keys or text, when the control channel is backed up', async () => {
  const h = harness()
  await h.live()
  const control = h.peer.channel('control')
  control.bufferedAmount = 1024 * 1024
  h.link.input(byName(CHANNEL.controlBrowserToStation, 'pointer move (no button change)') as never)
  h.advance(16)
  h.link.input(byName(CHANNEL.controlBrowserToStation, "key down, Space (an ordinary key: the window's own handlers decide what it does)") as never)
  h.link.input(byName(CHANNEL.controlBrowserToStation, 'committed text') as never)
  expect(control.types().filter(type => type !== 'heartbeat')).toEqual(['key', 'text'])
})

it('sends no input before the picture is live, and none the contract would refuse', async () => {
  const h = harness()
  await h.link.start(LEASE)
  h.peer.channel('control').open()
  h.link.input(byName(CHANNEL.controlBrowserToStation, 'key up, Space') as never)
  expect(h.peer.channel('control').types()).toEqual(['heartbeat'])
  const live = harness()
  await live.live()
  live.link.input(byName(CHANNEL.controlRefused, 'pointer outside the frame') as never)
  expect(live.peer.channel('control').types().filter(type => type !== 'heartbeat')).toEqual([])
})

it('ends when the socket goes, when the control channel closes, or when the link fails', async () => {
  const socket = harness()
  await socket.live()
  socket.link.disconnected()
  expect(socket.link.getSnapshot()).toMatchObject({ phase: 'ended', reason: 'streamUnavailable' })
  expect(socket.peer.closed).toBe(true)
  const channel = harness()
  await channel.live()
  channel.peer.channel('control').onclose?.()
  expect(channel.link.getSnapshot()).toMatchObject({ phase: 'ended', reason: 'connectionFailed' })
  const failed = harness()
  await failed.live()
  failed.peer.connectionState = 'failed'
  failed.peer.onconnectionstatechange?.()
  expect(failed.link.getSnapshot()).toMatchObject({ phase: 'ended', reason: 'connectionFailed' })
  expect(last(failed.signals)?.payload).toEqual({ kind: 'close' })
})

it('an operator\'s own close tells the station, lets go of PTT, and returns to idle with the timers gone', async () => {
  const h = harness()
  await h.live()
  h.link.holdPtt()
  const control = h.peer.channel('control'), ptt = h.peer.channel('ptt')
  h.link.close()
  expect(last(h.signals)?.payload).toEqual({ kind: 'close' })
  expect(last(ptt.sent)).toMatchObject({ type: 'pttRelease' })
  expect(h.link.getSnapshot()).toEqual({ phase: 'idle', reason: null, control: false, ptt: false, keyed: false, presence: null,
    mic: 'off', micProcessing: false, station: null, uplinkStalled: false })
  expect(h.video.srcObject).toBeNull()
  const sent = control.sent.length + ptt.sent.length
  h.advance(5000)
  expect(control.sent.length + ptt.sent.length).toBe(sent)
})

it('plays receive audio from the audio channel through the existing player, muted until asked', async () => {
  const decoded: Uint8Array[] = []
  const h = harness()
  ;(h.env.audio as { decoder: AudioEnvironment['decoder'] }).decoder = () => ({ configure: () => {}, decode: chunk => { decoded.push(chunk.data) }, close: () => {} })
  await h.live()
  const audio = h.peer.channel('audio')
  const bundle = byName(CHANNEL.audioStationToBrowser, 'receive audio bundle (the relay\'s audioRx, unchanged)')
  // Arriving before the operator asks: dropped, never played.
  audio.deliver(bundle)
  await vi.advanceTimersByTimeAsync(0)
  expect(decoded).toHaveLength(0)
  h.link.audio.listen(LEASE)
  await vi.advanceTimersByTimeAsync(0)
  audio.deliver({ ...bundle, seq: 1235 })
  await vi.advanceTimersByTimeAsync(0)
  expect(decoded.length).toBeGreaterThan(0)
  expect(h.link.audio.getSnapshot().phase).toBe('live')
  // Listening never leaves this page: nothing about it went to the station.
  expect(h.peer.channel('control').types().filter(type => type !== 'heartbeat')).toEqual([])
})

it('the station saying its audio started turns nothing on: Listen waits for the operator', async () => {
  const h = harness()
  await h.live()
  // Every stream's station says this the moment the audio channel opens. Nobody has asked to
  // listen, so nothing may play and the control must still offer Listen - not "Stop listening"
  // over a player that was never opened.
  h.peer.channel('audio').deliver(byName(CHANNEL.audioStationToBrowser, 'audio started'))
  await vi.advanceTimersByTimeAsync(0)
  expect(h.link.audio.getSnapshot().phase).toBe('off')
  h.link.audio.listen(LEASE)
  await vi.advanceTimersByTimeAsync(0)
  expect(h.link.audio.getSnapshot().phase).toBe('connecting')
})

it('a refusal the station already sent is what the operator hears about when they ask to listen', async () => {
  const h = harness()
  await h.live()
  h.peer.channel('audio').deliver(byName(CHANNEL.audioStationToBrowser, 'audio refused (a station from before the shared encoder, with a browser already listening)'))
  h.link.audio.listen(LEASE)
  await vi.advanceTimersByTimeAsync(0)
  expect(h.link.audio.getSnapshot()).toMatchObject({ phase: 'ended', reason: 'audioInUse' })
})

it('ends rather than waits when its offer cannot be sent', async () => {
  const h = harness()
  const link = new StreamLink(() => { throw Error('socket budget') }, h.env)
  await link.start(LEASE)
  expect(link.getSnapshot()).toMatchObject({ phase: 'ended', reason: 'streamUnsupported' })
})

function paged() {
  const listeners = new Map<string, Set<() => void>>()
  let visibilityState = 'visible'
  const target = {
    addEventListener: (type: string, f: () => void) => { if (!listeners.has(type)) listeners.set(type, new Set()); listeners.get(type)!.add(f) },
    removeEventListener: (type: string, f: () => void) => { listeners.get(type)?.delete(f) },
  }
  return {
    document: { get visibilityState() { return visibilityState }, ...target },
    window: target,
    fire: (type: string) => { if (type === 'hidden') { visibilityState = 'hidden'; type = 'visibilitychange' } for (const f of [...(listeners.get(type) ?? [])]) f() },
    count: () => [...listeners.values()].reduce((n, set) => n + set.size, 0),
  }
}

it('ends the stream when the tab hides or the page is left, and only lets go of PTT when focus goes', async () => {
  for (const event of ['hidden', 'pagehide']) {
    const page = paged(), h = harness()
    h.env.document = page.document; h.env.window = page.window
    await h.live()
    page.fire(event)
    expect(h.link.getSnapshot(), event).toMatchObject({ phase: 'ended', reason: 'streamHidden' })
    expect(last(h.signals)?.payload, event).toEqual({ kind: 'close' })
    expect(page.count(), 'the listeners go with the stream').toBe(0)
  }
  const page = paged(), h = harness()
  h.env.document = page.document; h.env.window = page.window
  await h.live()
  h.link.holdPtt()
  page.fire('blur')
  expect(h.link.getSnapshot()).toMatchObject({ phase: 'live', ptt: false })
  expect(last(h.peer.channel('ptt').sent)).toMatchObject({ type: 'pttRelease' })
})

// ── The microphone (S6, the audio design's §4.7 and M6/M8, the operator's rulings R1 and R2) ────────

it('the microphone is off until the operator turns it on, and only then is the browser asked, with the design\'s settings', async () => {
  const h = harness()
  await h.live()
  expect(h.micAsks, 'asked before the operator turned it on').toEqual([])
  expect(h.link.getSnapshot().mic).toBe('off')
  await h.link.setMic(true)
  expect(h.micAsks).toEqual([MIC_CONSTRAINTS])
  expect(MIC_CONSTRAINTS).toMatchObject({ echoCancellation: false, noiseSuppression: false, autoGainControl: false, channelCount: 1 })
  expect(h.peer.mic.track, 'the track is not on the microphone line').toBe(h.micTracks[0])
  expect(h.link.getSnapshot()).toMatchObject({ mic: 'on', micProcessing: false })
  // Off takes it off the line and stops it, so the browser's microphone indicator goes out.
  await h.link.setMic(false)
  expect(h.peer.mic.track).toBeNull()
  expect(h.micTracks[0].stopped).toBe(true)
  expect(h.link.getSnapshot().mic).toBe('off')
})

it('a microphone the browser refuses is said, and nothing is sent', async () => {
  const h = harness({ microphone: 'denied' })
  await h.live()
  await h.link.setMic(true)
  expect(h.link.getSnapshot().mic).toBe('denied')
  expect(h.peer.mic.replaced).toEqual([])
  // A browser with no way to capture at all says so too.
  const none = harness({ microphone: 'none' })
  await none.live()
  await none.link.setMic(true)
  expect(none.link.getSnapshot().mic).toBe('unavailable')
})

it('§4.7: processing the browser kept on despite the ask is said; CONTROL: none kept on, nothing said', async () => {
  const h = harness({ micSettings: { echoCancellation: false, noiseSuppression: true, autoGainControl: false } })
  await h.live()
  await h.link.setMic(true)
  expect(h.link.getSnapshot()).toMatchObject({ mic: 'on', micProcessing: true })
  const clean = harness()
  await clean.live()
  await clean.link.setMic(true)
  expect(clean.link.getSnapshot()).toMatchObject({ mic: 'on', micProcessing: false })
})

it('the stream ending stops the microphone and takes it off the line', async () => {
  const h = harness()
  await h.live()
  await h.link.setMic(true)
  h.link.close()
  expect(h.micTracks[0].stopped).toBe(true)
  expect(h.peer.mic.track).toBeNull()
  expect(h.link.getSnapshot().mic).toBe('off')
})

it('M8: a window that loses focus lets go of PTT and stops the microphone; focus back, it sends again', async () => {
  const h = harness()
  const listeners = new Map<string, () => void>()
  h.env.window = { addEventListener: (type, f) => { listeners.set(type, f) }, removeEventListener: type => { listeners.delete(type) } }
  await h.live()
  await h.link.setMic(true)
  h.link.holdPtt()
  expect(h.link.getSnapshot().ptt).toBe(true)
  expect(h.micTracks[0].enabled).toBe(true)
  listeners.get('blur')!()
  expect(h.link.getSnapshot().ptt, 'the blur kept PTT held').toBe(false)
  expect(h.micTracks[0].enabled, 'the blur left the microphone sending').toBe(false)
  listeners.get('focus')!()
  expect(h.micTracks[0].enabled, 'focus back and the microphone stays silent').toBe(true)
})

it('M6: an uplink that backs up lets go of the over within 250 ms and stops the microphone, and says so', async () => {
  const h = harness()
  await h.live()
  await h.link.setMic(true)
  h.link.holdPtt()
  const ptt = h.peer.channel('ptt')
  // CONTROL: a backlog shorter than the stall does not release.
  ptt.bufferedAmount = STREAM_UPLINK_BUDGET_BYTES + 1
  h.advance(100)
  ptt.bufferedAmount = 0
  h.advance(100)
  expect(h.link.getSnapshot()).toMatchObject({ ptt: true, uplinkStalled: false })
  // Now it stays backed up.
  ptt.bufferedAmount = STREAM_UPLINK_BUDGET_BYTES + 1
  h.advance(100); h.advance(100); h.advance(100)
  expect(h.link.getSnapshot()).toMatchObject({ ptt: false, uplinkStalled: true })
  expect(h.micTracks[0].enabled, 'the backed-up page kept sending its microphone').toBe(false)
  expect(last(ptt.sent)?.type, 'the release was not tried').toBe('pttRelease')
  // Drained: the next press clears the stall and the microphone sends again.
  ptt.bufferedAmount = 0
  h.link.holdPtt()
  expect(h.link.getSnapshot()).toMatchObject({ ptt: true, uplinkStalled: false })
  expect(h.micTracks[0].enabled).toBe(true)
})

it('shows the station\'s microphone over as the contract carries it, and refuses anything else', async () => {
  const h = harness()
  await h.live()
  const control = h.peer.channel('control')
  control.deliver(byName(CHANNEL.controlStationToBrowser, "mic armed, no audio yet (a held PTT keys nothing until the operator's voice arrives)"))
  expect(h.link.getSnapshot().station).toEqual({ armed: true, keyed: false, noPowerOut: false, ended: null })
  control.deliver(byName(CHANNEL.controlStationToBrowser, 'mic keyed, and the rig reports no power out (display only)'))
  expect(h.link.getSnapshot().station).toEqual({ armed: true, keyed: true, noPowerOut: true, ended: null })
  for (const why of MIC_ENDED) {
    control.deliver(byName(CHANNEL.controlStationToBrowser, `mic over ended: ${why}`))
    expect(h.link.getSnapshot().station).toEqual({ armed: false, keyed: false, noPowerOut: false, ended: why })
  }
  // Outside the contract: ignored, never guessed at.
  for (const bad of [
    { type: 'micState', armed: true, keyed: false, noPowerOut: false, ended: 'nonsense' },
    { type: 'micState', armed: true, keyed: false },
    { type: 'micState', armed: 'yes', keyed: false, noPowerOut: false },
    { type: 'micState', armed: true, keyed: false, noPowerOut: false, extra: 1 },
  ]) {
    expect(() => parseMicState(bad)).toThrow()
    control.deliver(bad)
    expect(h.link.getSnapshot().station?.ended, JSON.stringify(bad)).toBe('routeChanged')
  }
})

// THE RELAY. The service mints Cloudflare TURN credentials for this stream, and the page hands
// them to its peer beside the STUN server it always has. ICE tries every direct path before a relayed
// one, so the relay carries the stream only where nothing direct works; and whatever stops the page
// getting one leaves the stream exactly as it was before the relay existed: direct.
it('the relay the service mints rides beside STUN; a page with no relay to ask builds the peer it always did', async () => {
  let asked = 0
  const h = harness({ relay: async () => { asked++; return { iceServers: [RELAY] } } })
  await h.link.start(LEASE)
  expect(asked).toBe(1)
  expect(h.peer.iceServers).toEqual([...STREAM_ICE_SERVERS, RELAY])
  expect(h.signals.map(s => s.payload.kind)).toEqual(['offer'])
  const direct = harness()
  await direct.link.start(LEASE)
  expect(direct.peer.iceServers).toEqual(STREAM_ICE_SERVERS)
})

it('a relay the service will not or cannot give leaves the stream direct', async () => {
  for (const [what, relay] of [
    ['refused: signed out, not entitled, no relay key, the provider down', () => Promise.reject(Error('relayUnavailable'))],
    ['an answer with no TURN in it', async () => ({ iceServers: [{ urls: ['stun:stun.example.invalid:3478'] }] })],
    ['a TURN entry without its credential', async () => ({ iceServers: [{ urls: RELAY.urls, username: RELAY.username }] })],
    ['a TURN URL that is not one', async () => ({ iceServers: [{ ...RELAY, urls: ['turn:has a space:3478'] }] })],
    ['not an answer at all', async () => 'relay'],
  ] as const) {
    const h = harness({ relay })
    await h.link.start(LEASE)
    expect(h.peer.iceServers, what).toEqual(STREAM_ICE_SERVERS)
    expect(h.signals.map(s => s.payload.kind), what).toEqual(['offer'])
    expect(h.link.getSnapshot().phase, what).toBe('connecting')
  }
})

it('a relay slow to answer costs the relay, never the stream', async () => {
  let answer: (value: unknown) => void = () => {}
  const h = harness({ relay: () => new Promise(resolve => { answer = resolve }) })
  const starting = h.link.start(LEASE)
  await vi.advanceTimersByTimeAsync(STREAM_RELAY_WAIT_MS - 1)
  expect(h.peers, 'still waiting for the relay').toHaveLength(0)
  await vi.advanceTimersByTimeAsync(1)
  await starting
  expect(h.peer.iceServers).toEqual(STREAM_ICE_SERVERS)
  expect(h.signals.map(s => s.payload.kind)).toEqual(['offer'])
  // Its answer arriving after all changes nothing about the stream already offered.
  answer({ iceServers: [RELAY] })
  await vi.advanceTimersByTimeAsync(0)
  expect(h.peers).toHaveLength(1)
})

it('a browser that will not build a peer with the relay still streams direct', async () => {
  const h = harness({ relay: async () => ({ iceServers: [RELAY] }), refuseRelay: true })
  await h.link.start(LEASE)
  expect(h.peers).toHaveLength(1)
  expect(h.peer.iceServers).toEqual(STREAM_ICE_SERVERS)
  expect(h.signals.map(s => s.payload.kind)).toEqual(['offer'])
  expect(h.link.getSnapshot().phase).toBe('connecting')
})

it('a stream ended while its relay was being asked for creates nothing and sends nothing', async () => {
  for (const end of ['close', 'disconnected', 'dispose'] as const) {
    let answer: (value: unknown) => void = () => {}
    const h = harness({ relay: () => new Promise(resolve => { answer = resolve }) })
    const starting = h.link.start(LEASE)
    await Promise.resolve()
    h.link[end]()
    answer({ iceServers: [RELAY] })
    await starting
    expect(h.peers, end).toHaveLength(0)
    expect(h.signals, end).toEqual([])
  }
  // And one started again meanwhile is the only one that goes on.
  const answers: ((value: unknown) => void)[] = []
  const h = harness({ relay: () => new Promise(resolve => { answers.push(resolve) }) })
  const first = h.link.start(LEASE)
  await Promise.resolve()
  h.link.close()
  const second = h.link.start(LEASE)
  await Promise.resolve()
  answers[0]({ iceServers: [RELAY] })
  await first
  expect(h.peers, 'the stream that was closed').toHaveLength(0)
  answers[1]({ iceServers: [] })
  await second
  expect(h.peers).toHaveLength(1)
  expect(h.signals.map(s => s.payload.kind)).toEqual(['offer'])
})

it('the browser builds its peer with exactly the servers it is given, and no relay-only policy', () => {
  const built: unknown[] = []
  vi.stubGlobal('RTCPeerConnection', class { constructor(config: unknown) { built.push(config) } })
  try {
    browserStream().peer([...STREAM_ICE_SERVERS, RELAY])
    expect(built).toEqual([{ iceServers: [...STREAM_ICE_SERVERS, RELAY], bundlePolicy: 'max-bundle', rtcpMuxPolicy: 'require' }])
  } finally { vi.unstubAllGlobals() }
})

// The relay's test switch, set in the browser's console. Forced, the browser offers relay candidates
// alone, so a live picture can only have come through the relay, even on a network where a direct path
// works; with no relay from the service such a stream cannot connect at all, which is what a test of the
// relay needs to see. Anything but the exact word leaves the default, and it is read as each peer is built.
it('the relay test switch: only nexus.remote.relay = force builds a peer that uses the relay alone', () => {
  const built: unknown[] = []
  vi.stubGlobal('RTCPeerConnection', class { constructor(config: unknown) { built.push(config) } })
  const relayed = [...STREAM_ICE_SERVERS, RELAY], env = browserStream()
  const plain = (iceServers: unknown) => ({ iceServers, bundlePolicy: 'max-bundle', rtcpMuxPolicy: 'require' })
  try {
    for (const value of ['on', 'Force', 'relay', 'force ', '1']) { localStorage.setItem('nexus.remote.relay', value); env.peer(relayed) }
    localStorage.setItem('nexus.remote.relay', 'force')
    env.peer(relayed)
    env.peer([...STREAM_ICE_SERVERS])
    localStorage.removeItem('nexus.remote.relay')
    env.peer(relayed)
    expect(built).toEqual([...Array(5).fill(plain(relayed)),
      { ...plain(relayed), iceTransportPolicy: 'relay' },
      // The service gave no relay: still the relay alone, never a direct path.
      { ...plain([...STREAM_ICE_SERVERS]), iceTransportPolicy: 'relay' },
      // Taken away, the next peer is the default again, with no reload.
      plain(relayed)])
    // Storage the browser refuses to read is no switch.
    built.length = 0
    vi.stubGlobal('localStorage', { getItem: () => { throw new DOMException('denied', 'SecurityError') } })
    env.peer(relayed)
    expect(built).toEqual([plain(relayed)])
  } finally { vi.unstubAllGlobals(); localStorage.removeItem('nexus.remote.relay') }
})
