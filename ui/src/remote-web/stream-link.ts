// The stream, from the browser's side: one WebRTC link to the station, set up over the observe
// socket's signalling lane and then carrying everything on its own - the picture as VP8, the
// receive audio and every control message on data channels. The wire contract is
// remote/test/fixtures/stream/ (stream-protocol.ts parses it).
//
// WHAT THIS MAY NEVER DO WITHOUT A LEASE (A3, the page's half). An offer is made only for a lease
// the station issued to this browser's own session, and it carries that lease; the station checks
// it again before any WebRTC state exists there. No lease, no RTCPeerConnection, no signal.
//
// WHAT IT REFUSES FROM THE STATION (A4, the page's half). An answer that does not prove DTLS - no
// certificate fingerprint, or a plain-RTP or SDES media line - is refused by name before the browser
// is handed it (`secureAnswer`), and the negotiation ends.
//
// BLIND MEANS NO AUTHORITY (S9). Every heartbeat carries the RTP timestamp of the last frame the
// video element actually presented, in the station's own media clock, so the station can tell how
// old the picture this operator is looking at is without trusting this browser's clock. It answers
// each heartbeat with whether it still holds transmit presence; the page shows that, never decides it.
//
// A HELD KEY IS A STATE, NEVER A PAIR (S7). While PTT is held the page re-asserts it every 100 ms,
// under a fresh hold id per press; the station ends the over when it has heard nothing for 200 ms,
// so a lost key-up or a dropped link cannot leave the rig keyed. Every key and button held on the
// picture is re-asserted the same way (`held`), and Nexus's window lets go of whatever stops being
// re-asserted: a Space held in Phone arms the microphone through the cockpit's own handler, and is
// bounded so.
//
// THE MICROPHONE (S6, the audio design's M1). A held PTT ARMS an over at the station and the
// operator's voice keys it: a press with the microphone off keys nothing. The page offers an audio
// line it only ever sends, Opus only, with no track on it until the operator turns the microphone
// on - so the browser asks for nothing and sends nothing before that. Once on, the voice goes for
// as long as the page has focus (a blurred window stops it, M8) and the station takes it only
// while an over is armed (M5: no voice activation, ever). What the station's over is doing comes
// back as `micState`.
import { AudioLink, browserAudio, type AudioEnvironment } from './audio-listen'
import { IdleWatch } from './stream-idle'
import {
  MIC_CONSTRAINTS, STREAM_BLIND_MS, STREAM_CHANNELS, STREAM_CONTROL_BYTES, STREAM_HEARTBEAT_MS, STREAM_HELD_KEYS,
  STREAM_HELD_REASSERT_MS, STREAM_INPUT_FLUSH_MS, STREAM_PTT_REASSERT_MS, STREAM_SIGNAL_BYTES, STREAM_UPLINK_BUDGET_BYTES,
  STREAM_UPLINK_STALL_MS, parseHeld, parseMicState, parsePttState, parseReceivedMessage, parseStreamInput, secureAnswer,
  type BrowserStreamPayload, type MicEnded, type OfferSignature, type StreamInput, type StreamWheel,
} from './stream-protocol'

/** The page's microphone. `asking` is the browser's permission prompt; `denied` and `unavailable`
 *  are ends the operator is told about. */
export type MicPhase = 'off' | 'asking' | 'on' | 'denied' | 'unavailable'
/** What the station says its microphone over is doing (`micState`). */
export type StationMic = { armed: boolean; keyed: boolean; noPowerOut: boolean; ended: MicEnded | null }

/** What the operator is shown. Each is a different thing to do about it. */
export type StreamPhase =
  /** Nothing asked for yet. */
  | 'idle'
  /** Offered, and waiting for the station's answer and the first picture. */
  | 'connecting'
  /** Frames are arriving. */
  | 'live'
  /** Connected, but no new frame for STREAM_BLIND_MS: the station holds back transmit authority. */
  | 'stalled'
  /** A real end, with its reason. */
  | 'ended'
export type StreamView = {
  phase: StreamPhase
  reason: string | null
  /** The control channel is open: heartbeats, Stop and input can travel. */
  control: boolean
  /** This page is holding PTT (its own intent). */
  ptt: boolean
  /** The station's word on the current press: keyed, or not. */
  keyed: boolean
  /** The station's word, on the last heartbeat reply, on whether it holds transmit presence for this
   *  session. Null until it has said. */
  presence: boolean | null
  /** The page's microphone. */
  mic: MicPhase
  /** The browser kept processing on the microphone (echo cancellation, noise suppression or
   *  automatic gain) although the page asked it not to. */
  micProcessing: boolean
  /** The station's microphone over, or null until it has said anything this stream. */
  station: StationMic | null
  /** The page let go of the over because its uplink backed up (M6), until the backlog clears. */
  uplinkStalled: boolean
  /** The station's answer, on `control`, to the last Stop this link was asked to carry: none, still
   *  waiting, accepted (acceptance, never a claim that RF stopped) or refused. `idle` too when the
   *  channel could not carry that Stop, so no earlier answer stands for it. */
  stop: 'idle' | 'sending' | 'accepted' | 'refused'
}

