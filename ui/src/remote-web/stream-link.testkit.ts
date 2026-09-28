// Test doubles for the stream: a peer, its channels and a video element that presents frames on
// demand, plus a harness that builds a StreamLink over them. Shared by the link's own tests and the
// view's, so both drive the same fakes. Imported by tests only.
import { vi } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { StreamLink, type ChannelLike, type PeerLike, type StreamEnvironment, type VideoLike } from './stream-link'
import type { AudioEnvironment } from './audio-listen'
import type { BrowserStreamPayload } from './stream-protocol'

/** THE CONTRACT: the files the station's Rust parsers are tested against. */
export type ContractCase = { name: string; message: Record<string, unknown> }
const contract = (file: string) =>
  JSON.parse(readFileSync(resolve(process.cwd(), `../remote/test/fixtures/stream/${file}`), 'utf8')) as Record<string, ContractCase[]>
export const SIGNAL = contract('signal.json'), CHANNEL = contract('channel.json'), WEBVIEW = contract('webview.json')
export const byName = (list: ContractCase[], name: string): Record<string, unknown> => {
  const found = list.find(c => c.name === name)
  if (!found) throw Error(`no contract case named ${name}`)
  return structuredClone(found.message)
}
/** The contract's own lease, offer and answer: what a page and str0m actually exchange. */
export const LEASE = byName(SIGNAL.browserToRoom, 'offer').leaseId as string
export const OFFER = (byName(SIGNAL.browserToRoom, 'offer').payload as { sdp: string }).sdp
/** The contract's signed offer's key and signature: placeholders of the right shape, not key material. */
export const OFFER_SIGNATURE = (({ publicKey, signature }) => ({ publicKey, signature }))(byName(SIGNAL.browserToRoom, 'offer').payload as { publicKey: string; signature: string })
export const ANSWER = (byName(SIGNAL.roomToBrowser, 'answer').payload as { sdp: string }).sdp
export const FINGERPRINT = ANSWER.split('\r\n').find(line => line.startsWith('a=fingerprint:'))!
export const last = <T,>(items: readonly T[]): T | undefined => items[items.length - 1]

export class FakeChannel implements ChannelLike {
  readyState = 'connecting'
  bufferedAmount = 0
  sent: Record<string, unknown>[] = []
  onopen: (() => void) | null = null
  onclose: (() => void) | null = null
  onmessage: ((event: { data: unknown }) => void) | null = null
  constructor(readonly label: string, readonly init: { ordered: boolean; maxRetransmits?: number }) {}
  send(data: string) { if (this.readyState !== 'open') throw Error('not open'); this.sent.push(JSON.parse(data) as Record<string, unknown>) }
  close() { this.readyState = 'closed' }
  open() { this.readyState = 'open'; this.onopen?.() }
  deliver(value: unknown) { this.onmessage?.({ data: JSON.stringify(value) }) }
  types() { return this.sent.map(m => m.type) }
}
/** The microphone's line: what it is sending now (null before the operator turns it on). */
export class FakeSender {
  track: unknown = null
  replaced: unknown[] = []
  replaceTrack(track: unknown) { this.track = track; this.replaced.push(track); return Promise.resolve() }
}
/** The microphone as the browser hands it over: its settings, and whether it was stopped. */
export class FakeMicTrack {
  enabled = true
  stopped = false
  constructor(readonly settings: Record<string, unknown> = { echoCancellation: false, noiseSuppression: false, autoGainControl: false }) {}
  stop() { this.stopped = true }
  getSettings() { return this.settings }
}
export class FakePeer implements PeerLike {
  transceivers: { kind: string; direction: string }[] = []
  preferences: unknown = null
  micPreferences: unknown = null
  mic = new FakeSender()
  channels = new Map<string, FakeChannel>()
  local: unknown = null
  remote: { type: string; sdp: string } | null = null
  candidates: unknown[] = []
  closed = false
  connectionState = 'new'
  onicecandidate: PeerLike['onicecandidate'] = null
  ontrack: PeerLike['ontrack'] = null
  onconnectionstatechange: PeerLike['onconnectionstatechange'] = null
  addTransceiver(kind: 'video' | 'audio', init: { direction: 'recvonly' | 'sendonly' }) {
    this.transceivers.push({ kind, direction: init.direction })
    if (kind === 'audio') return { setCodecPreferences: (codecs: unknown) => { this.micPreferences = codecs }, sender: this.mic }
    return { setCodecPreferences: (codecs: unknown) => { this.preferences = codecs } }
  }
  createDataChannel(label: string, init: { ordered: boolean; maxRetransmits?: number }) {
    const channel = new FakeChannel(label, init); this.channels.set(label, channel); return channel
  }
  createOffer() { return Promise.resolve({ type: 'offer', sdp: OFFER }) }
  setLocalDescription(description: unknown) { this.local = description; return Promise.resolve() }
  setRemoteDescription(description: { type: 'answer'; sdp: string }) { this.remote = description; return Promise.resolve() }
  addIceCandidate(candidate: unknown) { this.candidates.push(candidate); return Promise.resolve() }
  close() { this.closed = true }
  channel(label: string) { return this.channels.get(label)! }
}
export class FakeVideo implements VideoLike {
  srcObject: unknown = null
  private callback: ((now: number, metadata: { rtpTimestamp?: number }) => void) | null = null
  requestVideoFrameCallback = (callback: (now: number, metadata: { rtpTimestamp?: number }) => void) => { this.callback = callback; return 1 }
  cancelVideoFrameCallback = () => { this.callback = null }
  present(rtpTimestamp?: number) { const f = this.callback; this.callback = null; f?.(0, rtpTimestamp === undefined ? {} : { rtpTimestamp }) }
}

