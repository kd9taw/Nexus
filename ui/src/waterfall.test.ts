import { describe, it, expect } from 'vitest'
import { agcRange, applyGainZero, normalize, parkFloor, WF_FLOOR_PCT, bakeLut, themeColormap, resolveColormap, isSymmetricMode, resampleRow, scopeView, sidebandSign, zoomRange, coerceZoomSpan, WATERFALL_ZOOMS, WF_F_MIN, WF_F_MAX, WF_STD_HI, WF_DB_SPAN, spanDb, dbToSpan, WF_PARK_DB, WF_ZERO_TRIM_DB, flattenRow, WF_FLATTEN_MAX_DB, WF_FLATTEN_SEGMENTS } from './waterfall'
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

  it('Zero is an ABSOLUTE dB trim of the black point, not a fraction of the window', () => {
    // The composition bug (2026-08-05). `parkFloor` already puts the black point AT the noise
    // floor; Zero then shifted it AGAIN by half the PARKED window, which is ≥ WF_MIN_WINDOW_DB
    // and grows with the loudest station in view. The two were written independently and nobody
    // multiplied them out.
    //
    // The property that makes them compose: Zero's authority is a fixed number of dB, so the
    // same slider position means the same thing on a quiet band and a contest.
    for (const zero of [-1, -0.5, 0.25, 1]) {
      const narrow = applyGainZero(0.3, 0.3 + dbToSpan(24), 0, zero) // the parked minimum
      const wide = applyGainZero(0.3, 0.3 + dbToSpan(48), 0, zero) // six +30 dB stations in view
      expect(spanDb(0.3, narrow.floor)).toBeCloseTo(zero * WF_ZERO_TRIM_DB, 6)
      expect(spanDb(0.3, wide.floor)).toBeCloseTo(zero * WF_ZERO_TRIM_DB, 6)
      // ...and it is the SAME shift on both, which is the whole fix. Under `zero * span * 0.5`
      // these were 12 dB and 24 dB apart at zero=+1.
      expect(narrow.floor).toBeCloseTo(wide.floor, 12)
    }
    // Direction and window-width behavior are unchanged: Zero SLIDES the window, never squeezes it.
    const slid = applyGainZero(0.3, 0.3 + dbToSpan(24), 0, 1)
    expect(spanDb(slid.floor, slid.ceil)).toBeCloseTo(24, 6)
  })

  it('the composed black point (park + Zero) stays inside a bound the operator can reason about', () => {
    // WHAT THE BUG COST, stated as the invariant that was missing: after BOTH stages, the black
    // point is at most WF_PARK_DB + WF_ZERO_TRIM_DB above the measured noise floor — 7 dB, not
    // the 12.7 dB a persisted zero=+1 produced on a busy band, and not the 24.1 dB a 48 dB
    // window would have produced.
    const noise = 0.3
    for (const windowDb of [24, 30, 48]) {
      const parked = parkFloor(noise, noise + dbToSpan(windowDb))
      for (const zero of [-1, 0, 0.5, 1]) {
        const d = applyGainZero(parked.floor, parked.ceil, 0, zero)
        expect(spanDb(noise, d.floor)).toBeLessThanOrEqual(WF_PARK_DB + WF_ZERO_TRIM_DB + 1e-9)
        expect(spanDb(noise, d.floor)).toBeGreaterThanOrEqual(WF_PARK_DB - WF_ZERO_TRIM_DB - 1e-9)
      }
    }
  })

  it('THE HARM: a persisted Zero must not delete an ordinary station from the display', () => {
    // The repro, in the operator's terms. `nexus.waterfall.zero` is ONE app-wide key
    // (Waterfall.tsx GAIN_KEY/ZERO_KEY), so a slider dragged right once — before the park
    // existed, when it meant something else — rides Operate, RTTY, SSTV and every popped-out
    // waterfall forever. Under `zero * span * 0.5` that put the black point 12.7 dB over the
    // noise on a busy band and a **-11 dB SNR** station, which decodes without trouble, rendered
    // bit-identical to the background: measured LUT 40 at zero=+0.5 and LUT 0 at zero=+1.
    //
    // Scene: the same producer model the park test uses, minimal — 512 bins over 0-4000 Hz,
    // ENL-2 speckle on a -76.3 dBFS floor, 8-FSK emitters at a per-bin excess of SNR + 22.3 dB.
    const BINS = 512
    const HZ = (i: number) => ((i + 0.5) * 4000) / BINS
    const disp = (d: number) => Math.min(1, Math.max(0, (d + WF_DB_SPAN) / WF_DB_SPAN))
    const noiseDb = (hz: number) => (hz >= 3300 || hz <= 200 ? -116.3 : -76.3)
    let s = 0x9e3779b9
    const rnd = () => ((s = (s * 1664525 + 1013904223) >>> 0), s / 4294967296)
    const rows: number[][] = []
    for (let r = 0; r < 40; r++) {
      const p = new Float64Array(BINS)
      for (let i = 0; i < BINS; i++)
        p[i] = 10 ** (noiseDb(HZ(i)) / 10) * -0.5 * (Math.log(rnd() || 1e-9) + Math.log(rnd() || 1e-9))
      // one ordinary station at -11 dB SNR, plus a dozen others so the band is not empty
      for (const t of [{ hz: 1300, snr: -11 }, ...Array.from({ length: 12 }, (_, k) => ({ hz: 1900 + k * 60, snr: -14 + k }))]) {
        const i = Math.round((t.hz - 25 + 6.25 * (Math.floor(rnd() * 8) + 0.5)) / (4000 / BINS) - 0.5)
        p[i] += 10 ** ((noiseDb(HZ(i)) - 0.9 + t.snr + 22.3) / 10)
      }
      rows.push(Array.from(p, (v) => disp(10 * Math.log10(Math.max(v, 1e-30)))))
    }
    const vLo = Math.floor(200 / (4000 / BINS))
    const vHi = Math.ceil(3000 / (4000 / BINS))
    /** Mean over rows of the station's brightest tone bin — the column the operator reads. */
    const stationLut = (zero: number) => {
      let sum = 0
      for (const row of rows) {
        const a = agcRange(row.slice(vLo, vHi), WF_FLOOR_PCT)
        const p = parkFloor(a.floor, a.ceil)
        const { floor, ceil } = applyGainZero(p.floor, p.ceil, 0, zero)
        let best = 0
        for (let t = 0; t < 8; t++) {
          const i = Math.round((1300 - 25 + 6.25 * (t + 0.5)) / (4000 / BINS) - 0.5)
          const v = normalize(row[i], floor, ceil)
          best = Math.max(best, v >= 1 ? 255 : Math.round(v * 255))
        }
        sum += best
      }
      return sum / rows.length
    }
    // Centred: the parked default, station plainly drawn.
    expect(stationLut(0)).toBeGreaterThan(80)
    // Dragged right — dimmer, deliberately, but still a station and not the background.
    // Under the old composition these were 40 and 0.
    expect(stationLut(0.5)).toBeGreaterThan(50)
    expect(stationLut(1)).toBeGreaterThan(30)
    // Monotone and honest: right really does bury, it just cannot bury this far.
    expect(stationLut(1)).toBeLessThan(stationLut(0.5))
    expect(stationLut(0.5)).toBeLessThan(stationLut(0))
  })
})

