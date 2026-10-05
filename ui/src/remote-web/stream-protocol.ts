// Remote as a stream: the wire grammar the relay, the page and Nexus's own window share. The contract
// is `remote/test/fixtures/stream/` (its README defines every field and bound); the station tests
// its Rust parsers against those files and the tests here parse the same ones, so no side can drift
// from the others without a test going red.
//
// The Cloudflare Worker compiles this file too (room.ts imports the relay) with no DOM library, so
// nothing here may reach a DOM type: a session description is a string here, never the browser's
// object.
//
// WHAT THE RELAY MAY KNOW. It checks an envelope, bounds it and routes it. It never interprets an
// SDP beyond "a bounded string that starts like one": the station parses what it is handed and
// refuses what it cannot use, and the page does the same with the answer (`secureAnswer`).

/** The lane's version, advertised by the station as `x-nexus-stream-version` and by the service
 *  in `/config`. A station that never sent the header is never handed a stream message: its
 *  message parser refuses unknown types, and a refusal there takes the whole control socket down. */
export const STREAM_VERSION = 1
/** Ceiling for one page -> relay signal. The room already refuses a browser message over
 *  OPERATION_REQUEST_BYTES (6144) before it parses anything, so this is the same number: the page
 *  never sends what the room would close its socket for. A VP8-only offer is about 2 KB. */
export const STREAM_SIGNAL_BYTES = 6144
/** The contract's ceiling for a stamped message the relay hands the station, measured as the JSON
 *  it sends. The station closes its control socket on any frame over 8,192 bytes - all of Remote,
 *  not just the stream - so this leaves a kilobyte of margin under that. */
export const STREAM_STATION_BYTES = 7168
/** The same ceiling for what the relay hands the page. */
export const STREAM_BROWSER_BYTES = 7168
/** One SDP, in bytes (an SDP is ASCII); one trickled candidate line; one media id. */
export const STREAM_SDP_BYTES = 6144
export const STREAM_CANDIDATE_CHARS = 512
const MID = /^[A-Za-z0-9_-]{1,32}$/

/** Why a stream is not running, in the station's words and the relay's (the contract's README). A
 *  page refuses anything else. The station sends the first seven, `remoteOff` when it ends a stream
 *  the relay ended with `streamEnd` (Remote switched off by hand mid-stream), and the device-key
 *  refusals (A5): `deviceNotPinned` for a browser it pinned no key for, `deviceKeyMismatch` for an
 *  offer not signed by the pinned key; the relay says `streamUnavailable` for a station that never
 *  advertised the lane, and `serviceAccessExpired` and `tryLater` for refusals of its own, which the
 *  station never sends. */
export const STREAM_STATE_REASONS = [
  'notController', 'streamDisabled', 'streamUnavailable', 'streamInUse', 'invalidOffer', 'connectionFailed', 'streamClosed',
  'remoteOff', 'deviceNotPinned', 'deviceKeyMismatch', 'serviceAccessExpired', 'tryLater',
] as const
export type StreamStateReason = (typeof STREAM_STATE_REASONS)[number]

export type StreamCandidate = { kind: 'candidate'; candidate: string; sdpMid: string }
/** An offer's device-key binding (A5): the browser's public key and its signature, together. */
export type OfferSignature = { publicKey: string; signature: string }
/** Page -> station. An offer is signed with the browser's device key, or not at all (a page from
 *  before the key): the station refuses an unsigned one at admission, by name. */
export type BrowserStreamPayload = ({ kind: 'offer'; sdp: string } & (OfferSignature | Record<never, never>)) | StreamCandidate | { kind: 'close' }
/** Station -> page. */
export type StationStreamPayload = { kind: 'answer'; sdp: string } | StreamCandidate
/** What the page sends. `leaseId` is its claim to station control; the station re-checks it
 *  against its own authority before any WebRTC state exists, so it is a request, not an assertion. */
export type BrowserStreamSignal = { type: 'streamSignal'; leaseId: string; payload: BrowserStreamPayload }
/** What the relay hands the station: the session and device are stamped by the relay from its own
 *  admission record, never read off the page's message. */
