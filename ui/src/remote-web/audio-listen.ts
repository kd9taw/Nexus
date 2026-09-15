// Listening, from the browser's side: feature detection, the decoder, the sequence
// rules, and the state an operator reads. The buffer itself is in `audio-worklet.ts`,
// where the audio clock is.
//
// WHY WEBCODECS AND NOT WEBRTC. A WebRTC receive leg would give Opus everywhere with
// loss concealment and an adaptive buffer thrown in - and its buffer, NetEq, cannot be
// turned off. NetEq time-stretches audio to manage depth. That is tuned for speech and
// it is corruption on CW, PSK31 or FT8 where timing IS the signal, silently, on any
// jittery link. So the packets come in raw and we own the buffer.
//
// WHY `window.AudioDecoder` AND NOT A WEBCODECS CHECK. Safari 16.4 through 18.7 shipped
// WebCodecs VIDEO only. A page that tested for WebCodecs would have offered listening to
// those browsers and failed at configure() time, which is the worst of both.
//
// THE HOLE, NAMED RATHER THAN HIDDEN: Firefox on Android has no AudioDecoder at all and
// there is no sign of one. Those operators are told the browser cannot do it, which is
// the honest answer and is far better than a control that does nothing.

import {
  AUDIO_FRAME_MS, AUDIO_MESSAGE_BYTES, AUDIO_STALL_MS,
  audioPackets, parseAudioBundle, parseAudioState,
} from './audio-protocol'
import {
  AUDIO_CEILING_MS, AUDIO_PREFILL_MS, AUDIO_WORKLET_NAME, AUDIO_WORKLET_SOURCE,
} from './audio-worklet'

/** What the operator is shown. Each one is a different thing to do about it. */
export type AudioPhase =
  /** Not listening. The page makes no sound until it is asked to. */
  | 'off'
  /** This browser has no AudioDecoder. Nothing to try; say so. */
  | 'unsupported'
  /** Asked, and waiting for the first audio to arrive and fill the buffer. */
  | 'connecting'
  /** Audio is playing. */
  | 'live'
  /** Bundles are being lost. Audio continues, with a bed under the holes. */
  | 'gap'
  /** Nothing has arrived for seconds. The link, not the band. */
  | 'stalled'
  /** A real end: the station's capture source changed, control was lost, or it refused. */
  | 'ended'

export type AudioView = {
  phase: AudioPhase
  /** The station's or the page's own word for why, from the shared vocabulary. */
  reason: string | null
  /** Does this browser have the decoder at all? Read before offering the control. */
  supported: boolean
}

/** Opus always decodes at 48 kHz, whatever rate it was encoded at. */
const DECODE_RATE = 48_000

/** Everything the page reaches for that a test has to stand in for. */
export type AudioEnvironment = {
  now: () => number
  decoderAvailable: () => boolean
  context: () => Promise<AudioPlayback>
  decoder: (handlers: { output: (frame: DecodedFrame) => void; error: () => void }) => AudioDecoderLike
  document?: { visibilityState: string; addEventListener: (type: string, f: () => void) => void; removeEventListener: (type: string, f: () => void) => void }
}
/** The worklet, behind the two things this file does to it. */
export type AudioPlayback = {
  push: (samples: Float32Array) => void
  reset: () => void
  close: () => Promise<void>
  sampleRate: number
}
export type DecodedFrame = { sampleRate: number; frames: number; copyTo: (target: Float32Array, options: { planeIndex: number; format: string }) => void; close: () => void }
export type AudioDecoderLike = {
  configure: (config: { codec: string; sampleRate: number; numberOfChannels: number }) => void
  decode: (chunk: { type: 'key'; timestamp: number; duration: number; data: Uint8Array }) => void
  close: () => void
  state?: string
}

