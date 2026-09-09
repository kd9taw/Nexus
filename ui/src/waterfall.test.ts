import { describe, it, expect } from 'vitest'
import { SCOPE_WINDOW_DB, TRACE_HOLD_MS, traceHoldDecay, agcRange, applyGainZero, normalize, parkFloor, WF_FLOOR_PCT, bakeLut, themeColormap, resolveColormap, isSymmetricMode, resampleRow, scopeView, cwScopeWindow, CW_SCOPE_SPAN_HZ, sidebandSign, zoomRange, zoomWindow, coerceZoomSpan, WATERFALL_ZOOMS, WF_F_MIN, WF_F_MAX, WF_STD_HI, WF_DB_SPAN, spanDb, dbToSpan, WF_PARK_DB, WF_ZERO_TRIM_DB, flattenRow, WF_FLATTEN_MAX_DB, WF_FLATTEN_SEGMENTS } from './waterfall'
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

describe('cwScopeWindow (CW audio scope window)', () => {
  it('centers the operator’s pitch in the window', () => {
    // THE DEFECT (operator, on air 2026-08-16): the window was hardcoded 300–1100, which
    // centers the 600 Hz default and nothing else. At 900 Hz the signal painted three
    // quarters of the way across; at 400 Hz, hard against the left edge.
    for (const pitch of [400, 600, 750, 900, 1200]) {
      const { loHz, hiHz } = cwScopeWindow(pitch)
      expect(hiHz - loHz, `span at ${pitch}`).toBe(CW_SCOPE_SPAN_HZ)
      expect((loHz + hiHz) / 2, `center at ${pitch}`).toBe(pitch)
      expect(loHz, `backend rejects a negative lo (${pitch})`).toBeGreaterThanOrEqual(0)
    }
  })

  it("tracks the rig's filter: span = filter + skirt, clamped, centered on the pitch", () => {
    // 500 Hz CW filter (the operator's screenshot, 2026-08-16): 625 Hz window, pitch
    // mid-window — no dead stopband margins. Unknown/zero filter keeps the fixed 800.
    const w = cwScopeWindow(600, 500)
    expect(w.hiHz - w.loHz).toBe(625)
    expect((w.loHz + w.hiHz) / 2).toBeCloseTo(600, -1)
    expect(cwScopeWindow(600, 200).hiHz - cwScopeWindow(600, 200).loHz).toBe(300) // floor
    expect(cwScopeWindow(600, 2400).hiHz - cwScopeWindow(600, 2400).loHz).toBe(800) // cap
    expect(cwScopeWindow(600, null).hiHz - cwScopeWindow(600, null).loHz).toBe(800)
    expect(cwScopeWindow(600).hiHz - cwScopeWindow(600).loHz).toBe(800)
  })

  it('floors at 0 for a pitch below half the span (never asks for a negative window)', () => {
    // A negative lo is silently refused by the engine, which answers the WHOLE 0–4000 row —
    // so below 400 Hz the marker sits left of center, which is the honest picture: there is
    // no audio below 0 Hz to put on the other side of it.
    expect(cwScopeWindow(300)).toEqual({ loHz: 0, hiHz: CW_SCOPE_SPAN_HZ })
    expect(cwScopeWindow(0)).toEqual({ loHz: 0, hiHz: CW_SCOPE_SPAN_HZ })
  })

  it('keeps the span the engine can honour (lo ≥ 0, hi ≤ 6000, span ≥ 50)', () => {
    for (const pitch of [300, 600, 1200]) {
      const { loHz, hiHz } = cwScopeWindow(pitch)
      expect(loHz).toBeGreaterThanOrEqual(0)
      expect(hiHz).toBeLessThanOrEqual(6000)
      expect(hiHz - loHz).toBeGreaterThanOrEqual(50)
    }
  })
})

