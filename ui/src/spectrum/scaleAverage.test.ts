import { describe, expect, it } from 'vitest'
import { aggregateRow } from './aggregate'
import { AVERAGE_STEPS_MS, LogRecursiveAverager } from './scaleAverage'
import { scaleDefaults } from './scaleSettings'
import type { SpectrumFrame } from './types'

const frame = (bins: number[], loHz = 0, hiHz = 4000, dbPerUnit = 120): SpectrumFrame => ({
  seq: 1,
  tMs: 0,
  loHz,
  hiHz,
  bins,
  dbPerUnit,
})

/** A filter as the measurements below see it: one bin's output after each frame. */
type Filter = (x: number, atMs: number) => number

const logRecursive = (tauMs: number): Filter => {
  const a = new LogRecursiveAverager(tauMs)
  return (x, atMs) => a.push(frame([x]), atMs).bins[0]
}

/** Thetis users' complaint, as code: a fixed 0.95 of the old value per FRAME, whatever the frame rate. */
const perFrameSmear = (): Filter => {
  let y = Number.NaN
  return (x) => (y = Number.isNaN(y) ? x : 0.95 * y + 0.05 * x)
}

/** A step from 0 to 1 (in dB-linear values) at time 0, frames every `dtMs`: the time after the step at
 *  which the output first reaches 1 − 1/e of it, and the time constant the first frame after the step
 *  implies (a log-recursive filter's single-frame response is 1 − exp(−Δt/τ)). */
function stepResponse(f: Filter, dtMs: number): { crossingMs: number; impliedTauMs: number } {
  f(0, -dtMs)
  f(0, 0)
  let crossingMs = Number.NaN
  let impliedTauMs = Number.NaN
  for (let k = 1; k <= 100_000 && Number.isNaN(crossingMs); k++) {
    const y = f(1, k * dtMs)
    if (k === 1) impliedTauMs = -dtMs / Math.log(1 - y)
    if (y >= 1 - Math.exp(-1)) crossingMs = k * dtMs
  }
  return { crossingMs, impliedTauMs }
}