/** Listening, as the page sees it. One per connection; it makes no sound until `listen`. */
export class AudioLink {
  private phase: AudioPhase = 'off'
  private reason: string | null = null
  private view: AudioView = { phase: 'off', reason: null, supported: false }
  private listeners = new Set<() => void>()
  private decoder: AudioDecoderLike | null = null
  private playback: AudioPlayback | null = null
  private opening: Promise<void> | null = null
  /** The next sequence number a bundle should carry. `null` until the first one lands. */
  private expected: number | null = null
  private epoch: string | null = null
  private heardAt = 0
  private timer: ReturnType<typeof setInterval> | undefined
  private stopVisibility: (() => void) | undefined
  /** Frames decoded, only ever counted up: the decoder's timestamps must be monotonic. */
  private frames = 0
  private closed = false

  constructor(
    private readonly send: (message: object) => void,
    private readonly env: AudioEnvironment,
  ) {
    this.view = { phase: 'off', reason: null, supported: env.decoderAvailable() }
  }

  subscribe = (f: () => void): (() => void) => { this.listeners.add(f); return () => { this.listeners.delete(f) } }
  getSnapshot = (): AudioView => this.view
  get supported(): boolean { return this.view.supported }

  /** Ask the station to start feeding this browser. `leaseId` is the browser's claim to
   *  station control; the station re-checks it against its own authority and refuses a
   *  logging-only lease, so this is a request and never an assertion. */
  listen(leaseId: string): void {
    if (this.closed) return
    if (!this.view.supported) { this.set('unsupported', 'audioUnsupported'); return }
    if (this.phase !== 'off' && this.phase !== 'ended') return
    this.expected = null; this.epoch = null; this.frames = 0
    this.heardAt = this.env.now()
    this.set('connecting', null)
    if (!this.tell({ type: 'audioListen', listening: true, leaseId })) { this.set('ended', 'audioUnavailable'); return }
    this.opening = this.open()
    // A tab that is hidden is a tab nobody is listening to, and a station that keeps
    // encoding for it is spending a shack's upload on nothing. The station has its own
    // backstop, but the browser is the one that actually knows.
    const doc = this.env.document
    if (doc) {
      const hidden = () => { if (doc.visibilityState === 'hidden') this.release() }
      doc.addEventListener('visibilitychange', hidden)
      this.stopVisibility = () => doc.removeEventListener('visibilitychange', hidden)
    }
    clearInterval(this.timer)
    this.timer = setInterval(() => this.watch(), 500)
  }

  /** Stop. Told to the station as well, because a station that is not told keeps
   *  encoding until its own re-check notices - and that is a second of a shack's upload
   *  spent on audio nobody will hear. */
  release(reason: string | null = null): void {
    if (this.phase === 'off') return
    this.tell({ type: 'audioListen', listening: false, leaseId: BLANK_LEASE })
    this.teardown()
    this.set(reason ? 'ended' : 'off', reason)
  }

  /** The socket went away. No message can be sent, so this only stops the page. */
  disconnected(): void {
    if (this.phase === 'off') return
    this.teardown()
    this.set('ended', 'audioUnavailable')
  }

  /** Permanent. Frees the audio device. */
  close(): void { this.closed = true; this.teardown(); this.set('off', null) }

