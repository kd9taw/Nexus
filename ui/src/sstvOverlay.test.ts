// The pure half of the SSTV text overlays: geometry across all five rasters, the
// shrink-to-fit rule, hit-testing (with a control that must miss), movement clamping,
// and the presets' standing agreement with the ID plate's reserved rectangle.
import { describe, it, expect } from 'vitest'
import {
  OVERLAY_COLORS,
  bannerPx,
  BANNER_MIN_PX,
  contrastCss,
  fitCrispSx,
  hitTest,
  moveItem,
  overlayRect,
  preset73,
  presetCq,
  presetReply,
  type OverlayItem,
  type BannerMeasure,
} from './sstvOverlay'
import { plateFor } from './sstvIdOverlay'

/** The five shipped rasters (sstvModes.ts / modespec.rs). */
const RASTERS: Array<[number, number]> = [
  [320, 256],
  [320, 240],
  [640, 496],
  [512, 400],
  [800, 616],
]

/** jsdom has no canvas — the banner measurer is injected, so tests approximate it. */
const measure: BannerMeasure = (text, px) => text.length * px * 0.6

const item = (over: Partial<OverlayItem>): OverlayItem => ({
  id: 't1',
  text: 'CQ CQ DE KD9TAW',
  cx: 0.5,
  cy: 0.5,
  size: 2,
  style: 'crisp',
  color: 'white',
  treatment: 'plate',
  ...over,
})

describe('crisp geometry', () => {
  it('fits every raster — a long CQ at any size step never overflows the width', () => {
    for (const [w, h] of RASTERS) {
      for (const size of [1, 2, 3, 4] as const) {
        const r = overlayRect(item({ size }), w, h, measure)
        expect(r.w, `${w}×${h} @ ${size}×`).toBeLessThanOrEqual(w)
      }
    }
  })

  it('honours the size step where it CAN — short text on a wide raster scales up', () => {
    const small = overlayRect(item({ text: '73', size: 1 }), 800, 616, measure)
    const big = overlayRect(item({ text: '73', size: 3 }), 800, 616, measure)
    expect(big.w).toBeGreaterThan(small.w)
    expect(big.h).toBeGreaterThan(small.h)
  })

  it('shrink-to-fit is the plate rule, not truncation: fitCrispSx floors at 1', () => {
    expect(fitCrispSx(320, 4, 64)).toBe(1)
  })
})

describe('banner geometry', () => {
  it('enforces the legibility floor at every raster and size', () => {
    for (const [, h] of RASTERS) {
      for (const size of [1, 2, 3, 4] as const) {
        expect(bannerPx(h, size)).toBeGreaterThanOrEqual(BANNER_MIN_PX)
        expect(bannerPx(h, size)).toBeLessThanOrEqual(h / 2)
      }
    }
  })
})

describe('hit test', () => {
  const it1 = item({ id: 'a', cx: 0.5, cy: 0.5 })
  it('hits the item under the point and returns the topmost of a stack', () => {
    const it2 = item({ id: 'b', cx: 0.5, cy: 0.5 })
    expect(hitTest([it1, it2], 160, 128, 320, 256, measure)?.id).toBe('b')
  })
  it('control: a point outside every rect misses', () => {
    expect(hitTest([it1], 2, 2, 320, 256, measure)).toBeNull()
  })
})

describe('movement', () => {
  it('moves by raster pixels and clamps the centre inside the raster', () => {
    const moved = moveItem(item({}), 32, -25.6, 320, 256)
    expect(moved.cx).toBeCloseTo(0.6, 5)
    expect(moved.cy).toBeCloseTo(0.4, 5)
    const flung = moveItem(item({}), 10_000, -10_000, 320, 256)
    expect(flung.cx).toBe(0.98)
    expect(flung.cy).toBe(0.02)
  })
})

describe('presets', () => {
  it('clear the ID plate rectangle at every raster (the ident is reserved space)', () => {
    // The plate is what Rust re-draws over arriving pixels; a preset that lands under
    // it would be partly covered on the air. Pinned against plateFor's REAL geometry.
    for (const [w, h] of RASTERS) {
      const plate = plateFor(w, h, 'KD9TAW')
      expect(plate).not.toBeNull()
      for (const p of [presetCq('KD9TAW'), preset73('KD9TAW'), presetReply('ON8ST', 'KD9TAW')]) {
        const r = overlayRect(p, w, h, measure)
        const overlaps = !(
          r.x >= plate!.x + plate!.w ||
          r.x + r.w <= plate!.x ||
          r.y >= plate!.y + plate!.h ||
          r.y + r.h <= plate!.y
        )
        expect(overlaps, `${p.text} vs plate at ${w}×${h}`).toBe(false)
      }
    }
  })

  it('reply carries both calls', () => {
    const p = presetReply('ON8ST', 'KD9TAW')
    expect(p.text).toBe('ON8ST DE KD9TAW 599')
  })
})

describe('palette', () => {
  it('every swatch gets a contrast side that spans the luminance range', () => {
    for (const c of OVERLAY_COLORS) {
      const back = contrastCss(c.id)
      expect(['#000000', '#ffffff']).toContain(back)
      // Light colours back onto black, dark onto white — never tone-on-tone.
      expect(back).toBe(c.lum >= 0.5 ? '#000000' : '#ffffff')
    }
  })
})