/** Stop, addressed as the station's own Stop is: its boot, the lease the token was issued under, and
 *  the transmit epoch. The page learns these from the station's state replies. */
export type StopTarget = { stationBootId: string; leaseId: string; transmitEpoch: string }

/** The parts of the browser this link reaches for, behind shapes a test can stand in for. */
export type ChannelLike = {
  readonly label: string
  readonly readyState: string
  readonly bufferedAmount: number
  send: (data: string) => void
  close: () => void
  onopen: (() => void) | null
  onclose: (() => void) | null
  onmessage: ((event: { data: unknown }) => void) | null
}
export type SenderLike = { replaceTrack: (track: unknown) => Promise<void> }
export type TransceiverLike = { setCodecPreferences?: (codecs: CodecLike[]) => void; sender?: SenderLike }
/** The microphone's track, as far as this link needs it. */
export type MicTrackLike = { enabled: boolean; stop: () => void; getSettings?: () => Record<string, unknown> }
export type CodecLike = { mimeType: string; clockRate: number; channels?: number; sdpFmtpLine?: string }
export type CandidateLike = { candidate: string; sdpMid: string | null }
export type ReceiverLike = { getSynchronizationSources?: () => { rtpTimestamp?: number }[] }
export type PeerLike = {
  addTransceiver: (kind: 'video' | 'audio', init: { direction: 'recvonly' | 'sendonly' }) => TransceiverLike
  createDataChannel: (label: string, init: { ordered: boolean; maxRetransmits?: number }) => ChannelLike
  createOffer: () => Promise<{ type: string; sdp?: string }>
  setLocalDescription: (description: { type: string; sdp?: string }) => Promise<void>
  setRemoteDescription: (description: { type: 'answer'; sdp: string }) => Promise<void>
  addIceCandidate: (candidate: CandidateLike) => Promise<void>
  close: () => void
  readonly connectionState: string
  onicecandidate: ((event: { candidate: CandidateLike | null }) => void) | null
  ontrack: ((event: { track: unknown; streams: readonly unknown[]; receiver?: ReceiverLike }) => void) | null
  onconnectionstatechange: (() => void) | null
}
/** The video element, as far as this link needs it: somewhere to put the picture, and a callback per
 *  presented frame that says which frame it was. */
export type VideoLike = {
  srcObject: unknown
  requestVideoFrameCallback?: (callback: (now: number, metadata: { rtpTimestamp?: number }) => void) => number
  cancelVideoFrameCallback?: (handle: number) => void
}
export type StreamEnvironment = {
  now: () => number
  peer: () => PeerLike
  /** The browser's VP8 codec entries (with the retransmission format that goes with them), or null
   *  where the browser cannot say - the offer then carries its defaults and the station picks VP8. */
  videoCodecs: () => CodecLike[] | null
  /** The browser's Opus entry, or null where it cannot say. */
  audioCodecs?: () => CodecLike[] | null
  /** The microphone, asked for with the audio design's settings. Called only when the operator
   *  turns the microphone on, so the browser's permission prompt appears then and never before. */
  microphone?: (constraints: typeof MIC_CONSTRAINTS) => Promise<MicTrackLike>
  /** Wraps a lone track in a stream when the station's answer names none. */
  mediaStream: (track: unknown) => unknown
  audio: AudioEnvironment
  uuid: () => string
  /** A5: this browser's signature over the offer (the contract's README), from its device key for
   *  this station, or null when it cannot sign. Absent, the offer goes unsigned; the station then
   *  refuses it by name, and never at its parser. */
  signOffer?: (sdp: string) => Promise<OfferSignature | null>
  /** Where "this tab is hidden" and "this window lost focus" come from. Optional so a test can
   *  leave them out; the real browser always has both. */
  document?: { readonly visibilityState: string; addEventListener: (type: string, f: () => void) => void; removeEventListener: (type: string, f: () => void) => void }
  window?: { addEventListener: (type: string, f: () => void) => void; removeEventListener: (type: string, f: () => void) => void }
  /** Whether this page has the focus now (a microphone turned on sends only while it does). */
  hasFocus?: () => boolean
}

/** One ICE server, no credentials: a direct connection first (the operator's pick, "Direct first,
 *  TURN later"). The contract asks the page for a STUN server so it has a reflexive candidate of its
 *  own; TURN arrives later with its own Worker route and short-lived credentials. */
export const STREAM_ICE_SERVERS = [{ urls: 'stun:stun.cloudflare.com:3478' }]
/** The control channel's own send budget for things that can wait: past it, a heartbeat or a move
 *  is dropped rather than queued. Stop and a release are sent past it, always. */
const CONTROL_BUDGET_BYTES = 16 * 1024
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/
const OFF: StreamView = {
  phase: 'idle', reason: null, control: false, ptt: false, keyed: false, presence: null,
  mic: 'off', micProcessing: false, station: null, uplinkStalled: false, stop: 'idle',
}

