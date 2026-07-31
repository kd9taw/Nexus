import { describe, it, expect } from 'vitest'
import { integerScaleStep, sstvImageWidth } from './sstvScale'

// THE INTEGER-STEP CHOOSER (census growers.md #9). The SSTV live decode is a low-res
// bitmap; any fractional CSS scale smears it (the design ruling: blur is worse than
// margin). The chooser therefore returns a whole multiple of the decode's native pixel
// size — the largest that fits the measured stage, capped — and the CSS side keeps a
// `min(100%, …)` yield so a stage smaller than 1× shrinks-to-fit instead of clipping
// (the one place a fractional scale is accepted: the alternative is unreachable pixels).

describe('integerScaleStep', () => {
  it('picks the largest whole multiple that fits the width', () => {
    // 160-wide decode in a 900px stage: 5×160=800 fits, 6×160=960 does not.
    expect(integerScaleStep(160, 128, 900, 10000)).toBe(5)
  })

  it('the height can be the binding axis (the ultrawide case: wide stage, short stage)', () => {
    // census #9 geometry: 3440 fullscreen, stage ≈ 3360×593 for a 160×128 decode —
    // width alone would allow 6× (cap), height allows only floor(593/128) = 4.
    expect(integerScaleStep(160, 128, 3360, 593)).toBe(4)
  })

  it('caps at 6× by default (a wall-size postage stamp is not crisper)', () => {
    expect(integerScaleStep(160, 128, 10000, 10000)).toBe(6)
  })

  it('honours a custom cap', () => {
    expect(integerScaleStep(160, 128, 10000, 10000, 3)).toBe(3)
  })

  it('exact fit counts; one pixel short steps down', () => {
    expect(integerScaleStep(160, 128, 480, 10000)).toBe(3)
    expect(integerScaleStep(160, 128, 479, 10000)).toBe(2)
  })

  it('never returns 0 — a stage smaller than the decode still gets 1× (CSS min(100%) shrinks it)', () => {
    expect(integerScaleStep(320, 256, 200, 150)).toBe(1)
  })

  it('degenerate inputs collapse to 1× rather than NaN/Infinity', () => {
    expect(integerScaleStep(0, 128, 900, 900)).toBe(1)
    expect(integerScaleStep(160, 0, 900, 900)).toBe(1)
    expect(integerScaleStep(NaN, 128, 900, 900)).toBe(1)
    expect(integerScaleStep(160, 128, NaN, 900)).toBe(1)
    expect(integerScaleStep(-160, 128, 900, 900)).toBe(1)
  })

  it('always returns an integer (the entire point)', () => {
    for (const [w, h, aw, ah] of [
      [160, 128, 777, 913],
      [320, 240, 1430, 380],
      [113, 97, 1000, 1000],
    ]) {
      const k = integerScaleStep(w, h, aw, ah)
      expect(Number.isInteger(k), `k=${k} for ${w}×${h} in ${aw}×${ah}`).toBe(true)
      expect(k).toBeGreaterThanOrEqual(1)
    }
  })
})

describe('sstvImageWidth', () => {
  // The stamp is a WIDTH, not a step: an integer multiple whenever ≥1× fits, plus the one
  // degenerate case integerScaleStep cannot express — a stage SHORTER than the native
  // decode. The CSS min(100%) yield covers the WIDTH axis only; `height: auto` then
  // follows the ratio into the stage's `overflow: hidden` (review 2026-07-31: stage
  // 638×146 rendered a 176px-tall picture, clipped top and bottom). The height-axis
  // yield therefore lives here, where the measured height is known.
  it('integer multiples whenever at least 1× fits (the normal path)', () => {
    expect(sstvImageWidth(160, 128, 900, 10000)).toBe(800) // k=5
    expect(sstvImageWidth(160, 128, 3360, 593)).toBe(640) // height-bound k=4
    expect(sstvImageWidth(160, 128, 10000, 10000)).toBe(960) // the 6× cap
  })

  it('height yield: a stage shorter than the decode gets the fractional width that fits', () => {
    // 160×240 native with 82px of vertical room: 1× is 240px tall — clipped. The width
    // whose ratio-derived height is exactly 82: floor(82·160/240) = 54.
    expect(sstvImageWidth(160, 240, 606, 82)).toBe(54)
  })

  it('width-degenerate stays 1× native (the CSS min(100%) yield owns that axis)', () => {
    expect(sstvImageWidth(320, 200, 200, 10000)).toBe(320)
  })

  it('never below 1px; an unmeasurable height falls back to 1× native', () => {
    expect(sstvImageWidth(160, 240, 606, 1)).toBe(1) // floor(1·160/240) = 0 → clamped
    expect(sstvImageWidth(160, 240, 606, 0)).toBe(160) // availH ≤ 0: width yield only
    expect(sstvImageWidth(160, 240, 606, -20)).toBe(160)
  })
})
