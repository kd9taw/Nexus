// Pure, testable waterfall render helpers — the perceptual + visual-AGC core
// extracted from Waterfall.tsx so the hot path (per-pixel color) is an integer
// LUT index, not a per-pixel sampleLut call, and so the math is unit-tested
// independently of the canvas.

import { sampleLut, type ColormapName } from './colormaps'

/** Floor below which a percentile span is widened so `normalize` never divides
 * by ~0 (magnitudes are 0..1, so this is comfortably sub-quantization). Exported
 * so the legend can treat a span this small as degenerate (a silent band reads
 * ~0 dBr rather than a fabricated full-scale range). */
export const MIN_SPAN = 1e-6

/**
 * Span of the spectrum row's intensity axis, in dB. A row value is LINEAR IN dB: `0` is
 * `-WF_DB_SPAN` dBFS and `1` is full scale.
 *
 * ⚠️ MIRRORS `tempo_core::spectrum::DB_SPAN` (crates/tempo-core/src/spectrum.rs) — the producer
 * is what puts values on this axis, and this is the only way a consumer turns one back into dB.
 * The two must move together.
 *
 * Before 2026-08-04 the axis was amplitude-linear against each row's own loudest bin, and dB
 * was recovered with `20·log10(a/b)`. Every such call site is now `spanDb`; a stray `log10` on
 * a row value is a bug that will not throw, it will just print a wrong number at the operator.
 *
 * ⚠️ And a row value is now a LEVEL, not a magnitude, so a threshold stated relative to the
 * noise floor is ADDITIVE (`floor + dbToSpan(6)`), never multiplicative. `floor * 2` meant
 * "6 dB up" on the old axis and means "twice the dB number" on this one — on a 120 dB span
 * that is a 30 dB threshold. It throws nothing and draws nothing; it just changes a decision.
 * `tuneSnap.ts::detectSignal` is where that bit, and it moves the radio.
 */
export const WF_DB_SPAN = 120

/** dB between two values on the row's intensity axis — the display-value → dB conversion.
 *  `spanDb(floor, ceil)` is the dynamic range a `{floor, ceil}` AGC window covers. */
export function spanDb(floor: number, ceil: number): number {
  return (ceil - floor) * WF_DB_SPAN
}

/** A display-value delta for `db` dB on the row's intensity axis — the inverse of `spanDb`,
 *  for clamps that are naturally stated in dB (PhoneScope's minimum visual span). */
export function dbToSpan(db: number): number {
  return db / WF_DB_SPAN
}

function clamp01(x: number): number {
  return x < 0 ? 0 : x > 1 ? 1 : x
}

/** Value at percentile `p`∈[0,1] of an ascending-sorted array (linear interp). */
function percentile(sorted: number[], p: number): number {
  const n = sorted.length
  if (n === 1) return sorted[0]
  const idx = clamp01(p) * (n - 1)
  const lo = Math.floor(idx)
  const hi = Math.ceil(idx)
  if (lo === hi) return sorted[lo]
  const frac = idx - lo
  return sorted[lo] * (1 - frac) + sorted[hi] * frac
}

/**
 * Visual-AGC: a robust floor/ceiling for one (or a window of) waterfall row(s).
 * The floor is the low percentile (the noise) and the ceiling the high
 * percentile (the strong signals) — clipping the outliers so a single hot
 * carrier doesn't black out the rest of the band. Non-finite samples are
 * dropped; empty/all-equal input returns a safe (non-degenerate) span. The
 * caller is expected to EMA-smooth `{floor, ceil}` across frames so the display
 * doesn't flicker as a signal keys up.
 */
export function agcRange(
  magnitudes: Float32Array | number[],
  loPct = 0.05,
  hiPct = 0.995,
): { floor: number; ceil: number } {
  const arr: number[] = []
  for (let i = 0; i < magnitudes.length; i++) {
    const v = magnitudes[i]
    if (Number.isFinite(v)) arr.push(v)
  }
  if (arr.length === 0) return { floor: 0, ceil: 1 }
  arr.sort((a, b) => a - b)
  const floor = percentile(arr, loPct)
  let ceil = percentile(arr, hiPct)
  if (!(ceil > floor)) ceil = floor + MIN_SPAN // all-equal / lo>=hi → safe span
  return { floor, ceil }
}