export class StreamLink {
  private view: StreamView = OFF
  private listeners = new Set<() => void>()
  private peer: PeerLike | null = null
  private channels: { control: ChannelLike; ptt: ChannelLike; audio: ChannelLike } | null = null
  private lease: string | null = null
  private answered = false
  /** This browser's own candidates, held until its offer has gone: signing it (A5) is a wait, and the
   *  station can only use a candidate for an offer it already has. Null once the offer is sent. */
  private unsent: { kind: 'candidate'; candidate: string; sdpMid: string }[] | null = []
  private pendingCandidates: CandidateLike[] = []
  private video: VideoLike | null = null
  private media: unknown = null
  private receiver: ReceiverLike | null = null
  private frameHandle: number | null = null
  /** The last presented frame: when this page saw it, and the station's RTP timestamp on it. */
  private frame: { at: number; rtp: number | null } | null = null
  /** Heartbeats in flight, so a reply is read as the answer to one of ours. */
  private heartbeats = new Set<string>()
  /** The last Stop sent on `control`, so its answer is read as that Stop's (`stop`). */
  private stopRequest: string | null = null
  private press: { holdId: string; seq: number } | null = null
  private heartbeat: ReturnType<typeof setInterval> | undefined
  private watch: ReturnType<typeof setInterval> | undefined
  private hold: ReturnType<typeof setInterval> | undefined
  /** What the picture holds down at the shack, its re-assertion timer, and the stream's count. */
  private heldSet: { keys: string[]; buttons: number } | null = null
  private heldTimer: ReturnType<typeof setInterval> | undefined
  private heldSeq = 0
  private flushTimer: ReturnType<typeof setTimeout> | undefined
  private pendingMove: StreamInput | null = null
  private pendingWheel: StreamWheel | null = null
  /** The station's last word on receive audio, replayed to the player when the operator asks. */
  private audioState: Record<string, unknown> | null = null
  private closed = false
  /** Removes the hidden/pagehide/blur listeners; set while a stream is running. */
  private unwatchPage: (() => void) | undefined
  /** The microphone's line and, while it is on, its track. */
  private micSender: SenderLike | null = null
  private micTrack: MicTrackLike | null = null
  /** Whether the page has the focus: a microphone that is on sends only while it does (M8). */
  private focused = true
  /** Since when the `ptt` channel's send queue has stood over its budget (M6), or null. */
  private backedUpSince: number | null = null
  /** Listening, from the `audio` channel. The existing WebCodecs player, unchanged: no NetEq. The
   *  station sends receive audio for the whole stream and the page is muted until the operator asks,
   *  so "listen" never leaves this page: it starts the player, and replays the station's last word
   *  on audio so a refusal (another browser listening) reads as that and not as a stalled link. */
  readonly audio: AudioLink
  /** How long since the operator last did anything on this stream, on this link's clock. It starts
   *  with each stream, and every re-assertion of a held PTT or of what the picture holds counts, so
   *  a press held down is never idle; the page adds its clicks and keys and decides what follows. */
  readonly idle: IdleWatch

  constructor(private readonly signal: (payload: BrowserStreamPayload, leaseId: string) => void, private readonly env: StreamEnvironment) {
    this.audio = new AudioLink(message => {
      if ((message as { listening?: unknown }).listening !== true) return
      const state = this.audioState
      if (state) queueMicrotask(() => this.audio.receive(state))
    }, env.audio)
    this.idle = new IdleWatch(() => this.env.now())
  }

  subscribe = (f: () => void): (() => void) => { this.listeners.add(f); return () => { this.listeners.delete(f) } }
  getSnapshot = (): StreamView => this.view

