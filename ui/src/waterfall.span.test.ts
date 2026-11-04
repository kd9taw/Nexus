// THE AUDIO VIEWS' SPAN RULE (2026-11-03): the audio row reaches 0–5000 Hz at the same 7.8 Hz bins,
// every Full view reaches it too, and the CW scope opens on Std rather than on the radio's filter.
//
// The row's own reach and resolution are pinned in Rust (tempo-audio rxdsp:
// `the_display_row_reaches_5000_hz_at_the_same_resolution`); these pin what the views do with it.
import { describe, it, expect } from 'vitest'
import {
  WF_F_MIN,
  WF_F_MAX,
  WF_STD_HI,
  WF_FLATTEN_FIT_HZ,
  zoomRange,
  zoomWindow,
  cwAudioView,
  coerceCwAudioSpan,
  RTTY_WATERFALL_SPANS,
  flattenRow,
  dbToSpan,
} from './waterfall'

/** The producer's row: 0–5000 Hz in 640 bins of 7.8125 Hz. */
const ROW_HI = 5000
const BINS = 640
const binOf = (hz: number) => Math.floor((hz / ROW_HI) * BINS)

/** A deterministic noise source (mulberry32), so a percentile here is the same on every run. */
function noise(seed: number) {
  let a = seed >>> 0
  return () => {
    a = (a + 0x6d2b79f5) >>> 0
    let t = a
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

/** A modelled receive row: band noise up to the radio's filter edge, the filter's 40 dB stopband
 *  above it, a few dB of grain on both, on the display's dB axis (1 unit = WF_DB_SPAN dB). */
function modelRow(filterHz: number, seed = 7): Float32Array {
  const r = noise(seed)
  const row = new Float32Array(BINS)
  for (let i = 0; i < BINS; i++) {
    const hz = (i + 0.5) * (ROW_HI / BINS)
    const level = hz < filterHz ? 0.5 : 0.5 - dbToSpan(40)
    row[i] = level + dbToSpan(4) * (r() - 0.5)
  }
  return row
}

const mean = (a: ArrayLike<number>, lo: number, hi: number) => {
  let s = 0
  for (let i = lo; i < hi; i++) s += a[i]
  return s / (hi - lo)
}

describe('every Full view reaches 5000 Hz', () => {
  it('the waterfalls: Full is 200–5000, Std stays 200–3000', () => {
    expect(WF_F_MAX).toBe(5000)
    expect(zoomRange(1500, -1)).toEqual({ lo: WF_F_MIN, hi: 5000 })
    expect(zoomWindow(null, 1500, -1)).toEqual({ lo: WF_F_MIN, hi: 5000 })
    expect(zoomRange(1500, 0)).toEqual({ lo: WF_F_MIN, hi: WF_STD_HI })
    // A zoom window near the top may now page up past 4000.
    expect(zoomRange(4800, 1000)).toEqual({ lo: 4000, hi: 5000 })
  })

  it('the CW scope: Full is 200–5000, Std 200–3000', () => {
    expect(cwAudioView('full', 600, 500)).toEqual({ loHz: 200, hiHz: 5000 })
    expect(cwAudioView('std', 600, 500)).toEqual({ loHz: 200, hiHz: 3000 })
  })

  it('RTTY’s widest button is the Full view, labelled by the edge it reaches', () => {
    const full = RTTY_WATERFALL_SPANS[RTTY_WATERFALL_SPANS.length - 1]
    expect(full).toEqual({ value: -1, label: '5k', widthHz: 4800 })
  })
})

describe('the CW audio scope opens on Std', () => {
  it('a window that never pressed a chip, or holds a value no chip names, opens on Std', () => {
    expect(coerceCwAudioSpan(null)).toBe('std')
    expect(coerceCwAudioSpan('')).toBe('std')
    expect(coerceCwAudioSpan('wide')).toBe('std')
  })

  it('a pick is kept — Filter included, since it is only ever written by pressing it', () => {
    expect(coerceCwAudioSpan('filter')).toBe('filter')
    expect(coerceCwAudioSpan('full')).toBe('full')
    expect(coerceCwAudioSpan('std')).toBe('std')
  })
})

describe('the flattener is fitted over 0–4000 Hz, as it was measured', () => {
  /** The fitted bins for the producer's row, the way Waterfall.tsx computes them. */
  const fit = ((WF_FLATTEN_FIT_HZ - 0) / (ROW_HI - 0)) * BINS

  it('fits exactly the bins the old 0–4000 row had', () => {
    expect(WF_FLATTEN_FIT_HZ).toBe(4000)
    expect(fit).toBe(512)
  })

  it('every bin below 4000 Hz is flattened exactly as the unwidened row was', () => {
    for (const filterHz of [2400, 2700, 3000, 3600]) {
      const row = modelRow(filterHz)
      const wide = new Float32Array(BINS)
      flattenRow(row, wide, fit)
      const old = new Float32Array(512)
      flattenRow(row.subarray(0, 512), old)
      expect(Array.from(wide.subarray(0, 512)), `${filterHz} Hz filter`).toEqual(Array.from(old))
    }
  })

  it('a 2.4 kHz filter: its stopband stays as deep as on the old row, up to 5000 Hz', () => {
    const row = modelRow(2400)
    const depth = (out: Float32Array, hi: number) =>
      (mean(out, binOf(600), binOf(2200)) - mean(out, binOf(2800), binOf(hi))) * 120
    const fitted = new Float32Array(BINS)
    flattenRow(row, fitted, fit)
    const old = new Float32Array(512)
    flattenRow(row.subarray(0, 512), old)
    const unfitted = new Float32Array(BINS)
    flattenRow(row, unfitted)
    const [dOld, dFit, dUnfit] = [depth(old, 4000), depth(fitted, 5000), depth(unfitted, 5000)]
    // The stopband is 40 dB deep in the row; the flattener holds it as it always did.
    expect(dFit).toBeCloseTo(dOld, 0)
    expect(dFit).toBeGreaterThan(35)
    // CONTROL: fitted over the whole 0–5000 row, over half of it is stopband, the median the
    // dead-segment rule measures against lands in it, and the cliff is ramped across.
    expect(dUnfit).toBeLessThan(dFit - 5)
  })
})