/** Map a magnitude to `t`∈[0,1] for the LUT, clamped. `ceil<=floor` → 0. */
export function normalize(mag: number, floor: number, ceil: number): number {
  if (!(ceil > floor)) return 0
  return clamp01((mag - floor) / (ceil - floor))
}

/**
 * Apply the operator's manual contrast knobs to an auto-AGC `{floor, ceil}` window
 * (WSJT-X "Gain"/"Zero" sliders). `zero`∈[-1,1] shifts the noise-floor baseline
 * (brightness); `gain`∈[-1,1] narrows (>0, more contrast) or widens (<0, flatter) the
 * dynamic-range window. Both `0` = pure auto-AGC (identity), so the sliders only ever
 * adjust the automatic display rather than replacing it.
 */
export function applyGainZero(
  floor: number,
  ceil: number,
  gain: number,
  zero: number,
): { floor: number; ceil: number } {
  const span = Math.max(ceil - floor, MIN_SPAN)
  const f = floor + zero * span * 0.5 // ±½ span floor shift
  // gain>0 → 0.4×span (punchy); gain<0 → 2×span (flat). gain=0 → unchanged.
  const widthFactor = gain >= 0 ? 1 - 0.6 * gain : 1 - gain
  let c = f + span * widthFactor
  if (!(c > f)) c = f + MIN_SPAN
  return { floor: f, ceil: c }
}

/**
 * Percentile `agcRange` takes as the row's NOISE LEVEL for the parked-floor chain below:
 * the MEDIAN, not a low tail.
 *
 * A low percentile is not a noise statistic you can park against — it is a statistic of the
 * distribution's LEFT TAIL, and three unrelated things move that tail without moving the noise:
 * the 15% of every row that sits in the rig's SSB stopband (the row spans 0-4000 Hz, every
 * filter cuts by ~3.3 kHz, and a 40 dB cliff owns the bottom 5% outright), the passband tilt,
 * and band occupancy. Measured over the modelled scenes, `loPct` 0.05 lands anywhere from
 * -10 to -44 dB relative to the passband noise median; `loPct` 0.5 lands within 0.7 dB of it on a
 * dead band, a busy band and a contest alike, within 1.1 dB with 15 dB of passband tilt, and
 * within 2.5 dB with the stopband in view (the "Full" 200-4000 Hz zoom).
 * That stability is the entire reason the offset below can be stated in absolute dB at all.
 */
export const WF_FLOOR_PCT = 0.5

/**
 * dB ABOVE the row's noise median at which `parkFloor` puts the black point — WSJT-X's
 * `plotZero` (`plotter.cpp:194`), expressed against a measured floor instead of a manual one.
 *
 * The floor was previously the 5th percentile, i.e. 95% of the noise population rendered as
 * visible palette gradient, and the dB intensity axis (2026-08-04) gave that population enough
 * tonal resolution to read as a bright dancing field: measured noise median LUT 176 of 255 on a
 * busy band, 0% of the field black. The operator's report is that picture ("the back is dark and
 * not over noisy" — 2026-08-05).
 *
 * ⚠️ 3 dB, and NOT MORE, and the reason is weak signals. A display bin is 7.81 Hz of a 2500 Hz
 * SNR reference and the peak-hold over ~3 raw FFT bins biases the noise up, so a signal's per-bin
 * excess over the noise median is `SNR + 24.6 dB`: the FT8 decode floor of -21 dB SNR is only
 * +3.6 dB/bin. Parking at +6 puts the black point ABOVE it and deletes exactly the signals FT8
 * exists to dig out — a worse product than the noisy background, and invisible to the operator
 * because he cannot see what stopped being drawn. Measured column brightness of a -21 dB SNR
 * station on a busy band: LUT 38 at +3 dB parking, LUT 8 at +6, LUT 0 at +10.
 */
export const WF_PARK_DB = 3

/**
 * Minimum dB the display window may cover. Same clamp PhoneScope (`MIN_DYN_DB`) and
 * MiniSpectrum already carry at 10 dB; the FT waterfall was the one surface without one.
 *
 * Parking the floor is what makes it load-bearing here: the window becomes
 * `[noise median + 3 dB, p99.5]`, and on a signal-free band that is only 4.9 dB wide (measured),
 * so the first station to key up would slam straight to LUT 255 with no strength discrimination
 * at all. 24 dB rather than 10 because 10 still saturates every signal above about -14 dB SNR.
 * The cost is real and is the reason this is a named constant: on a QUIET band the clamp widens
 * the window from ~18 to 24 dB, which dims a -21 dB SNR station from LUT 57 to LUT 41.
 */