  /** Offer a stream under `leaseId`, the lease this browser's session holds right now. Refuses -
   *  creating nothing and sending nothing - without one. */
  async start(leaseId: string | null): Promise<void> {
    if (this.closed || !leaseId || !UUID.test(leaseId)) return
    if (this.view.phase === 'connecting' || this.view.phase === 'live' || this.view.phase === 'stalled') return
    this.teardown()
    this.lease = leaseId
    this.heldSeq = 0
    this.idle.active()
    this.set({ ...OFF, phase: 'connecting' })
    this.watchPage()
    let peer: PeerLike
    try { peer = this.env.peer() } catch { this.end('streamUnsupported'); return }
    this.peer = peer
    peer.onicecandidate = event => {
      // The contract carries no end-of-candidates marker, and every browser candidate names its media.
      const c = event.candidate
      if (this.peer !== peer || !c?.candidate || !c.sdpMid) return
      const candidate = { kind: 'candidate' as const, candidate: c.candidate, sdpMid: c.sdpMid }
      if (this.unsent) { if (this.unsent.length < 64) this.unsent.push(candidate) } else this.tell(candidate)
    }
    peer.ontrack = event => {
      if (this.peer !== peer) return
      this.media = event.streams[0] ?? this.env.mediaStream(event.track)
      this.receiver = event.receiver ?? null
      this.bindVideo()
    }
    peer.onconnectionstatechange = () => {
      if (this.peer !== peer) return
      if (peer.connectionState === 'failed') this.end('connectionFailed')
    }
    try {
      const transceiver = peer.addTransceiver('video', { direction: 'recvonly' })
      // VP8 only: the station encodes nothing else, and an offer listing every codec the browser
      // knows is several times the size for nothing.
      const vp8 = this.env.videoCodecs()
      if (vp8?.length && transceiver.setCodecPreferences) transceiver.setCodecPreferences(vp8)
      // The microphone's line (S6): sent only, Opus only (the station decodes nothing else), and
      // empty until the operator turns the microphone on.
      const mic = peer.addTransceiver('audio', { direction: 'sendonly' })
      const opus = this.env.audioCodecs?.()
      if (opus?.length && mic.setCodecPreferences) mic.setCodecPreferences(opus)
      this.micSender = mic.sender ?? null
      const channel = (spec: { label: string; ordered: boolean; maxRetransmits?: number }) =>
        peer.createDataChannel(spec.label, { ordered: spec.ordered, ...(spec.maxRetransmits === undefined ? {} : { maxRetransmits: spec.maxRetransmits }) })
      this.channels = { control: channel(STREAM_CHANNELS.control), ptt: channel(STREAM_CHANNELS.ptt), audio: channel(STREAM_CHANNELS.audio) }
      this.wireChannels(peer)
      const offer = await peer.createOffer()
      if (this.peer !== peer) return
      await peer.setLocalDescription(offer)
      if (this.peer !== peer) return
      const sdp = offer.sdp ?? ''
      if (!sdp.startsWith('v=0\r\n')) { this.end('streamUnsupported', false); return }
      // A5: signed with this browser's device key for this station when it can be; an offer it
      // cannot sign still goes, and the station says why it will not stream (deviceNotPinned or
      // deviceKeyMismatch), which is what the operator needs to hear.
      const signed = this.env.signOffer ? await this.env.signOffer(sdp).catch(() => null) : null
      if (this.peer !== peer) return
      // An offer the socket would not take is an end, never a stream left waiting for an answer
      // that cannot come.
      if (!this.tell(signed ? { kind: 'offer', sdp, ...signed } : { kind: 'offer', sdp })) { this.end('streamUnsupported', false); return }
      const held = this.unsent ?? []
      this.unsent = null
      for (const candidate of held) this.tell(candidate)
    } catch { if (this.peer === peer) this.end('streamUnsupported') }
  }

  /** One `stream*` message off the socket. Never throws for content: a malformed message costs the
   *  stream, never the session the socket carries. */
  receive(raw: Record<string, unknown>): void {
    let message
    try { message = parseReceivedMessage(raw) } catch { if (this.peer) this.end('streamFailed'); return }
    if (message.type === 'streamState') {
      // `streaming: true` is the station's admission; the picture says the rest. Anything else is an
      // end, in the station's (or the relay's) own word, and is not answered.
      if (!message.streaming && this.view.phase !== 'idle' && this.view.phase !== 'ended') this.end(message.reason ?? 'streamClosed', false)
      return
    }
    const peer = this.peer
    if (!peer) return
    const payload = message.payload
    if (payload.kind === 'answer') {
      if (this.answered) return
      // A4: refused by name, before the browser is handed it.
      if (!secureAnswer(payload.sdp)) { this.end('insecureAnswer'); return }
      this.answered = true
      void peer.setRemoteDescription({ type: 'answer', sdp: payload.sdp }).then(() => {
        if (this.peer !== peer) return
        const queued = this.pendingCandidates
        this.pendingCandidates = []
        for (const candidate of queued) void peer.addIceCandidate(candidate).catch(() => {})
      }, () => { if (this.peer === peer) this.end('streamFailed') })
      return
    }
    const candidate = { candidate: payload.candidate, sdpMid: payload.sdpMid }
    // A candidate can overtake the answer it belongs to; it waits for it rather than being lost.
    if (!this.answered) { if (this.pendingCandidates.length < 64) this.pendingCandidates.push(candidate); return }
    void peer.addIceCandidate(candidate).catch(() => {})
  }

  /** Where the picture goes. Called with null when the element goes away. */
  attachVideo(video: VideoLike | null): void {
    if (this.video === video) return
    this.unbindVideo()
    this.video = video
    this.bindVideo()
  }

  /** Stop, on the control channel whenever it is open, past every budget. The caller sends it over
   *  the socket as well; the first acceptance is the answer, the second is harmless. The station's
   *  answer on this channel is `stop` in the view, so a refusal here is said, not dropped. */
  stopTransmit(target: StopTarget | null): boolean {
    this.stopRequest = null
    const control = this.channels?.control
    if (target && control?.readyState === 'open') {
      const requestId = this.env.uuid()
      try {
        control.send(JSON.stringify({ type: 'stopTransmit', requestId, ...target }))
        this.stopRequest = requestId
        this.set({ stop: 'sending' })
        return true
      } catch { /* not carried, as below */ }
    }
    this.set({ stop: 'idle' })
    return false
  }

