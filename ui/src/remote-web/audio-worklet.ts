// The numbers the receive-audio worklet plays with, and the name it registers under. The
// worklet itself - the playback sink and its jitter buffer, and the rules they hold - is
// `audio-worklet-processor.js`, a file of its own: the hosted page's policy refuses a module
// built from a blob: URL.

/** -40 dBFS. Audible in a quiet room, inaudible under any real signal. */
export const AUDIO_BED_LEVEL = 0.01
/** Samples of audio to gather before playback starts, and the depth to fall back to
 *  after an overrun. 120 ms at 48 kHz: enough to ride out ordinary WAN jitter, short
 *  enough that a remote operator can still work a pileup. */
export const AUDIO_PREFILL_MS = 120
/** The most the buffer will ever hold. Past this the OLDEST samples go: a listener who
 *  has fallen a third of a second behind wants the band as it is now, not a recording of
 *  the band as it was. This is the resync, and it is a drop, never a speed-up. */
export const AUDIO_CEILING_MS = 400

export const AUDIO_WORKLET_NAME = 'nexus-receive-audio'

