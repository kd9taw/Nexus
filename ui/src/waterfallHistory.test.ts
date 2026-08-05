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
    // New (bottom) row: its col-0 carrier is centered at 625 Hz, so the peak sits at x=4
    // (pixel center 562.5, below the bin center → edge-held) and FALLS OFF smoothly
    // rather than filling a hard block.
    expect(out[(1 * 8 + 4) * 4]).toBe(255)
    expect(out[(1 * 8 + 5) * 4]).toBeGreaterThan(0)
    expect(out[(1 * 8 + 5) * 4]).toBeLessThan(255)
    expect(out[(1 * 8 + 6) * 4]).toBeLessThan(out[(1 * 8 + 5) * 4])
    // New row's columns below its 500 Hz lower edge render the palette floor (0 here).
    expect(out[(1 * 8 + 1) * 4]).toBe(0)
  })

  it('COLD PATH: a stored row wider than one pixel is interpolated, never blocked', () => {
    // The bug the operator saw as "8 bit": the live path interpolated each new bottom row
    // while renderInto point-sampled, so every palette switch / zoom / resize / pause
    // repainted the whole accumulated waterfall as hard-edged rectangles. 2 stored columns
    // across 8 output pixels used to be two 4-px blocks of 0 and 255.
    const h = new WaterfallHistory(2, 4)
    h.push([0, 1], 0, 200, 0)
    const out = render(h, 8, 1, 0, 200)
    const cols = Array.from({ length: 8 }, (_, x) => out[x * 4])
    expect(cols).toEqual([0, 0, 32, 96, 159, 223, 255, 255])
    expect(new Set(cols).size).toBeGreaterThan(2) // not two flat blocks
  })

  it('an upsampling push softens a one-bin carrier — why Waterfall sizes cols to the feed', () => {
    // Interpolating on the way IN is still better than duplicating (it is what removes the
    // staircase), but it costs peak: no stored column lands exactly on the source bin
    // center. Waterfall therefore builds its history with the spectrum row's own 512
    // columns, so a row is stored untouched and the only resample is the one to device
    // pixels. This pins both halves of that reasoning.
    const row = new Array(8).fill(0.2)
    row[3] = 1 // a single-bin FT8 tone
    const aligned = new WaterfallHistory(8, 4)
    aligned.push(row, 0, 800, 0)
    expect(render(aligned, 8, 1, 0, 800)[3 * 4]).toBe(255) // exact — full brightness kept
    const upsampled = new WaterfallHistory(16, 4)
    upsampled.push(row, 0, 800, 0)
    const peak = Math.max(...Array.from({ length: 16 }, (_, x) => render(upsampled, 16, 1, 0, 800)[x * 4]))
    expect(peak).toBe(204) // 0.8× — the tone is already dimmed before any pixel is drawn
  })

  it('push INTERPOLATES a row narrower than cols instead of duplicating bins', () => {
    // push used to max-pool in both directions, which at cols>n degenerates to plain
    // duplication (each source bin landing in ⌈cols/n⌉ identical columns) — a staircase
    // baked into history that no amount of render-side smoothing can undo.
    const h = new WaterfallHistory(4, 4)
    h.push([0, 1], 0, 400, 0)
    const out = render(h, 4, 1, 0, 400) // 4 px over 4 cols = exact read-back
    const cols = Array.from({ length: 4 }, (_, x) => out[x * 4])
    expect(cols).toEqual([0, 63, 191, 255])
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