  /** Turn the page's microphone on or off. On is the only place the browser is asked for it, so its
   *  permission prompt appears when the operator asks and at no other time. Off stops the track,
   *  and the browser's own microphone indicator goes out with it. */
  async setMic(on: boolean): Promise<void> {
    if (!on) { this.micOff(); this.set({ mic: 'off', micProcessing: false }); return }
    if (this.view.mic === 'asking' || this.view.mic === 'on') return
    const sender = this.micSender, ask = this.env.microphone
    if (!sender || !ask) { this.set({ mic: 'unavailable' }); return }
    this.set({ mic: 'asking' })
    let track: MicTrackLike
    try { track = await ask(MIC_CONSTRAINTS) } catch { if (this.micSender === sender) this.set({ mic: 'denied' }); return }
    // The stream ended, or the operator turned it off again, while the browser asked.
    if (this.micSender !== sender || !this.micAsking()) { try { track.stop() } catch { /* already stopped */ } return }
    this.micTrack = track
    track.enabled = this.focused && !this.view.uplinkStalled
    // A request that "succeeded" proves nothing: some browsers keep the processing on and say so
    // only here. The operator is told; the audio still goes, because refusing it would be worse.
    const settings = track.getSettings?.() ?? {}
    const processing = settings.echoCancellation === true || settings.noiseSuppression === true || settings.autoGainControl === true
    try { await sender.replaceTrack(track) } catch { this.micOff(); this.set({ mic: 'unavailable' }); return }
    this.set({ mic: 'on', micProcessing: processing })
  }

  /** PTT pressed: a fresh hold id, re-asserted every STREAM_PTT_REASSERT_MS until released. The
   *  station releases the over itself if the re-assertion stops for any reason. */
  holdPtt(): void {
    if (this.view.ptt || this.channels?.ptt.readyState !== 'open') return
    this.press = { holdId: this.env.uuid(), seq: 0 }
    this.set({ ptt: true, keyed: false })
    this.sendHold()
    clearInterval(this.hold)
    this.hold = setInterval(() => this.sendHold(), STREAM_PTT_REASSERT_MS)
  }
  /** PTT released. The explicit release only makes the unkey sooner; the re-assertion stopping is
   *  what the station actually relies on. */
  releasePtt(): void {
    clearInterval(this.hold); this.hold = undefined
    const press = this.press
    this.press = null
    if (!this.view.ptt && !press) return
    this.set({ ptt: false })
    if (press) this.sendPtt({ type: 'pttRelease', holdId: press.holdId, seq: press.seq })
  }

  /** What the picture holds down at the shack now: re-asserted on `ptt` at once when it changes and
   *  every STREAM_HELD_REASSERT_MS while anything is, and not at all when nothing is. Nexus's window
   *  lets go of whatever stops being re-asserted (the dead-man), so this is what keeps a held key
   *  held there - and all that does. */
  holdInput(keys: string[], buttons: number): void {
    const next = keys.length || buttons ? { keys: keys.slice(0, STREAM_HELD_KEYS), buttons } : null
    const changed = JSON.stringify(next) !== JSON.stringify(this.heldSet)
    this.heldSet = next
    if (!next) { clearInterval(this.heldTimer); this.heldTimer = undefined; return }
    if (changed) this.sendHeld()
    if (this.heldTimer === undefined) this.heldTimer = setInterval(() => this.sendHeld(), STREAM_HELD_REASSERT_MS)
  }

  /** One input event for Nexus's window. Moves and wheel deltas are coalesced to ~60 Hz; a press, a
   *  release, a key or text flushes what is pending first, so nothing is ever reordered. */
  input(event: StreamInput): void {
    if (this.channels?.control.readyState !== 'open' || (this.view.phase !== 'live' && this.view.phase !== 'stalled')) return
    if (event.type === 'pointer' && event.action === 'move') { this.pendingMove = event; this.schedule(); return }
    if (event.type === 'wheel') {
      // Deltas add up between flushes; a change of modifiers or delta mode is a different gesture and
      // flushes what was pending first, so the two are never summed together.
      const held = this.pendingWheel
      if (held && (held.modifiers !== event.modifiers || held.deltaMode !== event.deltaMode)) this.flushInput()
      const base = this.pendingWheel
      const bound = (value: number) => Math.max(-10_000, Math.min(10_000, value))
      this.pendingWheel = { ...event, deltaX: bound((base?.deltaX ?? 0) + event.deltaX), deltaY: bound((base?.deltaY ?? 0) + event.deltaY) }
      this.schedule()
      return
    }
    this.flushInput()
    this.sendInput(event, false)
  }

  /** End the stream at this browser's request. */
  close(reason: string | null = null): void {
    if (this.view.phase === 'idle' || this.view.phase === 'ended') { this.teardown(); if (reason === null) this.set(OFF); return }
    this.end(reason ?? 'streamClosed', true, reason === null)
  }
  /** The socket went away: the lease went with it, so the stream goes too. Nothing can be sent. */
  disconnected(): void {
    if (this.view.phase === 'idle' || this.view.phase === 'ended') return
    this.teardown()
    this.set({ ...OFF, phase: 'ended', reason: 'streamUnavailable' })
  }
  /** Permanent. */
  dispose(): void { this.closed = true; this.teardown(); this.audio.close(); this.set(OFF) }

