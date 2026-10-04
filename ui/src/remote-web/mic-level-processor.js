// The operator's microphone level, in the audio thread, between the browser's microphone and the
// stream's audio line: every sample times the level, and a limiter that never lets one past the peak.
//
// THE LEVEL, AND NEVER A CLIPPED SAMPLE. The page asks the browser for no automatic gain, so the voice
// arrives at the microphone's own level, and the station plays it at unity: this is the only place
// it is lifted. A sample the level would carry past the peak is brought down to it at once, and the
// level comes back over 100 ms (the receive side's limiter, the same numbers): audio under the peak
// passes untouched, so at 0 dB a microphone under the peak goes exactly as it did before there was a
// level. The peak is under full scale to leave the codec room (MIC_PEAK says why).
//
// WHAT IT TELLS THE PAGE. Every `every` samples, the loudest sample it sent and whether the limiter
// held one down: the meter beside the level. Nothing else, and nothing here keys anything.
//
// A FILE OF ITS OWN, served exactly as written: an AudioWorklet module can only be fetched, and the
// hosted page's policy (`script-src 'self'`) refuses one built from a blob: URL. Plain JavaScript,
// because nothing compiles it. The numbers come from the page (`mic-level.ts`), and the name it
// registers at the bottom must stay that file's MIC_LEVEL_WORKLET_NAME: mic-level.test.ts evaluates
// this very file and checks.

class NexusMicLevel extends AudioWorkletProcessor {
  constructor(options) {
    super()
    const o = (options && options.processorOptions) || {}
    this.gain = typeof o.gain === 'number' && o.gain >= 0 ? o.gain : 1
    this.peak = typeof o.peak === 'number' && o.peak > 0 ? o.peak : 0.708 // MIC_PEAK; the page passes it
    this.every = (o.every | 0) >= 128 ? o.every | 0 : 2400 // MIC_METER_MS at the context's rate; the page passes it
    this.limit = 1
    this.release = 1 - Math.exp(-1 / (0.1 * (typeof sampleRate === 'number' ? sampleRate : 48000)))
    this.loudest = 0
    this.held = false
    this.count = 0
    this.port.onmessage = event => {
      const message = event.data
      if (message && typeof message.gain === 'number' && message.gain >= 0) this.gain = message.gain
    }
  }
  process(inputs, outputs) {
    const output = outputs[0] && outputs[0][0]
    if (!output) return true
    const input = inputs[0] && inputs[0][0]
    for (let i = 0; i < output.length; i++) {
      const v = (input ? input[i] : 0) * this.gain
      const size = v < 0 ? -v : v
      const limit = size > this.peak ? this.peak / size : 1
      if (limit < 1) this.held = true
      this.limit = Math.min(limit, this.limit + (1 - this.limit) * this.release)
      const sent = v * this.limit
      output[i] = sent
      const s = sent < 0 ? -sent : sent
      if (s > this.loudest) this.loudest = s
    }
    this.count += output.length
    if (this.count >= this.every) {
      this.port.postMessage({ peak: this.loudest, limited: this.held })
      this.count = 0
      this.loudest = 0
      this.held = false
    }
    return true
  }
}
registerProcessor('nexus-mic-level', NexusMicLevel)
