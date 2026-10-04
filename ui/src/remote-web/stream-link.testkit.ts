// Test doubles for the stream: a peer, its channels and a video element that presents frames on
// demand, plus a harness that builds a StreamLink over them. Shared by the link's own tests and the
// view's, so both drive the same fakes. Imported by tests only.
import { vi } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { StreamLink, STREAM_ICE_SERVERS, type ChannelLike, type IceServerLike, type PeerLike, type StreamEnvironment, type VideoLike } from './stream-link'
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
/** The operator's level as the page builds it: the track it was built from, every gain it was given,
 *  its line's own track, the meter it reports to, and whether it was closed. */
export class FakeLevelGraph {
  readonly track = new FakeMicTrack()
  readonly gains: number[]
  closed = false
  constructor(readonly from: unknown, gain: number, readonly meter: (peak: number, limited: boolean) => void) { this.gains = [gain] }
  gain(linear: number) { this.gains.push(linear) }
  close() { this.closed = true }
}
export class FakePeer implements PeerLike {
  /** The ICE servers the page built this peer with. */
  constructor(readonly iceServers: readonly IceServerLike[] = []) {}
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
/** A relay as the service hands it over: synthetic names, never a minted credential. */
export const RELAY = { urls: ['turn:turn.example.invalid:3478?transport=udp', 'turns:turn.example.invalid:443?transport=tcp'], username: 'synthetic-username', credential: 'synthetic-credential' }
export function harness(options: { codecs?: boolean; peerThrows?: boolean; microphone?: 'granted' | 'denied' | 'none'; micSettings?: Record<string, unknown>;
  /** The DOMException name a `denied` microphone is refused with (NotAllowedError when absent), and this site's microphone
   *  permission as the browser states it afterwards: null where it cannot say, absent where the page cannot ask. */
  micRefusal?: string; micPermission?: 'granted' | 'denied' | 'prompt' | null; sign?: false | (() => Promise<{ publicKey: string; signature: string } | null>);
  /** The service's relay, when the page has one to ask; `refuseRelay` is a browser that will not build a peer with it. */
  relay?: () => Promise<unknown>; refuseRelay?: boolean
  /** The operator's Mic level: a browser that builds it (`ok`), one that cannot (`fails`), one that builds it only when
   *  the test says (`slow`, with `releaseLevel`); absent, a page with no level at all. `storedLevel` is what this browser
   *  kept, as its storage hands it back. */
  micLevel?: 'ok' | 'fails' | 'slow'; storedLevel?: unknown } = {}) {
  let clock = 1000
  const peers: FakePeer[] = []
  const signals: { payload: BrowserStreamPayload; leaseId: string }[] = []
  /** Every time the page asked the browser for the microphone, with what it asked for. */
  const micAsks: unknown[] = []
  const micTracks: FakeMicTrack[] = []
  /** Every level the page built, and every level it asked this browser to keep. */
  const levelGraphs: FakeLevelGraph[] = []
  const savedLevels: number[] = []
  let releaseLevel = () => {}
  /** Where the page's picture area reports its size, while the link watches it. */
  let report: ((width: number, height: number) => void) | null = null
  let ids = 0
  const env: StreamEnvironment = {
    now: () => clock,
    peer: iceServers => {
      if (options.peerThrows) throw Error('no WebRTC')
      if (options.refuseRelay && iceServers.length > STREAM_ICE_SERVERS.length) throw Error('InvalidAccessError')
      const p = new FakePeer(iceServers); peers.push(p); return p
    },
    relayServers: options.relay,
    videoCodecs: () => options.codecs === false ? null : [{ mimeType: 'video/VP8', clockRate: 90000 }],
    audioCodecs: () => options.codecs === false ? null : [{ mimeType: 'audio/opus', clockRate: 48000, channels: 2, sdpFmtpLine: 'minptime=10;useinbandfec=1' }],
    microphone: options.microphone === 'none' ? undefined : constraints => {
      micAsks.push(constraints)
      if (options.microphone === 'denied') return Promise.reject(new DOMException('Permission denied', options.micRefusal ?? 'NotAllowedError'))
      const track = new FakeMicTrack(options.micSettings)
      micTracks.push(track)
      return Promise.resolve(track)
    },
    micPermission: options.micPermission === undefined ? undefined : async () => options.micPermission ?? null,
    micLevel: options.micLevel === undefined ? undefined : {
      graph: async (track, gain, meter) => {
        if (options.micLevel === 'fails') throw new DOMException('No AudioWorklet', 'NotSupportedError')
        const graph = new FakeLevelGraph(track, gain, meter)
        levelGraphs.push(graph)
        if (options.micLevel === 'slow') await new Promise<void>(resolve => { releaseLevel = resolve })
        return graph
      },
      store: { load: () => options.storedLevel ?? null, save: db => { savedLevels.push(db) } },
    },
    mediaStream: track => ({ wrapped: track }),
    audio: silentAudio,
    uuid: () => `00000000-0000-4000-8000-${String(++ids).padStart(12, '0')}`,
    // A5: signs as the contract's offer is signed, unless a test asks otherwise.
    signOffer: options.sign === false ? undefined : options.sign ?? (async () => OFFER_SIGNATURE),
    watchSize: (_video, f) => { report = f; return () => { report = null } },
  }
  const link = new StreamLink((payload, leaseId) => signals.push({ payload, leaseId }), env)
  const video = new FakeVideo()
  link.attachVideo(video)
  return {
    link, env, peers, signals, video, micAsks, micTracks, levelGraphs, savedLevels,
    get peer() { return peers[peers.length - 1] },
    /** A `slow` level is built now. */
    releaseLevel: () => releaseLevel(),
    advance: (ms: number) => { clock += ms; vi.advanceTimersByTime(ms) },
    /** Move the link's clock only, for tests on real timers. */
    tick: (ms: number) => { clock += ms },
    /** The picture area takes a new size, in device pixels, as the browser reports it. */
    resize: (width: number, height: number) => report?.(width, height),
    /** Is the link watching the picture area's size? */
    get watching() { return report !== null },
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