describe('scopeView (Phone/CW scope window per feed source)', () => {
  it('audio row: the view window passes through unchanged, marker untouched', () => {
    expect(scopeView(0, 4000, 'audio', 300, 1100, 600, 1)).toEqual({
      loHz: 300,
      hiHz: 1100,
      markerAtHz: 600,
      mirrored: false,
    })
  })

  it('audio row: the window clamps into the captured row (legacy 200–2900 behavior)', () => {
    expect(scopeView(200, 2900, '', 0, 4000, null, 1)).toEqual({
      loHz: 200,
      hiHz: 2900,
      markerAtHz: null,
      mirrored: false,
    })
  })

  it('audio row, carrier-centered (Phone): the DIAL sits at the 1/9 mark, sideband to its right', () => {
    // Rig geometry: audio 0 Hz IS the dial (the suppressed carrier), so on a rig-style
    // scope it belongs on a reference line inside the picture — not at the far left, which
    // is where the plain audio window put it (operator, on air 2026-08-16: "the signal is
    // at the left edge; my FTdx10 and IC-9700 draw it in the middle").
    //
    // It was ±W, dial dead center. But an SSB receiver hands this scope ONE side of the
    // carrier, so the other half of the panel could never hold a signal and the voice was
    // squeezed into ~30% of the width (operator screenshot, 2026-08-16: "the voice is
    // compressed into one section").
    //
    // The guard was then W/3, and on 2026-08-23 the same operator read THAT as a fault the
    // other way: "the dial looks off to the left... it looks mismatched from the signal I am
    // tuned into." It was not — on USB the dial IS the carrier, so the voice sits wholly above
    // it and the dial belongs at the LOW edge of the energy — but a third of the panel standing
    // empty beside the marker reads as the marker being misplaced. W/8 keeps a visible gap and
    // gives the voice 8/9 of the width.
    const v = scopeView(0, 4000, 'rx', 0, 2700, null, 1, null, false, true)
    expect(v.loHz).toBe(-2700 / 8) // W/8 of guard band on the empty (LSB) side
    expect(v.hiHz).toBe(2700) // the full requested W of occupied sideband
    expect((0 - v.loHz) / (v.hiHz - v.loHz), 'audio 0 = the dial = the 1/9 mark').toBeCloseTo(
      1 / 9,
      12,
    )
  })

  it('audio row, carrier-centered: USB occupies the RIGHT of the dial, LSB the LEFT (mirrored)', () => {
    // The axis is RF OFFSET FROM THE DIAL, so which side holds the voice is the sideband's
    // business: USB audio f is at dial+f, LSB at dial−f. `mirrored` is how the caller reads
    // the row backwards to paint an LSB signal below the dial, exactly as a panadapter does.
    const usb = scopeView(0, 4000, 'rx', 0, 4000, null, 1, null, false, true)
    const lsb = scopeView(0, 4000, 'rx', 0, 4000, null, -1, null, false, true)
    expect(usb.mirrored).toBe(false)
    expect(lsb.mirrored).toBe(true)
    // The two axes are REFLECTIONS of each other about the dial — same width, opposite hand.
    expect([lsb.loHz, lsb.hiHz]).toEqual([-usb.hiHz, -usb.loHz])
  })

  it('audio row, carrier-centered: the axis is ASYMMETRIC — W of sideband, W/8 of guard', () => {
    // The bug this pins: a symmetric axis spends half the panel on a side the receiver
    // cannot fill. Both edges, both sidebands, stated as numbers.
    const usb = scopeView(0, 4000, 'rx', 0, 2700, null, 1, null, false, true)
    expect([usb.loHz, usb.hiHz]).toEqual([-2700 / 8, 2700])
    const lsb = scopeView(0, 4000, 'rx', 0, 2700, null, -1, null, false, true)
    expect([lsb.loHz, lsb.hiHz]).toEqual([-2700, 2700 / 8])
    // 8/9 of the panel goes to the voice either way — that is the whole point of the change.
    expect(usb.hiHz / (usb.hiHz - usb.loHz), 'USB voice fills 8/9').toBeCloseTo(8 / 9, 12)
    expect(-lsb.loHz / (lsb.hiHz - lsb.loHz), 'LSB voice fills 8/9').toBeCloseTo(8 / 9, 12)
    // The presets keep their meaning exactly: W is the SIDEBAND width the operator picked
    // (Full 4000, Voice 2700, 1500, 800), never a half-width, and the guard rides along.
    for (const w of [4000, 2700, 1500, 800]) {
      const v = scopeView(0, 4000, 'rx', 0, w, null, 1, null, false, true)
      expect([v.loHz, v.hiHz], `preset ${w}`).toEqual([-w / 8, w])
    }
  })

  it('carrier-centering is Phone-only: it never touches a native RF row', () => {
    // CW's one-sided pitch window and the RF branch both stay exactly as they were —
    // the flag is an explicit Phone opt-in, not a global change of axis.
    const rf = scopeView(6_925_000, 7_125_000, 'flex', 300, 1100, 600, 1, null, false, true)
    expect(rf.loHz).toBe(7_024_700)
    expect(rf.hiHz).toBe(7_025_500)
    expect(rf.markerAtHz).toBe(7_025_000)
    expect(rf.mirrored).toBe(false)
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
    // this asserts BOTH against real palette indices — every number is computed by the shipping
    // helpers over a replica of the real producer, none is asserted by eye.
    //
    // ⚠️ THE SCENE AND THE METRIC WERE BOTH REPLACED (2026-08-05, second pass) AND THE REPORTED
    // WEAK-SIGNAL MARGIN FELL FROM 9 LUT TO 3.2. Neither the chain nor `WF_PARK_DB` changed —
    // the old margin was an artefact of the measurement, on three counts that compounded:
    //
    //  (1) THE NULL CONTROL WAS NOT PAIRED, though its comment claimed it was. The `drop` list
    //      skipped an emitter with `continue` BEFORE its tone draw `Math.floor(rnd2()*8)`, so
    //      omitting a station consumed one fewer RNG value and EVERY LATER DRAW SHIFTED. The
    //      control measured a different noise realisation, not the station's absence (measured
    //      on the old generator: row 1 differed in 505 of 512 bins).
    //  (2) THE METRIC WAS A MAX OVER THE 8 TONE BINS. The max of N samples has a null
    //      expectation that GROWS WITH N — about median + sd·√(2 ln N) — whatever the signal is,
    //      so a fixed bar on it is satisfiable by grain alone. That is exactly how this guard
    //      shipped vacuous the first time (it passed with the station deleted from its scene).
    //  (3) THE SCENE PUT A STATION'S WHOLE POWER IN ONE BIN PER ROW. Real FT8 is 8-FSK and the
    //      row's 270 ms support spans ~1.7 symbols, so the power is divided across the bins the
    //      tones visited; the grain was modelled as the mean of two exponentials (ENL 2, no
    //      spatial correlation) where the real display bin is ENL ~3.1 with lag-1 correlation
    //      0.61, because adjacent display bins SHARE a raw bin under `power_spectrum`'s
    //      peak-hold.
    //
    // All three are fixed below, and the fixes are STRUCTURAL rather than careful:
    //  - the tone is a PURE HASH of (emitter, symbol) and THE RNG IS CONSUMED ONLY BY THE NOISE,
    //    in a fixed order, so deleting an emitter leaves every noise draw bit-identical and the
    //    null separation is EXACTLY 0 — not small, zero, at every seed (verified, see below);
    //  - the metric is a per-bin time MEAN (null expectation is a constant), with the argmax
    //    taken on the SIGNAL side and read at that SAME bin index on the null side, so the
    //    selection cannot bias the difference;
    //  - the scene is a replica of `tempo_core::spectrum::power_spectrum` + `RowAverage`.
    //
    // MODEL VALIDATION against the real chain (this replica / measured / error):
    //   constant-carrier per-bin excess over the displayed noise median  SNR+22.84 / SNR+22.8
    //   FT8 per-row peak vs the same-power carrier                       −1.27 / −1.20 dB
    //   FT8 column-integrated peak vs that carrier                       −3.73 / −4.19 dB
    //   display-bin grain                       2.54 dB rms / 2.51,  lag-1 0.613 / 0.611
    // The 8-FSK penalty is what makes this scene 4–6 dB harsher than the old one: a station is
    // barely dimmer than a carrier IN ONE ROW (−1.3 dB) but much dimmer INTEGRATED (−3.7 dB),
    // because the tone hops and no single column stays lit. Only the integrated view is honest,
    // so that is what is measured.
    const BINS = 512
    const NRAW = 684 // raw bins covering 0–4004 Hz at 12 kHz / FFT_N 2048
    const HZ_RAW = 12000 / 2048 // 5.859375 Hz — `power_spectrum`'s hz_per_bin
    const ROWS = 105 // one FT8 over: 79 symbols × 0.16 s = 12.64 s at rowMs 120
    const BURN = 40 // discarded before any statistic: the AGC EMA (α 0.1) has τ ≈ 10 rows
    const HZ = (i: number) => ((i + 0.5) * 4000) / BINS

    // Hann leakage, analytic. A Hann window is three frequency taps, so a tone at fractional
    // raw-bin offset d lands with power |0.5·D(d) + 0.25·D(d−1) + 0.25·D(d+1)|²/0.25 —
    // 1 at the bin, 0.25 at ±1, and EXACTLY 0 at ±2.
    const D = (x: number) =>
      Math.abs(x) < 1e-9 ? 1 : Math.sin(Math.PI * x) / (2048 * Math.sin((Math.PI * x) / 2048))
    const leak = (d: number) => ((0.5 * D(d) + 0.25 * D(d - 1) + 0.25 * D(d + 1)) / 0.5) ** 2

    // Noise mean per raw bin: −76.3 dBFS, 5 dB of passband tilt, ±1 dB of filter ripple, and
    // the ~40 dB dead cliff every SSB/DATA filter leaves outside ~300–3300 Hz. The RIPPLE IS
    // LOAD-BEARING: a pure linear tilt is removable by a first-order detrend, so without
    // curvature a flattener that cannot follow a passband would score as well as one that can.
    const noiseDb = (hz: number) => {
      const u = Math.min(1, Math.max(0, (hz - 300) / 3000))
      const roll =
        40 *
        Math.max(
          Math.min(1, Math.max(0, (300 - hz) / 300)),
          Math.min(1, Math.max(0, (hz - 3300) / 300)),
        )
      return -76.3 + 5 * (0.5 - u) + Math.cos(2 * Math.PI * u) - roll
    }

    /** Tone index for (emitter, symbol) — A PURE HASH, never the RNG. This is the whole of the
     *  anti-vacuity property: dropping an emitter consumes no draws, so the noise is identical. */
    const toneOf = (id: number, sym: number) => {
      let h = Math.imul(id + 1, 0x9e3779b1) ^ Math.imul(sym + 1, 0x85ebca6b)
      h = Math.imul(h ^ (h >>> 13), 0xc2b2ae35)
      return ((h ^ (h >>> 16)) >>> 0) % 8
    }

    const probes = [-21, -18, -11, -1].map((snr, k) => ({ id: k, hz: 700 + k * 300, snr }))
    const stations = Array.from({ length: 12 }, (_, k) => ({
      id: 100 + k,
      hz: 1900 + k * 60,
      snr: -14 + k,
    }))

    /** Build the scene. `drop` omits those emitter ids — and, by construction, changes NOTHING
     *  else: the RNG below is touched only by the noise loop, in a fixed order. */
    const buildRows = (drop: number[] = [], seed = 1) => {
      let s = seed >>> 0
      const rnd = () => {
        s = (s + 0x6d2b79f5) >>> 0
        let t = s
        t = Math.imul(t ^ (t >>> 15), t | 1)
        t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
        return ((t ^ (t >>> 14)) >>> 0) / 4294967296
      }
      let spare: number | null = null
      const gauss = () => {
        if (spare !== null) {
          const v = spare
          spare = null
          return v
        }
        let u = 0
        while (u <= 1e-12) u = rnd()
        const r = Math.sqrt(-2 * Math.log(u))
        const th = 2 * Math.PI * rnd()
        spare = r * Math.sin(th)
        return r * Math.cos(th)
      }
      const prof = new Float64Array(NRAW)
      for (let k = 0; k < NRAW; k++) prof[k] = 10 ** (noiseDb(k * HZ_RAW) / 10)
      const out: Float32Array[] = []
      const p = new Float64Array(NRAW)
      const ur = new Float64Array(NRAW + 2)
      const ui = new Float64Array(NRAW + 2)
      for (let r = 0; r < ROWS + BURN; r++) {
        p.fill(0)
        // NOISE — the exact one-frame Hann frequency structure (X[k] = 0.5u[k] − 0.25u[k−1] −
        // 0.25u[k+1], E|X|² = 0.375), two weighted looks standing in for the 6 overlapped frames
        // `RowAverage` means. THE ONLY CONSUMER OF THE RNG.
        for (const w of [0.7, 0.3]) {
          for (let k = 0; k < NRAW + 2; k++) {
            ur[k] = gauss() * Math.SQRT1_2
            ui[k] = gauss() * Math.SQRT1_2
          }
          for (let k = 0; k < NRAW; k++) {
            const j = k + 1
            const xr = 0.5 * ur[j] - 0.25 * ur[j - 1] - 0.25 * ur[j + 1]
            const xi = 0.5 * ui[j] - 0.25 * ui[j - 1] - 0.25 * ui[j + 1]
            p[k] += (w * (xr * xr + xi * xi)) / 0.375
          }
        }
        for (let k = 0; k < NRAW; k++) p[k] *= prof[k]
        // SIGNALS — 8-FSK. The row's Hann-weighted centre is 135 ms into its 270 ms support, so
        // it straddles two 160 ms symbols; 0.80/0.20 is the measured split (range 0.54–0.96).
        const sym = Math.floor((r * 120 + 135) / 160)
        for (const t of [...probes, ...stations]) {
          if (drop.includes(t.id)) continue
          // SNR + 24.54 dB is the analytic peak-raw-bin-signal over mean-raw-bin-noise ratio,
          // = 10log10(FFT_N/7.2). The chain then subtracts the peak-hold and scalloping terms
          // by itself, landing at the measured SNR + 22.8 over the DISPLAYED median — so no
          // constant in this test states the excess; it is produced.
          const A = 10 ** ((t.snr + 24.54) / 10) * 10 ** (noiseDb(t.hz + 22) / 10)
          for (const [sy, w] of [
            [sym, 0.8],
            [sym - 1, 0.2],
          ] as [number, number][]) {
            const kf = (t.hz + 6.25 * toneOf(t.id, sy)) / HZ_RAW
            const k0 = Math.round(kf)
            for (let d = -3; d <= 3; d++) {
              const k = k0 + d
              if (k >= 0 && k < NRAW) p[k] += A * w * leak(k - kf)
            }
          }
        }
        // DISPLAY BINS — peak-hold over the raw bins each covers, then the absolute dB axis.
        // 511 of 512 display bins cover exactly 3 raw bins and adjacent ones SHARE one, which
        // is why the grain is spatially correlated (lag-1 0.61) rather than iid.
        const row = new Float32Array(BINS)
        for (let i = 0; i < BINS; i++) {
          const klo = Math.min(NRAW - 1, Math.max(1, Math.floor(((4000 * i) / BINS) / HZ_RAW)))
          const khi = Math.min(NRAW - 1, Math.max(klo, Math.ceil(((4000 * (i + 1)) / BINS) / HZ_RAW)))
          let m = 0
          for (let k = klo; k <= khi; k++) if (p[k] > m) m = p[k]
          row[i] = Math.min(1, Math.max(0, (10 * Math.log10(Math.max(m, 1e-300)) + WF_DB_SPAN) / WF_DB_SPAN))
        }
        out.push(row)
      }
      return out
    }

    // Background = visible passband bins clear of every emitter's 50 Hz tone group.
    const bgBins: number[] = []
    for (let i = 0; i < BINS; i++) {
      const hz = HZ(i)
      if (hz < 350 || hz > 3250) continue
      if ([...probes, ...stations].some((t) => hz > t.hz - 30 && hz < t.hz + 6.25 * 7 + 30)) continue
      bgBins.push(i)
    }
    const groupBins = (hz0: number) => {
      const g: number[] = []
      for (let i = 0; i < BINS; i++) if (HZ(i) >= hz0 - 8 && HZ(i) <= hz0 + 6.25 * 7 + 8) g.push(i)
      return g
    }
    const pctl = (a: number[], q: number) => {
      const b = [...a].sort((x, y) => x - y)
      return b[Math.min(b.length - 1, Math.round(q * (b.length - 1)))]
    }
    const lutOf = (v: number, floor: number, ceil: number) => {
      const t = normalize(v, floor, ceil)
      return t >= 1 ? 255 : Math.round(t * 255)
    }
    const vLo = Math.floor(200 / (4000 / BINS))
    const vHi = Math.ceil(3000 / (4000 / BINS))

    /** Run the shipping display chain (Waterfall.tsx:466-499) → per-bin LUT columns + bg stats. */
    const run = (scene: Float32Array[], parked = true) => {
      const buf = new Float32Array(BINS)
      const col = new Float64Array(BINS) // per-bin time mean of the LUT index
      const bg: number[] = []
      let af = 0
      let ac = 1
      let init = false
      for (let r = 0; r < scene.length; r++) {
        flattenRow(scene[r], buf)
        const { floor, ceil } = parked
          ? agcRange(buf.slice(vLo, vHi), WF_FLOOR_PCT)
          : agcRange(buf)
        if (!init) {
          af = floor
          ac = ceil
          init = true
        } else {
          af += (floor - af) * 0.1
          ac += (ceil - ac) * 0.1
        }
        const w = parked ? parkFloor(af, ac) : { floor: af, ceil: ac }
        if (r < BURN) continue
        for (let i = 0; i < BINS; i++) col[i] += lutOf(buf[i], w.floor, w.ceil) / ROWS
        for (const i of bgBins) bg.push(lutOf(buf[i], w.floor, w.ceil))
      }
      return {
        col,
        med: pctl(bg, 0.5),
        p95: pctl(bg, 0.95),
        blackPct: (100 * bg.filter((v) => v === 0).length) / bg.length,
      }
    }

    /** Paired column separation: the bin is chosen by the SIGNAL side's argmax and read at that
     *  SAME index on the null side. Choosing per-side (or by the difference) would re-introduce
     *  the selection bias this replaces. */
    const sep = (sig: { col: Float64Array }, nul: { col: Float64Array }, hz0: number) => {
      let best = -Infinity
      let atNull = 0
      for (const i of groupBins(hz0)) {
        if (sig.col[i] > best) {
          best = sig.col[i]
          atNull = nul.col[i]
        }
      }
      return best - atNull
    }

    const scene = buildRows()
    const after = run(scene)
    // BEFORE — the shipping chain until 2026-08-05: whole-row AGC at the 5th percentile, no park.
    const before = run(scene, false)

    // (a) THE BACKGROUND. Before: the noise sat in the bright half of the palette and NOTHING was
    //     black — the operator's "over noisy". After: the noise median IS the palette floor.
    expect(before.med).toBeGreaterThan(140)
    expect(before.blackPct).toBeLessThan(1)
    expect(after.med).toBe(0)
    // Measured over 8 base seeds (mean ± sd): median LUT 0 ± 0, p95 13.6 ± 0.47 (13–14),
    // %black 86.7 ± 0.27 (86.4–87.2). Bars sit ≥6 outside the observed range at every seed.
    // p95 is the gated darkness statistic rather than %black because it is what sets perceived
    // boil: turbo's bottom is deliberately long and dark (`colormaps.ts` — L*≥1 only at LUT 5,
    // L*≥5 at 19), so the difference between a field at LUT 0 and one at LUT 3 is ΔL* 0.69,
    // well under one JND, while p95 is the grain the eye actually reads as noise.
    expect(after.blackPct).toBeGreaterThan(80)
    expect(after.p95).toBeLessThan(20)

    // (b) THE SIGNALS, against the NULL CONTROL — the same scene with that station omitted, the
    //     same noise draws (bit-identical, see buildRows), the same chain.
    const nulls = probes.map((p) => run(buildRows([p.id])))
    const seps = probes.map((p, k) => sep(after, nulls[k], p.hz))

    // ⚠️ THE HONEST DECODE-FLOOR NUMBER IS 3.2 LUT, NOT THE 9 THIS FILE USED TO CLAIM, and the
    // difference is entirely (1)+(2)+(3) above — the chain is unchanged. It is recorded rather
    // than tuned away: a −21 dB SNR FT8 station is genuinely ~1 dB over the noise in a 7.8 Hz
    // bin, and once its power is spread across the tones it visits, its column-integrated
    // brightness is a few palette indices. There is no display map that turns that into 8 LUT
    // (see WF_PARK_DB — a soft knee below the park was built and measured for exactly this and
    // came out DOMINATED by simply moving the park). What makes it visible is the operator's own
    // temporal integration over the ~105 rows an over lasts, which is what this metric models.
    // Measured over 8 base seeds: 3.15 ± 0.39 (2.67–3.89).
    expect(
      seps[0],
      'the -21 dB station must lead its own absence — the null control is the identical scene ' +
        'with it removed and IDENTICAL noise, so anything above 0 here is the station itself',
    ).toBeGreaterThan(2)
    // Strength ordering survives: louder is brighter, at every step. Measured (8 seeds):
    // -18 → 13.0 ± 0.75, -11 → 49.6 ± 0.52, -1 → 137.3 ± 0.72.
    expect(seps[1]).toBeGreaterThan(seps[0])
    expect(seps[2]).toBeGreaterThan(seps[1])
    expect(seps[3]).toBeGreaterThan(seps[2])
  })

  it('NULL CONTROL: with the station deleted the weak-signal statistic is EXACTLY zero', () => {
    // The anti-vacuity proof, as a TEST rather than a comment — a comment rots and cannot fail.
    // This runs the identical measurement over the identical scene with the probe dropped from
    // BOTH arms. Because the scene's RNG is consumed only by the noise and tones come from a
    // pure hash, the two arms are bit-identical arrays and the statistic is exactly 0.0 — not
    // small, ZERO, at every seed. So the guard above cannot be satisfied by grain AT ALL.
    //
    // This is the property the previous null control was written to have and did not: it skipped
    // an RNG draw when it dropped an emitter, so its two arms carried different noise (row 1
    // differed in 505 of 512 bins) and its 9-vs-8 margin was one draw wide — re-running the old
    // statistic over 8 base seeds gave 9.0 8.6 5.3 9.2 10.8 5.3 10.2 10.5, i.e. it FAILED ITS
    // OWN BAR on 2 of 8 seeds.
    //
    // VERIFIED RED by deleting what it measures: with the probe restored to one arm only this
    // reads 4.85 at seed 1 instead of 0 and `toBe(0)` fails. The guard above was proved red the
    // same way — dropping the -21 dB station from the SIGNAL arm too makes its separation
    // EXACTLY 0 against a bar of 2 (`expected 0 to be greater than 2`), which is the strongest
    // form of the property: the statistic cannot be satisfied by grain at any seed, because with
    // no station the two arrays are identical.
    const BINS = 512
    const NRAW = 684
    const HZ_RAW = 12000 / 2048
    const ROWS = 105
    const BURN = 40
    const D = (x: number) =>
      Math.abs(x) < 1e-9 ? 1 : Math.sin(Math.PI * x) / (2048 * Math.sin((Math.PI * x) / 2048))
    const leak = (d: number) => ((0.5 * D(d) + 0.25 * D(d - 1) + 0.25 * D(d + 1)) / 0.5) ** 2
    const noiseDb = (hz: number) => {
      const u = Math.min(1, Math.max(0, (hz - 300) / 3000))
      const roll =
        40 *
        Math.max(
          Math.min(1, Math.max(0, (300 - hz) / 300)),
          Math.min(1, Math.max(0, (hz - 3300) / 300)),
        )
      return -76.3 + 5 * (0.5 - u) + Math.cos(2 * Math.PI * u) - roll
    }
    const toneOf = (id: number, sym: number) => {
      let h = Math.imul(id + 1, 0x9e3779b1) ^ Math.imul(sym + 1, 0x85ebca6b)
      h = Math.imul(h ^ (h >>> 13), 0xc2b2ae35)
      return ((h ^ (h >>> 16)) >>> 0) % 8
    }
    const emitters = [{ id: 0, hz: 700, snr: -21 }]
    const build = (drop: number[], seed: number) => {
      let s = seed >>> 0
      const rnd = () => {
        s = (s + 0x6d2b79f5) >>> 0
        let t = s
        t = Math.imul(t ^ (t >>> 15), t | 1)
        t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
        return ((t ^ (t >>> 14)) >>> 0) / 4294967296
      }
      let spare: number | null = null
      const gauss = () => {
        if (spare !== null) {
          const v = spare
          spare = null
          return v
        }
        let u = 0
        while (u <= 1e-12) u = rnd()
        const r = Math.sqrt(-2 * Math.log(u))
        const th = 2 * Math.PI * rnd()
        spare = r * Math.sin(th)
        return r * Math.cos(th)
      }
      const prof = new Float64Array(NRAW)
      for (let k = 0; k < NRAW; k++) prof[k] = 10 ** (noiseDb(k * HZ_RAW) / 10)
      const out: Float32Array[] = []
      const p = new Float64Array(NRAW)
      const ur = new Float64Array(NRAW + 2)
      const ui = new Float64Array(NRAW + 2)
      for (let r = 0; r < ROWS + BURN; r++) {
        p.fill(0)
        for (const w of [0.7, 0.3]) {
          for (let k = 0; k < NRAW + 2; k++) {
            ur[k] = gauss() * Math.SQRT1_2
            ui[k] = gauss() * Math.SQRT1_2
          }
          for (let k = 0; k < NRAW; k++) {
            const j = k + 1
            const xr = 0.5 * ur[j] - 0.25 * ur[j - 1] - 0.25 * ur[j + 1]
            const xi = 0.5 * ui[j] - 0.25 * ui[j - 1] - 0.25 * ui[j + 1]
            p[k] += (w * (xr * xr + xi * xi)) / 0.375
          }
        }
        for (let k = 0; k < NRAW; k++) p[k] *= prof[k]
        const sym = Math.floor((r * 120 + 135) / 160)
        for (const t of emitters) {
          if (drop.includes(t.id)) continue
          const A = 10 ** ((t.snr + 24.54) / 10) * 10 ** (noiseDb(t.hz + 22) / 10)
          for (const [sy, w] of [
            [sym, 0.8],
            [sym - 1, 0.2],
          ] as [number, number][]) {
            const kf = (t.hz + 6.25 * toneOf(t.id, sy)) / HZ_RAW
            const k0 = Math.round(kf)
            for (let d = -3; d <= 3; d++) {
              const k = k0 + d
              if (k >= 0 && k < NRAW) p[k] += A * w * leak(k - kf)
            }
          }
        }
        const row = new Float32Array(BINS)
        for (let i = 0; i < BINS; i++) {
          const klo = Math.min(NRAW - 1, Math.max(1, Math.floor(((4000 * i) / BINS) / HZ_RAW)))
          const khi = Math.min(NRAW - 1, Math.max(klo, Math.ceil(((4000 * (i + 1)) / BINS) / HZ_RAW)))
          let m = 0
          for (let k = klo; k <= khi; k++) if (p[k] > m) m = p[k]
          row[i] = Math.min(1, Math.max(0, (10 * Math.log10(Math.max(m, 1e-300)) + WF_DB_SPAN) / WF_DB_SPAN))
        }
        out.push(row)
      }
      return out
    }
    const vLo = Math.floor(200 / (4000 / BINS))
    const vHi = Math.ceil(3000 / (4000 / BINS))
    const col = (scene: Float32Array[]) => {
      const buf = new Float32Array(BINS)
      const c = new Float64Array(BINS)
      let af = 0
      let ac = 1
      let init = false
      for (let r = 0; r < scene.length; r++) {
        flattenRow(scene[r], buf)
        const { floor, ceil } = agcRange(buf.slice(vLo, vHi), WF_FLOOR_PCT)
        if (!init) {
          af = floor
          ac = ceil
          init = true
        } else {
          af += (floor - af) * 0.1
          ac += (ceil - ac) * 0.1
        }
        const w = parkFloor(af, ac)
        if (r < BURN) continue
        for (let i = 0; i < BINS; i++) {
          const t = normalize(buf[i], w.floor, w.ceil)
          c[i] += (t >= 1 ? 255 : Math.round(t * 255)) / ROWS
        }
      }
      return c
    }
    const gb: number[] = []
    for (let i = 0; i < BINS; i++) {
      const hz = ((i + 0.5) * 4000) / BINS
      if (hz >= 700 - 8 && hz <= 700 + 6.25 * 7 + 8) gb.push(i)
    }
    for (const seed of [1, 2, 3, 4, 5, 6, 7, 8]) {
      const a = col(build([0], seed))
      const b = col(build([0], seed))
      let best = -Infinity
      let atNull = 0
      for (const i of gb)
        if (a[i] > best) {
          best = a[i]
          atNull = b[i]
        }
      expect(
        best - atNull,
        `seed ${seed}: with the station deleted from BOTH arms the statistic must be exactly 0 — ` +
          'anything else means the null control is not paired and the guard above can be ' +
          'satisfied by grain',
      ).toBe(0)
    }
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

describe('trace peak-hold (rig scope)', () => {
  const DIT_25WPM_MS = 1200 / 25 // 48 ms — PARIS timing
  const ROW_MS = 50 // PhoneScope's poll cadence

  it('lets 25 WPM keying modulate the trace at the CW default', () => {
    // The complaint this fixes: on the CW scope, keying renders as a static bar. A dit gap has
    // to give back a real fraction of the trace height. At the old single 400 ms constant it
    // gave back 11%, which is why nothing appeared to move.
    expect(traceHoldDecay(DIT_25WPM_MS, TRACE_HOLD_MS.fast)).toBeLessThan(0.75)
  })

  it('does not strobe: no preset collapses within one row period', () => {
    // The OTHER end, and the reason the hold exists at all — the "flashing vertical line"
    // report. A preset that decayed to near zero between polls would flicker at the row rate.
    // Both directions are pinned here on purpose: a guard that can only fail one way is half
    // a guard, and "make CW faster" has an obvious wrong answer (tau -> 0) that this rejects.
    for (const [name, tau] of Object.entries(TRACE_HOLD_MS)) {
      expect(traceHoldDecay(ROW_MS, tau), `${name} strobes between rows`).toBeGreaterThan(0.5)
    }
  })

  it('orders the presets, and slow is still what shipped before', () => {
    expect(TRACE_HOLD_MS.fast).toBeLessThan(TRACE_HOLD_MS.normal)
    expect(TRACE_HOLD_MS.normal).toBeLessThan(TRACE_HOLD_MS.slow)
    expect(TRACE_HOLD_MS.slow, 'slow must stay the pre-2026-08 value').toBe(400)
  })
})

describe('rig scope vertical scale (a loud signal must draw TALL)', () => {
  const d = (dbfs: number) => (dbfs + WF_DB_SPAN) / WF_DB_SPAN
  /** A realistic audio row: SSB passband 300-2700 Hz, a 40 dB stopband outside it. */
  function audioRow(noiseDbfs: number, snrDb: number, voiceBins: number, n = 512) {
    const r = new Float32Array(n)
    for (let i = 0; i < n; i++) {
      const hz = (i / (n - 1)) * 4000
      const inBand = hz > 300 && hz < 2700
      r[i] = d(inBand ? noiseDbfs + (i % 7) - 3 : noiseDbfs - 40 + (i % 5) - 2)
    }
    const start = Math.floor(n * 0.3)
    for (let i = start; i < start + voiceBins; i++) r[i] = d(noiseDbfs + snrDb)
    return r
  }
  /** Exactly what PhoneScope draws: median floor, FIXED window above it. */
  function scope(r: Float32Array) {
    const floor = agcRange(r, WF_FLOOR_PCT).floor
    const ceil = floor + SCOPE_WINDOW_DB / WF_DB_SPAN
    return {
      peak: normalize(Math.max(...r), floor, ceil),
      // A passband bin with no signal on it (the voice block starts at 0.3n).
      noise: normalize(r[Math.floor(r.length * 0.12)], floor, ceil),
    }
  }

  it('draws a +40 dB signal far taller than a +12 dB one', () => {
    // THE REPORT, as an assertion (operator 2026-08-15, FTDX10): "I see big vertical spikes
    // where the voice is; on Nexus it seems like it's all smoothed out without the aggressive
    // peaks." Under the old fitted window BOTH of these normalised to exactly 1.000, because
    // the ceiling WAS the peak — the control below proves that rather than asserting it.
    const weak = scope(audioRow(-95, 12, 6)).peak
    const loud = scope(audioRow(-95, 40, 6)).peak
    expect(weak).toBeCloseTo(0.28, 1)
    expect(loud).toBeCloseTo(0.84, 1)
    expect(loud - weak).toBeGreaterThan(0.4)

    // CONTROL: the OLD window really did flatten both to the top, so the assertions above are
    // measuring the change and not a property the scope always had.
    for (const snr of [12, 40]) {
      const r = audioRow(-95, snr, 6)
      const { floor, ceil } = agcRange(r) // the 5% default + 99.5% ceiling, as it shipped
      expect(normalize(Math.max(...r), floor, ceil)).toBe(1)
    }
  })

  it('keeps the noise floor DOWN, not at 96% of the panel', () => {
    // The other half of the report — "the whole spectrum is up". A 5th-percentile floor landed
    // in the SSB stopband, 42 dB below the passband noise, so the noise itself rendered near
    // the top and nothing had room to rise above it.
    const r = audioRow(-95, 0, 0)
    expect(scope(r).noise).toBeLessThan(0.2)
    const old = agcRange(r) // control, again: the old statistic really does float it up
    expect(normalize(r[Math.floor(r.length * 0.12)], old.floor, old.ceil)).toBeGreaterThan(0.8)
  })

  it('the median floor is barely moved by a wide phone signal', () => {
    // The risk in choosing the median: a phone signal is WIDE, and a statistic that followed it
    // would put the floor inside the voice and flatten the picture again.
    const quiet = agcRange(audioRow(-95, 0, 0), WF_FLOOR_PCT).floor
    const busy = agcRange(audioRow(-95, 40, 200), WF_FLOOR_PCT).floor
    expect(Math.abs(busy - quiet) * WF_DB_SPAN).toBeLessThan(6)
  })

  it('is self-scaling: a noisier band lifts the floor with it', () => {
    // Why a FIXED window needs no band-conditions control.
    const quietBand = agcRange(audioRow(-95, 0, 0), WF_FLOOR_PCT).floor
    const noisyBand = agcRange(audioRow(-75, 0, 0), WF_FLOOR_PCT).floor
    expect((noisyBand - quietBand) * WF_DB_SPAN).toBeGreaterThan(15)
  })
})

// #164 (akhepcat): "the waterfall is a pannable viewport, not a bandwidth-defined window."
//
// `zoomRange` re-centres on the RX marker, and a left-click MOVES that marker — so every click
// slid the display by up to half a span, with no scrollbar and no fixed reference. The window
// chased the cursor instead of showing a slice of the passband.
//
// ⚠️ IT CANNOT SIMPLY STOP FOLLOWING. The centring is the #115 fix (same reporter): before it,
// a persisted zoom centred on whatever `rxOffsetHz` was at FIRST RENDER — 0 before the first
// snapshot — and stayed pinned there for the life of the mount, so an operator listening at
// 2500 Hz read an axis labelled 200–800. Undoing it would hand #115 straight back.
//
// Both wants are satisfiable at once, and that is what this pins: the window must CONTAIN the
// marker, and must not MOVE while the marker is already inside it. Tuning within the visible
// span changes nothing; tuning outside pages the window to bring the marker back in.
describe('zoomWindow — a slice that holds still (#164) without going stale (#115)', () => {
  it('does not move while the marker is inside it', () => {
    const first = zoomWindow(null, 1500, 1000) // 1000..2000
    expect(first).toEqual({ lo: 1000, hi: 2000 })
    // Clicks anywhere inside must leave the window exactly where it is.
    for (const hz of [1010, 1200, 1500, 1800, 1990]) {
      expect(zoomWindow(first, hz, 1000), `marker at ${hz}`).toEqual(first)
    }
  })

  it('pages when the marker leaves, so it is never stale (#115)', () => {
    const w = zoomWindow(null, 1500, 1000)
    const moved = zoomWindow(w, 2600, 1000)
    expect(moved).not.toEqual(w)
    expect(2600 >= moved.lo && 2600 <= moved.hi, 'the marker is inside again').toBe(true)
    expect(moved.hi - moved.lo, 'the span is unchanged').toBe(1000)
  })

  it('a span CHANGE always rebuilds, even with the marker inside', () => {
    const w = zoomWindow(null, 1500, 1000)
    const narrower = zoomWindow(w, 1500, 500)
    expect(narrower.hi - narrower.lo).toBe(500)
  })

  it('the fixed windows are untouched — Std and Full never follow anything', () => {
    expect(zoomWindow(null, 2500, 0)).toEqual({ lo: WF_F_MIN, hi: WF_STD_HI })
    expect(zoomWindow({ lo: 0, hi: 100 }, 2500, -1)).toEqual({ lo: WF_F_MIN, hi: WF_F_MAX })
  })

  it('still clamps inside the passband when it does page', () => {
    const w = zoomWindow(null, 1500, 1000)
    const low = zoomWindow(w, 100, 1000)
    expect(low.lo).toBe(WF_F_MIN)
    expect(low.hi - low.lo).toBe(1000)
    const high = zoomWindow(w, 3950, 1000)
    expect(high.hi).toBe(WF_F_MAX)
    expect(high.hi - high.lo).toBe(1000)
  })
})