export const silentAudio: AudioEnvironment = {
  now: () => 0, decoderAvailable: () => true,
  context: () => Promise.resolve({ sampleRate: 48000, push: () => {}, reset: () => {}, close: () => Promise.resolve() }),
  decoder: () => ({ configure: () => {}, decode: () => {}, close: () => {} }),
}
export function harness(options: { codecs?: boolean; peerThrows?: boolean; microphone?: 'granted' | 'denied' | 'none'; micSettings?: Record<string, unknown>; sign?: false | (() => Promise<{ publicKey: string; signature: string } | null>) } = {}) {
  let clock = 1000
  const peers: FakePeer[] = []
  const signals: { payload: BrowserStreamPayload; leaseId: string }[] = []
  /** Every time the page asked the browser for the microphone, with what it asked for. */
  const micAsks: unknown[] = []
  const micTracks: FakeMicTrack[] = []
  let ids = 0
  const env: StreamEnvironment = {
    now: () => clock,
    peer: () => { if (options.peerThrows) throw Error('no WebRTC'); const p = new FakePeer(); peers.push(p); return p },
    videoCodecs: () => options.codecs === false ? null : [{ mimeType: 'video/VP8', clockRate: 90000 }],
    audioCodecs: () => options.codecs === false ? null : [{ mimeType: 'audio/opus', clockRate: 48000, channels: 2, sdpFmtpLine: 'minptime=10;useinbandfec=1' }],
    microphone: options.microphone === 'none' ? undefined : constraints => {
      micAsks.push(constraints)
      if (options.microphone === 'denied') return Promise.reject(Error('NotAllowedError'))
      const track = new FakeMicTrack(options.micSettings)
      micTracks.push(track)
      return Promise.resolve(track)
    },
    mediaStream: track => ({ wrapped: track }),
    audio: silentAudio,
    uuid: () => `00000000-0000-4000-8000-${String(++ids).padStart(12, '0')}`,
    // A5: signs as the contract's offer is signed, unless a test asks otherwise.
    signOffer: options.sign === false ? undefined : options.sign ?? (async () => OFFER_SIGNATURE),
  }
  const link = new StreamLink((payload, leaseId) => signals.push({ payload, leaseId }), env)
  const video = new FakeVideo()
  link.attachVideo(video)
  return {
    link, env, peers, signals, video, micAsks, micTracks,
    get peer() { return peers[peers.length - 1] },
    advance: (ms: number) => { clock += ms; vi.advanceTimersByTime(ms) },
    /** Move the link's clock only, for tests on real timers. */
    tick: (ms: number) => { clock += ms },
    /** Offer, answer securely, open the channels and present a first frame: a live stream. */
    async live(rtp = 1000) {
      await link.start(LEASE)
      link.receive({ type: 'streamSignal', payload: { kind: 'answer', sdp: ANSWER } })
      await Promise.resolve()
      this.peer.ontrack?.({ track: 'track', streams: ['media'] })
      for (const label of ['control', 'ptt', 'audio']) this.peer.channel(label).open()
      video.present(rtp)
    },
  }
}

