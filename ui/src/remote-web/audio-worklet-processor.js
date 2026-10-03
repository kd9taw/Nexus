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
// propagation. A loss the page KNOWS of (the station's sequence skipped) is a gap of exactly
// the audio that was lost, played as the bed where the audio would have been: what follows it
// plays when it would have, so a lost bundle never eats into the buffer and comes back seconds
// later as an underrun.
//
// THE LISTENER'S GAIN, AND NEVER A CLIPPED SAMPLE. The shack sets its RX level for the
// decoders, some 40 dB under a normal listen, so the page lifts it by the listener's volume.
// The bed stands in for that same audio and gets the same gain. A sample the gain would carry
// past the peak is brought down to it at once, and the gain comes back over 100 ms: audio
// under the peak passes untouched.
//
// A FILE OF ITS OWN, served exactly as written. An AudioWorklet module can only be fetched,
// never imported, and the hosted page's policy (`script-src 'self'`) refuses one built from a
// blob: URL - which is how Listen went out telling the operator the station had no audio
// (2026-10-02). So it is a file of the page's own origin, which the build emits beside the page's
// scripts and never inlines. It is plain JavaScript because nothing compiles it. The numbers it
// plays with come from the page (`audio-worklet.ts`), and the name it registers at the bottom must
// stay that file's AUDIO_WORKLET_NAME: audio-worklet.test.ts evaluates this very file and checks.

class NexusReceiveAudio extends AudioWorkletProcessor {
  constructor(options) {
    super()
    const o = (options && options.processorOptions) || {}
    this.capacity = Math.max(1, o.capacity | 0)
    this.prefill = Math.max(1, o.prefill | 0)
    this.ceiling = Math.max(this.prefill, o.ceiling | 0)
    this.level = typeof o.level === 'number' ? o.level : 0.0006 // AUDIO_BED_LEVEL; the page passes it
    this.peak = typeof o.peak === 'number' && o.peak > 0 ? o.peak : 0.9 // AUDIO_PEAK; the page passes it
    this.gain = 1
    this.limit = 1
    this.release = 1 - Math.exp(-1 / (0.1 * (typeof sampleRate === 'number' ? sampleRate : 48000)))
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
      else if (message.gap) this.hole(message.gap)
      else if (typeof message.gain === 'number' && message.gain >= 0) this.gain = message.gain
      // A real end (the station's capture source changed, or listening stopped): throw
      // the buffer away rather than playing out audio from a receiver that is gone.
      else if (message.reset) { this.read = this.write = this.held = 0; this.playing = false }
    }
  }
  // A known loss, as long as the audio that was lost, marked NaN (no decoder makes one) and
  // played as the bed. Only while playing: once the ring has run dry, the bed has already stood
  // in for the lost audio, and the refill starts the timeline again.
  hole(samples) {
    const n = Math.min(this.ceiling, Math.floor(samples) || 0)
    if (this.playing && n > 0) this.push(new Float32Array(n).fill(NaN))
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
    return this.bedState * this.level * 4 * this.gain
  }
  limited(sample) {
    const v = sample * this.gain
    const size = v < 0 ? -v : v
    this.limit = Math.min(size > this.peak ? this.peak / size : 1, this.limit + (1 - this.limit) * this.release)
    return v * this.limit
  }
  process(inputs, outputs) {
    const channel = outputs[0] && outputs[0][0]
    if (!channel) return true
    if (!this.playing && this.held >= this.prefill) this.playing = true
    for (let i = 0; i < channel.length; i++) {
      if (this.playing && this.held > 0) {
        const sample = this.ring[this.read]
        channel[i] = sample === sample ? this.limited(sample) : this.bed()
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
registerProcessor('nexus-receive-audio', NexusReceiveAudio)