export type RoutedStreamSignal = BrowserStreamSignal & { sessionId: string; deviceId: string }
/** Whether the station is streaming to a session, and why not. */
export type StreamState = { type: 'streamState'; streaming: boolean; reason?: StreamStateReason }
/** A station message, addressed to one session; and what the page receives, without the address. */
export type StationStreamMessage = ({ type: 'streamSignal'; payload: StationStreamPayload } | StreamState) & { sessionId: string }
export type ReceivedStreamMessage = { type: 'streamSignal'; payload: StationStreamPayload } | StreamState

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/
const CONTROL = /[\u0000-\u001f\u007f]/

function fields(raw: unknown, keys: string[]): Record<string, unknown> {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) throw Error('invalidStream')
  const value = raw as Record<string, unknown>
  const present = Object.keys(value)
  if (present.length !== keys.length || !keys.every(key => present.includes(key))) throw Error('invalidStream')
  return value
}
function sdp(value: unknown): string {
  // `v=0` first, as RFC 8866 requires. Nothing else about its content is the relay's business.
  if (typeof value !== 'string' || !value.startsWith('v=0\r\n') || new TextEncoder().encode(value).length > STREAM_SDP_BYTES) throw Error('invalidStream')
  return value
}
function candidate(raw: unknown): StreamCandidate {
  const { candidate: line, sdpMid } = fields(raw, ['kind', 'candidate', 'sdpMid'])
  if (typeof line !== 'string' || !line.startsWith('candidate:') || line.length > STREAM_CANDIDATE_CHARS || CONTROL.test(line)) throw Error('invalidStream')
  if (typeof sdpMid !== 'string' || !MID.test(sdpMid)) throw Error('invalidStream')
  return { kind: 'candidate', candidate: line, sdpMid }
}
function kind(raw: unknown): string {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) throw Error('invalidStream')
  const value = (raw as Record<string, unknown>).kind
  if (typeof value !== 'string') throw Error('invalidStream')
  return value
}

export function parseBrowserPayload(raw: unknown): BrowserStreamPayload {
  const which = kind(raw)
  if (which === 'offer') {
    // Signed, with both, or unsigned, with neither: never one without the other.
    const signed = 'publicKey' in (raw as object) || 'signature' in (raw as object)
    const v = fields(raw, signed ? ['kind', 'sdp', 'publicKey', 'signature'] : ['kind', 'sdp'])
    if (!signed) return { kind: 'offer', sdp: sdp(v.sdp) }
    if (!deviceKey(v.publicKey) || !lowerHex(v.signature, STREAM_SIGNATURE_HEX_CHARS)) throw Error('invalidStream')
    return { kind: 'offer', sdp: sdp(v.sdp), publicKey: v.publicKey, signature: v.signature }
  }
  if (which === 'candidate') return candidate(raw)
  if (which === 'close') { fields(raw, ['kind']); return { kind: 'close' } }
  throw Error('invalidStream')
}
export function parseStationPayload(raw: unknown): StationStreamPayload {
  const which = kind(raw)
  if (which === 'answer') return { kind: 'answer', sdp: sdp(fields(raw, ['kind', 'sdp']).sdp) }
  if (which === 'candidate') return candidate(raw)
  throw Error('invalidStream')
}
/** A page's signal, exactly: one carrying an identity of its own is refused, never merged, because
 *  the relay's stamping is only load-bearing if nothing can be smuggled past it. */