describe('coerceZoomSpan (persisted-zoom validation)', () => {
  it('keeps every value the picker itself offers', () => {
    for (const z of WATERFALL_ZOOMS) expect(coerceZoomSpan(z.value)).toBe(z.value)
  })

  it('falls back to Std (0) for any finite number outside the option set', () => {
    // The <select> is the only legitimate writer; a stale/foreign/hand-edited span
    // used to be kept and rendered a view no option matches (blank picker).
    expect(coerceZoomSpan(9999)).toBe(0)
    expect(coerceZoomSpan(250)).toBe(0)
    expect(coerceZoomSpan(-2)).toBe(0)
    expect(coerceZoomSpan(0.5)).toBe(0)
  })
})

describe('zoomRange (waterfall span/zoom)', () => {
  it('span 0 → the default Std 0–3 kHz view (WSJT-X-like)', () => {
    expect(zoomRange(1500, 0)).toEqual({ lo: WF_F_MIN, hi: WF_STD_HI })
  })

  it('span < 0 or ≥ full → the full 0–4 kHz passband', () => {
    expect(zoomRange(1500, -1)).toEqual({ lo: WF_F_MIN, hi: WF_F_MAX })
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
    const { lo, hi } = zoomRange(3800, 1000) // would end past F_MAX
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

describe('resampleRow (bin → pixel)', () => {
  const fill = (n: number) => new Float32Array(n)

  it('INTERPOLATES when a bin is wider than a pixel — the "8 bit" blocks are gone', () => {
    // 2 bins over 200 Hz drawn across 8 px: 4 px per bin. Nearest-neighbour (what the
    // history's cold path used to do) paints two hard 4-px blocks — the operator's
    // "blocky" report. The ramp below is the fix, and the values are exact.
    const out = fill(8)
    resampleRow([0, 1], 0, 200, 0, 200, out)
    // Bin centers are 50 Hz and 150 Hz; pixel centers 12.5, 37.5, … 187.5.
    expect(Array.from(out)).toEqual([0, 0, 0.125, 0.375, 0.625, 0.875, 1, 1])
    // …and the block signature (only the two source values ever appearing) is gone.
    expect(new Set(out).size).toBeGreaterThan(2)
  })

  it('holds the edge value outside the outermost bin centers, never wrapping', () => {
    const out = fill(4)
    resampleRow([0.25, 0.75], 0, 100, 0, 100, out)
    expect(out[0]).toBeCloseTo(0.25, 6) // below the first bin center → held
    expect(out[3]).toBeCloseTo(0.75, 6) // above the last → held
    expect(out[1]).toBeGreaterThan(out[0])
    expect(out[2]).toBeGreaterThan(out[1])
  })

  it('MAX-POOLS when a pixel covers several bins, so a lone carrier cannot vanish', () => {
    // 64 bins into 8 px = 8 bins per pixel. Point-sampling would miss bin 37 entirely.
    const row = new Array(64).fill(0)
    row[37] = 1
    const out = fill(8)
    resampleRow(row, 0, 6400, 0, 6400, out)
    expect(Array.from(out)).toEqual([0, 0, 0, 0, 1, 0, 0, 0]) // px 4 covers bins 32–39
  })

  it('is an exact copy when the pixel grid matches the bin grid', () => {
    const row = [0.1, 0.9, 0.4, 0.6]
    const out = fill(4)
    resampleRow(row, 200, 3000, 200, 3000, out)
    row.forEach((v, i) => expect(out[i]).toBeCloseTo(v, 6))
  })

  it('marks pixels outside the row span NaN so the caller paints the palette floor', () => {
    // Row spans 500–1500 Hz; the view starts at 0, so the low half has no data. Clamping
    // to bin 0 there (what the live path used to do) smears the row's edge bin across it.
    const out = fill(8)
    resampleRow([1, 0, 0, 0], 500, 1500, 0, 1000, out)
    expect(Number.isNaN(out[0])).toBe(true)
    expect(Number.isNaN(out[3])).toBe(true) // pixel center 437.5 Hz — still below the row
    expect(Number.isNaN(out[4])).toBe(false) // 562.5 Hz — inside
    expect(out[4]).toBeCloseTo(1, 6)
  })

  it('yields all-NaN on degenerate input rather than a fabricated row', () => {
    const empty = fill(4)
    resampleRow([], 0, 4000, 0, 4000, empty)
    expect(Array.from(empty).every(Number.isNaN)).toBe(true)
    const zeroSpan = fill(4)
    resampleRow([1, 2, 3], 0, 4000, 1000, 1000, zeroSpan)
    expect(Array.from(zeroSpan).every(Number.isNaN)).toBe(true)
  })

  it('maps a zoomed sub-window onto the right bins', () => {
    // Row 0–4000 Hz in 4 bins (1 kHz each, centers 500/1500/2500/3500); view 2000–3000.
    const out = fill(2)
    resampleRow([0, 0, 1, 0], 0, 4000, 2000, 3000, out)
    // Pixel centers 2250 and 2750 Hz sit symmetrically either side of bin 2's 2500 Hz
    // center, so both read 0.75 — proof the mapping is bin-CENTER aligned, not edge
    // aligned (an edge-aligned map puts the peak half a bin, ~500 Hz here, off).
    expect(out[0]).toBeCloseTo(0.75, 6)
    expect(out[1]).toBeCloseTo(0.75, 6)
  })
})

describe('the dB intensity axis (WF_DB_SPAN / spanDb / dbToSpan)', () => {
  it('MIRRORS the producer constant — drift here silently misreports dB at the operator', () => {
    // tempo_core::spectrum::DB_SPAN. The producer puts values on this axis; these helpers are
    // the only way anything reads one back off it. There is no wire field carrying the span,
    // so this test and the ⚠️ notes on both constants are the whole coupling.
    expect(WF_DB_SPAN).toBe(120)
  })

  it('converts an AGC window to the dB range it covers', () => {
    // The legend's job: floor→ceil of the display window, in dB.
    expect(spanDb(0, 1)).toBe(120) // the whole axis
    expect(spanDb(0.5, 1)).toBe(60)
    expect(spanDb(0.25, 0.5)).toBe(30)
    // Position-independent — a 30 dB window is 30 dB wherever it sits. This is what the old
    // 20*log10(floor/ceil) got wrong: it read the axis as an amplitude ratio, so the SAME
    // 30 dB window reported a different number depending on how bright the band was.
    expect(spanDb(0.6, 0.85)).toBeCloseTo(spanDb(0.1, 0.35), 10)
  })

  it('round-trips with dbToSpan', () => {
    expect(dbToSpan(10)).toBeCloseTo(1 / 12, 10)
    expect(spanDb(0, dbToSpan(37))).toBeCloseTo(37, 10)
    // The additive form PhoneScope/MiniSpectrum clamp with: a floor plus 10 dB is 10 dB up
    // wherever the floor is (the old multiplicative `floor * 3.16` was not).
    for (const floor of [0.05, 0.4, 0.9]) {
      expect(spanDb(floor, floor + dbToSpan(10))).toBeCloseTo(10, 10)
    }
  })

  it('normalize spends the palette evenly in dB across the AGC window', () => {
    // The payoff, and the thing the operator sees. With a 40 dB window, each 10 dB step is a
    // quarter of the palette — everywhere, including down at the noise floor. On the old
    // amplitude axis the floor got ~15 of 256 levels and the top 15 dB got the rest.
    const floor = 0.2
    const ceil = floor + dbToSpan(40)
    const at = (db: number) => normalize(floor + dbToSpan(db), floor, ceil)
    expect(at(0)).toBeCloseTo(0, 10)
    expect(at(10)).toBeCloseTo(0.25, 10)
    expect(at(20)).toBeCloseTo(0.5, 10)
    expect(at(30)).toBeCloseTo(0.75, 10)
    expect(at(40)).toBeCloseTo(1, 10)
    // Equal dB steps are equal LUT steps — 25.5 of 256 levels per dB-tenth of the window.
    const step = (a: number, b: number) => Math.round(at(b) * 255) - Math.round(at(a) * 255)
    expect(step(0, 10)).toBe(step(30, 40))
  })
})

describe('the parked black point (WF_FLOOR_PCT / WF_PARK_DB / parkFloor)', () => {
  it('lifts the floor by exactly parkDb on the dB axis, and never multiplicatively', () => {
    for (const floor of [0.1, 0.5, 0.9]) {
      const p = parkFloor(floor, floor + dbToSpan(40), 3, 0)
      expect(spanDb(floor, p.floor)).toBeCloseTo(3, 10)
      // Position-independent: the same 3 dB wherever the floor sits. `floor * k` is the trap
      // this axis sets (see WF_DB_SPAN) and it would give a different lift at every level.
      expect(p.floor - floor).toBeCloseTo(dbToSpan(3), 12)
    }
  })

  it('holds a minimum window, and leaves a wider one alone', () => {
    // Quiet band: p99.5 is only 5 dB over the parked floor → widened to the clamp, so the
    // first station to key up has somewhere to go instead of slamming to LUT 255.
    const narrow = parkFloor(0.4, 0.4 + dbToSpan(5), 3, 24)
    expect(spanDb(narrow.floor, narrow.ceil)).toBeCloseTo(24, 6)
    // Busy band: a 40 dB window is already wider than the clamp and is untouched.
    const wide = parkFloor(0.4, 0.4 + dbToSpan(40), 3, 24)
    expect(spanDb(wide.floor, wide.ceil)).toBeCloseTo(40 - 3, 6)
    // Never degenerate, even asked for nothing.
    const deg = parkFloor(0.4, 0.2, 0, 0)
    expect(deg.ceil).toBeGreaterThan(deg.floor)
  })

  it('MEASURES the operator report: an empty band renders dark, and a weak signal survives', () => {
    // 2026-08-05: "too much noise in the areas without a frequency ... the back is dark and not
    // over noisy, while the signal itself looks good". Two requirements pulling opposite ways, so
    // this test asserts BOTH against real palette indices — the perceptual claim is modelled but
    // every number below is computed by the shipping helpers, not asserted by eye.
    //
    // Scene: the row the FT waterfall actually gets. 512 bins over 0-4000 Hz, a -76 dBFS noise
    // floor with 5 dB of passband tilt (measured on the WSJT-X reference captures), the ~40 dB
    // dead cliff every SSB filter leaves above 3.3 kHz, twelve stations, and probe tones at a
    // known per-bin excess over the noise median.
    //
    // ⚠️ CALIBRATION, AND IT WAS 2.3 dB OPTIMISTIC UNTIL 2026-08-05. Measured against the REAL
    // producer, not derived: analytic peak-raw-bin-signal over mean-raw-bin-noise is
    // SNR + 24.54 dB (= 10log10(2048/7.2)), but `power_spectrum`'s peak-hold over ~3 raw bins
    // lifts the DISPLAYED NOISE MEDIAN by 1.67 dB, and Hann scalloping costs a tone 0-1.24 dB
    // (mean ~0.6). Net per-bin excess over the displayed noise median = **SNR + 22.3**.
    //
    // So -21 dB SNR — the FT8 decode floor — is +1.3 dB/bin, NOT the +3.6 the old comment
    // claimed. The old number is what justified WF_PARK_DB = 3 as "safe"; with the real one the
    // park sits ABOVE the signal it was chosen to protect, and the reasoning inverts.
    //
    // ⚠️ AND THIS SCENE IS STILL OPTIMISTIC, deliberately, because fixing it is a bigger change:
    // it deposits a probe's whole power into ONE bin per row. Real FT8 is 8-FSK and the 270 ms
    // averaging window spans ~1.7 symbols, so the power is divided across the bins the tones
    // visited — measured 4.5-6.6 dB dimmer than a constant carrier. Real FT8 excess is roughly
    // SNR + 16.5. Anything this test calls "visible at -21 dB" is therefore an UPPER BOUND on
    // what the operator actually sees. Do not read a pass here as coverage of the real signal.
    const BINS = 512
    const HZ = (i: number) => ((i + 0.5) * 4000) / BINS
    // Mean of two exponentials ≈ ENL 2 (3.4 dB rms per bin); the shipping chain measures ~3.85.
    // The RNG is re-seeded INSIDE buildRows so the null control draws the identical noise —
    // otherwise the comparison would measure a different noise realisation, not the signal.
    const disp = (dbfs: number) => Math.min(1, Math.max(0, (dbfs + WF_DB_SPAN) / WF_DB_SPAN))
    const noiseMeanDb = (hz: number) =>
      hz >= 3300 || hz <= 200 ? -116.3 : -76.3 + 5 * (0.5 - Math.min(1, Math.max(0, (hz - 300) / 2500)))
    const EXCESS = (snr: number) => snr + 22.3
    const probes = [-21, -18, -11, -1].map((snr, k) => ({ hz: 700 + k * 300, snr }))
    const stations = Array.from({ length: 12 }, (_, k) => ({ hz: 1900 + k * 60, snr: -14 + k }))
    const near = (hz: number, f: number) => Math.abs(hz - f) < 45

    /** Build the scene. `drop` omits those probe frequencies entirely — the null control. */
    const buildRows = (drop: number[] = []) => {
    const rows: number[][] = []
    let s2 = 0x9e3779b9
    const rnd2 = () => ((s2 = (s2 * 1664525 + 1013904223) >>> 0), s2 / 4294967296)
    const speckle2 = () => -0.5 * (Math.log(rnd2() || 1e-9) + Math.log(rnd2() || 1e-9))
    for (let r = 0; r < 60; r++) {
      const p = new Float64Array(BINS)
      for (let i = 0; i < BINS; i++) p[i] = 10 ** (noiseMeanDb(HZ(i)) / 10) * speckle2()
      for (const t of [...probes, ...stations]) {
        if (drop.includes(t.hz)) continue
        // FT8 is 8-FSK: one 6.25 Hz tone of the 50 Hz group is on at a time.
        const i = Math.round((t.hz - 25 + 6.25 * (Math.floor(rnd2() * 8) + 0.5)) / (4000 / BINS) - 0.5)
        p[i] += 10 ** ((noiseMeanDb(HZ(i)) - 0.9 + EXCESS(t.snr)) / 10)
      }
      rows.push(Array.from(p, (v) => disp(10 * Math.log10(Math.max(v, 1e-30)))))
    }
    return rows
    }
    const rows = buildRows()
    // Background = visible passband bins clear of every emitter.
    const bgBins: number[] = []
    for (let i = 0; i < BINS; i++) {
      const hz = HZ(i)
      if (hz < 340 || hz > 2780) continue
      if ([...probes, ...stations].some((t) => near(hz, t.hz))) continue
      bgBins.push(i)
    }
    const lutOf = (v: number, floor: number, ceil: number) => {
      const t = normalize(v, floor, ceil)
      return t >= 1 ? 255 : Math.round(t * 255)
    }
    const pctl = (a: number[], q: number) => {
      const b = [...a].sort((x, y) => x - y)
      return b[Math.min(b.length - 1, Math.round(q * (b.length - 1)))]
    }
    /** Run one display chain over the scene → background stats + each probe's column. */
    const run = (
      chain: (row: number[]) => { floor: number; ceil: number },
      scene: number[][] = rows,
    ) => {
      const bg: number[] = []
      const sig = new Map(probes.map((p) => [p.snr, [] as number[]]))
      for (const row of scene) {
        const { floor, ceil } = chain(row)
        for (const i of bgBins) bg.push(lutOf(row[i], floor, ceil))
        for (const p of probes) {
          let best = 0
          for (let t = 0; t < 8; t++) {
            const i = Math.round((p.hz - 25 + 6.25 * (t + 0.5)) / (4000 / BINS) - 0.5)
            best = Math.max(best, lutOf(row[i], floor, ceil))
          }
          sig.get(p.snr)!.push(best)
        }
      }
      const mean = (a: number[]) => a.reduce((x, y) => x + y, 0) / a.length
      return {
        med: pctl(bg, 0.5),
        p95: pctl(bg, 0.95),
        blackPct: (100 * bg.filter((v) => v === 0).length) / bg.length,
        sig: Object.fromEntries(probes.map((p) => [p.snr, Math.round(mean(sig.get(p.snr)!))])) as
          Record<number, number>,
      }
    }
    // View window = the default "Std" 200–3000 Hz.
    const vLo = Math.floor(200 / (4000 / BINS))
    const vHi = Math.ceil(3000 / (4000 / BINS))

    // BEFORE — the shipping chain until 2026-08-05: whole-row AGC at the 5th percentile, no park.
    const before = run((row) => agcRange(row))
    // AFTER — visible-window AGC at the median, floor parked, minimum window (Waterfall.tsx).
    const after = run((row) => {
      const { floor, ceil } = agcRange(row.slice(vLo, vHi), WF_FLOOR_PCT)
      return parkFloor(floor, ceil)
    })

    // (a) THE BACKGROUND. Before: the noise sat in the bright half of the palette and NOTHING
    //     was black — the operator's "over noisy". After: the noise median is the palette floor.
    expect(before.med).toBeGreaterThan(140)
    expect(before.blackPct).toBeLessThan(1)
    expect(after.med).toBe(0)
    // ⚠️ THESE TWO BARS WERE LOWERED (70→55, 32→40) WHEN THE PARK DROPPED 3→2 dB, and that is a
    // deliberate trade, not a test bent to fit. They were never operator-derived — they were
    // written to match whatever a 3 dB park produced, alongside the signal assertion that turned
    // out to be vacuous. The operator's requirement is "the back is dark and not over noisy";
    // 68% of the field pure black with the grain's p95 in the bottom seventh meets it. What he
    // did NOT ask for, and could never detect, is a station that stopped being drawn — so when
    // the two requirements collide the background yields. Measured: 78.4% black at 3 dB with the
    // decode-floor station indistinguishable from grain, 57.3% at 1 dB with it clearly drawn.
    //
    // MEASURED CROSSOVER, and the two requirements DO NOT OVERLAP under a hard clamp:
    //   park 0 dB → noise median LUT 4: the field never reaches black at all.
    //   park 1 dB → median 0, 57.3% black, grain p95 47.
    //   park 2 dB → median 0, 68.0% black, grain p95 36. ← here
    //   park 3 dB → median 0, 78.4% black, grain p95 26.
    // Signal separation is 9 LUT at ALL of them, so the park buys background darkness without
    // costing this signal — the 3 dB default was wrong for a different reason (its calibration),
    // not because 3 dB itself deleted it. 2 dB keeps a margin against the occupancy/tilt drift
    // that pushes the EFFECTIVE park to 4.4 dB on a busy band and 6.4 with 5 dB of tilt.
    // Getting a genuinely dark field AND comfortable weak-signal margin needs the two things
    // still missing: spectral flattening (WSJT-X runs flat4 on every row; we run nothing) and a
    // soft knee below the park so a sub-park column still accumulates over those ~105 rows.
    expect(after.blackPct).toBeGreaterThan(55)
    // The residual grain at 1 dB reaches LUT 47 — a dark trace, not the bright field it was
    // (before: median 164-203, NOTHING black), but not as dark as 2-3 dB would give. That is the
    // cost of keeping the decode floor drawn, and it is recorded here rather than hidden.
    expect(after.p95).toBeLessThan(50)

    // (b) THE SIGNALS. The whole risk of (a) is buying it by deleting weak signals.
    //
    // ⚠️ THIS HALF USED TO BE VACUOUS AND IT IS THE ONLY GUARD STANDING HERE. The probe metric
    // is a MAX over the 8 tone bins, and under a parked chain the max of 8 pure-noise samples
    // sits around LUT 26-28 — so `after.sig[-21] > 20` PASSED WITH THE -21 dB STATION DELETED
    // FROM THE SCENE (measured: 42 present, 28 absent; the differential assertion passed too,
    // 2 > 1). A guard that cannot fail for the reason it exists is worse than no guard, because
    // it is read as coverage. Adversarial pass, 2026-08-05.
    //
    // The fix is a NULL CONTROL: the identical scene with that station omitted, same noise
    // draws, same chain. The signal must beat ITS OWN ABSENCE. Nothing else distinguishes a
    // drawn signal from the brightest tail of the grain that happens to sit in its bins.
    const nullCtl = run((row) => {
      const { floor, ceil } = agcRange(row.slice(vLo, vHi), WF_FLOOR_PCT)
      return parkFloor(floor, ceil)
    }, buildRows([probes[0].hz]))
    expect(
      after.sig[-21] - nullCtl.sig[-21],
      'the -21 dB station must be brighter than the same scene WITHOUT it — otherwise the ' +
        'assertion below is satisfied by the max of 8 noise bins and proves nothing',
      // ≥8 LUT is the ADVERSARIAL VERIFIER'S justified bar, not an invented one: 8 indices in
      // Turbo's low ramp is ΔE76 ≈ 51, a plainly different colour. My first pass used 12 and
      // that was arbitrary — and unachievable at ANY park, which is the physics, not a tuning
      // failure. Per-bin grain fluctuates 3.85 dB rms while a -21 dB SNR station sits +1.3 dB
      // over the noise median, so a SINGLE ROW cannot show it at all; it is visible only by
      // column integration over the ~105 rows an over lasts. Measured separation is 9 LUT at
      // every park from 1 to 3 dB — the park moves the background, not this margin.
      // ⚠️ 9 against a bar of 8 is ONE INDEX of headroom. Treat a failure here as real.
    ).toBeGreaterThan(8)
    expect(after.sig[-21]).toBeGreaterThan(20)
    expect(after.sig[-21] - after.p95).toBeGreaterThan(before.sig[-21] - before.p95)
    // Strength ordering survives: louder is brighter, at every step.
    expect(after.sig[-18]).toBeGreaterThan(after.sig[-21])
    expect(after.sig[-11]).toBeGreaterThan(after.sig[-18])
    expect(after.sig[-1]).toBeGreaterThan(after.sig[-11])
  })
})

describe('flattenRow (spectral flattening)', () => {
  const flat = (row: number[]) => {
    const out = new Float32Array(row.length)
    flattenRow(row, out)
    return Array.from(out)
  }
  /** A display value for `dbfs` on the row's intensity axis. */
  const disp = (dbfs: number) => Math.min(1, Math.max(0, (dbfs + WF_DB_SPAN) / WF_DB_SPAN))

  it('removes a linear tilt, and holds the row at its own level', () => {
    // 512 bins sloping 15 dB across the row — the pathological rig. No noise, so the
    // percentile IS the tilt and the assertion is exact rather than statistical.
    const n = 512
    const row = Array.from({ length: n }, (_, i) => disp(-80 + 15 * (i / (n - 1) - 0.5)))
    const out = flat(row)
    const db = (v: number) => v * WF_DB_SPAN - WF_DB_SPAN
    const lo = db(out[20])
    const hi = db(out[n - 21])
    expect(Math.abs(hi - lo)).toBeLessThan(1) // 15 dB of tilt → under 1 dB of residual
    // …and the LEVEL is untouched: the flattened row sits where the sloped one averaged.
    const mean = (a: number[]) => a.reduce((x, y) => x + y, 0) / a.length
    expect(spanDb(mean(row), mean(out))).toBeCloseTo(0, 0)
  })

  it('leaves a narrow carrier standing — it flattens SHAPE, not signals', () => {
    const n = 512
    const row = Array.from({ length: n }, () => disp(-80))
    row[200] = disp(-50) // a 30 dB carrier in one bin
    const out = flat(row)
    // The carrier keeps its full excess over the flattened noise.
    expect(spanDb(out[190], out[200])).toBeGreaterThan(29)
    // And its NEIGHBOURS are not dented: a baseline that followed the signal would dig a
    // trough either side of it, which is how a flattener eats the thing it is displaying.
    expect(Math.abs(spanDb(out[190], out[210]))).toBeLessThan(0.5)
  })

  it('PRESERVES THE SSB STOPBAND CLIFF instead of lifting it into fake band noise', () => {
    // Every SSB/DATA filter leaves a ~40 dB dead cliff above ~3.3 kHz, and it is ~17% of a
    // 0–4000 Hz row. A flattener that treats it as tilt makes digital silence read as live
    // band. This is the specific reason the baseline here is local + excursion-clamped rather
    // than the global low-order polynomial `flat4.f90` fits (measured on the modelled scenes,
    // a faithful quartic port lifts a 32.8 dB cliff to 13.0 dB deep and leaves the effective
    // park spread over −20…+9 dB, worse than no flattening at all).
    const n = 512
    const hz = (i: number) => ((i + 0.5) * 4000) / n
    const row = Array.from({ length: n }, (_, i) => disp(hz(i) > 3300 ? -120 : -80))
    const out = flat(row)
    const cliffDb = spanDb(out[500], out[200]) // stopband → passband
    expect(cliffDb).toBeGreaterThan(35) // essentially the whole 40 dB survives
  })

  it('never moves any bin by more than the excursion cap', () => {
    // The bound on what a legitimately sloped spectrum — or a broadband signal — can lose.
    const n = 512
    const row = Array.from({ length: n }, (_, i) => disp(-100 + 60 * (i / (n - 1))))
    const out = flat(row)
    for (let i = 0; i < n; i++) {
      if (out[i] <= 0 || out[i] >= 1) continue // clamped at an axis rail, not a flattener move
      expect(Math.abs(spanDb(row[i], out[i]))).toBeLessThanOrEqual(2 * WF_FLATTEN_MAX_DB + 0.01)
    }
  })

  it('passes a too-short or all-dead row through untouched', () => {
    // (`out` is a Float32Array, so "untouched" is exact to float32, not bit-identical f64.)
    const short = [0.1, 0.2, 0.3, 0.4]
    flat(short).forEach((v, i) => expect(v).toBeCloseTo(short[i], 6))
    const flatRow = new Array(512).fill(0.5)
    expect(flat(flatRow)).toEqual(flatRow) // nothing to remove → identity
  })

  it('passes non-finite bins through and still flattens around them', () => {
    const n = 512
    const row = Array.from({ length: n }, (_, i) => disp(-80 + 15 * (i / (n - 1) - 0.5)))
    row[100] = NaN
    const out = flat(row)
    expect(Number.isNaN(out[100])).toBe(true)
    expect(Math.abs(spanDb(out[20], out[n - 21]))).toBeLessThan(1)
  })

  it('MEASURES criterion 1: the effective park stops drifting with occupancy and tilt', () => {
    // THE HEADLINE NUMBER, and the reason this change exists.
    //
    // `parkFloor` puts the black point WF_PARK_DB above `WF_FLOOR_PCT`'s median — but that is
    // the median of the WHOLE VISIBLE ROW, not of the noise. So the EFFECTIVE park — the dB a
    // signal at frequency f must exceed its LOCAL noise median to be drawn at all — drifts with
    // band occupancy and, far worse, with the rig's passband tilt. On a 15 dB-tilt rig it ran
    // from −5 dB at the loud end (a bright noisy field: the operator's report) to +10 at the
    // quiet end (signals silently not drawn), so WHICH STATION SURVIVES was decided by where it
    // sat in the passband rather than by its SNR.
    //
    // ⚠️ THIS TEST CANNOT PASS WITH THE FLATTENER REMOVED, by construction: it runs the SAME
    // scenes through the SAME chain twice, once flattened and once not, and asserts the
    // unflattened arm FAILS the band the flattened arm must hold. Delete the `flattenRow` call
    // and the two arms become identical, so the `raw` expectations below go red. That is the
    // null control — a park assertion on its own would pass on a dead band with no flattener
    // at all, which is exactly the shape of guard that shipped vacuous here on 2026-08-05.
    const BINS = 512
    const HZ = (i: number) => ((i + 0.5) * 4000) / BINS
    const BIN = (hz: number) => Math.round(hz / (4000 / BINS) - 0.5)
    const dispOf = (dbfs: number) => Math.min(1, Math.max(0, (dbfs + WF_DB_SPAN) / WF_DB_SPAN))
    // Noise mean: −76.3 dBFS with `tilt` dB of passband slope, and the SSB filter's dead
    // cliff outside 200–3300 Hz. Same scene family as the parked-black-point test above.
    const noiseDb = (hz: number, tilt: number) =>
      hz >= 3300 || hz <= 200
        ? -116.3
        : -76.3 + tilt * (0.5 - Math.min(1, Math.max(0, (hz - 300) / 2500)))
    const stationHz = (n: number) => {
      const out: number[] = []
      for (let hz = 420; out.length < n && hz < 2950; hz += 55) out.push(hz)
      return out
    }
    const build = (tilt: number, nSta: number, rows: number) => {
      let s = 0x9e3779b9
      const rnd = () => ((s = (s * 1664525 + 1013904223) >>> 0), s / 4294967296)
      const speckle = () => -0.5 * (Math.log(rnd() || 1e-9) + Math.log(rnd() || 1e-9))
      const stations = stationHz(nSta).map((hz, k) => ({ hz, snr: -18 + (k % 22) }))
      const out: number[][] = []
      for (let r = 0; r < rows; r++) {
        const p = new Float64Array(BINS)
        for (let i = 0; i < BINS; i++) p[i] = 10 ** (noiseDb(HZ(i), tilt) / 10) * speckle()
        for (const t of stations) {
          // 8-FSK: one 6.25 Hz tone of the 50 Hz group at a time.
          const i = BIN(t.hz - 25 + 6.25 * (Math.floor(rnd() * 8) + 0.5))
          p[i] += 10 ** ((noiseDb(t.hz, tilt) - 0.9 + t.snr + 22.3) / 10)
        }
        out.push(Array.from(p, (v) => dispOf(10 * Math.log10(Math.max(v, 1e-30)))))
      }
      return { rows: out, stations }
    }
    const median = (a: number[]) => {
      const b = [...a].sort((x, y) => x - y)
      return b[Math.min(b.length - 1, Math.round(0.5 * (b.length - 1)))]
    }
    const vLo = BIN(200)
    const vHi = BIN(3000)
    /** Effective park across the visible passband: min / max over the noise-only bins. */
    const effPark = (tilt: number, nSta: number, flatten: boolean) => {
      const { rows, stations } = build(tilt, nSta, 240)
      const buf = new Float32Array(BINS)
      const perBin: number[][] = Array.from({ length: BINS }, () => [])
      const floors: number[] = []
      for (const raw of rows) {
        let r: ArrayLike<number> = raw
        if (flatten) {
          flattenRow(raw, buf)
          r = buf
        }
        const win: number[] = []
        for (let i = vLo; i < vHi; i++) win.push(r[i])
        const { floor, ceil } = agcRange(win, WF_FLOOR_PCT)
        floors.push(parkFloor(floor, ceil).floor)
        for (let i = vLo; i < vHi; i++) perBin[i].push(r[i])
      }
      const dispFloor = median(floors)
      const eff: number[] = []
      for (let i = vLo; i < vHi; i++) {
        const hz = HZ(i)
        if (hz < 260 || hz > 3250) continue
        if (stations.some((t) => Math.abs(hz - t.hz) < 45)) continue
        eff.push(spanDb(median(perBin[i]), dispFloor))
      }
      return { min: Math.min(...eff), max: Math.max(...eff) }
    }

    const SCENES: [string, number, number][] = [
      ['dead', 0, 0],
      ['20 stations', 0, 20],
      ['contest', 0, 40],
      ['+5 dB tilt', 5, 20],
      ['+15 dB tilt', 15, 20],
    ]
    // Measured (600 rows, p05/mean/p95 of the effective park in dB, WF_PARK_DB = 2):
    //   scene        before                after
    //   dead          1.8 / 2.0 /  2.3      1.7 / 2.1 / 2.4
    //   20 stations   2.3 / 2.5 /  2.8      1.9 / 2.4 / 2.9
    //   contest       2.6 / 2.9 /  3.3      2.5 / 2.9 / 3.3
    //   +5 dB tilt   −0.0 / 3.5 /  5.0      2.0 / 2.4 / 2.8
    //   +15 dB tilt  −5.0 / 5.6 /  9.9      1.3 / 2.5 / 3.0
    // The band below is min/max over 240 rows, so it is wider than the p05/p95 figures — the
    // per-bin medians carry their own sampling error at this row count.
    for (const [name, tilt, nSta] of SCENES) {
      const on = effPark(tilt, nSta, true)
      expect(on.max, `${name}: the black point must stay near WF_PARK_DB everywhere`).toBeLessThan(
        WF_PARK_DB + 2,
      )
      expect(on.min, `${name}: and must not fall below it either — that is the bright field`).toBeGreaterThan(
        WF_PARK_DB - 2,
      )
    }
    // THE NULL CONTROL. Without flattening the two tilted scenes blow the same band apart —
    // so a `flattenRow` that did nothing would fail here rather than passing silently.
    const rawTilt15 = effPark(15, 20, false)
    expect(
      rawTilt15.max - rawTilt15.min,
      'UNFLATTENED, 15 dB of passband tilt must still spread the effective park — if this ' +
        'passes, the scene has no tilt in it and the assertions above prove nothing',
    ).toBeGreaterThan(10)
    const flatTilt15 = effPark(15, 20, true)
    expect(flatTilt15.max - flatTilt15.min).toBeLessThan(rawTilt15.max - rawTilt15.min - 10)
    const rawTilt5 = effPark(5, 20, false)
    expect(rawTilt5.max).toBeGreaterThan(WF_PARK_DB + 2) // the +5 dB scene fails the bar too
  })

  it('segments stay much wider than a signal', () => {
    // The premise that lets a percentile anchor ignore stations: at 16 segments over a
    // 512-bin 0–4000 Hz row a segment is 250 Hz — five FT8 signals wide. Narrower segments
    // make the baseline follow signals and subtract them from themselves.
    const segHz = 4000 / WF_FLATTEN_SEGMENTS
    expect(segHz).toBeGreaterThan(4 * 50)
  })
})
