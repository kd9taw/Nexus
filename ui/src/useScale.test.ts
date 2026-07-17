import { describe, it, expect } from 'vitest'
import { fitScale, pickInitialZoom, SCALE_STEPS } from './useScale'

// Fit model: NAT_W=1200, NAT_H=900. Auto NEVER upscales (default cap 100), so 1080p
// full-screen and anything bigger sit at 100%; only SMALLER windows scale down (gently)
// toward the 65% floor. A raised cap (Settings) lets big panels go above 100%.

describe('fitScale', () => {
  it('keeps 1080p full-screen (and bigger) at 100% — no upscaling by default', () => {
    expect(fitScale(1920, 1080)).toBe(100)
    expect(fitScale(1920, 1040)).toBe(100) // maximized (taskbar eats a little height)
    expect(fitScale(2560, 1080)).toBe(100) // ultrawide 1080-tall
    expect(fitScale(3840, 1080)).toBe(100) // very wide but short
    expect(fitScale(3840, 2160)).toBe(100) // 4K — capped at 100 unless the operator raises it
  })

  it('opens the default 1200×720 window roomy (~80%)', () => {
    expect(fitScale(1200, 720)).toBe(80) // 720/900=0.80 → 80
  })

  it('scales DOWN gently on smaller windows, roomy then smaller as needed', () => {
    expect(fitScale(1366, 768)).toBe(85) // 768/900=0.853 → 85
    expect(fitScale(1280, 720)).toBe(80) // 720/900=0.80 → 80
    expect(fitScale(1100, 700)).toBe(75) // 700/900=0.777 → 75
    expect(fitScale(1536, 864)).toBe(90) // "1080p @125% OS" — 864/900=0.96 → 90
  })

  it('floors at 65% and never below', () => {
    expect(fitScale(900, 600)).toBe(65) // 600/900=0.667 → floored to 65
    expect(fitScale(700, 500)).toBe(65) // tiny → still 65
  })

  it('scales UP on big panels only when the cap is raised', () => {
    expect(fitScale(3840, 2160, 125)).toBe(125) // 2160/900=2.4, cap 125
    expect(fitScale(2560, 1440, 125)).toBe(125) // 1440/900=1.6, cap 125
  })

  it('honours a raised cap and snaps DOWN to a real step', () => {
    // 1440/900 = 1.6 → target 160. Steps ≤160 and ≤cap 175: largest is 150.
    expect(fitScale(2560, 1440, 175)).toBe(150)
    // 2160/900 = 2.4 → target 240. Cap 150 → largest step ≤150 is 150.
    expect(fitScale(3840, 2160, 150)).toBe(150)
    expect(fitScale(3840, 2160, 175)).toBe(175)
  })

  it('respects width when width is the binding axis', () => {
    // Very tall, narrow window: 900 wide / 1200 = 0.75 binds over height (2000/900).
    expect(fitScale(900, 2000)).toBe(75)
  })

  it('applies hysteresis: holds the current step within the dead-band', () => {
    // target ~99 (891/900): without prev picks 90; with prev=100 and |99-100|=1 ≤ 100*0.03 → holds 100.
    expect(fitScale(1920, 891)).toBe(90)
    expect(fitScale(1920, 891, 125, 100)).toBe(100)
    // Far from prev → releases: prev=100 but a 768-tall window demands ~85.
    expect(fitScale(1366, 768, 125, 100)).toBe(85)
  })

  it('is a fixed point (no oscillation): feeding the result back does not move it', () => {
    const z = fitScale(1600, 900)
    expect(fitScale(1600, 900, 125, z)).toBe(z)
  })
})

describe('pickInitialZoom (synchronous seed)', () => {
  it('matches fitScale at the default cap', () => {
    expect(pickInitialZoom(1920, 1080)).toBe(fitScale(1920, 1080))
    expect(pickInitialZoom(1366, 768)).toBe(fitScale(1366, 768))
  })

  it('only ever returns a valid scale step', () => {
    for (const [w, h] of [
      [800, 600],
      [1366, 768],
      [1920, 1080],
      [2560, 1440],
      [3840, 2160],
      [1024, 700],
    ] as const) {
      expect(SCALE_STEPS).toContain(pickInitialZoom(w, h))
    }
  })
})