export function parseBrowserSignal(raw: unknown): BrowserStreamSignal {
  const value = fields(raw, ['type', 'leaseId', 'payload'])
  if (value.type !== 'streamSignal' || typeof value.leaseId !== 'string' || !UUID.test(value.leaseId)) throw Error('invalidStream')
  return { type: 'streamSignal', leaseId: value.leaseId, payload: parseBrowserPayload(value.payload) }
}
function parseState(raw: unknown, addressed: boolean): StreamState & { sessionId?: string } {
  const has = !!raw && typeof raw === 'object' && 'reason' in (raw as object)
  const value = fields(raw, ['type', ...(addressed ? ['sessionId'] : []), 'streaming', ...(has ? ['reason'] : [])])
  if (value.type !== 'streamState' || typeof value.streaming !== 'boolean') throw Error('invalidStream')
  if (has && !STREAM_STATE_REASONS.includes(value.reason as StreamStateReason)) throw Error('invalidStream')
  return { type: 'streamState', streaming: value.streaming, ...(has ? { reason: value.reason as StreamStateReason } : {}),
    ...(addressed ? { sessionId: value.sessionId as string } : {}) }
}
/** What the station sends: a signal or a state, addressed to one session. */
export function parseStationMessage(raw: unknown): StationStreamMessage {
  const type = raw && typeof raw === 'object' ? (raw as Record<string, unknown>).type : undefined
  const address = raw && typeof raw === 'object' ? (raw as Record<string, unknown>).sessionId : undefined
  if (typeof address !== 'string' || !UUID.test(address)) throw Error('invalidStream')
  if (type === 'streamState') return parseState(raw, true) as StationStreamMessage
  const value = fields(raw, ['type', 'sessionId', 'payload'])
  if (value.type !== 'streamSignal') throw Error('invalidStream')
  return { type: 'streamSignal', sessionId: address, payload: parseStationPayload(value.payload) }
}
/** What the page receives from the relay. */
export function parseReceivedMessage(raw: unknown): ReceivedStreamMessage {
  const type = raw && typeof raw === 'object' ? (raw as Record<string, unknown>).type : undefined
  if (type === 'streamState') return parseState(raw, false)
  const value = fields(raw, ['type', 'payload'])
  if (value.type !== 'streamSignal') throw Error('invalidStream')
  return { type: 'streamSignal', payload: parseStationPayload(value.payload) }
}

// ── The device-key binding (A5) ───────────────────────────────────────────────────────────────────
// The contract's README says what each of these is. The page signs its offer with a non-extractable
// P-256 key; the station checks the signature against the key it pinned when the operator approved
// this browser at the radio. These are the shapes and the signed bytes; the cryptography is WebCrypto's.

/** A device key: the SPKI DER of a P-256 public key, as lowercase hex. */
export const STREAM_DEVICE_KEY_HEX_CHARS = 182
export const P256_SPKI_PREFIX_HEX = '3059301306072a8648ce3d020106082a8648ce3d030107034200'
/** An offer's signature: ECDSA P-256 over SHA-256, IEEE P1363 r||s (64 bytes), as lowercase hex. */
export const STREAM_SIGNATURE_HEX_CHARS = 128
/** What the signed bytes begin with, so a signature made for this is never one for anything else. */
export const OFFER_BINDING_LABEL = 'nexus-stream-offer/1'

function lowerHex(value: unknown, chars: number): value is string {
  return typeof value === 'string' && value.length === chars && /^[0-9a-f]*$/.test(value)
}
function deviceKey(value: unknown): value is string {
  return lowerHex(value, STREAM_DEVICE_KEY_HEX_CHARS) && value.startsWith(`${P256_SPKI_PREFIX_HEX}04`)
}
export function toHex(bytes: Uint8Array): string {
  return Array.from(bytes, b => b.toString(16).padStart(2, '0')).join('')
}
/** The offer's DTLS certificate fingerprint: the 32 bytes of its `a=fingerprint:sha-256` value, which
 *  every such line must carry alike (a browser writes one per media section). Null when there is
 *  none, when they disagree, or when one is not 32 colon-separated hex bytes. */
export function offerFingerprint(description: string): Uint8Array | null {
  let found: string | null = null
  for (const line of description.split(/\r\n|\n/)) {
    const value = line.trim().match(/^a=fingerprint:(\S+) (.+)$/)
    if (!value || value[1].toLowerCase() !== 'sha-256') continue
    const digest = value[2].trim().toLowerCase()
    if (!/^[0-9a-f]{2}(:[0-9a-f]{2}){31}$/.test(digest)) return null
    if (found !== null && found !== digest) return null
    found = digest
  }
  return found === null ? null : Uint8Array.from(found.split(':'), pair => parseInt(pair, 16))
}
/** A key's fingerprint (SHA-256 of its SPKI, lowercase hex) as both ends show it, for the operator to
 *  compare: its first sixteen bytes (128 bits), eight groups of four uppercase hex digits, the form the
 *  LAN pairing shows too (security review S3-L1). Eight bytes were too few to compare against a party
 *  that knows the key and can bring up the question at will. A Nexus from before showed the first
 *  four of these groups, so those still compare. */
