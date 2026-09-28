import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { StreamLink } from './stream-link'
import type { AudioEnvironment } from './audio-listen'
import { parseReceivedMessage, parseStreamInput, secureAnswer } from './stream-protocol'
import { ANSWER, CHANNEL, FINGERPRINT, LEASE, OFFER, SIGNAL, byName, harness, last } from './stream-link.testkit'

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
  expect(h.signals).toEqual([{ payload: { kind: 'offer', sdp: OFFER }, leaseId: LEASE }])
  expect({ type: 'streamSignal', leaseId: LEASE, payload: h.signals[0].payload }).toEqual(byName(SIGNAL.browserToRoom, 'offer'))
  expect(h.link.getSnapshot().phase).toBe('connecting')
})

it('offers VP8-only receive video and the contract\'s three channels, each with its own delivery', async () => {
  const h = harness()
  await h.link.start(LEASE)
  expect(h.peer.transceivers).toEqual([{ kind: 'video', direction: 'recvonly' }])
  expect(h.peer.preferences).toEqual([{ mimeType: 'video/VP8', clockRate: 90000 }])
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
    ['no fingerprint (a stripped SDP)', ANSWER.replace(`${FINGERPRINT}\r\n`, '')],
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
  control.deliver({ ...byName(CHANNEL.controlStationToBrowser, 'ptt keyed'), holdId })
  expect(h.link.getSnapshot().keyed).toBe(true)
  // Another press's news does not key this one.
  control.deliver({ ...byName(CHANNEL.controlStationToBrowser, 'ptt keyed') })
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
  h.link.input(byName(CHANNEL.controlBrowserToStation, 'key down, Space, the Phone cockpit\'s PTT key') as never)
  h.link.input(byName(CHANNEL.controlBrowserToStation, 'committed text') as never)
  expect(control.types().filter(type => type !== 'heartbeat')).toEqual(['key', 'text'])
})

it('sends no input before the picture is live, and none the contract would refuse', async () => {
  const h = harness()
  await h.link.start(LEASE)
  h.peer.channel('control').open()
  h.link.input(byName(CHANNEL.controlBrowserToStation, 'key up') as never)
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
  expect(h.link.getSnapshot()).toEqual({ phase: 'idle', reason: null, control: false, ptt: false, keyed: false, presence: null })
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

it('a refusal the station already sent is what the operator hears about when they ask to listen', async () => {
  const h = harness()
  await h.live()
  h.peer.channel('audio').deliver(byName(CHANNEL.audioStationToBrowser, 'audio refused'))
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
