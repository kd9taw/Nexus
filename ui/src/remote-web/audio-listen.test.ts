import { beforeEach, afterEach, expect, it, vi } from 'vitest'
import { AudioLink, resample } from './audio-listen'
import type { AudioEnvironment, DecodedFrame } from './audio-listen'

const LEASE = '8aa041cb-c642-459c-83f3-11a5b720647d'
const EPOCH = '0000000000000001'

/** One bundle of `count` two-byte packets, framed exactly as the station frames them. */
function bundle(seq: number, count = 3, epoch = EPOCH) {
  const bytes: number[] = []
  for (let i = 0; i < count; i++) bytes.push(0, 2, 0x10 + i, 0x20 + i)
  return { type: 'audioRx', seq, epoch, firstFrameMs: seq * 20, frameMs: 20, count, payload: btoa(String.fromCharCode(...bytes)) }
}

function harness(options: { supported?: boolean; contextRate?: number } = {}) {
  let clock = 1000
  const sent: Record<string, unknown>[] = []
  const pushed: Float32Array[] = []
  const resets: number[] = []
  const decoded: Uint8Array[] = []
  const closes = { decoder: 0, playback: 0 }
  let handlers: { output: (f: DecodedFrame) => void; error: () => void } | null = null
  const rate = options.contextRate ?? 48000
  const listeners = { visibility: [] as (() => void)[] }
  let visibilityState = 'visible'
  const env: AudioEnvironment = {
    now: () => clock,
    decoderAvailable: () => options.supported !== false,
    context: () => Promise.resolve({
      sampleRate: rate,
      push: samples => { pushed.push(samples) },
      reset: () => { resets.push(clock) },
      close: () => { closes.playback++; return Promise.resolve() },
    }),
    decoder: h => {
      handlers = h
      return {
        configure: () => {},
        decode: chunk => { decoded.push(chunk.data) },
        close: () => { closes.decoder++ },
      }
    },
    document: {
      get visibilityState() { return visibilityState },
      addEventListener: (_type, f) => listeners.visibility.push(f),
      removeEventListener: (_type, f) => { listeners.visibility = listeners.visibility.filter(g => g !== f) },
    },
  }
  const link = new AudioLink(message => sent.push(message as Record<string, unknown>), env)
  return {
    link, sent, pushed, resets, decoded, closes,
    advance: (ms: number) => { clock += ms; vi.advanceTimersByTime(ms) },
    hide: () => { visibilityState = 'hidden'; for (const f of listeners.visibility) f() },
    // Opus ALWAYS decodes at 48 kHz whatever the output device runs at, which is the
    // whole reason the conversion below has to exist.
    render: (samples: number) => handlers?.output(frame(samples, 48000)),
    fail: () => handlers?.error(),
    get watching() { return listeners.visibility.length },
  }
}
function frame(frames: number, sampleRate: number): DecodedFrame {
  return {
    frames, sampleRate,
    copyTo: target => { for (let i = 0; i < target.length; i++) target[i] = (i + 1) / 1000 },
    close: () => {},
  }
}
/** Let the link's internal `await this.opening` settle. */
const settle = () => Promise.resolve().then(() => {}).then(() => {}).then(() => {})

beforeEach(() => { vi.useFakeTimers() })
afterEach(() => { vi.useRealTimers() })

it('makes no sound and asks for nothing until it is asked to', () => {
  const h = harness()
  expect(h.link.getSnapshot()).toEqual({ phase: 'off', reason: null, supported: true })
  expect(h.sent).toEqual([])
  // A bundle that arrives unasked is taken off the wire and thrown away, never played.
  expect(h.link.receive(bundle(0))).toBe(true)
  expect(h.pushed).toEqual([])
  expect(h.link.getSnapshot().phase).toBe('off')
})