export function shortFingerprint(fingerprint: string): string {
  return (fingerprint.slice(0, 32).toUpperCase().match(/.{1,4}/g) ?? []).join(' ')
}
/** The bytes an offer's signature covers: the label, SHA-256 of the fingerprint (the caller hashes),
 *  and the station, device and session ids as the relay stamps them - 160 bytes. */
export function offerBinding(fingerprintDigest: Uint8Array, stationId: string, deviceId: string, sessionId: string): Uint8Array {
  const text = new TextEncoder()
  return new Uint8Array([...text.encode(OFFER_BINDING_LABEL), ...fingerprintDigest, ...text.encode(stationId), ...text.encode(deviceId), ...text.encode(sessionId)])
}

// ── The answer, signed by the station (security review S3-M1) ────────────────────────────────────
// The other way round: the station signs its answer with its own key, the one its pairing record at
// the service holds, over both DTLS fingerprints and the three ids, on one session-level SDP line.
// The page checks it before the browser is handed the answer (`station-key.ts`), so a relay that
// answers the page's offer itself cannot stand in for the station.

/** What the station's signature over its answer begins with. */
export const ANSWER_BINDING_LABEL = 'nexus-stream-answer/1'
/** The session-level line that carries it: this, then the signature as lowercase hex. A browser
 *  ignores an attribute it does not know, so the line asks nothing of the browser. */
export const ANSWER_SIGNATURE_ATTRIBUTE = 'a=nexus-station-signature:'
/** The bytes the station's answer signature covers: the label, SHA-256 of the answer's fingerprint,
 *  SHA-256 of the offer's (the caller hashes), and the station, device and session ids - 193 bytes. */
export function answerBinding(answerDigest: Uint8Array, offerDigest: Uint8Array, stationId: string, deviceId: string, sessionId: string): Uint8Array {
  const text = new TextEncoder()
  return new Uint8Array([...text.encode(ANSWER_BINDING_LABEL), ...answerDigest, ...offerDigest,
    ...text.encode(stationId), ...text.encode(deviceId), ...text.encode(sessionId)])
}
/** The station's signature an answer carries: exactly one such line, in its session section, of the
 *  right length. Null for none, two, one inside a media section, or one of the wrong shape. */
export function answerSignature(description: string): string | null {
  const lines = description.split(/\r\n|\n/)
  const media = lines.findIndex(line => line.startsWith('m='))
  const signed = lines.flatMap((line, at) => line.startsWith(ANSWER_SIGNATURE_ATTRIBUTE) ? [{ line, at }] : [])
  if (signed.length !== 1 || (media >= 0 && signed[0].at > media)) return null
  const signature = signed[0].line.slice(ANSWER_SIGNATURE_ATTRIBUTE.length)
  return lowerHex(signature, STREAM_SIGNATURE_HEX_CHARS) ? signature : null
}

/** Every media section's transport a stream may use, per the contract: SRTP keyed by DTLS for the
 *  picture, SCTP over DTLS for the data channels. Plain RTP (`RTP/AVP`, `RTP/AVPF`) and SDES-keyed
 *  SRTP (`RTP/SAVP`, `RTP/SAVPF`) are refused, whatever else the answer says. */
const SECURE_PROTOCOLS = new Set(['UDP/TLS/RTP/SAVPF', 'UDP/DTLS/SCTP'])
/** A4, the page's half: is this answer end-to-end encrypted as far as its SDP can say? Every `m=`
 *  section must be on a DTLS transport and covered by a SHA-256 DTLS certificate fingerprint - its
 *  own, or one at session level, which covers them all. str0m writes one per media section, so a
 *  section stripped of its own is uncovered even while another section keeps one. The browser would
 *  refuse most of this itself; checking here makes the refusal the page's own and names it, instead
 *  of trusting a library error to arrive. */
