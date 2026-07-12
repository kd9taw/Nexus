import { describe, it, expect } from 'vitest'
import { agcRange, applyGainZero, normalize, bakeLut, themeColormap, resolveColormap, isSymmetricMode, scopeView, sidebandSign, zoomRange, WF_F_MIN, WF_F_MAX } from './waterfall'
import { sampleLut } from './colormaps'

describe('agcRange (visual-AGC)', () => {
  it('returns the percentile floor/ceil of a known distribution', () => {
    // 0,1,...,100 → with lo=0.1, hi=0.9 the percentile indices are 10 and 90.
    const arr = Array.from({ length: 101 }, (_, i) => i)
    const { floor, ceil } = agcRange(arr, 0.1, 0.9)
    expect(floor).toBeCloseTo(10, 6)
    expect(ceil).toBeCloseTo(90, 6)
  })

  it('clips outliers so one hot bin does not own the ceiling', () => {
    // a flat-ish floor at ~0.1 with a single 1.0 spike; 99.5th pct stays low.
    const arr = [...Array(199).fill(0.1), 1.0]
    const { floor, ceil } = agcRange(arr)
    expect(floor).toBeCloseTo(0.1, 6)
    expect(ceil).toBeLessThan(0.5) // the spike is clipped away
  })

  it('returns a safe span for empty input', () => {
    expect(agcRange([])).toEqual({ floor: 0, ceil: 1 })
  })

  it('drops non-finite samples (and is empty-safe if all are non-finite)', () => {
    expect(agcRange([NaN, Infinity, -Infinity])).toEqual({ floor: 0, ceil: 1 })
  })

  it('returns a non-degenerate span for all-equal input', () => {
    const { floor, ceil } = agcRange([0.5, 0.5, 0.5, 0.5])
    expect(floor).toBeCloseTo(0.5, 6)
    expect(ceil).toBeGreaterThan(floor) // never floor===ceil → normalize stays finite
  })

  it('handles a single sample', () => {
    const { floor, ceil } = agcRange([0.3])
    expect(floor).toBeCloseTo(0.3, 6)
    expect(ceil).toBeGreaterThan(floor)
  })

  it('accepts a Float32Array', () => {
    const { floor, ceil } = agcRange(new Float32Array([0, 0.25, 0.5, 0.75, 1]), 0, 1)
    expect(floor).toBeCloseTo(0, 6)
    expect(ceil).toBeCloseTo(1, 6)
  })
})

describe('normalize', () => {
  it('linearly maps floor..ceil to 0..1', () => {
    expect(normalize(5, 0, 10)).toBeCloseTo(0.5, 6)
    expect(normalize(0, 0, 10)).toBe(0)
    expect(normalize(10, 0, 10)).toBe(1)
  })

  it('clamps below the floor and above the ceiling', () => {
    expect(normalize(-5, 0, 10)).toBe(0)
    expect(normalize(15, 0, 10)).toBe(1)
  })

  it('returns 0 when ceil<=floor (degenerate range, no divide-by-zero)', () => {
    expect(normalize(5, 10, 10)).toBe(0)
    expect(normalize(5, 10, 5)).toBe(0)
    expect(Number.isFinite(normalize(5, 10, 10))).toBe(true)
  })
})

describe('applyGainZero (manual contrast)', () => {
  it('is the identity at gain=zero=0 (pure auto-AGC)', () => {
    const r = applyGainZero(0.2, 0.8, 0, 0)
    expect(r.floor).toBeCloseTo(0.2, 6)
    expect(r.ceil).toBeCloseTo(0.8, 6)
  })

  it('gain>0 narrows the window (more contrast); gain<0 widens it', () => {
    const span = 0.6
    const narrow = applyGainZero(0.2, 0.8, 1, 0)
    const wide = applyGainZero(0.2, 0.8, -1, 0)
    expect(narrow.ceil - narrow.floor).toBeLessThan(span)
    expect(wide.ceil - wide.floor).toBeGreaterThan(span)
  })

  it('zero>0 raises the floor (dimmer); zero<0 lowers it (more noise shown)', () => {
    expect(applyGainZero(0.2, 0.8, 0, 1).floor).toBeGreaterThan(0.2)
    expect(applyGainZero(0.2, 0.8, 0, -1).floor).toBeLessThan(0.2)
  })

  it('never returns a degenerate window (ceil > floor)', () => {
    const r = applyGainZero(0.5, 0.5, 1, 1) // zero span + max gain
    expect(r.ceil).toBeGreaterThan(r.floor)
  })
})

