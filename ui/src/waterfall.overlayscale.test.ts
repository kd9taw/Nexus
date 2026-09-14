// #215 — THE WATERFALL'S FREQUENCY DIGITS FOLLOW THE UI SCALE.
//
// Field ask (Discord, 2026-09-12): the axis digits "get lost in the waterfall". They were drawn
// at a fixed 10 px, and on Chromium (WebView2 — the Windows build) that meant 10 px at EVERY UI
// scale: measured in headless Chrome, a 600 px canvas inside `zoom: 1.25` reports
// getBoundingClientRect().width 750 and device-pixel-content-box 750, while offsetWidth stays
// 600. The overlay transform is device px ÷ rect px = 1 (× dpr), so the zoom never reached the
// text. The ratio rect ÷ offsetWidth IS the zoom there, and it is 1 on an engine whose rect is
// already unzoomed — where the transform already carries the zoom. One formula, both engines.
import { describe, it, expect } from 'vitest'
import { overlayTextScale } from './waterfall'

describe('overlayTextScale (#215)', () => {
  it('Chromium: the rect is zoomed and offsetWidth is not, so the scale is the UI zoom', () => {
    expect(overlayTextScale(750, 600)).toBeCloseTo(1.25) // measured at zoom 1.25
    expect(overlayTextScale(660, 600)).toBeCloseTo(1.1) // measured at zoom 1.10
  })

  it('an engine whose rect is already unzoomed gets 1 (its transform carries the zoom)', () => {
    expect(overlayTextScale(600, 600)).toBe(1)
  })

  it('a box that has not laid out reads as 1, never 0 or NaN', () => {
    expect(overlayTextScale(0, 0)).toBe(1)
    expect(overlayTextScale(500, 0)).toBe(1)
    expect(overlayTextScale(0, 500)).toBe(1)
    expect(overlayTextScale(Number.NaN, 600)).toBe(1)
  })

  it('an implausible ratio is clamped to the UI scale range, not trusted', () => {
    expect(overlayTextScale(10_000, 10)).toBeLessThanOrEqual(2)
    expect(overlayTextScale(10, 10_000)).toBeGreaterThanOrEqual(0.5)
  })
})
