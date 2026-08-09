// Amateur band-edge helpers.
//
// Maps a dial frequency in MHz to a conventional band label (e.g. 14.090 ->
// "20m"). Ranges are the standard ham allocations; anything outside them
// returns "" so callers can decide how to handle an unknown frequency.

interface BandRange {
  lo: number
  hi: number
  label: string
}

// Ordered low -> high. Ranges are generous within each amateur allocation.
const BAND_RANGES: BandRange[] = [
  { lo: 1.8, hi: 2.0, label: '160m' },
  { lo: 3.5, hi: 4.0, label: '80m' },
  { lo: 5.3, hi: 5.41, label: '60m' },
  { lo: 7.0, hi: 7.3, label: '40m' },
  { lo: 10.1, hi: 10.15, label: '30m' },
  { lo: 14.0, hi: 14.35, label: '20m' },
  { lo: 18.068, hi: 18.168, label: '17m' },
  { lo: 21.0, hi: 21.45, label: '15m' },
  { lo: 24.89, hi: 24.99, label: '12m' },
  { lo: 28.0, hi: 29.7, label: '10m' },
  { lo: 50.0, hi: 54.0, label: '6m' },
  // IARU Region 1 only (CEPT secondary, footnote ECA9) — the backend band plan has it.
  // 70.0–71.0 is the ADIF Band Enumeration, WSJT-X's models/Bands.cpp and `band_for_dial`'s
  // own arm; this used to stop at 70.5, so a UK NoV holder above it read as no band at all
  // while the backend still called it 4 m. National edges vary widely INSIDE this range —
  // the range is the ADIF identity, never a licence claim.
  { lo: 70.0, hi: 71.0, label: '4m' },
  { lo: 144.0, hi: 148.0, label: '2m' },
  { lo: 222.0, hi: 225.0, label: '1.25m' },
  { lo: 420.0, hi: 450.0, label: '70cm' },
  { lo: 1240.0, hi: 1300.0, label: '23cm' },
]

/**
 * Conventional band label for a dial frequency in MHz, or "" if it falls
 * outside the known amateur bands.
 */
export function bandLabelForMhz(mhz: number): string {
  if (!Number.isFinite(mhz)) return ''
  for (const r of BAND_RANGES) {
    if (mhz >= r.lo && mhz <= r.hi) return r.label
  }
  return ''
}

/** The [lo, hi] MHz edges of a band label (e.g. "20m" → {lo: 14.0, hi: 14.35}), or null for an
 * unknown label. Used by the band-strip to lay spots + the dial marker on a proportional scale. */
export function bandRangeForLabel(label: string): { lo: number; hi: number } | null {
  const r = BAND_RANGES.find((b) => b.label === label)
  return r ? { lo: r.lo, hi: r.hi } : null
}

// The top of each band's CW sub-band (MHz) — the "CW portion" boundary, kept in
// sync with the backend band plan (`model.rs` `hf_segment` cw_top; VHF weak-signal
// CW windows 50.0–50.1 / 144.0–144.1). The CW sub-band is [band bottom, CW top).
const CW_TOP: Record<string, number> = {
  '160m': 1.81,
  '80m': 3.57,
  '40m': 7.04,
  '30m': 10.13,
  '20m': 14.07,
  '17m': 18.095,
  '15m': 21.07,
  '12m': 24.915,
  '10m': 28.07,
  '6m': 50.1,
  '2m': 144.1,
}

/** The [lo, hi] MHz edges of a band's CW sub-band (band bottom → CW top), or null if the band
 * has no distinct CW segment. Lets the CW cockpit's band strip show ONLY the CW portion of the
 * band instead of spanning the whole allocation. */
export function cwRangeForLabel(label: string): { lo: number; hi: number } | null {
  const top = CW_TOP[label]
  const r = BAND_RANGES.find((b) => b.label === label)
  return top !== undefined && r ? { lo: r.lo, hi: top } : null
}

/** The sideband convention for a dial: LSB below 10 MHz, USB at or above. */
export function sidebandForMhz(mhz: number): 'LSB' | 'USB' {
  return mhz < 10 ? 'LSB' : 'USB'
}

/**
 * Which sideband to command when moving the dial from `fromMhz` to `toMhz` (#45).
 *
 * Every QSY call site used to pass `snap.radio.sideband` — whatever the rig is on RIGHT NOW —
 * which is correct for tuning inside a band and wrong the moment the band changes. Picking 20 m
 * from 40 m carried LSB across, so the menu offered "14.230 USB" and the rig was commanded to
 * 14.230 LSB. Reported by patpell on an FT-710, and he noted it happens in more places than
 * SSTV, which fits: four call sites shared the same expression.
 *
 * ⚠️ It corrects only when the move CROSSES the 10 MHz convention boundary. Inside one side the
 * current sideband is kept, because an operator who has deliberately put the rig on USB down on
 * 40 m — legitimate for some digital work — must not have it reverted by an ordinary retune. The
 * bug was carrying a sideband ACROSS the boundary, not keeping one within it.
 *
 * Anything that is not a plain sideband (FM, AM, a DATA submode) passes straight through: those
 * are a mode class rather than a side, and this convention has nothing to say about them.
 */
export function sidebandForQsy(toMhz: number, fromMhz: number, current?: string | null): string {
  const cur = (current ?? '').trim().toUpperCase()
  if (cur !== 'LSB' && cur !== 'USB') return cur || sidebandForMhz(toMhz)
  return sidebandForMhz(toMhz) === sidebandForMhz(fromMhz) ? cur : sidebandForMhz(toMhz)
}