  private wireChannels(peer: PeerLike): void {
    const channels = this.channels!
    channels.control.onopen = () => {
      if (this.peer !== peer) return
      this.set({ control: true })
      this.sendHeartbeat()
      clearInterval(this.heartbeat)
      this.heartbeat = setInterval(() => this.sendHeartbeat(), STREAM_HEARTBEAT_MS)
    }
    channels.control.onclose = () => { if (this.peer === peer) this.end('connectionFailed', false) }
    channels.control.onmessage = event => { if (this.peer === peer) this.fromControl(event.data) }
    channels.audio.onmessage = event => {
      if (this.peer !== peer || typeof event.data !== 'string') return
      let message: Record<string, unknown>
      try { message = JSON.parse(event.data) as Record<string, unknown> } catch { return }
      if (!message || typeof message !== 'object') return
      if (message.type === 'audioState') this.audioState = message
      // AudioLink validates both shapes itself and never throws for content.
      this.audio.receive(message)
    }
    this.watch = setInterval(() => this.checkPicture(), 250)
  }

  /** The station's replies and reports on `control`: its answer to a heartbeat (with whether it
   *  still holds presence), its answer to this link's last Stop, and the state of the current PTT
   *  press. Nothing else here is acted on. */
  private fromControl(data: unknown): void {
    if (typeof data !== 'string') return
    let message: Record<string, unknown>
    try { message = JSON.parse(data) as Record<string, unknown> } catch { return }
    if (!message || typeof message !== 'object') return
    if (message.type === 'pttState') {
      let state
      try { state = parsePttState(message) } catch { return }
      if (state.holdId === this.press?.holdId) this.set({ keyed: state.keyed })
      else if (!state.keyed) this.set({ keyed: false })
      return
    }
    if (message.type === 'micState') {
      let state
      try { state = parseMicState(message) } catch { return }
      this.set({ station: { armed: state.armed, keyed: state.keyed, noPowerOut: state.noPowerOut, ended: state.ended ?? null } })
      return
    }
    if (message.type === 'operationResponse' && this.stopRequest !== null && message.requestId === this.stopRequest) {
      this.stopRequest = null
      // Anything but the station's acceptance is read as a refusal: the direction that has the
      // operator press again, never the one that tells them a Stop took.
      const value = message.value as Record<string, unknown> | undefined
      this.set({ stop: !('error' in message) && value?.stop === 'accepted' ? 'accepted' : 'refused' })
      return
    }
    if (message.type === 'operationResponse' && typeof message.requestId === 'string' && this.heartbeats.delete(message.requestId)) {
      // A refusal (the lease went) is presence lost; a value carries the station's own word.
      this.set({ presence: 'error' in message ? false : message.presence === true })
    }
  }

  /** A hidden tab is a tab nobody is looking at: the stream ends (the control lease is released on
   *  hide anyway, and a blind page may not transmit), and so does a page being unloaded. Losing
   *  window focus only lets go of PTT - the courtesy release the audio design asks for (M8); the
   *  200 ms re-assertion gap is what the station actually relies on. */
  private watchPage(): void {
    this.unwatchPage?.()
    const doc = this.env.document, win = this.env.window
    const hidden = () => { if (doc?.visibilityState === 'hidden') this.end('streamHidden') }
    const leaving = () => this.end('streamHidden')
    // M8: a window that loses focus lets go of PTT and stops its microphone; getting it back lets
    // the microphone send again (the operator presses PTT again to talk).
    const blurred = () => { this.releasePtt(); this.focus(false) }
    const focused = () => this.focus(true)
    this.focused = this.env.hasFocus?.() ?? true
    doc?.addEventListener('visibilitychange', hidden)
    win?.addEventListener('pagehide', leaving)
    win?.addEventListener('blur', blurred)
    win?.addEventListener('focus', focused)
    this.unwatchPage = () => {
      doc?.removeEventListener('visibilitychange', hidden)
      win?.removeEventListener('pagehide', leaving)
      win?.removeEventListener('blur', blurred)
      win?.removeEventListener('focus', focused)
    }
  }
  private focus(focused: boolean): void {
    this.focused = focused
    if (this.micTrack) this.micTrack.enabled = focused && !this.view.uplinkStalled
  }
  /** Read fresh: the browser's prompt is awaited, and the operator may have turned it off meanwhile. */
  private micAsking(): boolean { return this.view.mic === 'asking' }
  private micOff(): void {
    const track = this.micTrack
    this.micTrack = null
    if (this.micSender) void this.micSender.replaceTrack(null).catch(() => {})
    try { track?.stop() } catch { /* already stopped */ }
  }
  /** M6 over WebRTC. Every re-assertion of the over (a hold, the held keys) is a send on `ptt`; a
   *  send queue that stays over its budget for STREAM_UPLINK_STALL_MS means the uplink has backed
   *  up, and the voice queued behind it would reach the air late if it reached it at all. The page
   *  lets go of the over and stops its microphone, and says so, until the queue drains. The
   *  station's own 200 ms gap does not rely on any of this. */
  private uplinkBackedUp(): boolean {
    const ptt = this.channels?.ptt
    if (!ptt || ptt.bufferedAmount <= STREAM_UPLINK_BUDGET_BYTES) {
      this.backedUpSince = null
      if (this.view.uplinkStalled) { this.set({ uplinkStalled: false }); if (this.micTrack) this.micTrack.enabled = this.focused }
      return false
    }
    const now = this.env.now()
    this.backedUpSince ??= now
    if (now - this.backedUpSince < STREAM_UPLINK_STALL_MS) return false
    if (!this.view.uplinkStalled) {
      this.set({ uplinkStalled: true })
      if (this.micTrack) this.micTrack.enabled = false
      this.releasePtt()
      this.heldSet = null
      clearInterval(this.heldTimer); this.heldTimer = undefined
    }
    return true
  }