it('says a browser with no decoder cannot do it, rather than failing silently', () => {
  const h = harness({ supported: false })
  expect(h.link.getSnapshot().supported).toBe(false)
  h.link.listen(LEASE)
  expect(h.link.getSnapshot()).toMatchObject({ phase: 'unsupported', reason: 'audioUnsupported' })
  expect(h.sent).toEqual([])
  // The control: the same sequence on a browser that HAS the decoder does ask.
  const able = harness()
  able.link.listen(LEASE)
  expect(able.sent[0]).toEqual({ type: 'audioListen', listening: true, leaseId: LEASE })
})

it('goes live on the first audio and back to off when released', async () => {
  const h = harness()
  h.link.listen(LEASE)
  expect(h.link.getSnapshot().phase).toBe('connecting')
  await settle()
  h.link.receive({ type: 'audioState', listening: true })
  h.link.receive(bundle(0))
  expect(h.link.getSnapshot().phase).toBe('live')
  await settle()
  expect(h.decoded).toHaveLength(3)
  h.render(960)
  expect(h.pushed).toHaveLength(1)
  h.link.release()
  expect(h.link.getSnapshot().phase).toBe('off')
  expect(h.sent[h.sent.length - 1]).toMatchObject({ type: 'audioListen', listening: false })
  expect(h.closes).toEqual({ decoder: 1, playback: 1 })
})

it('surfaces a sequence gap as concealment the operator can see, never as silence', async () => {
  const h = harness()
  h.link.listen(LEASE)
  await settle()
  h.link.receive(bundle(0))
  expect(h.link.getSnapshot().phase).toBe('live')
  // Three bundles lost. The station's sequence is dense, so the hole is KNOWN here and
  // is told to the operator rather than left to be discovered as a quiet band.
  h.link.receive(bundle(12))
  expect(h.link.getSnapshot()).toMatchObject({ phase: 'gap', reason: 'audioGap' })
  await settle()
  expect(h.decoded).toHaveLength(6)
  // And it recovers on its own: the next bundle in sequence goes back to live.
  h.link.receive(bundle(15))
  expect(h.link.getSnapshot().phase).toBe('live')
})

it('drops a replayed or reordered bundle instead of playing it late', async () => {
  const h = harness()
  h.link.listen(LEASE)
  await settle()
  h.link.receive(bundle(0))
  h.link.receive(bundle(3))
  await settle()
  expect(h.decoded).toHaveLength(6)
  // The same bundle again, and one from before it. Audio is never retried, so the only
  // thing a late bundle can do is arrive out of order - and out of order is wrong order.
  h.link.receive(bundle(3))
  h.link.receive(bundle(0))
  await settle()
  expect(h.decoded).toHaveLength(6)
  expect(h.link.getSnapshot().phase).toBe('live')
})

it('treats a capture-source change as an end, never as a gap', async () => {
  const h = harness()
  h.link.listen(LEASE)
  await settle()
  h.link.receive(bundle(0))
  h.link.receive(bundle(3, 3, '0000000000000002'))
  expect(h.link.getSnapshot()).toMatchObject({ phase: 'ended', reason: 'sourceChanged' })
  // The buffer is thrown away rather than played out: the old samples came from a
  // receiver that no longer exists.
  expect(h.resets).toHaveLength(1)
  expect(h.closes.decoder).toBe(1)
})

it('calls it stalled when nothing arrives, and distinguishes that from a quiet band', async () => {
  const h = harness()
  h.link.listen(LEASE)
  await settle()
  h.link.receive(bundle(0))
  expect(h.link.getSnapshot().phase).toBe('live')
  // Control first: audio that keeps arriving is never called stalled, however long the
  // operator listens.
  for (let seq = 3; seq < 300; seq += 3) { h.advance(60); h.link.receive(bundle(seq)) }
  expect(h.link.getSnapshot().phase).toBe('live')
  h.advance(2500)
  expect(h.link.getSnapshot()).toMatchObject({ phase: 'stalled', reason: 'audioStalled' })
  // And it comes back by itself when the link does.
  h.link.receive(bundle(300))
  expect(h.link.getSnapshot().phase).toBe('live')
})

