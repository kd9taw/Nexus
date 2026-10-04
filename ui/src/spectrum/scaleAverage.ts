// Log-recursive averaging: each bin's level is smoothed toward each new frame with a TIME constant, in
// dB, the way deskHPSDR drives WDSP's analyzer (`AVERAGE_MODE_LOG_RECURSIVE`, 250 ms by default, its
// per-frame weight `exp(−1 / (fps · τ))`).
//
// IN dB, DELIBERATELY, and that is the opposite of the backend's `RowAverage` (engine.rs), which sums
// POWER and calls a mean of dB values a biased geometric mean. Both are right for what they do. The
// backend's is a decimation — the frames between two reads folded into one row — and must not change a
// level. This is a display filter, and averaging the logarithm is the point of the log-recursive mode:
// it is what flattens a noise floor's grain while a steady carrier keeps its level. Its cost is known:
// a noise floor settles under its power mean, by 2.5 dB for one FFT bin's exponentially distributed
// power (the mean of its 10·log10) and by less once the producer's own peak hold and decimation have
// narrowed the spread; the auto range's mean − 5 dB then measures from that. A row value is linear in
// dB, so averaging the values IS averaging in dB.
//
// A TIME CONSTANT, NOT A PER-FRAME FACTOR. A new frame folds in with weight `1 − exp(−Δt / τ)`, where
// Δt is the time since the last one, so τ means the same at the audio feed's 50 frames a second and at a
// 3-sweep-a-second CI-V scope. A fixed per-frame factor (Thetis users' 0.95 smear) is a time constant
// of about 20 frames: 0.4 s on one source and 6.5 s on the other, which is what blurs CW, syllables and
// FT8 slots into continuous noise. τ = 0 is off: the frame passes through untouched.
//
// It restarts rather than blends when the frames stop describing the same picture (a span, bin count or
// scale change: a retune, a zoom, another source), as RowAverage does. A gap needs no rule: after a gap
// of several τ the new frame's weight is already all but 1.
//
// Per BIN, before the detector: the renderer reduces bins to pixels when it draws, so an average kept
// per bin survives pan and zoom. The one place that differs from WDSP's order (detector, then average):
// a signal hopping between bins inside one pixel reads lower under PEAK than WDSP would draw it.

import type { SpectrumFrame } from './types'

/** The averaging choices an operator is offered, ms; 0 = off. */
export const AVERAGE_STEPS_MS = [0, 50, 100, 250, 500, 1000, 2000] as const
/** The longest time constant offered, ms. */
export const AVERAGE_MAX_MS = 2000

export class LogRecursiveAverager {
  private tau: number
  /** The running per-bin average, in value units (dB). Double precision: the frames are JSON doubles. */
  private avg = new Float64Array(0)
  private at = 0
  private loHz = NaN
  private hiHz = NaN
  private dbPerUnit = NaN
  private primed = false

  constructor(tauMs: number) {
    this.tau = clampTau(tauMs)
  }

  get tauMs(): number {
    return this.tau
  }

  /** A new time constant takes effect from the next frame; the average so far is kept. */
  set tauMs(ms: number) {
    this.tau = clampTau(ms)
  }

  /** Forget the average: the next frame starts it again. */
  reset(): void {
    this.primed = false
  }

  /**
   * Fold `frame` in at `atMs` (a monotonic clock: `performance.now()` at receipt) and return the frame
   * to draw. With τ = 0 that is `frame` itself. Otherwise it is `frame` with this averager's own bins,
   * which the next `push` overwrites: commit or copy them before then. A time that does not advance
   * folds nothing in, and a non-finite bin passes through and restarts its own average.
   */
  push(frame: SpectrumFrame, atMs: number): SpectrumFrame {
    if (!(this.tau > 0)) {
      this.primed = false
      return frame
    }
    const bins = frame.bins
    const n = bins.length
    if (
      !this.primed ||
      n !== this.avg.length ||
      frame.loHz !== this.loHz ||
      frame.hiHz !== this.hiHz ||
      frame.dbPerUnit !== this.dbPerUnit
    ) {
      if (this.avg.length !== n) this.avg = new Float64Array(n)
      for (let i = 0; i < n; i++) this.avg[i] = bins[i]
      this.loHz = frame.loHz
      this.hiHz = frame.hiHz
      this.dbPerUnit = frame.dbPerUnit
      this.at = atMs
      this.primed = true
      return { ...frame, bins: this.avg }
    }
    const dt = atMs - this.at
    if (dt > 0) {
      this.at = atMs
      // The share of the old average left after `dt`: exp(−Δt/τ).
      const keep = Math.exp(-dt / this.tau)
      const avg = this.avg
      for (let i = 0; i < n; i++) {
        const x = bins[i]
        const a = avg[i]
        avg[i] = Number.isFinite(x) && Number.isFinite(a) ? x + (a - x) * keep : x
      }
    }
    return { ...frame, bins: this.avg }
  }
}

function clampTau(ms: number): number {
  return Number.isFinite(ms) ? Math.min(AVERAGE_MAX_MS, Math.max(0, ms)) : 0
}
