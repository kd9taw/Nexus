import { describe, expect, it } from 'vitest'
import { M_CEIL, M_FLOOR, M_N, MAX_BINS, META, SpectrumRing } from './ring'
import type { SpectrumFrame } from './types'

const frame = (seq: number, bins: ArrayLike<number>, loHz = 0, hiHz = 4000): SpectrumFrame => ({
  seq,
  tMs: 1000 + seq,
  loHz,
  hiHz,
  bins,
  dbPerUnit: 120,
})
const RANGE = { floor: 0.2, ceil: 0.7 }

/** The values the ring holds for the row `age` back. */
function valuesAt(ring: SpectrumRing, age: number): number[] {
  const r = ring.indexAt(age)
  const n = ring.meta[r * META + M_N]
  return Array.from(ring.values.subarray(r * ring.cols, r * ring.cols + n))
}

describe('SpectrumRing', () => {
  it('keeps the newest `depth` rows, newest at age 0, with their frames', () => {
    const ring = new SpectrumRing(4, 8)
    for (let s = 0; s < 6; s++) ring.push(frame(s, [s / 10, s / 10]), RANGE)
    expect(ring.count).toBe(4)
    expect(ring.serial).toBe(6)
    expect([0, 1, 2, 3].map((a) => ring.frameAt(a)?.seq)).toEqual([5, 4, 3, 2])
    expect(ring.frameAt(4)).toBeNull()
    expect(ring.frameAt(-1)).toBeNull()
    expect(valuesAt(ring, 1)).toEqual([Math.fround(0.4), Math.fround(0.4)])
    expect(ring.frameAt(0)).toEqual({ loHz: 0, hiHz: 4000, tMs: 1005, seq: 5 })
  })

  it('stores the display range each row was committed in', () => {
    const ring = new SpectrumRing(4, 8)
    ring.push(frame(1, [0.5]), { floor: 0.1, ceil: 0.9 })
    ring.push(frame(2, [0.5]), { floor: 0.3, ceil: 0.4 })
    const r = ring.indexAt(1)
    expect([ring.meta[r * META + M_FLOOR], ring.meta[r * META + M_CEIL]]).toEqual([0.1, 0.9])
  })

  it('widens for a wider source and keeps every stored row exactly', () => {
    const ring = new SpectrumRing(8, 512)
    const audio = Array.from({ length: 512 }, (_, i) => (i % 97) / 97)
    ring.push(frame(1, audio), RANGE)
    const gen = ring.generation
    const flex = Array.from({ length: 2048 }, (_, i) => (i % 31) / 31)
    ring.push(frame(2, flex, 14e6, 14.2e6), RANGE)
    expect(ring.cols).toBe(2048)
    expect(ring.generation).toBe(gen + 1)
    expect(valuesAt(ring, 1)).toEqual(audio.map(Math.fround))
    expect(valuesAt(ring, 0)).toEqual(flex.map(Math.fround))
  })

  it('stores a non-finite value as the floor of the axis, never NaN', () => {
    const ring = new SpectrumRing(2, 4)
    ring.push(frame(1, [NaN, Infinity, -Infinity, 0.5]), RANGE)
    expect(valuesAt(ring, 0)).toEqual([0, 0, 0, 0.5])
  })

  it('max-pools a frame wider than MAX_BINS, so a one-bin carrier survives', () => {
    const ring = new SpectrumRing(2, 8)
    const wide = new Array(MAX_BINS * 2 + 3).fill(0.1)
    wide[5001] = 0.9
    ring.push(frame(1, wide), RANGE)
    const got = valuesAt(ring, 0)
    expect(got.length).toBe(MAX_BINS)
    expect(Math.max(...got)).toBe(Math.fround(0.9))
    expect(got.filter((v) => v > 0.5).length).toBe(1)
  })

  it('empties on clear and says so through its generation', () => {
    const ring = new SpectrumRing(4, 4)
    ring.push(frame(1, [0.5]), RANGE)
    const gen = ring.generation
    ring.clear()
    expect(ring.count).toBe(0)
    expect(ring.frameAt(0)).toBeNull()
    expect(ring.generation).toBe(gen + 1)
  })
})
