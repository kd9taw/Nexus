// The numbers the receive-audio worklet plays with, and the name it registers under. The
// worklet itself - the playback sink and its jitter buffer, and the rules they hold - is
// `audio-worklet-processor.js`, a file of its own: the hosted page's policy refuses a module
// built from a blob: URL.

/** The bed, in the station's own scale before the listener's gain: about -68 dBFS RMS, some 8 dB
 *  under the band noise of a receiver set where Nexus's level meter asks (~30 dB, about -60 dBFS).
 *  It was -44 dBFS RMS with no gain, which at that real level put every gap 16 dB OVER the band:
 *  a burst of static (2026-10-03). At the default volume it plays at about -44 dBFS again. */
export const AUDIO_BED_LEVEL = 0.0006
/** The loudest sample the player ever outputs, -0.9 dBFS. The listener's gain can lift a strong
 *  signal past full scale; the limiter holds it here instead of clipping it. */
export const AUDIO_PEAK = 0.9
/** The listener's volume: gain in dB over the station's own level. The shack sets its RX level
 *  for the decoders (Nexus's meter asks for ~30 dB: about -60 dBFS of band noise), some 40 dB
 *  under a normal listen, and nothing between the capture and the speaker adds any: measured
 *  unity, end to end (2026-10-03). The default lifts that band noise to about -36 dBFS, so a
 *  signal 20 dB over it plays near a normal listen; 0 dB is the station's level as it is. */
export const AUDIO_VOLUME_DB = { min: 0, max: 42, default: 24 } as const
/** Samples of audio to gather before playback starts, and the depth to fall back to
 *  after an overrun. Playback starts the moment a bundle lands, so the cushion against the
 *  next one is this less the bundle in hand (60 ms): 120 ms, enough to ride out a lost
 *  bundle and the station's own cadence, short enough that a remote operator can still work
 *  a pileup. At 120 here the cushion was 60 ms, and a single lost bundle ran it dry: a burst
 *  of the bed, then 120 ms of refill (measured end to end, 2026-10-03). */
export const AUDIO_PREFILL_MS = 180
/** The most the buffer will ever hold. Past this the OLDEST samples go: a listener who
 *  has fallen a third of a second behind wants the band as it is now, not a recording of
 *  the band as it was. This is the resync, and it is a drop, never a speed-up. */
export const AUDIO_CEILING_MS = 400

export const AUDIO_WORKLET_NAME = 'nexus-receive-audio'

