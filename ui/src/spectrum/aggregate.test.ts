import { describe, expect, it } from 'vitest'
import { normalize, resampleRow } from '../waterfall'
import { aggregateRow, lutIndex, strengthIn } from './aggregate'

/** mulberry32: the same rows on every run. */
function prng(seed: number): () => number {
  let a = seed >>> 0
  return () => {
    a = (a + 0x6d2b79f5) >>> 0
    let t = a
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

const dbToValue = (db: number) => 1 + db / 120

describe('aggregateRow', () => {
  it('is resampleRow under the peak detector, in every regime', () => {
    const r = prng(7)
    // [bins, outW, view relative to the row]: decimating, upsampling, equal, wider than the row,
    // offset past one edge, and a view inside a single bin.
    const cases: [number, number, number, number][] = [
      [2048, 640, 0, 1],
      [512, 1900, 0.05, 0.75],
      [475, 475, 0, 1],
      [850, 300, -0.4, 1.3],
      [512, 200, 0.6, 1.6],
      [64, 50, 0.501, 0.503],
    ]
    for (const [bins, outW, a, b] of cases) {
      const row = Array.from({ length: bins }, () => r())
      const lo = 14_000_000
      const hi = 14_200_000
      const vlo = lo + a * (hi - lo)
      const vhi = lo + b * (hi - lo)
      const want = new Float32Array(outW)
      const got = new Float32Array(outW)
      resampleRow(row, lo, hi, vlo, vhi, want)
      aggregateRow(row, 0, bins, lo, hi, vlo, vhi, got, 'peak', 120)
      expect(Array.from(got)).toEqual(Array.from(want))
    }
  })

  it('reads the bins of a row stored at an offset, and only its own', () => {
    const store = new Float32Array([9, 9, 0.1, 0.2, 0.3, 0.4, 9, 9])
    const out = new Float32Array(2)
    aggregateRow(store, 2, 4, 0, 4, 0, 4, out, 'peak', 120)
    expect(Array.from(out)).toEqual([Math.fround(0.2), Math.fround(0.4)])
  })

  it('averages POWER, not dB, where a pixel covers several bins', () => {
    // -10 and -20 dBFS: their power mean is 10·log10((0.1 + 0.01) / 2) = -12.596 dBFS.
    const out = new Float32Array(1)
    aggregateRow([dbToValue(-10), dbToValue(-20)], 0, 2, 0, 2, 0, 2, out, 'average', 120)
    expect((out[0] - 1) * 120).toBeCloseTo(10 * Math.log10(0.055), 4)
    // The dB mean (-15) is the geometric-mean trap: a carrier among noise reads several dB low.
    expect((out[0] - 1) * 120).toBeGreaterThan(-15 + 2)
  })

  it('keeps a level under both detectors when every covered bin has it', () => {
    const row = new Array(16).fill(dbToValue(-73))
    for (const d of ['peak', 'average'] as const) {
      const out = new Float32Array(3)
      aggregateRow(row, 0, 16, 0, 16, 0, 16, out, d, 120)
      for (const v of out) expect((v - 1) * 120).toBeCloseTo(-73, 4)
    }
  })

  it('interpolates whatever the detector when a bin is wider than a pixel', () => {
    const row = [0.2, 0.6]
    const peak = new Float32Array(8)
    const avg = new Float32Array(8)
    aggregateRow(row, 0, 2, 0, 2, 0, 2, peak, 'peak', 120)
    aggregateRow(row, 0, 2, 0, 2, 0, 2, avg, 'average', 120)
    expect(Array.from(avg)).toEqual(Array.from(peak))
    expect(peak[3]).toBeGreaterThan(0.2)
    expect(peak[3]).toBeLessThan(0.6)
  })

  it('marks a pixel outside the row NaN, under either detector', () => {
    for (const d of ['peak', 'average'] as const) {
      const out = new Float32Array(4)
      aggregateRow([0.5, 0.5], 0, 2, 100, 200, 0, 400, out, d, 120)
      expect(Number.isNaN(out[0])).toBe(true)
      expect(Number.isNaN(out[1])).toBe(false)
      expect(Number.isNaN(out[3])).toBe(true)
    }
  })
})

describe('lutIndex', () => {
  it("is the components' normalize-then-round, ceiling and floor included", () => {
    const r = prng(11)
    for (let i = 0; i < 2000; i++) {
      const floor = r() * 0.6
      const ceil = floor + 0.05 + r() * 0.4
      const v = r() * 1.2 - 0.1
      const t = normalize(v, floor, ceil)
      expect(lutIndex(v, floor, ceil)).toBe(t >= 1 ? 255 : Math.round(t * 255))
      expect(strengthIn(v, floor, ceil)).toBe(t)
    }
    expect(lutIndex(0.5, 0.6, 0.6)).toBe(0)
  })
})