describe('zoomRange (waterfall span/zoom)', () => {
  it('span 0 (or ≥ full) → the full passband', () => {
    expect(zoomRange(1500, 0)).toEqual({ lo: WF_F_MIN, hi: WF_F_MAX })
    expect(zoomRange(1500, 9999)).toEqual({ lo: WF_F_MIN, hi: WF_F_MAX })
  })

  it('centers the window on the center frequency away from the edges', () => {
    expect(zoomRange(1500, 1000)).toEqual({ lo: 1000, hi: 2000 })
  })

  it('clamps to the low edge without shrinking the span', () => {
    const { lo, hi } = zoomRange(300, 1000) // would start at -200
    expect(lo).toBe(WF_F_MIN)
    expect(hi - lo).toBe(1000)
  })

  it('clamps to the high edge without shrinking the span', () => {
    const { lo, hi } = zoomRange(2800, 1000) // would end past F_MAX
    expect(hi).toBe(WF_F_MAX)
    expect(hi - lo).toBe(1000)
  })
})

describe('scopeView (Phone/CW scope window per feed source)', () => {
  it('audio row: the view window passes through unchanged, marker untouched', () => {
    expect(scopeView(0, 4000, 'audio', 300, 1100, 600, 1)).toEqual({
      loHz: 300,
      hiHz: 1100,
      markerAtHz: 600,
    })
  })

  it('audio row: the window clamps into the captured row (legacy 200–2900 behavior)', () => {
    expect(scopeView(200, 2900, '', 0, 4000, null, 1)).toEqual({
      loHz: 200,
      hiHz: 2900,
      markerAtHz: null,
    })
  })

  it('RF row (flex): the CW window maps around the dial with the marker exactly ON it (zero-beat)', () => {
    // 200 kHz pan centered on a 7.025 MHz dial; CW pitch 600 anchors the marker on the dial.
    const v = scopeView(6_925_000, 7_125_000, 'flex', 300, 1100, 600, 1)
    expect(v.loHz).toBe(7_024_700)
    expect(v.hiHz).toBe(7_025_500)
    expect(v.markerAtHz).toBe(7_025_000)
  })

  it('RF row: LSB/CW-L mirrors the window below the dial (marker still on the dial)', () => {
    const v = scopeView(6_925_000, 7_125_000, 'flex', 300, 1100, 600, -1)
    expect(v.loHz).toBe(7_024_500)
    expect(v.hiHz).toBe(7_025_300)
    expect(v.markerAtHz).toBe(7_025_000)
  })

  it('RF row: the mapped window clamps at the row edges', () => {
    // a narrow ±200 Hz pan: the requested window overflows both edges → clamp, marker intact
    const v = scopeView(7_024_800, 7_025_200, 'civ', 300, 1100, 600, 1)
    expect(v.loHz).toBe(7_024_800)
    expect(v.hiHz).toBe(7_025_200)
    expect(v.markerAtHz).toBe(7_025_000)
  })

  it('REGRESSION: an RF row never degenerates to the 50 Hz sliver at the pan low edge', () => {
    // Phone view (no marker) on a native pan: the old inline clamp gave lo=rowLo,
    // hi=rowLo+50 — a 50 Hz sliver ~100 kHz below the dial stretched across the canvas.
    const v = scopeView(6_925_000, 7_125_000, 'civ', 0, 4000, null, 1)
    expect(v.hiHz - v.loHz).not.toBe(50)
    expect(v.hiHz - v.loHz).toBe(4000) // the full requested audio span, mapped to RF
    expect(v.loHz).toBeGreaterThanOrEqual(7_025_000 - 4000) // anchored at the dial, not the pan edge
  })

  it('RF row: a known dial anchors the marker on the DIAL, not the row center (Flex RETUNE_EPS)', () => {
    // The Flex pan only recenters after >500 Hz dial moves, so during fine tuning the row
    // center sits up to 500 Hz off the dial. Pan stuck at 6.9750–7.1750 (center 7.0750),
    // dial fine-tuned to 7.0254 → zero-beat marker must land ON the dial.
    const v = scopeView(6_975_000, 7_175_000, 'flex', 300, 1100, 600, 1, 7_025_400)
    expect(v.markerAtHz).toBe(7_025_400)
    expect(v.loHz).toBe(7_025_100)
    expect(v.hiHz).toBe(7_025_900)
  })

  it('RF row: a fixed-edge sweep (Icom FIXED mode) anchors the window on the dial, not the sweep center', () => {
    // Fixed-edge civ sweep 7.000–7.100 (center 7.050) with the dial at 7.028: the Phone
    // window must sit at the dial, not 22 kHz away at the sweep center.
    const v = scopeView(7_000_000, 7_100_000, 'civ', 0, 4000, null, 1, 7_028_000)
    expect(v.loHz).toBe(7_028_000)
    expect(v.hiHz).toBe(7_032_000)
  })

  it('RF row: symmetric modes (FM/AM) center the window on the dial (carrier mid-window)', () => {
    // FM is carrier-symmetric: the dial must land at the window CENTER, not its low edge.
    const v = scopeView(6_980_000, 7_080_000, 'civ', 0, 4000, null, 1, 7_028_000, true)
    expect(v.loHz).toBe(7_026_000)
    expect(v.hiHz).toBe(7_030_000)
  })

  it('RF row: an unknown or out-of-row dial falls back to the row center (previous behavior)', () => {
    const noDial = scopeView(6_925_000, 7_125_000, 'flex', 300, 1100, 600, 1, null)
    expect(noDial.markerAtHz).toBe(7_025_000)
    // a stale dial outside the row (e.g. band change before the scope re-tunes) is ignored
    const outside = scopeView(6_925_000, 7_125_000, 'flex', 300, 1100, 600, 1, 14_025_000)
    expect(outside.markerAtHz).toBe(7_025_000)
  })
})

