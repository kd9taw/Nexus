// The playback sink, and the jitter buffer with it. It runs in the audio thread because
// that is the only place that knows when a sample is actually due; the page thread's
// timers do not, and a jitter buffer driven by setInterval is a jitter buffer that jitters.
//
// THE RULE THIS EXISTS TO HOLD: never time-stretch to catch up. WebRTC's own receive
// buffer, NetEq, cannot be turned off and does exactly that - it accelerates and expands
// audio to manage its depth, which is tuned for speech intelligibility and is silent
// corruption on CW, PSK31 or FT8, where the timing IS the signal. So this buffer owns
// itself and has exactly two moves: play the samples it has, or drop the oldest ones and
// resync. Nothing here changes the rate a sample is played at.
//
// AND: a gap must sound like a gap. Once playback has started, an empty ring outputs a
// faint, obviously-synthetic bed rather than digital silence, because silence on a radio
// is a dead band and an operator must be able to tell that from a dead link without
// reading the screen. The bed is deliberately dull and far below any real signal - it is
// not comfort noise shaped to sound like the band, and it must never be mistaken for
// propagation.
//
// Source as a string, loaded through a blob URL. An AudioWorklet module cannot be
// imported, only fetched, and a separate file would be a build asset to keep in step
// with this one.

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

export const AUDIO_WORKLET_SOURCE = `
class NexusReceiveAudio extends AudioWorkletProcessor {
  constructor(options) {
    super()
    const o = (options && options.processorOptions) || {}
    this.capacity = Math.max(1, o.capacity | 0)
    this.prefill = Math.max(1, o.prefill | 0)
    this.ceiling = Math.max(this.prefill, o.ceiling | 0)
    this.level = typeof o.level === 'number' ? o.level : ${AUDIO_BED_LEVEL}
    this.ring = new Float32Array(this.capacity)
    this.read = 0
    this.write = 0
    this.held = 0
    // Playback starts only once prefill is met, and stops the moment the ring empties.
    // Both transitions are reported, because they are the gap the operator is told about.
    this.playing = false
    this.underruns = 0
    this.dropped = 0
    this.seed = 22695477
    this.bedState = 0
    this.ticks = 0
    this.port.onmessage = event => {
      const message = event.data
      if (!message) return
      if (message.pcm) this.push(new Float32Array(message.pcm))
      // A real end (the station's capture source changed, or listening stopped): throw
      // the buffer away rather than playing out audio from a receiver that is gone.
      else if (message.reset) { this.read = this.write = this.held = 0; this.playing = false }
    }
  }
  push(samples) {
    for (let i = 0; i < samples.length; i++) {
      this.ring[this.write] = samples[i]
      this.write = (this.write + 1) % this.capacity
      if (this.held < this.capacity) this.held++
      else this.read = (this.read + 1) % this.capacity
    }
    // Drop-and-resync. Never a speed-up, and never a silent one: the count goes out with
    // the next report so the page can say the link fell behind.
    if (this.held > this.ceiling) {
      const excess = this.held - this.prefill
      this.read = (this.read + excess) % this.capacity
      this.held -= excess
      this.dropped += excess
    }
  }
  // A cheap LCG through a one-pole lowpass. Dull and obviously synthetic on purpose: it
  // must never be mistaken for band noise.
  bed() {
    this.seed = (this.seed * 1103515245 + 12345) & 0x7fffffff
    const white = (this.seed / 0x3fffffff) - 1
    this.bedState = this.bedState * 0.85 + white * 0.15
    return this.bedState * this.level * 4
  }
  process(inputs, outputs) {
    const channel = outputs[0] && outputs[0][0]
    if (!channel) return true
    if (!this.playing && this.held >= this.prefill) this.playing = true
    for (let i = 0; i < channel.length; i++) {
      if (this.playing && this.held > 0) {
        channel[i] = this.ring[this.read]
        this.read = (this.read + 1) % this.capacity
        this.held--
      } else {
        if (this.playing) { this.playing = false; this.underruns++ }
        // Silence before the first sample ever plays - the page says "connecting" and a
        // hiss on open would be worse than nothing. The bed only stands in for audio
        // that was expected and did not come.
        channel[i] = this.underruns > 0 ? this.bed() : 0
      }
    }
    // One report every ~85 ms at 48 kHz. Often enough for a state line, rarely enough
    // that postMessage is not part of the audio budget.
    if (++this.ticks >= 32) {
      this.ticks = 0
      this.port.postMessage({ held: this.held, underruns: this.underruns, dropped: this.dropped, playing: this.playing })
    }
    return true
  }
}
registerProcessor(${JSON.stringify(AUDIO_WORKLET_NAME)}, NexusReceiveAudio)
`