export const WF_MIN_WINDOW_DB = 24

/**
 * Park an auto-AGC `{floor, ceil}` window: lift the black point `parkDb` dB above the measured
 * noise level (so the noise-only part of the band clamps to the palette floor instead of
 * rendering as gradient) and hold the window at least `minWindowDb` wide.
 *
 * Both operations are ADDITIVE on this axis (see `WF_DB_SPAN`) — a dB offset here is
 * `dbToSpan(db)`, never a multiply. Runs BEFORE `applyGainZero`, so the operator's Zero knob
 * still slides ±½ window around this default rather than replacing it.
 *
 * ⚠️ Deliberately NOT folded into `agcRange`'s defaults. `agcRange` is shared with PhoneScope
 * and MiniSpectrum, which draw a TRACE from the same values — clamping their noise to the
 * palette/plot floor would flatten the trace onto the baseline, which is the one thing those
 * two surfaces exist to show ("is my audio alive").
 */
export function parkFloor(
  floor: number,
  ceil: number,
  parkDb = WF_PARK_DB,
  minWindowDb = WF_MIN_WINDOW_DB,
): { floor: number; ceil: number } {
  const f = floor + dbToSpan(parkDb)
  return { floor: f, ceil: Math.max(ceil, f + dbToSpan(minWindowDb), f + MIN_SPAN) }
}

/**
 * Resample one spectrum row onto `out.length` output pixels spanning [`viewLoHz`,
 * `viewHiHz`] — the ONE bin→pixel mapping every waterfall surface uses, live and
 * re-rendered alike.
 *
 * A row's bin `i` covers [lo + i·w, lo + (i+1)·w) and holds the PEAK power in that
 * span (`tempo_core::spectrum::power_spectrum`), so its representative frequency is
 * the bin CENTER, lo + (i+0.5)·w. Output pixel `x` covers its own band the same way.
 * Two regimes fall out of that, and both are needed:
 *
 * - **A pixel covers more than one bin (decimating).** MAX over the bins it covers,
 *   so a single-bin carrier can never fall between pixels.
 *   ⚠️ THIS IS A DELIBERATE DIVERGENCE FROM UPSTREAM, NOT A MATCH — an earlier version
 *   of this comment claimed WSJT-X does the same and that was FALSE. `widegraph.cpp`
 *   dataSink2 computes both and ships the SUM: `m_swide[j]=nbpp*ss` (:187), with the
 *   max-hold line sitting COMMENTED OUT directly above it (`// m_swide[j]=nbpp*smax;`
 *   :186). Upstream tried max-hold and abandoned it.
 *   We diverge because our row is not theirs: ours is already a dB display value, so
 *   summing or averaging it is arithmetic on a log axis — the geometric-mean trap that
 *   `RowAverage` converts through `display_to_power` specifically to avoid. Max is the
 *   one aggregate that is meaningful on a log axis without converting.
 *   The cost is real and is the price of that: max-pooling noise biases the floor UP and
 *   ADDS variance where a mean would reduce it. Only reached when a waterfall is narrower
 *   than ~377 device px (pxHz > binHz), which is not the FT case. If this ever moves to
 *   the hot path, convert to power and mean instead.
 * - **A bin covers more than one pixel (upsampling — the normal FT case: 512 bins,
 *   ~360 in view, across 1200–1900 device px).** LINEAR INTERPOLATION between the
 *   neighbouring bin centers. Point-sampling here paints each bin as a hard 3–5 px
 *   rectangle: the operator's "looks so 8 bit" (2026-08-03). Below the first bin
 *   center / above the last the edge value is held rather than extrapolated.
 *
 * Pixels whose center falls outside [`rowLoHz`, `rowHiHz`] — and every pixel of a
 * degenerate row/view — are written **NaN**, so the caller paints the palette floor
 * instead of smearing the row's edge bin across a band that has no data.
 *
 * Writes exactly `out.length` values and allocates nothing; `out` is a caller-owned
 * scratch buffer reused across rows.
 */
