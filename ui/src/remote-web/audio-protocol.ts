// The receive-audio lane: the only wire shape the station, the relay and the browser
// player all agree on. Deliberately separate from the operation and application
// protocols, because audio must never consume their credit (design 3.4 rule 3) and
// because everything here is throw-away: a bundle that does not arrive is a 60 ms gap,
// never a retry.
//
// TIME NEVER CROSSES THIS BOUNDARY AS A WALL CLOCK. `firstFrameMs` is the sender's own
// monotonic mark and is used for RELATIVE spacing inside one stream only. Freshness is
// measured by the receiver against its own arrival clock. Get that wrong and swapping
// this envelope for a data channel later changes what "late" means.

/** One Opus frame. Also the station's RX DSP tick. */
export const AUDIO_FRAME_MS = 20
/** Frames per message. 3 x 20 ms: at one packet per message the TCP+TLS+WS header
 *  alone is ~30 kbit/s against a 24 kbit/s payload, which is the whole argument for
 *  bundling (design 3.3). At 3 the header cost falls to ~10 kbit/s. */
export const AUDIO_BUNDLE_FRAMES = 3
/** Hard ceiling for one station -> browser bundle. A 3-frame bundle at 24 kbit/s is
 *  ~240 base64 bytes plus ~160 of envelope; this is room for any VBR excursion and is
 *  still far inside every byte bound the relay and the station already impose. */
export const AUDIO_MESSAGE_BYTES = 1024
/** Ceiling for the browser -> station control message. */
export const AUDIO_CONTROL_BYTES = 256
/** Bundles the player buffers before it gives up and calls the stream stalled. */
export const AUDIO_STALL_MS = 2000

/** Station -> browser. `count` and `frameMs` are stated, never inferred from the
 *  payload length, so a decoder never has to derive a duration from a byte count. */
export type AudioBundle = {
  type: 'audioRx'
  /** Dense and monotonic within one epoch. A gap is a loss to conceal; a repeat or a
   *  decrease is a replay to drop. */
  seq: number
  /** The station's capture generation. Changes only when the capture source does, and
   *  a source change ENDS the stream - it is not a gap. */
  epoch: string
  firstFrameMs: number
  frameMs: number
  count: number
  /** base64 of concatenated 2-byte-big-endian-length-prefixed Opus packets. */
  payload: string
}
/** Station -> browser: whether the station is feeding this session, and why not. */
export type AudioState = { type: 'audioState'; listening: boolean; reason?: string }
/** Browser -> station. The lease is the browser's claim to station control; the
 *  station re-checks it against its own authority and refuses a logging-only lease. */
export type AudioListen = { type: 'audioListen'; listening: boolean; leaseId: string }
/** What the relay actually hands the station: the browser's identity is stamped by the
 *  relay, never asserted by the browser. */
export type AudioListenRouted = AudioListen & { sessionId: string; deviceId: string }

export const AUDIO_STATE_REASONS = [
  // Another browser is already listening. One listener per station in v1: the station
  // feed has a single reader, and silently stealing it would kill the first listener.
  'audioInUse',
  // The browser does not hold station control, or its lease lapsed.
  'notController',
  // The station has no capture source right now (audio closed, or never opened).
  'audioUnavailable',
  // The capture device changed under the stream. A real end, never a gap.
  'sourceChanged',
  'audioStopped',
] as const
export type AudioStateReason = (typeof AUDIO_STATE_REASONS)[number]

const EPOCH = /^[0-9a-f]{16}$/
const BASE64 = /^[A-Za-z0-9+/]*={0,2}$/

function fields(raw: unknown, keys: string[]): Record<string, unknown> {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) throw Error('invalidAudio')
  const value = raw as Record<string, unknown>
  const present = Object.keys(value)
  if (present.length !== keys.length || !keys.every(key => present.includes(key))) throw Error('invalidAudio')
  return value
}
function counted(value: unknown, low: number, high: number): number {
  if (!Number.isSafeInteger(value) || (value as number) < low || (value as number) > high) throw Error('invalidAudio')
  return value as number
}

/** Validate a station bundle WITHOUT decoding its payload. The relay calls this and
 *  forwards; it never base64-decodes and never stores. */
export function parseAudioBundle(raw: unknown): AudioBundle {
  const value = fields(raw, ['type', 'seq', 'epoch', 'firstFrameMs', 'frameMs', 'count', 'payload'])
  if (value.type !== 'audioRx') throw Error('invalidAudio')
  if (typeof value.epoch !== 'string' || !EPOCH.test(value.epoch)) throw Error('invalidAudio')
  if (typeof value.payload !== 'string' || !BASE64.test(value.payload) || value.payload.length % 4 !== 0) throw Error('invalidAudio')
  const count = counted(value.count, 1, 5)
  const frameMs = counted(value.frameMs, AUDIO_FRAME_MS, AUDIO_FRAME_MS)
  // 0xffffffff is the encoder's own ceiling; it ends a stream rather than wrapping,
  // because a wrapped sequence reads to a receiver as a replay.
  const seq = counted(value.seq, 0, 0xffffffff)
  const firstFrameMs = counted(value.firstFrameMs, 0, Number.MAX_SAFE_INTEGER)
  return { type: 'audioRx', seq, epoch: value.epoch, firstFrameMs, frameMs, count, payload: value.payload }
}
export function parseAudioState(raw: unknown): AudioState {
  const keys = raw && typeof raw === 'object' && 'reason' in (raw as object) ? ['type', 'listening', 'reason'] : ['type', 'listening']
  const value = fields(raw, keys)
  if (value.type !== 'audioState' || typeof value.listening !== 'boolean') throw Error('invalidAudio')
  if ('reason' in value && !AUDIO_STATE_REASONS.includes(value.reason as AudioStateReason)) throw Error('invalidAudio')
  return { type: 'audioState', listening: value.listening, ...('reason' in value ? { reason: value.reason as string } : {}) }
}
export function parseAudioListen(raw: unknown): AudioListen {
  const value = fields(raw, ['type', 'listening', 'leaseId'])
  if (value.type !== 'audioListen' || typeof value.listening !== 'boolean') throw Error('invalidAudio')
  if (typeof value.leaseId !== 'string' || !/^[0-9a-f-]{36}$/.test(value.leaseId)) throw Error('invalidAudio')
  return { type: 'audioListen', listening: value.listening, leaseId: value.leaseId }
}

/** Split a decoded bundle payload into its packets. Length-prefixed rather than
 *  equal-sized, because Opus is VBR and a decoder must never guess a boundary. */
export function audioPackets(payload: Uint8Array, count: number): Uint8Array[] {
  const packets: Uint8Array[] = []
  let at = 0
  for (let i = 0; i < count; i++) {
    if (at + 2 > payload.length) throw Error('invalidAudio')
    const length = (payload[at] << 8) | payload[at + 1]
    at += 2
    if (length === 0 || at + length > payload.length) throw Error('invalidAudio')
    packets.push(payload.subarray(at, at + length))
    at += length
  }
  if (at !== payload.length) throw Error('invalidAudio')
  return packets
}