it('stops when the tab is hidden, and tells the station so', async () => {
  const h = harness()
  h.link.listen(LEASE)
  await settle()
  h.link.receive(bundle(0))
  expect(h.watching).toBe(1)
  h.hide()
  expect(h.link.getSnapshot().phase).toBe('off')
  expect(h.sent[h.sent.length - 1]).toMatchObject({ type: 'audioListen', listening: false })
  expect(h.closes).toEqual({ decoder: 1, playback: 1 })
  // The listener is removed with it: a second hide must not send a second stop.
  expect(h.watching).toBe(0)
})

it('ends with the station\'s own word when the station stops it', async () => {
  for (const reason of ['notController', 'audioInUse', 'sourceChanged'] as const) {
    const h = harness()
    h.link.listen(LEASE)
    await settle()
    h.link.receive({ type: 'audioState', listening: false, reason })
    expect(h.link.getSnapshot()).toMatchObject({ phase: 'ended', reason })
    expect(h.closes.playback).toBe(1)
  }
})

it('costs the audio and never the session when a message is malformed', async () => {
  const h = harness()
  h.link.listen(LEASE)
  await settle()
  // Deliberately wrong in three different ways. Each ends the audio and none throws,
  // because the socket this arrived on also carries the operator's Stop control.
  for (const broken of [
    { type: 'audioRx', seq: -1, epoch: EPOCH, firstFrameMs: 0, frameMs: 20, count: 3, payload: '' },
    { type: 'audioState', listening: 'yes' },
    { type: 'audioRx', seq: 0, epoch: 'nope', firstFrameMs: 0, frameMs: 20, count: 3, payload: '' },
  ]) {
    const one = harness()
    one.link.listen(LEASE)
    await settle()
    expect(() => one.link.receive(broken as unknown as Record<string, unknown>)).not.toThrow()
    expect(one.link.getSnapshot().phase).toBe('ended')
  }
  // And a message that is not ours is left alone for the caller to handle.
  expect(h.link.receive({ type: 'observation' })).toBe(false)
})

it('ends on a decoder failure rather than playing nothing and looking live', async () => {
  const h = harness()
  h.link.listen(LEASE)
  await settle()
  h.link.receive(bundle(0))
  await settle()
  h.fail()
  expect(h.link.getSnapshot()).toMatchObject({ phase: 'ended', reason: 'audioUnavailable' })
})

it('adapts to an output device that is not at 48 kHz instead of assuming one', async () => {
  const h = harness({ contextRate: 44100 })
  h.link.listen(LEASE)
  await settle()
  h.link.receive(bundle(0))
  await settle()
  h.render(960)
  // 20 ms at 44.1 kHz. A fixed ratio for the hardware, not a stretch for buffer depth.
  expect(h.pushed[0].length).toBe(882)
})

it('converts by a fixed ratio and leaves a matching rate untouched', () => {
  const samples = Float32Array.from({ length: 4 }, (_, i) => i)
  expect(resample(samples, 48000, 48000)).toBe(samples)
  expect(resample(samples, 48000, 24000)).toHaveLength(2)
  expect(resample(samples, 24000, 48000)).toHaveLength(8)
  // Endpoints are preserved, so nothing is invented at the edges of a frame.
  const up = resample(samples, 24000, 48000)
  expect(up[0]).toBe(0)
  expect(up[up.length - 1]).toBe(3)
})

it('does not ask twice while it is already listening', async () => {
  const h = harness()
  h.link.listen(LEASE)
  h.link.listen(LEASE)
  await settle()
  h.link.receive(bundle(0))
  h.link.listen(LEASE)
  expect(h.sent.filter(m => m.listening === true)).toHaveLength(1)
})