export function resampleRow(
  row: ArrayLike<number>,
  rowLoHz: number,
  rowHiHz: number,
  viewLoHz: number,
  viewHiHz: number,
  out: Float32Array,
): void {
  const outW = out.length
  if (outW === 0) return
  const nBins = row.length
  const rowSpan = rowHiHz - rowLoHz
  const viewSpan = viewHiHz - viewLoHz
  if (nBins === 0 || !(rowSpan > 0) || !(viewSpan > 0)) {
    out.fill(NaN)
    return
  }
  const binHz = rowSpan / nBins
  const pxHz = viewSpan / outW
  const decimating = pxHz > binHz
  for (let x = 0; x < outW; x++) {
    const fLo = viewLoHz + x * pxHz
    const fMid = fLo + pxHz * 0.5
    if (fMid < rowLoHz || fMid > rowHiHz) {
      out[x] = NaN
      continue
    }
    if (decimating) {
      let i0 = Math.floor((fLo - rowLoHz) / binHz)
      let i1 = Math.ceil((fLo + pxHz - rowLoHz) / binHz) - 1
      if (i0 < 0) i0 = 0
      if (i1 > nBins - 1) i1 = nBins - 1
      if (i1 < i0) i1 = i0
      let m = row[i0]
      for (let i = i0 + 1; i <= i1; i++) {
        const v = row[i]
        if (v > m) m = v
      }
      out[x] = m
    } else {
      let t = (fMid - rowLoHz) / binHz - 0.5
      if (t < 0) t = 0
      else if (t > nBins - 1) t = nBins - 1
      const b0 = Math.floor(t)
      const b1 = b0 + 1 < nBins ? b0 + 1 : nBins - 1
      const frac = t - b0
      out[x] = row[b0] * (1 - frac) + row[b1] * frac
    }
  }
}

/**
 * Pre-bake a colormap to a `size`×RGBA lookup table (default 256) so the render
 * hot path is `lut[round(t*255)*4]` instead of a per-pixel linear-light
 * `sampleLut`. Alpha is fully opaque. Throws (via sampleLut) on an unknown map.
 */
export function bakeLut(name: ColormapName, size = 256): Uint8ClampedArray {
  const out = new Uint8ClampedArray(size * 4)
  const denom = size > 1 ? size - 1 : 1
  for (let i = 0; i < size; i++) {
    const [r, g, b] = sampleLut(name, i / denom)
    const o = i * 4
    out[o] = r
    out[o + 1] = g
    out[o + 2] = b
    out[o + 3] = 255
  }
  return out
}

/**
 * The colormap for a theme — v1 rides the one-color-language theme token rather
 * than an explicit picker (deferred). dark→inferno (warm perceptual),
 * light→cividis (CVD-safe, reads on a bright screen). Anything else → inferno.
 */
export function themeColormap(theme: string): ColormapName {
  switch (theme) {
    case 'light':
      return 'cividis'
    default:
      return 'inferno'
  }
}

/** Audio passband shown on the waterfall (matches the engine's 4 kHz spectrum span, so
 * stations calling above ~2.9 kHz are visible + clickable, not off the top edge). */
export const WF_F_MIN = 200
export const WF_F_MAX = 4000
/** The default resting view: the WSJT-X-familiar 0–3 kHz window. FT8/FT4 (and RTTY/SSTV)
 * activity clusters here, so the wider 200–4000 passband made the default view feel sparse;
 * the top of the band stays one click away via the "Full" option. */
export const WF_STD_HI = 3000

/** A zoom view window for the waterfall. `spanHz === 0` → the default "Std" 0–3 kHz view
 * (a FIXED window, not RX-centered); `spanHz < 0` or ≥ the full passband → "Full" 0–4 kHz;
 * any positive value → a `spanHz`-wide window centered on `centerHz`, clamped inside the
 * passband so it never runs off either edge (the displaced half is taken from the other). */
export function zoomRange(centerHz: number, spanHz: number): { lo: number; hi: number } {
  const full = WF_F_MAX - WF_F_MIN
  if (spanHz === 0) return { lo: WF_F_MIN, hi: WF_STD_HI }
  if (spanHz < 0 || spanHz >= full) return { lo: WF_F_MIN, hi: WF_F_MAX }
  let lo = centerHz - spanHz / 2
  if (lo < WF_F_MIN) lo = WF_F_MIN
  if (lo + spanHz > WF_F_MAX) lo = WF_F_MAX - spanHz
  return { lo, hi: lo + spanHz }
}

/** Scope feeds whose rows span ABSOLUTE RF Hz (a native panadapter retuned to the dial:
 * 'flex' = SmartSDR VITA, 'civ' = Icom CI-V scope). ''/'audio' = the soundcard FFT,
 * whose rows span demodulated audio-passband Hz. */