describe('isSymmetricMode', () => {
  it('true for the carrier-symmetric modes (FM/AM), case/whitespace-insensitive', () => {
    expect(isSymmetricMode('FM')).toBe(true)
    expect(isSymmetricMode('fm')).toBe(true)
    expect(isSymmetricMode(' AM ')).toBe(true)
  })

  it('false for sideband/CW/unknown modes', () => {
    expect(isSymmetricMode('USB')).toBe(false)
    expect(isSymmetricMode('LSB')).toBe(false)
    expect(isSymmetricMode('CW')).toBe(false)
    expect(isSymmetricMode('')).toBe(false)
  })
})

describe('sidebandSign', () => {
  it('+1 for USB-side modes (USB / CW / FM / unknown)', () => {
    expect(sidebandSign('USB')).toBe(1)
    expect(sidebandSign('CW')).toBe(1)
    expect(sidebandSign('FM')).toBe(1)
    expect(sidebandSign('')).toBe(1)
  })

  it('-1 for LSB-side modes (LSB / CW-L / CW-R), case-insensitive', () => {
    expect(sidebandSign('LSB')).toBe(-1)
    expect(sidebandSign('lsb')).toBe(-1)
    expect(sidebandSign('CW-L')).toBe(-1)
    expect(sidebandSign('CWR')).toBe(-1)
    expect(sidebandSign('CW-R')).toBe(-1)
  })
})

describe('resolveColormap (palette picker)', () => {
  it("'auto' rides the theme", () => {
    expect(resolveColormap('auto', 'light')).toBe('cividis')
    expect(resolveColormap('auto', 'dark')).toBe('inferno')
  })

  it('an explicit palette wins over the theme', () => {
    expect(resolveColormap('digipan', 'dark')).toBe('digipan')
    expect(resolveColormap('grayscale', 'light')).toBe('grayscale')
  })

  it('an unknown/stale value falls back to the theme map', () => {
    expect(resolveColormap('bogus', 'light')).toBe('cividis')
  })
})

describe('bakeLut', () => {
  it('builds a 256×RGBA table by default with opaque alpha', () => {
    const lut = bakeLut('inferno')
    expect(lut).toBeInstanceOf(Uint8ClampedArray)
    expect(lut.length).toBe(256 * 4)
    expect(lut[3]).toBe(255)
    expect(lut[256 * 4 - 1]).toBe(255)
  })

  it('matches sampleLut at the endpoints (t=0, t=1)', () => {
    const lut = bakeLut('inferno')
    expect([lut[0], lut[1], lut[2]]).toEqual(sampleLut('inferno', 0))
    const last = (256 - 1) * 4
    expect([lut[last], lut[last + 1], lut[last + 2]]).toEqual(sampleLut('inferno', 1))
  })

  it('honors a custom size', () => {
    const lut = bakeLut('viridis', 64)
    expect(lut.length).toBe(64 * 4)
  })

  it('throws on an unknown colormap (via sampleLut)', () => {
    // @ts-expect-error intentional bad name
    expect(() => bakeLut('nope')).toThrow()
  })
})

describe('themeColormap', () => {
  it('maps each theme token to its perceptual colormap', () => {
    expect(themeColormap('dark')).toBe('inferno')
    expect(themeColormap('light')).toBe('cividis')
  })

  it('falls back to inferno for an unknown theme', () => {
    expect(themeColormap('whatever')).toBe('inferno')
  })
})