export function secureAnswer(description: string): boolean {
  const sections: string[][] = [[]]
  for (const line of description.split(/\r\n|\n/)) {
    if (line.startsWith('m=')) sections.push([])
    sections[sections.length - 1].push(line)
  }
  const [session, ...media] = sections
  if (!media.length) return false
  const fingerprinted = (lines: string[]) => lines.some(line => /^a=fingerprint:sha-256 [0-9A-Fa-f]{2}(:[0-9A-Fa-f]{2}){31}$/.test(line))
  const everywhere = fingerprinted(session)
  return media.every(lines => SECURE_PROTOCOLS.has(lines[0].split(' ')[2] ?? '') && (everywhere || fingerprinted(lines)))
}

// ── The data channels ─────────────────────────────────────────────────────────────────────────────
// The page creates all three before it offers; the station tells them apart by label. `control`
// carries today's operation requests, the input, their replies and the PTT's state; `ptt` carries
// the held PTT itself, where a late packet is worse than a lost one; `audio` carries receive audio,
// exactly the relay lane's `audioRx` bundle and `audioState`. The relay never sees any of it.

export const STREAM_CHANNELS = {
  control: { label: 'control', ordered: true },
  ptt: { label: 'ptt', ordered: false, maxRetransmits: 0 },
  audio: { label: 'audio', ordered: false, maxRetransmits: 0 },
} as const
/** Every `control` message from the page is at most this. */
export const STREAM_CONTROL_BYTES = 1024

/** How often the page heartbeats on `control`. The station's presence permit lapses 5 s after the
 *  last fresh-frame heartbeat, and it judges a picture stale at 2 s; every half second keeps its
 *  view of the picture well inside that. */
export const STREAM_HEARTBEAT_MS = 500
/** S7: a held PTT is re-asserted this often, and the station releases the over once it has heard
 *  nothing for STREAM_PTT_GAP_MS. A lost key-up can never leave the rig keyed. */
export const STREAM_PTT_REASSERT_MS = 100
export const STREAM_PTT_GAP_MS = 200
/** S9: a picture older than this is "blind", and a blind page holds no transmit authority. The
 *  station decides that; the page uses the same number only to SAY so. */
export const STREAM_BLIND_MS = 2000
/** Input is coalesced to at most 60 events a second. */
export const STREAM_INPUT_FLUSH_MS = 16

/** `view`: the page's picture area, in its own device pixels. The station encodes the picture no
 *  larger than that, scaled once from its window and never enlarged, so the page can show it pixel
 *  for pixel; while the operator has zoomed into the picture on the page, the area times the zoom,
 *  which is the size the page then draws it at. Sent when `control` opens, and again once a new
 *  size has held for STREAM_VIEW_SETTLE_MS, so a window being dragged is told once, where it stops.
 *  Not input: it needs no presence, and a station from before it drops it unread. */
export type StreamViewSize = { type: 'view'; width: number; height: number }
export const STREAM_VIEW_MAX = 16384
export const STREAM_VIEW_SETTLE_MS = 250
/** The `view` message for a picture area of `width`×`height` device pixels, or null for one the
 *  contract cannot carry: whole numbers from 1 to STREAM_VIEW_MAX. `zoom` is how far the operator
 *  has zoomed into the picture on the page: a good area is asked for that many times over, at most
 *  STREAM_VIEW_MAX a side, so the station sends more of its pixels instead of the page enlarging few. */
export function viewMessage(width: number, height: number, zoom = 1): StreamViewSize | null {
  const side = (n: number) => Number.isInteger(n) && n >= 1 && n <= STREAM_VIEW_MAX
  if (!side(width) || !side(height)) return null
  const zoomed = (n: number) => zoom > 1 ? Math.min(STREAM_VIEW_MAX, Math.round(n * zoom)) : n
  return { type: 'view', width: zoomed(width), height: zoomed(height) }
}

export const PTT_STATE_REASONS = ['refused', 'lapsed', 'released', 'stopped'] as const
export type PttState = { type: 'pttState'; holdId: string; keyed: boolean; reason?: (typeof PTT_STATE_REASONS)[number] }
export function parsePttState(raw: unknown): PttState {
  const has = !!raw && typeof raw === 'object' && 'reason' in (raw as object)
  const value = fields(raw, ['type', 'holdId', 'keyed', ...(has ? ['reason'] : [])])
  if (value.type !== 'pttState' || typeof value.holdId !== 'string' || !UUID.test(value.holdId) || typeof value.keyed !== 'boolean') throw Error('invalidStream')
  if (has && !PTT_STATE_REASONS.includes(value.reason as PttState['reason'] & string)) throw Error('invalidStream')
  return value as PttState
}