  private bindVideo(): void {
    const video = this.video
    if (!video || !this.media) return
    video.srcObject = this.media
    if (!video.requestVideoFrameCallback) return
    const loop = (_now: number, metadata: { rtpTimestamp?: number }) => {
      if (this.video !== video) return
      // The contract's order: the frame's own RTP timestamp; failing that, the receiver's latest -
      // read only here, inside a callback that proves frames are still being shown; never an estimate.
      const fromFrame = metadata.rtpTimestamp
      const rtp = fromFrame ?? this.receiver?.getSynchronizationSources?.()[0]?.rtpTimestamp
      this.frame = { at: this.env.now(), rtp: Number.isSafeInteger(rtp) && rtp! >= 0 && rtp! <= 0xffffffff ? rtp! : null }
      this.checkPicture()
      this.frameHandle = video.requestVideoFrameCallback!(loop)
    }
    this.frameHandle = video.requestVideoFrameCallback(loop)
  }
  private unbindVideo(): void {
    const video = this.video
    if (!video) return
    if (this.frameHandle !== null) video.cancelVideoFrameCallback?.(this.frameHandle)
    this.frameHandle = null
    video.srcObject = null
  }

  private checkPicture(): void {
    if (this.view.phase !== 'connecting' && this.view.phase !== 'live' && this.view.phase !== 'stalled') return
    if (!this.frame) return
    const fresh = this.env.now() - this.frame.at < STREAM_BLIND_MS
    if (fresh && this.view.phase !== 'live') this.set({ phase: 'live', reason: null })
    else if (!fresh && this.view.phase === 'live') {
      // Blind: say so, and stop holding PTT - the station will not key for a blind page anyway,
      // and a held button that does nothing reads as a fault.
      this.releasePtt()
      this.set({ phase: 'stalled', reason: 'pictureStalled' })
    }
  }

  /** The last presented frame's RTP timestamp as it is, stale or not: the station measures its age
   *  against its own media clock and decides. `null` before the first frame, or in a browser that
   *  cannot say which frame it showed - which the station reads as blind. */
  private sendHeartbeat(): void {
    if (!this.lease) return
    const requestId = this.env.uuid()
    if (this.sendControl({ type: 'heartbeat', requestId, leaseId: this.lease, decodedFrameAt: this.frame?.rtp ?? null }, true)) {
      this.heartbeats.add(requestId)
      // Only the last few can still be answered usefully; the set never grows past them.
      if (this.heartbeats.size > 8) this.heartbeats.delete(this.heartbeats.values().next().value!)
    }
  }
  private sendHold(): void {
    if (this.uplinkBackedUp()) return
    const press = this.press
    if (!press || !this.sendPtt({ type: 'pttHold', holdId: press.holdId, seq: press.seq })) { this.releasePtt(); return }
    press.seq = Math.min(press.seq + 1, 0xffffffff)
    this.idle.active()
  }
  private sendHeld(): void {
    if (this.uplinkBackedUp()) return
    const set = this.heldSet
    if (!set) return
    const message = { type: 'held', keys: set.keys, buttons: set.buttons, seq: this.heldSeq }
    try { parseHeld(message) } catch { return }
    if (!this.sendPtt(message)) return
    this.heldSeq = Math.min(this.heldSeq + 1, 0xffffffff)
    // Held on the picture counts as held here: it may be the cockpit's own PTT (Space, or its button).
    this.idle.active()
  }
  private sendPtt(message: object): boolean {
    const ptt = this.channels?.ptt
    if (ptt?.readyState !== 'open') return false
    try { ptt.send(JSON.stringify(message)); return true } catch { return false }
  }
  /** `budgeted` messages can wait and are dropped past the channel's budget; the rest never are. */
  private sendControl(message: object, budgeted: boolean): boolean {
    const control = this.channels?.control
    if (control?.readyState !== 'open') return false
    const text = JSON.stringify(message)
    if (new TextEncoder().encode(text).length > STREAM_CONTROL_BYTES) return false
    if (budgeted && control.bufferedAmount + text.length > CONTROL_BUDGET_BYTES) return false
    try { control.send(text); return true } catch { return false }
  }
  private schedule(): void {
    if (this.flushTimer !== undefined) return
    this.flushTimer = setTimeout(() => { this.flushTimer = undefined; this.flushInput() }, STREAM_INPUT_FLUSH_MS)
  }
  private flushInput(): void {
    clearTimeout(this.flushTimer); this.flushTimer = undefined
    const move = this.pendingMove, wheel = this.pendingWheel
    this.pendingMove = null; this.pendingWheel = null
    if (move) this.sendInput(move, true)
    if (wheel) this.sendInput(wheel, true)
  }
  /** Checked against the contract before it leaves: what this page sends is exactly what the
   *  station's parser and the dispatcher in Nexus's window will accept. */
  private sendInput(event: StreamInput, droppable: boolean): void {
    try { parseStreamInput(event) } catch { return }
    this.sendControl(event, droppable)
  }

