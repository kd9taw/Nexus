// The microphone's level, from the browser's side: how loud the operator's voice goes to the station,
// a limiter that keeps it from ever clipping, and a meter of what goes. The level and the limiter run
// in the audio thread (`mic-level-processor.js`), between the browser's microphone and the stream's
// audio line; this file holds their numbers, the level kept by this browser, and the meter.
//
// WHY THE PAGE, AND WHY A CONTROL. Measured end to end (2026-10-03, the real station Session against
// this page in Chrome): the station plays what arrives at unity. A tone from the browser's microphone
// reaches the transmit route 0.1 dB down, and speech 0.4 dB down, which is what the route's 12 kHz
// leaves out above 6 kHz. The page asks the browser for no automatic gain (MIC_CONSTRAINTS: AGC pumps
// the level and wrecks ALC discipline), so the voice goes at the microphone's own level, and a quiet
// microphone is quiet on the air: the browser's own AGC would have lifted the same quiet speech by
// 17 dB. How much to add is a property of this microphone in this browser, so it is set and kept here.
//
// TODAY'S LEVEL IS THE DEFAULT. 0 dB sends the microphone exactly as before this control; nothing in
// the chain loses level to make up.
//
// THE LEVEL NEVER KEYS ANYTHING. A held PTT arms an over and the first audio the station accepts keys
// it, at whatever level (the audio design's M1 and M5). A level changes what goes out, never whether or
// when anything is transmitted, and moving it sends nothing to the station.

import type { MicTrackLike } from './stream-link'

/** The operator's microphone level: gain in dB over the microphone's own level, in whole dB. 0 dB is the
 *  microphone as the browser gives it. The top is a hard ceiling: +20 dB lifts a quiet laptop
 *  microphone's speech (about -40 dBFS) past where the browser's own automatic gain would put it
 *  (about -23), and goes no further, so a room's noise stays under the -40 dBFS the station counts as
 *  voice for its no-power warning unless it is louder than -60 dBFS at the microphone. Below 0, a hot
 *  microphone can be turned down. */
export const MIC_LEVEL_DB = { min: -12, max: 20, default: 0 } as const
/** The loudest sample the page ever sends, -3 dBFS. The level can lift a loud word past full scale: the
 *  limiter holds it here instead of clipping it, and audio under it passes untouched. Not -0.9, the
 *  receive side's peak: Opus carries a limited voice up to 1.4 dB over its peak, and the station's decoder
 *  stops at full scale, so a word held at -0.9 dBFS clipped there (measured end to end at the top of the
 *  range, 2026-10-03). Held at -3 dBFS it reached -1.6. */
export const MIC_PEAK = 10 ** (-3 / 20)
/** How often the audio thread tells the meter the loudest sample it sent. */
export const MIC_METER_MS = 50
/** The meter falls 12 dB a second after a peak (0.6 dB a report), so a word can be read. */
export const MIC_METER_FALL = 10 ** (-0.6 / 20)
/** The meter's ends, in dBFS: its right end is the peak, where the limiter takes over. */
export const MIC_METER_FLOOR_DB = -48
export const MIC_METER_TOP_DB = 20 * Math.log10(MIC_PEAK)
/** How many reports the limiter's mark stays up after the limiter last held a word down: half a second. */
const LIMIT_HOLD = 10

export const MIC_LEVEL_WORKLET_NAME = 'nexus-mic-level'

/** The level in whole dB, inside what the control offers. Anything unreadable (nothing kept yet, a value
 *  from another version, a typo) is today's level, never silence or a blast. */
export function micLevel(value: unknown): number {
  const db = typeof value === 'number' ? value : typeof value === 'string' && value.trim() !== '' ? Number(value) : Number.NaN
  if (!Number.isFinite(db)) return MIC_LEVEL_DB.default
  return Math.min(MIC_LEVEL_DB.max, Math.max(MIC_LEVEL_DB.min, Math.round(db)))
}

/** dB of level to the multiplier the audio thread applies. */
export function micGain(db: number): number {
  return 10 ** (db / 20)
}

/** A sample's size in dBFS, for the meter: no lower than its left end. */
export function meterDb(peak: number): number {
  return peak > 0 ? Math.max(MIC_METER_FLOOR_DB, 20 * Math.log10(peak)) : MIC_METER_FLOOR_DB
}

export type MicMeterView = {
  /** The level is running: the microphone is on and goes through it. */
  live: boolean
  /** The loudest sample sent lately, falling after each word. */
  peak: number
  /** The limiter held a word down within the last half second. */
  limited: boolean
}
const QUIET: MicMeterView = { live: false, peak: 0, limited: false }

/** What the microphone is sending, for the meter beside the level. Its own small store, so twenty reports a
 *  second redraw the meter and nothing else. */