export function isRfScopeSource(source: string): boolean {
  return source === 'flex' || source === 'civ'
}

/** Why a rig-scope control pane cannot appear when no native panadapter is streaming —
 *  the operator-facing form of the line above, shown on the ⊞ Panels entry that offers
 *  that pane (Phone's Rig Scope Controls, CW's Scope Controls). Lives here so the reason
 *  and the rule it explains cannot drift apart. */
export const NO_NATIVE_SCOPE_REASON =
  'your radio is not streaming its own scope — these appear with an Icom CI-V or FlexRadio panadapter'

/** Carrier-symmetric modes (FM/AM): the signal straddles the carrier, so an RF scope
 * window should CENTER on the dial rather than hang off one side of it. */
export function isSymmetricMode(mode: string): boolean {
  const m = mode.trim().toUpperCase()
  return m === 'FM' || m === 'AM'
}

/** Sideband sign for mapping audio-offset Hz onto RF: +1 = USB-side (USB/CW-U — a higher
 * pitch sits ABOVE the carrier), -1 = LSB-side (LSB/CW-L/CW-R — below). FM/unknown → +1. */
export function sidebandSign(sideband: string): 1 | -1 {
  switch (sideband.trim().toUpperCase()) {
    case 'LSB':
    case 'CW-L':
    case 'CWL':
    case 'CW-R':
    case 'CWR':
      return -1
    default:
      return 1
  }
}

/**
 * Project the requested audio view window onto one spectrum row for the Phone/CW scope.
 *
 * Audio rows (soundcard FFT) already span passband Hz, so the view window applies
 * directly — clamped into the row, hi held ≥ lo+50 so an odd view never yields a
 * degenerate window (the pre-existing behavior, unchanged).
 *
 * Native-panadapter rows span ABSOLUTE RF Hz, but the row center only APPROXIMATES the
 * dial: the Flex pan recenters only after >500 Hz dial moves (RETUNE_EPS), and an Icom
 * FIXED/edge-mode sweep may not track the dial at all — so when the live dial is known
 * and inside the row, anchor there; otherwise fall back to the row center. Clamping the
 * audio window into such a row degenerates to a 50 Hz sliver at the pan's LOW edge
 * (~100 kHz off frequency) — instead map the audio offsets onto RF around the dial:
 * rf(f) = center + sign·(f − anchor), where anchor is the CW pitch when a marker is
 * requested (the marker then lands exactly ON the dial = zero-beat), the view midpoint
 * for carrier-symmetric modes (`symmetric` — FM/AM center on the dial), or 0 for
 * sideband Phone; then clamp to the row.
 */
export function scopeView(
  rowLoHz: number,
  rowHiHz: number,
  source: string,
  viewLoHz: number,
  viewHiHz: number,
  markerHz: number | null,
  sign: 1 | -1,
  dialHz: number | null = null,
  symmetric = false,
): { loHz: number; hiHz: number; markerAtHz: number | null } {
  if (!isRfScopeSource(source)) {
    const loHz = Math.max(rowLoHz, viewLoHz)
    const hiHz = Math.min(rowHiHz, Math.max(viewHiHz, loHz + 50))
    return { loHz, hiHz, markerAtHz: markerHz }
  }
  const center =
    dialHz != null && dialHz >= rowLoHz && dialHz <= rowHiHz ? dialHz : (rowLoHz + rowHiHz) / 2
  const anchor = markerHz ?? (symmetric ? (viewLoHz + viewHiHz) / 2 : 0)
  const rf = (f: number) => center + sign * (f - anchor)
  const a = rf(viewLoHz)
  const b = rf(viewHiHz) // LSB mirrors the window, so a/b may arrive swapped
  return {
    loHz: Math.max(rowLoHz, Math.min(a, b)),
    hiHz: Math.min(rowHiHz, Math.max(a, b)),
    markerAtHz: markerHz == null ? null : rf(markerHz),
  }
}

/** Waterfall view options for the picker. 0 = the default "Std" 0–3 kHz view (WSJT-X-like);
 * -1 = "Full" 0–4 kHz; the rest are `spanHz`-wide windows zoomed around the RX marker. */
export const WATERFALL_ZOOMS: { value: number; label: string }[] = [
  { value: 0, label: 'Std' },
  { value: -1, label: 'Full' },
  { value: 2000, label: '2 kHz' },
  { value: 1500, label: '1.5 kHz' },
  { value: 1000, label: '1 kHz' },
  { value: 600, label: '600 Hz' },
]