  private tell(payload: BrowserStreamPayload): boolean {
    if (!this.lease) return false
    try {
      if (new TextEncoder().encode(JSON.stringify({ type: 'streamSignal', leaseId: this.lease, payload })).length > STREAM_SIGNAL_BYTES) return false
      this.signal(payload, this.lease)
      return true
    } catch { return false }
  }
  /** End with a reason. `notify` tells the station, which is right for every end the PAGE decides
   *  and wrong for one the station announced. `quiet` ends to idle, for an operator's own stop. */
  private end(reason: string, notify = true, quiet = false): void {
    if (notify && this.peer) this.tell({ kind: 'close' })
    this.teardown()
    this.set(quiet ? OFF : { ...OFF, phase: 'ended', reason })
  }
  private teardown(): void {
    this.unwatchPage?.(); this.unwatchPage = undefined
    clearInterval(this.heartbeat); this.heartbeat = undefined
    clearInterval(this.watch); this.watch = undefined
    // Let go of PTT on the channel while it is still there to carry the release.
    this.releasePtt()
    clearInterval(this.heldTimer); this.heldTimer = undefined; this.heldSet = null
    clearTimeout(this.flushTimer); this.flushTimer = undefined
    this.pendingMove = null; this.pendingWheel = null
    this.heartbeats.clear(); this.stopRequest = null; this.audioState = null
    this.micOff(); this.micSender = null; this.backedUpSince = null
    this.audio.disconnected()
    const peer = this.peer, channels = this.channels
    this.peer = null; this.channels = null; this.media = null; this.receiver = null; this.frame = null
    this.answered = false; this.pendingCandidates = []; this.unsent = []
    if (this.video) { if (this.frameHandle !== null) this.video.cancelVideoFrameCallback?.(this.frameHandle); this.frameHandle = null; this.video.srcObject = null }
    if (channels) for (const channel of [channels.control, channels.ptt, channels.audio]) {
      channel.onopen = null; channel.onclose = null; channel.onmessage = null
      try { channel.close() } catch { /* already closed */ }
    }
    if (peer) {
      peer.onicecandidate = null; peer.ontrack = null; peer.onconnectionstatechange = null
      try { peer.close() } catch { /* already closed */ }
    }
  }
  private set(value: Partial<StreamView>): void {
    const next = { ...this.view, ...value }
    if ((Object.keys(next) as (keyof StreamView)[]).every(key => next[key] === this.view[key])) return
    this.view = next
    for (const f of this.listeners) f()
  }
}

/** The real browser. Split out so the link above can be driven without WebRTC. */
export function browserStream(): StreamEnvironment {
  return {
    now: () => performance.now(),
    peer: () => new RTCPeerConnection({ iceServers: STREAM_ICE_SERVERS, bundlePolicy: 'max-bundle', rtcpMuxPolicy: 'require' }) as unknown as PeerLike,
    videoCodecs: () => {
      const codecs = typeof RTCRtpReceiver !== 'undefined' ? RTCRtpReceiver.getCapabilities?.('video')?.codecs : undefined
      if (!codecs) return null
      const vp8 = codecs.filter(codec => codec.mimeType.toLowerCase() === 'video/vp8')
      // The retransmission format rides with VP8 so a lost packet can be resent instead of waiting
      // for a keyframe; without VP8 there is nothing to prefer and the browser keeps its defaults.
      return vp8.length ? [...vp8, ...codecs.filter(codec => codec.mimeType.toLowerCase() === 'video/rtx')] : null
    },
    audioCodecs: () => {
      const codecs = typeof RTCRtpSender !== 'undefined' ? RTCRtpSender.getCapabilities?.('audio')?.codecs : undefined
      const opus = codecs?.filter(codec => codec.mimeType.toLowerCase() === 'audio/opus')
      return opus?.length ? opus : null
    },
    microphone: async constraints => {
      const media = await navigator.mediaDevices.getUserMedia({ audio: constraints, video: false })
      const [track] = media.getAudioTracks()
      if (!track) throw Error('noMicrophone')
      return track as unknown as MicTrackLike
    },
    mediaStream: track => new MediaStream([track as MediaStreamTrack]),
    audio: browserAudio(),
    uuid: () => crypto.randomUUID(),
    document: typeof document === 'undefined' ? undefined : document,
    window: typeof window === 'undefined' ? undefined : window,
    hasFocus: () => typeof document === 'undefined' || document.hasFocus(),
  }
}