export class MicMeter {
  private view: MicMeterView = QUIET
  private hold = 0
  private readonly listeners = new Set<() => void>()

  subscribe = (f: () => void): (() => void) => { this.listeners.add(f); return () => { this.listeners.delete(f) } }
  getSnapshot = (): MicMeterView => this.view

  /** The level is running. */
  start(): void { this.hold = 0; this.set({ live: true, peak: 0, limited: false }) }
  /** The level stopped: a report still in flight from it is not counted. */
  stop(): void { this.hold = 0; this.set(QUIET) }
  /** The audio thread's report: the loudest sample it sent since the last, and whether it was held down. */
  report(peak: number, limited: boolean): void {
    if (!this.view.live) return
    this.hold = limited ? LIMIT_HOLD : Math.max(0, this.hold - 1)
    this.set({ live: true, peak: Math.max(peak, this.view.peak * MIC_METER_FALL), limited: this.hold > 0 })
  }

  private set(view: MicMeterView): void {
    this.view = view
    for (const f of this.listeners) f()
  }
}

/** The level in the browser: the microphone in, the level and the limiter, the stream's line out. */
export type MicLevelGraph = {
  /** The track the line carries: the microphone through the level and the limiter. */
  track: MicTrackLike
  /** A new level, as a multiplier on the microphone's own, from the next sample on. */
  gain: (linear: number) => void
  /** Frees the audio device. The caller stops the tracks. */
  close: () => void
}
export type MicLevelEnvironment = {
  /** Builds the level for `track` at `gain`, reporting to `meter` every MIC_METER_MS. Rejects where the browser
   *  cannot (no AudioWorklet, a context it will not start): the microphone then goes as it is, as it always did. */
  graph?: (track: MicTrackLike, gain: number, meter: (peak: number, limited: boolean) => void) => Promise<MicLevelGraph>
  /** Where this browser keeps the operator's level. */
  store?: { load: () => unknown; save: (db: number) => void }
}

/** Where a browser keeps the operator's level. */
const LEVEL_KEY = 'nexus.remote.micLevel'
/** How long a context may take to start before the microphone goes without its level. */
const START_MS = 2000

/** The real browser. Split out so the link can be driven without an audio device. */
export function browserMicLevel(): MicLevelEnvironment {
  return {
    graph: async (track, gain, meter) => {
      // The output device's own rate, never a requested one: the microphone's track lives at the rate of the
      // browser's audio graph, and Firefox refuses a source at any other.
      const context = new AudioContext({ latencyHint: 'interactive' })
      try {
        // A file of the page's own origin, as the receive worklet is: the hosted page's `script-src 'self'`
        // refuses a module built from a blob: URL. The build emits it beside the page's scripts and never
        // inlines it (vite.remote.config.ts).
        await context.audioWorklet.addModule(new URL('./mic-level-processor.js', import.meta.url).href)
        const source = context.createMediaStreamSource(new MediaStream([track as unknown as MediaStreamTrack]))
        // One channel in: a microphone the browser hands over in stereo is averaged, as the station's own
        // capture folds one.
        const node = new AudioWorkletNode(context, MIC_LEVEL_WORKLET_NAME, {
          numberOfInputs: 1, numberOfOutputs: 1, outputChannelCount: [1],
          channelCount: 1, channelCountMode: 'explicit', channelInterpretation: 'speakers',
          processorOptions: { gain, peak: MIC_PEAK, every: Math.round((context.sampleRate * MIC_METER_MS) / 1000) },
        })
        node.port.onmessage = event => {
          const report = event.data as { peak?: unknown; limited?: unknown } | null
          if (typeof report?.peak === 'number') meter(report.peak, report.limited === true)
        }
        const line = context.createMediaStreamDestination()
        line.channelCount = 1
        source.connect(node)
        node.connect(line)
        // A press of Mic is a user activation, so this starts; a context that will not (a browser's autoplay
        // rule) renders nothing at all while looking healthy, so it is never put on the line.
        await Promise.race([context.resume(), new Promise(resolve => setTimeout(resolve, START_MS))])
        const [out] = line.stream.getAudioTracks()
        if (context.state !== 'running' || !out) throw new DOMException('The audio context did not start', 'NotAllowedError')
        return {
          track: out as unknown as MicTrackLike,
          gain: linear => node.port.postMessage({ gain: linear }),
          close: () => {
            node.port.onmessage = null
            source.disconnect()
            node.disconnect()
            void context.close().catch(() => {})
          },
        }
      } catch (error) {
        void context.close().catch(() => {})
        throw error
      }
    },
    store: { load: () => localStorage.getItem(LEVEL_KEY), save: db => localStorage.setItem(LEVEL_KEY, String(db)) },
  }
}
