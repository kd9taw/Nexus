import { describe, expect, it } from 'vitest'
import { WaterfallHistory, ageLabel } from './waterfallHistory'

// A trivial 256-entry LUT where index i → rgb(i, i, i): intensity reads back directly.
function grayLut(): Uint8ClampedArray {
  const lut = new Uint8ClampedArray(256 * 4)
  for (let i = 0; i < 256; i++) {
    lut[i * 4] = i
    lut[i * 4 + 1] = i
    lut[i * 4 + 2] = i
    lut[i * 4 + 3] = 255
  }
  return lut
}

function render(h: WaterfallHistory, w: number, ht: number, lo: number, hi: number, off = 0) {
  const out = new Uint8ClampedArray(w * ht * 4)
  h.renderInto(out, w, ht, lo, hi, grayLut(), off)
  return out
}

describe('WaterfallHistory', () => {
  it('stores rows newest-at-bottom and renders intensities through the LUT', () => {
    const h = new WaterfallHistory(4, 8)
    h.push([0, 0.5, 1, 0.25], 0, 4000, 1000)
    h.push([1, 0, 0, 0], 0, 4000, 2000)
    const out = render(h, 4, 2, 0, 4000)
    // Bottom row (y=1) is the NEWEST push ([1,0,0,0]).
    expect(out[(1 * 4 + 0) * 4]).toBe(255)
    expect(out[(1 * 4 + 1) * 4]).toBe(0)
    // Top row (y=0) is the older row; column 2 stored 1.0 → 255.
    expect(out[(0 * 4 + 2) * 4]).toBe(255)
    expect(out[(0 * 4 + 1) * 4]).toBe(127) // 0.5 → 127
  })

  it('ring-wraps at depth and length caps', () => {
    const h = new WaterfallHistory(2, 3)
    for (let i = 0; i < 5; i++) h.push([i / 10, i / 10], 0, 100, i)
    expect(h.length).toBe(3)
    // Newest frame is the last push (ts 4); the oldest surviving is ts 2.
    expect(h.frameAt(0)?.tsMs).toBe(4)
    expect(h.frameAt(2)?.tsMs).toBe(2)
    expect(h.frameAt(3)).toBeNull()
  })

  it('peak-preserving resample keeps a narrow carrier when the row is wider than cols', () => {
    const h = new WaterfallHistory(4, 4)
    const row = new Array(16).fill(0)
    row[9] = 1 // a single hot bin — must survive 16→4 pooling (lands in cell 2)
    h.push(row, 0, 1600, 0)
    const out = render(h, 4, 1, 0, 1600)
    expect(out[2 * 4]).toBe(255)
    expect(out[0]).toBe(0)
  })

  it('re-renders through each row own frequency frame (retune-honest history)', () => {
    const h = new WaterfallHistory(4, 8)
    // Older row spans 0..1000 Hz with a carrier at ~875 Hz (col 3).
    h.push([0, 0, 0, 1], 0, 1000, 0)
    // Newer row spans 500..1500 Hz (a retune) with a carrier at ~625 Hz (col 0).
    h.push([1, 0, 0, 0], 500, 1500, 1)
    // View 0..1000: old row's carrier at x≈3/4 of width; new row's carrier at 625 Hz → x≈2/4.
    const out = render(h, 8, 2, 0, 1000)
    // Old (top) row: columns near 875 Hz hot.
    expect(out[(0 * 8 + 7) * 4]).toBe(255)
    // New (bottom) row: 625 Hz → x = 5 (bin centers at 62.5+125k); 500-750 Hz maps to col 0 (hot).
    expect(out[(1 * 8 + 5) * 4]).toBe(255)
    // New row's columns below its 500 Hz lower edge render the palette floor (0 here).
    expect(out[(1 * 8 + 1) * 4]).toBe(0)
  })

  it('scrollback offset shows older rows and maxOffset bounds it', () => {
    const h = new WaterfallHistory(1, 16)
    for (let i = 0; i < 10; i++) h.push([i / 10], 0, 100, i)
    expect(h.maxOffset(4)).toBe(6)
    // Offset 6 with height 4: bottom row is age 6 → ts 3 → intensity 0.3 → 76.
    const out = render(h, 1, 4, 0, 100, 6)
    expect(out[3 * 4]).toBe(Math.floor(0.3 * 255))
  })

  it('clear drops everything (band change)', () => {
    const h = new WaterfallHistory(2, 4)
    h.push([1, 1], 0, 100, 0)
    h.clear()
    expect(h.length).toBe(0)
    const out = render(h, 2, 1, 0, 100)
    expect(out[0]).toBe(0) // palette floor
  })
})

describe('ageLabel', () => {
  it('formats now/seconds/minutes', () => {
    expect(ageLabel(300)).toBe('now')
    expect(ageLabel(12_000)).toBe('12s')
    expect(ageLabel(65_000)).toBe('1m05')
  })
})