/** Why the station's microphone over ended (the contract's `micState.ended`). */
export const MIC_ENDED = ['released', 'stopped', 'audioGap', 'presence', 'ceiling', 'watchdog', 'routeChanged'] as const
export type MicEnded = (typeof MIC_ENDED)[number]
/** The station's microphone over: armed by a press, keyed by the operator's voice (a held PTT with
 *  no voice keys nothing), `noPowerOut` (display only: the voice arrives and the rig reports no power
 *  out), and on the message that reports an over's end, why it ended. */
export type MicState = { type: 'micState'; armed: boolean; keyed: boolean; noPowerOut: boolean; ended?: MicEnded }
export function parseMicState(raw: unknown): MicState {
  const has = !!raw && typeof raw === 'object' && 'ended' in (raw as object)
  const value = fields(raw, ['type', 'armed', 'keyed', 'noPowerOut', ...(has ? ['ended'] : [])])
  if (value.type !== 'micState' || typeof value.armed !== 'boolean' || typeof value.keyed !== 'boolean'
    || typeof value.noPowerOut !== 'boolean') throw Error('invalidStream')
  if (has && !MIC_ENDED.includes(value.ended as MicEnded)) throw Error('invalidStream')
  return value as MicState
}

/** The page's microphone, as the audio design asks (§4.7): no echo cancellation, no noise
 *  suppression and no automatic gain - AGC pumps the level and wrecks ALC discipline, and NS is
 *  trained on speech and chews tones - one channel at 48 kHz. Browsers may ignore these, so the
 *  track's own settings are read back afterwards and the operator is told (`micProcessing`). */
export const MIC_CONSTRAINTS = {
  echoCancellation: false, noiseSuppression: false, autoGainControl: false, channelCount: 1, sampleRate: 48000,
} as const
/** M6 over WebRTC: the page's held PTT and held keys travel on `ptt`; when that channel's send
 *  queue stays over this for STREAM_UPLINK_STALL_MS, the uplink has backed up and the page lets go
 *  of the over itself, and says so. The station's own 200 ms audio gap is the guarantee. */
export const STREAM_UPLINK_BUDGET_BYTES = 2048
export const STREAM_UPLINK_STALL_MS = 200

/** Modifier bits on every input event. */
export const MOD_SHIFT = 1, MOD_CTRL = 2, MOD_ALT = 4, MOD_META = 8
/** Input is a DOM-level description of what the operator did over the picture, never an OS event.
 *  `x` and `y` are fractions of the decoded frame - which is Nexus's window and nothing else. */
export type StreamPointer = { type: 'pointer'; action: 'down' | 'move' | 'up' | 'cancel'; x: number; y: number; button: number; buttons: number; modifiers: number; pointerType: 'mouse' | 'touch' | 'pen'; clicks: number }
export type StreamWheel = { type: 'wheel'; x: number; y: number; deltaX: number; deltaY: number; deltaMode: 0 | 1 | 2; modifiers: number }
export type StreamKey = { type: 'key'; action: 'down' | 'up'; key: string; code: string; modifiers: number; repeat: boolean }
export type StreamText = { type: 'text'; text: string }
export type StreamInput = StreamPointer | StreamWheel | StreamKey | StreamText
/** What the page holds down over the picture, re-asserted on `ptt` every STREAM_HELD_REASSERT_MS
 *  while anything is (the lead's dead-man ruling): each key by its DOM `code`, and the pointer
 *  buttons as the same mask a pointer event carries. Nexus's window lets go of whatever stops being
 *  re-asserted for STREAM_HELD_GAP_MS - the held PTT's own numbers (S7) - so nothing held over the
 *  stream, a Space that keys PTT in Phone included, outlives a dead link by more than that. It never
 *  presses anything. `seq` counts up from 0 per stream, so an overtaken one is known and ignored. */