describe('log-recursive averaging: time constants, measured', () => {
  it('0 = no averaging: the frame passes through untouched, at any rate', () => {
    const a = new LogRecursiveAverager(0)
    for (const dt of [20, 66.7, 333]) {
      for (let k = 0; k < 20; k++) {
        const f = frame([Math.sin(k), 0.5, k / 20])
        expect(a.push(f, k * dt)).toBe(f)
      }
    }
    // Turned off mid-stream: the next frame is drawn as it arrived, not blended.
    const b = new LogRecursiveAverager(250)
    b.push(frame([0]), 0)
    b.push(frame([0]), 20)
    b.tauMs = 0
    const raw = frame([1])
    expect(b.push(raw, 40)).toBe(raw)
  })

  it('reaches 1 − 1/e of a step one time constant after it, at the audio, Flex and CI-V frame rates', () => {
    // 20 ms: the audio producer's tick. 66.7 ms: a Flex pan at 15 frames a second. 333 ms: a CI-V scope
    // at three sweeps a second.
    for (const tau of AVERAGE_STEPS_MS.filter((ms) => ms > 0)) {
      for (const dt of [20, 66.7, 333]) {
        const { crossingMs, impliedTauMs } = stepResponse(logRecursive(tau), dt)
        expect(impliedTauMs, `τ ${tau} ms at ${dt} ms frames`).toBeCloseTo(tau, 6)
        // The first frame at or past τ: the crossing is within one frame of the time constant (a frame
        // landing exactly on τ may round to either side of 1 − 1/e).
        expect(crossingMs, `τ ${tau} ms at ${dt} ms frames`).toBeGreaterThanOrEqual(tau - 1e-9)
        expect(crossingMs, `τ ${tau} ms at ${dt} ms frames`).toBeLessThanOrEqual(tau + dt + 1e-9)
      }
    }
  })

  it('means the same at any frame rate; CONTROL: a per-frame 0.95 does not', () => {
    const at = (f: () => Filter, dt: number) => stepResponse(f(), dt).crossingMs
    const tau = 1000
    expect(Math.abs(at(() => logRecursive(tau), 20) - at(() => logRecursive(tau), 333))).toBeLessThan(333)
    // The smear's time constant is ~20 frames: 0.4 s at 50 frames a second, 6.7 s at three.
    const fast = at(perFrameSmear, 20)
    const slow = at(perFrameSmear, 333)
    expect(fast).toBe(400)
    expect(slow).toBeCloseTo(6660, 6)
    expect(Math.abs(fast - slow)).toBeGreaterThan(333)
  })

  it('flattens 25 WPM keying at 250 ms, which is why CW starts with it off', () => {
    // A carrier 40 dB over the noise keyed in dits (48 ms on, 48 ms off), frames every 20 ms (the audio
    // producer's tick): the swing of the averaged carrier bin over the last full second, in dB. Measured:
    // τ 0 → 40, 50 → 24.4, 100 → 15.2, 250 → 6.6, 500 → 3.5.
    const swingDb = (tau: number): number => {
      const f = logRecursive(tau)
      let lo = Infinity
      let hi = -Infinity
      for (let t = 0; t <= 3000; t += 20) {
        const y = f(t % 96 < 48 ? 0.25 + 40 / 120 : 0.25, t)
        if (t >= 2000) {
          lo = Math.min(lo, y)
          hi = Math.max(hi, y)
        }
      }
      return (hi - lo) * 120
    }
    expect(swingDb(0)).toBeCloseTo(40, 9)
    // Under a fifth of the keyed swing is left: a near-static bar, the picture CW's fast trace hold was
    // chosen to remove (a dip of 11% was already "a static bar" then).
    expect(swingDb(250)).toBeLessThan(40 / 5)
    expect(swingDb(50)).toBeGreaterThan(40 / 2)
    expect(swingDb(scaleDefaults('cw').averageMs)).toBeGreaterThan(40 / 2)
    // And Phone keeps the specification's 250 ms.
    expect(scaleDefaults('phone').averageMs).toBe(250)
  })
})

describe('log-recursive averaging: what it keeps and what it drops', () => {
  it('averages the dB values themselves: a steady carrier keeps its level exactly', () => {
    const a = new LogRecursiveAverager(500)
    let out = a.push(frame([0.8, 0.1]), 0)
    for (let k = 1; k < 50; k++) out = a.push(frame([0.8, k % 2 ? 0.3 : 0.1]), k * 20)
    expect(out.bins[0]).toBe(0.8)
    // The alternating bin settles near the mean of its two dB values, not the power mean (which sits
    // within a fraction of a dB of the louder one).
    expect(out.bins[1]).toBeGreaterThan(0.18)
    expect(out.bins[1]).toBeLessThan(0.22)
  })

  it('restarts instead of blending across a retune, a zoom or another scale', () => {
    for (const next of [
      frame([0.9, 0.9], 10, 4000),
      frame([0.9, 0.9, 0.9], 0, 4000),
      frame([0.9, 0.9], 0, 4000, 80),
    ]) {
      const a = new LogRecursiveAverager(2000)
      a.push(frame([0.1, 0.1]), 0)
      a.push(frame([0.1, 0.1]), 20)
      expect(Array.from(a.push(next, 40).bins)).toEqual(Array.from(next.bins))
    }
    // Control: the same frames on the same span DO blend, so the equality above is the restart.
    const a = new LogRecursiveAverager(2000)
    a.push(frame([0.1, 0.1]), 0)
    expect(a.push(frame([0.9, 0.9]), 20).bins[0]).toBeLessThan(0.2)
  })

  it('folds nothing in when time does not advance, and lets a non-finite bin through without poisoning it', () => {
    const a = new LogRecursiveAverager(250)
    a.push(frame([0.5]), 100)
    expect(a.push(frame([0.9]), 100).bins[0]).toBe(0.5)
    expect(a.push(frame([0.9]), 50).bins[0]).toBe(0.5)
    expect(a.push(frame([Number.NaN]), 120).bins[0]).toBeNaN()
    // The next finite value starts that bin again.
    expect(a.push(frame([0.7]), 140).bins[0]).toBe(0.7)
  })
})