/** Coerce a persisted zoom span to the picker's own vocabulary. The `<select>` above is
 * the only legitimate writer, so any other finite number (stale format, foreign surface,
 * hand-edited store) falls back to Std (0) rather than rendering a span no option
 * represents (the picker shows blank and the view is irreproducible from the UI). */
export function coerceZoomSpan(v: number): number {
  return WATERFALL_ZOOMS.some((z) => z.value === v) ? v : 0
}

/** Pickable waterfall palettes in menu order — `'auto'` rides the theme; the rest are
 * explicit (the perceptual set + the familiar WSJT-X/fldigi looks). */
export const WATERFALL_PALETTES: { value: ColormapName | 'auto'; label: string }[] = [
  { value: 'auto', label: 'Auto (theme)' },
  { value: 'inferno', label: 'Inferno' },
  { value: 'viridis', label: 'Viridis' },
  { value: 'cividis', label: 'Cividis (CVD-safe)' },
  { value: 'turbo', label: 'Turbo' },
  { value: 'sdr-green', label: 'SDR Green' },
  { value: 'amber-crt', label: 'Amber CRT' },
  { value: 'blue', label: 'Blue' },
  { value: 'cyan', label: 'Cyan' },
  { value: 'brown', label: 'Brown' },
  { value: 'grayscale', label: 'Grayscale' },
  { value: 'digipan', label: 'Digipan' },
  { value: 'linrad', label: 'Linrad' },
  { value: 'negative', label: 'Negative' },
]

/** The curated MASTER palette set shown in the per-mode pickers — one clean choice of ~8
 * that rides across every scope (FT8, CW, Phone). A perceptual default set plus the most
 * familiar SDR/retro looks; `resolveColormap` still accepts any value in `WATERFALL_PALETTES`
 * so a legacy stored palette keeps working even if it's not offered here. */
export const MASTER_PALETTES: { value: ColormapName | 'auto'; label: string }[] = [
  { value: 'auto', label: 'Auto (theme)' },
  { value: 'inferno', label: 'Inferno' },
  { value: 'viridis', label: 'Viridis' },
  { value: 'turbo', label: 'Turbo' },
  { value: 'sdr-green', label: 'SDR Green' },
  { value: 'amber-crt', label: 'Amber CRT' },
  { value: 'blue', label: 'Blue' },
  { value: 'grayscale', label: 'Grayscale' },
]

/** Resolve the waterfall colormap: an explicit palette choice wins; `'auto'` (or an
 * unknown/stale value) falls back to the theme's default map. */
export function resolveColormap(palette: string, theme: string): ColormapName {
  const explicit = WATERFALL_PALETTES.some((p) => p.value === palette && p.value !== 'auto')
  return explicit ? (palette as ColormapName) : themeColormap(theme)
}

/**
 * Which marker a waterfall click moves — the wide-graph gesture map.
 *
 * LEFT = RX is the one gesture that must never move: it is identical in stock WSJT-X and in
 * JTDX, so it is universal muscle memory. Everything else layers on top:
 *
 * | gesture      | target | origin                          |
 * |--------------|--------|---------------------------------|
 * | left         | rx     | WSJT-X + JTDX                   |
 * | Shift + left | tx     | WSJT-X                          |
 * | right        | tx     | JTDX (operator ask 2026-07-26)  |
 * | Ctrl + left  | both   | WSJT-X                          |
 *
 * Right-click is ADDITIVE: stock WSJT-X binds no right-button action, so supporting JTDX's
 * here costs a WSJT-X operator nothing and both conventions work at once.
 *
 * ⚠️ Nexus once shipped left=TX / right=RX — its own invention, which moved the WRONG marker
 * for anyone arriving from either mainstream client. Do not "restore" it.
 *
 * Buttons other than left(0) and right(2) return null so middle-click and the mouse's
 * back/forward buttons cannot retune the radio by accident.
 */
export function tuneTarget(
  button: number,
  ctrlKey: boolean,
  shiftKey: boolean,
): 'tx' | 'rx' | 'both' | null {
  if (button === 2) return 'tx' // JTDX right-click, before any modifier
  if (button !== 0) return null
  if (ctrlKey) return 'both'
  if (shiftKey) return 'tx'
  return 'rx'
}