  /** One `audio*` message off the socket. Returns false when it was not one of ours, so
   *  the caller can carry on looking. Never throws for content: a malformed audio
   *  message costs the audio, never the session. */
  receive(raw: Record<string, unknown>): boolean {
    if (raw.type === 'audioState') {
      let state
      try { state = parseAudioState(raw) } catch { this.fail('audioUnavailable'); return true }
      if (state.listening) {
        // The station agreed. The phase stays `connecting` until audio actually plays -
        // an agreement is not a sound.
        if (this.phase === 'ended' || this.phase === 'off') this.set('connecting', null)
      } else if (this.phase !== 'off') {
        this.teardown()
        this.set('ended', state.reason ?? 'audioStopped')
      }
      return true
    }
    if (raw.type !== 'audioRx') return false
    if (this.phase === 'off' || this.phase === 'unsupported') return true
    let bundle
    try { bundle = parseAudioBundle(raw) } catch { this.fail('audioUnavailable'); return true }
    if (JSON.stringify(raw).length > AUDIO_MESSAGE_BYTES) { this.fail('audioUnavailable'); return true }
    this.heardAt = this.env.now()
    // A new capture generation is an END, not a gap: a different receiver at a different
    // rate. Splicing it onto what came before would smear two sample rates together, and
    // would let an operator carry on believing they were hearing the same radio.
    if (this.epoch !== null && bundle.epoch !== this.epoch) {
      this.teardown()
      this.set('ended', 'sourceChanged')
      return true
    }
    this.epoch = bundle.epoch
    let concealed = 0
    if (this.expected !== null) {
      // A repeat or a decrease is a replay. Dropped, never played: audio is not retried,
      // so the only thing a late bundle can do is arrive out of order.
      if (bundle.seq < this.expected) return true
      concealed = bundle.seq - this.expected
    }
    this.expected = bundle.seq + bundle.count
    if (concealed > 0) {
      // Told, never inferred from a decode error. The station's sequence is dense, so a
      // hole here is a known loss - and the operator is shown it rather than left to
      // wonder why the band went quiet.
      this.set('gap', 'audioGap')
    } else if (this.phase === 'connecting' || this.phase === 'gap' || this.phase === 'stalled') {
      this.set('live', null)
    }
    void this.play(bundle.payload, bundle.count)
    return true
  }

  private async play(payload: string, count: number): Promise<void> {
    await this.opening
    if (!this.decoder || this.closed) return
    let packets: Uint8Array[]
    try {
      const bytes = Uint8Array.from(atob(payload), character => character.charCodeAt(0))
      packets = audioPackets(bytes, count)
    } catch { this.fail('audioUnavailable'); return }
    for (const packet of packets) {
      try {
        // Raw RFC 6716 packets. `description` is deliberately never set: its presence
        // means an Ogg container, and omitting it is what selects raw Opus. Do not
        // synthesise an OpusHead.
        this.decoder.decode({
          type: 'key',
          timestamp: this.frames * AUDIO_FRAME_MS * 1000,
          duration: AUDIO_FRAME_MS * 1000,
          data: packet,
        })
        this.frames++
      } catch { this.fail('audioUnavailable'); return }
    }
  }

  private async open(): Promise<void> {
    try {
      const playback = await this.env.context()
      if (this.phase === 'off' || this.closed) { void playback.close(); return }
      this.playback = playback
      const decoder = this.env.decoder({
        output: frame => this.rendered(frame),
        error: () => this.fail('audioUnavailable'),
      })
      // Opus always decodes at 48 kHz whatever it was encoded at, and the output frame
      // states its own rate anyway - which is what `rendered` reads, rather than trusting
      // this number.
      decoder.configure({ codec: 'opus', sampleRate: DECODE_RATE, numberOfChannels: 1 })
      this.decoder = decoder
    } catch { this.fail('audioUnavailable') }
  }

  private rendered(frame: DecodedFrame): void {
    try {
      const playback = this.playback
      if (!playback) return
      const samples = new Float32Array(frame.frames)
      frame.copyTo(samples, { planeIndex: 0, format: 'f32-planar' })
      // The context's rate is a REQUEST, not a guarantee, so it is read back and adapted
      // to. A fixed ratio conversion to match the output device is not the adaptive
      // time-stretching this design refuses - that one changes rate to manage buffer
      // depth, and nothing here ever does.
      playback.push(playback.sampleRate === frame.sampleRate ? samples : resample(samples, frame.sampleRate, playback.sampleRate))
    } finally { frame.close() }
  }

  /** Nothing has arrived for a while. The link, and it is said as the link. */
  private watch(): void {
    if (this.phase === 'off' || this.phase === 'unsupported' || this.phase === 'ended') return
    if (this.env.now() - this.heardAt >= AUDIO_STALL_MS) this.set('stalled', 'audioStalled')
  }

  /** The socket's own budget can refuse this - it is small on purpose, so that audio can
   *  never spend the queue an operation response needs. A refusal is the end of the
   *  audio and nothing else; it never becomes an exception in a click handler. */
  private tell(message: object): boolean {
    try { this.send(message); return true } catch { return false }
  }