export type StreamHeld = { type: 'held'; keys: string[]; buttons: number; seq: number }
export const STREAM_HELD_REASSERT_MS = STREAM_PTT_REASSERT_MS
export const STREAM_HELD_GAP_MS = STREAM_PTT_GAP_MS
export const STREAM_HELD_KEYS = 16
/** What reaches Nexus's own window: the admitted input, the page's re-assertion of what it holds,
 *  and `reset` when a stream ends or its presence lapses, which releases anything still held down. */
export type WebviewInput = StreamInput | StreamHeld | { type: 'reset' }

const unit = (value: unknown): value is number => typeof value === 'number' && Number.isFinite(value) && value >= 0 && value <= 1
const within = (value: unknown, low: number, high: number): value is number => Number.isSafeInteger(value) && (value as number) >= low && (value as number) <= high
const delta = (value: unknown): value is number => typeof value === 'number' && Number.isFinite(value) && Math.abs(value) <= 10_000
const chars = (value: string) => [...value].length
/** One input event, exactly as the contract bounds it. The page checks what it sends with this, and
 *  the dispatcher in Nexus's window parses what it receives with it again. */
export function parseStreamInput(raw: unknown): StreamInput {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) throw Error('invalidStream')
  const type = (raw as Record<string, unknown>).type
  if (type === 'pointer') {
    const v = fields(raw, ['type', 'action', 'x', 'y', 'button', 'buttons', 'modifiers', 'pointerType', 'clicks'])
    if (!['down', 'move', 'up', 'cancel'].includes(v.action as string) || !unit(v.x) || !unit(v.y) || !within(v.button, -1, 4) || !within(v.buttons, 0, 31)
      || !within(v.modifiers, 0, 15) || !['mouse', 'touch', 'pen'].includes(v.pointerType as string) || !within(v.clicks, 0, 3)) throw Error('invalidStream')
    return v as StreamPointer
  }
  if (type === 'wheel') {
    const v = fields(raw, ['type', 'x', 'y', 'deltaX', 'deltaY', 'deltaMode', 'modifiers'])
    if (!unit(v.x) || !unit(v.y) || !delta(v.deltaX) || !delta(v.deltaY) || !within(v.deltaMode, 0, 2) || !within(v.modifiers, 0, 15)) throw Error('invalidStream')
    return v as StreamWheel
  }
  if (type === 'key') {
    const v = fields(raw, ['type', 'action', 'key', 'code', 'modifiers', 'repeat'])
    if ((v.action !== 'down' && v.action !== 'up') || typeof v.key !== 'string' || chars(v.key) < 1 || chars(v.key) > 32 || CONTROL.test(v.key)
      || typeof v.code !== 'string' || !/^[A-Za-z0-9]{0,32}$/.test(v.code) || !within(v.modifiers, 0, 15) || typeof v.repeat !== 'boolean') throw Error('invalidStream')
    return v as StreamKey
  }
  if (type === 'text') {
    const v = fields(raw, ['type', 'text'])
    if (typeof v.text !== 'string' || chars(v.text) < 1 || chars(v.text) > 256 || CONTROL.test(v.text)) throw Error('invalidStream')
    return v as StreamText
  }
  throw Error('invalidStream')
}
export function parseHeld(raw: unknown): StreamHeld {
  const v = fields(raw, ['type', 'keys', 'buttons', 'seq'])
  if (v.type !== 'held' || !Array.isArray(v.keys) || v.keys.length > STREAM_HELD_KEYS || new Set(v.keys).size !== v.keys.length
    || !v.keys.every(code => typeof code === 'string' && /^[A-Za-z0-9]{1,32}$/.test(code))
    || !within(v.buttons, 0, 31) || !within(v.seq, 0, 0xffffffff)) throw Error('invalidStream')
  return v as StreamHeld
}
export function parseWebviewInput(raw: unknown): WebviewInput {
  if (raw && typeof raw === 'object' && (raw as Record<string, unknown>).type === 'reset') { fields(raw, ['type']); return { type: 'reset' } }
  if (raw && typeof raw === 'object' && (raw as Record<string, unknown>).type === 'held') return parseHeld(raw)
  return parseStreamInput(raw)
}
