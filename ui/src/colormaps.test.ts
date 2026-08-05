import { describe, it, expect } from 'vitest'
import {
  sampleLut,
  relLuminance,
  lutTexture,
  SEQUENTIAL,
  DEFAULT_COLORMAP,
  type ColormapName,
} from './colormaps'

describe('colormaps', () => {
  it('returns the exact endpoints at t=0 and t=1', () => {
    // inferno endpoints: near-black → near-white.
    expect(sampleLut('inferno', 0)).toEqual([0, 0, 4])
    expect(sampleLut('inferno', 1)).toEqual([252, 255, 164])
  })

  it('clamps t outside [0,1]', () => {
    expect(sampleLut('viridis', -5)).toEqual(sampleLut('viridis', 0))
    expect(sampleLut('viridis', 5)).toEqual(sampleLut('viridis', 1))
  })

  it('sequential maps are luminance-monotonic (the property the old t*t palette lacked)', () => {
    for (const name of SEQUENTIAL) {
      let prev = -1
      for (let i = 0; i <= 64; i++) {
        const lum = relLuminance(sampleLut(name, i / 64))
        // allow a tiny epsilon for rounding at 8-bit quantization
        expect(lum).toBeGreaterThanOrEqual(prev - 0.01)
        prev = lum
      }
    }
  })

  it('always returns in-gamut 8-bit values', () => {
    const names: ColormapName[] = ['inferno', 'viridis', 'cividis', 'turbo', 'sdr-green', 'amber-crt']
    for (const name of names) {
      for (let i = 0; i <= 32; i++) {
        for (const c of sampleLut(name, i / 32)) {
          expect(c).toBeGreaterThanOrEqual(0)
          expect(c).toBeLessThanOrEqual(255)
          expect(Number.isInteger(c)).toBe(true)
        }
      }
    }
  })

  it('turbo holds a DARK RUN at the bottom (the parked waterfall floor renders in it)', () => {
    // Stops are evenly spaced, so the low end's steepness is set by how far stop[1] is from
    // black. With 14 stops and `#4145ab` (L*34.6) at index 19.6 it was 2.68 L*/index, and the
    // waterfall's parked noise grain — which lives around LUT 20 — came out saturated blue.
    // WSJT-X's Default.pal reaches L*5 at index 18 and L*10 at 25; this asserts the same shape.
    // CIE L*, not relLuminance: linear light understates how bright a dark pixel LOOKS, which
    // is the entire quantity in dispute here.
    const lstar = (t: number) => {
      const Y = relLuminance(sampleLut('turbo', t))
      return Y > 0.008856 ? 116 * Math.cbrt(Y) - 16 : 903.3 * Y
    }
    const crosses = (th: number) => {
      for (let i = 0; i < 256; i++) if (lstar(i / 255) >= th) return i
      return 256
    }
    expect(crosses(5)).toBeGreaterThanOrEqual(14)
    expect(crosses(10)).toBeGreaterThanOrEqual(20)
    expect(crosses(20)).toBeGreaterThanOrEqual(30)
    // Still black at the very bottom, and still turbo: the identity is the hue sequence and the
    // endpoint, neither of which the dark run touches.
    expect(sampleLut('turbo', 0)).toEqual([0, 0, 0])
    expect(sampleLut('turbo', 1)).toEqual([122, 4, 3])
    const [r, g, b] = sampleLut('turbo', 0.5)
    expect(g).toBeGreaterThan(r) // mid-scale is the green plateau
    expect(g).toBeGreaterThan(b)
  })

  it('throws on an unknown colormap', () => {
    // @ts-expect-error intentional bad name
    expect(() => sampleLut('nope', 0.5)).toThrow()
  })

  it('builds a 256-entry RGBA texture row', () => {
    const tex = lutTexture(DEFAULT_COLORMAP)
    expect(tex.length).toBe(256 * 4)
    expect(tex[3]).toBe(255) // alpha
    expect(tex[256 * 4 - 1]).toBe(255)
  })
})