  private fail(reason: string): void { this.teardown(); this.set('ended', reason) }

  private teardown(): void {
    clearInterval(this.timer); this.timer = undefined
    this.stopVisibility?.(); this.stopVisibility = undefined
    this.playback?.reset()
    try { this.decoder?.close() } catch { /* already closed */ }
    this.decoder = null
    const playback = this.playback
    this.playback = null
    this.opening = null
    void playback?.close().catch(() => {})
  }

  private set(phase: AudioPhase, reason: string | null): void {
    if (this.phase === phase && this.reason === reason) return
    this.phase = phase; this.reason = reason
    this.view = { phase, reason, supported: this.view.supported }
    for (const f of this.listeners) f()
  }
}

/** The station ignores the lease on a stop - it only ever stops the session that asked -
 *  but the wire shape states one, so a stop carries a well-formed placeholder rather than
 *  a second message shape nobody else uses. */
const BLANK_LEASE = '00000000-0000-0000-0000-000000000000'

/** Fixed-ratio linear interpolation, for the uncommon device that is not at 48 kHz. Not
 *  adaptive and never driven by buffer depth: the ratio is a property of the hardware. */
export function resample(samples: Float32Array, from: number, to: number): Float32Array {
  if (from === to || from <= 0 || to <= 0) return samples
  const length = Math.max(1, Math.round((samples.length * to) / from))
  const out = new Float32Array(length)
  const step = (samples.length - 1) / Math.max(1, length - 1)
  for (let i = 0; i < length; i++) {
    const at = i * step
    const low = Math.min(samples.length - 1, Math.floor(at))
    const high = Math.min(samples.length - 1, low + 1)
    out[i] = samples[low] + (samples[high] - samples[low]) * (at - low)
  }
  return out
}

/** The real browser. Split out so the link above can be driven without an audio device. */
export function browserAudio(): AudioEnvironment {
  return {
    now: () => performance.now(),
    // `window.AudioDecoder`, NOT a WebCodecs check: Safari shipped WebCodecs video-only
    // for two years, and a WebCodecs check would have said yes to every one of them.
    decoderAvailable: () => typeof AudioDecoder !== 'undefined',
    decoder: handlers => new AudioDecoder({
      output: frame => handlers.output(frame as unknown as DecodedFrame),
      error: handlers.error,
    }) as unknown as AudioDecoderLike,
    context: async () => {
      const context = new AudioContext({ sampleRate: DECODE_RATE, latencyHint: 'interactive' })
      // A blob URL, because an AudioWorklet module can only be fetched and a separate
      // file would be a build asset to keep in step with the source it came from.
      const url = URL.createObjectURL(new Blob([AUDIO_WORKLET_SOURCE], { type: 'text/javascript' }))
      try { await context.audioWorklet.addModule(url) } finally { URL.revokeObjectURL(url) }
      const rate = context.sampleRate
      const node = new AudioWorkletNode(context, AUDIO_WORKLET_NAME, {
        numberOfInputs: 0,
        outputChannelCount: [1],
        processorOptions: {
          capacity: Math.ceil((rate * (AUDIO_CEILING_MS + AUDIO_PREFILL_MS * 2)) / 1000),
          prefill: Math.ceil((rate * AUDIO_PREFILL_MS) / 1000),
          ceiling: Math.ceil((rate * AUDIO_CEILING_MS) / 1000),
        },
      })
      node.connect(context.destination)
      // Autoplay policy: the listen control is a click, so this resolves - but a page
      // restored from bfcache can land here suspended, and a suspended context renders
      // nothing at all while looking perfectly healthy.
      await context.resume().catch(() => {})
      return {
        sampleRate: rate,
        push: samples => node.port.postMessage({ pcm: samples.buffer }, [samples.buffer]),
        reset: () => node.port.postMessage({ reset: true }),
        close: async () => { node.disconnect(); await context.close() },
      }
    },
    document: typeof document === 'undefined' ? undefined : document,
  }
}