describe('AVG and PEAK on a burst', () => {
  // A Flex-like pan: 2048 bins over 200 kHz drawn on 320 px, 6.4 bins a pixel, 15 frames a second. A
  // carrier one bin wide bursts 40 dB out of a −100 dBFS floor for two frames.
  const N = 2048
  const W = 320
  const BURST_BIN = 1000
  const v = (db: number) => 1 + db / 120
  const row = (burst: boolean) => Array.from({ length: N }, (_, i) => v(i === BURST_BIN && burst ? -60 : -100))
  const burstPixel = Math.floor(((BURST_BIN + 0.5) / N) * W)
  /** The burst pixel's level, dB, as the renderer draws it under `detector`. */
  const drawn = (bins: ArrayLike<number>, detector: 'peak' | 'average') => {
    const px = new Float32Array(W)
    aggregateRow(bins, 0, N, 0, 200_000, 0, 200_000, px, detector, 120)
    return (px[burstPixel] - 1) * 120
  }
  /** The bins that pixel covers, by the renderer's cell rule. */
  const cell = (() => {
    const binHz = 200_000 / N
    const pxHz = 200_000 / W
    const fLo = burstPixel * pxHz
    return { i0: Math.floor(fLo / binHz), i1: Math.ceil((fLo + pxHz) / binHz) - 1 }
  })()

  it('with averaging off: PEAK draws the burst at its level, AVG at its power mean over the pixel', () => {
    expect(drawn(row(true), 'peak')).toBeCloseTo(-60, 4)
    const k = cell.i1 - cell.i0 + 1
    expect(k).toBeGreaterThanOrEqual(6)
    const powerMean = 10 * Math.log10((1e-6 + (k - 1) * 1e-10) / k)
    expect(drawn(row(true), 'average')).toBeCloseTo(powerMean, 3)
    // A narrow burst in a wide view: the average detector dilutes it by the bins it shares a pixel with.
    expect(drawn(row(true), 'peak') - drawn(row(true), 'average')).toBeGreaterThan(7)
    // Off the burst both detectors draw the floor.
    expect(drawn(row(false), 'peak')).toBeCloseTo(-100, 4)
    expect(drawn(row(false), 'average')).toBeCloseTo(-100, 4)
  })

  it('through 250 ms of averaging: both rise and decay with the time constant, AVG never above PEAK', () => {
    const dt = 1000 / 15
    const run = (detector: 'peak' | 'average') => {
      const a = new LogRecursiveAverager(250)
      const levels: number[] = []
      for (let k = 0; k < 30; k++) levels.push(drawn(a.push(frame(row(k === 10 || k === 11), 0, 200_000), k * dt).bins, detector))
      return levels
    }
    const peak = run('peak')
    const avg = run('average')
    // Log-recursive in dB: after two frames of the burst the bin stands 40·(1 − e^(−2Δt/τ)) dB up.
    const rise = 40 * (1 - Math.exp((-2 * dt) / 250))
    expect(peak[11]).toBeCloseTo(-100 + rise, 3)
    expect(Math.max(...peak)).toBeCloseTo(-100 + rise, 3)
    // …and one time constant after it ends it has fallen back by 1 − 1/e of that.
    const after = Math.round(250 / dt)
    expect(peak[11 + after] + 100).toBeCloseTo(rise * Math.exp((-after * dt) / 250), 3)
    for (let k = 0; k < 30; k++) expect(avg[k]).toBeLessThanOrEqual(peak[k] + 1e-6)
    expect(peak[11] - avg[11]).toBeGreaterThan(3)
  })
})
