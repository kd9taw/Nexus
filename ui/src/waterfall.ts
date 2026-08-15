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
function percentile(sorted: ArrayLike<number>, p: number): number {
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
 * Sort scratch for `agcRange`, reused across calls.
 *
 * Four canvases call `agcRange` up to 20x/second each, and it used to build a fresh `number[]`
 * and sort it every time — ~500 numbers of garbage per row per scope, for a result that is two
 * floats. `Float64Array`, deliberately NOT `Float32Array`: the rows arrive from JSON as doubles,
 * and narrowing them here would move the AGC floor in the last few bits — a real (if tiny)
 * behaviour change to buy nothing, since the win is the allocation, not the width.
 *
 * Safe to share: `agcRange` is synchronous and calls nothing that could re-enter it, so no two
 * users of the scratch are ever live at once.
 */
let agcScratch = new Float64Array(0)

/**
 * Trace peak-hold decay constants for the rig scope (`PhoneScope`), in ms.
 *
 * The panadapter trace holds each column's peak and decays it, so a bursty signal does not
 * strobe at frame rate with every gap in speech or keying (the "flashing vertical line"
 * report). The hold is REQUIRED — what was wrong was having ONE time constant for two very
 * different signals:
 *
 *   - CW at 25 WPM keys a 48 ms dit. A 400 ms hold gives back 11% of the trace height between
 *     elements, so keying renders as a static bar and the operator sees no rhythm at all.
 *   - Voice syllables run ~150-300 ms, and a hold that short would flicker on every one.
 *
 * So CW gets `fast` and phone gets `normal`. `slow` is the pre-2026-08 value, kept because it
 * is the right answer for a very slow or very weak signal and costs one line to keep.
 */
export const TRACE_HOLD_MS = { fast: 120, normal: 250, slow: 400 } as const

/**
 * Fraction of a held trace peak still standing `ms` after the signal stops — `exp(-ms/tau)`.
 *
 * Time-based rather than per-frame so the fade runs at the same speed under reduced-motion's
 * slower row cadence. Exported for the guard: the two ends of this decision (does keying show?
 * does it strobe?) are arithmetic, and arithmetic can be pinned without a browser.
 */
export function traceHoldDecay(ms: number, tauMs: number): number {
  if (!(tauMs > 0)) return 0
  return Math.exp(-Math.max(0, ms) / tauMs)
}

/**
 * The RIG SCOPE's display window, in dB above the noise floor.
 *
 * ⚠️ THE WHOLE POINT IS THAT IT IS FIXED. `agcRange`'s ceiling is the 99.5th percentile — the
 * signal itself — so the scope re-fitted its own peak to full scale on EVERY row, and a signal
 * could not get taller because it was already at the top. Measured on a modelled row, the peak
 * normalises to exactly 1.000 at +12 dB SNR and exactly 1.000 at +40 dB:
 *
 *     CW quiet tone  +12dB   ceil=-83.0dBFS   PEAK->1.000
 *     CW loud tone   +40dB   ceil=-55.0dBFS   PEAK->1.000
 *
 * That is the operator's report (2026-08-15): "on my FTDX10 I see big vertical spikes where the
 * voice is; on Nexus it seems like it's all smoothed out without the aggressive peaks." A
 * hardware scope has a FIXED vertical scale, so loud draws tall. This restores that.
 *
 * 50 dB is chosen against real signal levels rather than taste: a +40 dB voice peak draws at
 * 80% height, a +12 dB weak one at 24%, and both are unmistakably different — which is the
 * property that was missing. A signal more than 50 dB over the noise clips at the top, exactly
 * as it does on a rig, and the Gain slider already widens the window (`applyGainZero` takes it
 * to 2x at G-1) for anyone who wants the headroom back.
 *
 * ANCHORED TO THE NOISE, so it is self-scaling: on a noisy band the floor rises and the window
 * rises with it. That is why a fixed window does not need a band-conditions control.
 *
 * ⚠️ THE FLOOR IT SITS ON MUST BE `WF_FLOOR_PCT`, NOT `agcRange`'s 5% DEFAULT — the scope passed
 * the default and that was the second half of the same report. A low percentile is a statistic
 * of the row's LEFT TAIL, which on an audio row is the rig's SSB stopband: measured 42 dB below
 * the passband noise, which put the noise floor itself at 0.956 of full panel height. The median
 * lands on the passband noise (see WF_FLOOR_PCT's own note) and is barely moved by a wide phone
 * signal — 2 dB across a 200-bin voice on the modelled row.
 *
 * Measured end to end at 50 dB, floor = median, on a modelled 0-4000 Hz row with a real filter
 * skirt — this is the whole point of the change, in one table:
 *
 *     no signal        peak 0.100   noise 0.100
 *     CW tone +12 dB   peak 0.280   noise 0.100
 *     CW tone +40 dB   peak 0.840   noise 0.100
 */
export const SCOPE_WINDOW_DB = 50

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
  // ArrayLike, so a caller can pass a reusable scratch of its own (PhoneScope's AGC window)
  // instead of slicing a fresh array per row. Read-only here.
  magnitudes: ArrayLike<number>,
  loPct = 0.05,
  hiPct = 0.995,
): { floor: number; ceil: number } {
  if (agcScratch.length < magnitudes.length) agcScratch = new Float64Array(magnitudes.length)
  let n = 0
  for (let i = 0; i < magnitudes.length; i++) {
    const v = magnitudes[i]
    if (Number.isFinite(v)) agcScratch[n++] = v
  }
  if (n === 0) return { floor: 0, ceil: 1 }
  const arr = agcScratch.subarray(0, n)
  arr.sort() // TypedArray sorts NUMERICALLY by default — the `number[]` version needed a comparator
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
 * dB the Zero slider may move the black point at either extreme — an ABSOLUTE trim, not a
 * fraction of the display window.
 *
 * This is WSJT-X's own authority for the same control, converted: `plotter.cpp:194` maps
 * `y1 = 10·gain·y + m_plotZero` into a 0-254 palette index, so at unity gain the palette
 * spends 10 indices per dB, and `widegraph.ui`'s zeroSlider runs -50..+50 — ±5 dB, and
 * WINDOW-INDEPENDENT, because it is added in index space after the scaling.
 *
 * ⚠️ THE REASON THIS IS A CONSTANT AND NOT `span * 0.5` (the 2026-08-05 fix). "±½ the display
 * window" was a defensible shift while the floor was the 5th percentile of the row. Once
 * `parkFloor` put the black point AT the noise floor by construction, the two composed and
 * nobody checked the composition: the shift is taken from the PARKED window, which is at
 * least `WF_MIN_WINDOW_DB` (24 dB) and grows with the loudest station in view, so a persisted
 * right-of-centre Zero stacked a large, band-dependent offset on top of the park.
 *
 * Measured on the modelled busy band (25 stations, `WF_PARK_DB` 2), black point above the
 * noise median and the palette index each probe reached:
 *
 *   zero        old (±½ window)                 new (±5 dB trim)
 *   +0.25       noise +3.7 dB   -21:  9  -11: 72     noise +2.0 dB   -21: 26  -11: 91
 *   +0.5        noise +6.7 dB   -21:  0  -11: 40     noise +3.2 dB   -21: 13  -11: 77
 *   +1.0        noise +12.7 dB  -21:  0  -11:  0     noise +5.7 dB   -21:  1  -11: 51
 *
 * At the right-hand stop the old composition deleted a **-11 dB SNR** station — an ordinary,
 * comfortably decodable signal — rendering it bit-identical to the background on Operate,
 * RTTY, SSTV and every popped-out waterfall at once, because `nexus.waterfall.zero` is one
 * app-wide key. The operator cannot see a station that stopped being drawn.
 *
 * And the old form was not merely large, it was DYNAMIC: the shift scaled with the parked
 * window, which the p99.5 ceiling widens when strong signals appear. With zero persisted at
 * +0.5, the black point measured noise +6.4 dB on a quiet band and noise +12.6 dB once six
 * +30 dB stations were in view — weak signals went dark exactly when the band got good, which
 * reads as propagation, not as a display setting. The trim below is flat at +2.9/+3.0 dB
 * across that whole sweep.
 *
 * Re-measured on the honest 8-FSK scene (2026-08-05) as MDS — the weakest station the waterfall
 * actually draws, which is the operator-meaningful form of the same harm. 20 stations, 5 dB tilt:
 *
 *   zero          0        +0.25      +0.5       +1.0
 *   old (±½ win)  -19.05   -15.08     -11.72     **-5.40**  dB SNR
 *   new (±5 dB)   -19.05   -17.38     -15.95     -13.09
 *
 * At the right-hand stop the old composition put the minimum displayable signal at -5.4 dB SNR:
 * the ENTIRE FT8 decode range from -21 to -6 was rendered as background, on every waterfall at
 * once. The trim costs 5.96 dB across the slider's full travel, which is the intended amount of
 * authority for a brightness control.
 */
export const WF_ZERO_TRIM_DB = 5

/**
 * Apply the operator's manual contrast knobs to an auto-AGC `{floor, ceil}` window
 * (WSJT-X "Gain"/"Zero" sliders). `zero`∈[-1,1] trims the black point up to
 * ±`WF_ZERO_TRIM_DB` (brightness); `gain`∈[-1,1] narrows (>0, more contrast) or widens (<0,
 * flatter) the dynamic-range window. Both `0` = pure auto-AGC (identity), so the sliders only
 * ever adjust the automatic display rather than replacing it.
 *
 * The window WIDTH rides with the shifted floor (`c = f + span`), so Zero slides the window
 * rather than squeezing it — unchanged, and the reason `parkFloor`'s minimum-window guarantee
 * survives the operator's knobs.
 */
export function applyGainZero(
  floor: number,
  ceil: number,
  gain: number,
  zero: number,
): { floor: number; ceil: number } {
  const span = Math.max(ceil - floor, MIN_SPAN)
  // ADDITIVE on the dB axis (see WF_DB_SPAN), and a fixed dB — never a fraction of `span`,
  // which is what let this stack on the park. See WF_ZERO_TRIM_DB.
  const f = floor + dbToSpan(zero * WF_ZERO_TRIM_DB)
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
 * ⚠️ 2 dB, and the number that sets the ceiling is the FT8 DECODE FLOOR. This was 3, justified by
 * a per-bin excess of `SNR + 24.6 dB` which made -21 dB SNR "+3.6 dB/bin", comfortably above a
 * 3 dB park. That calibration was 2.3 dB optimistic. Measured against the real producer:
 * analytic peak-raw-bin / mean-raw-bin-noise is SNR + 24.54 dB, but `power_spectrum`'s peak-hold
 * over ~3 raw bins lifts the DISPLAYED noise median 1.67 dB and Hann scalloping costs a tone
 * ~0.6 dB — net **SNR + 22.3**. So -21 dB SNR is **+1.3 dB/bin**, and a 3 dB park sits ABOVE the
 * signal it was chosen to protect. The old comment's own rule selected the wrong number.
 *
 * The trade this constant makes: the operator can SEE a background that is not dark enough and
 * ask for more, and can NEVER see a station that stopped being drawn. So when the two collide,
 * the background yields.
 *
 * ⚠️ THE "LEADS ITS OWN ABSENCE BY 9 LUT" FIGURE THAT USED TO SIT HERE WAS AN ARTEFACT AND IS
 * WITHDRAWN (2026-08-05, second pass). It came from a scene that put a station's whole power in
 * ONE BIN PER ROW and a metric that took a MAX over 8 tone bins — an extremum whose null
 * expectation grows with the bin count, measured against a null control that was not actually
 * paired. On the honest 8-FSK scene with an exactly-paired null control the same chain measures
 * **3.15 ± 0.39 LUT** (8 base seeds) for a -21 dB SNR station. The chain did not change; the
 * measurement did. See `waterfall.test.ts` for the full account and the seed sweep.
 *
 * ⚠️ HOW OPTIMISTIC 2 dB STILL IS, corrected against the honest model.
 * (1) Real FT8 is 8-FSK and the row's 270 ms support spans ~1.7 symbols, so its power divides
 *     across the bins its tones visited. The penalty depends on WHICH VIEW you measure, and an
 *     earlier "4.5-6.6 dB dimmer, real excess ≈ SNR + 16.5" conflated them: measured against a
 *     same-power constant carrier, a station is only **-1.27 dB** in the PER-ROW peak (one
 *     170 ms FFT is 1.07 symbols, so the dominant symbol keeps most of the coherent gain) but
 *     **-3.73 dB** COLUMN-INTEGRATED, because the tone hops and no single column stays lit.
 *     Against the displayed noise median a constant carrier is SNR + 22.84 dB, so real FT8 is
 *     ≈ SNR + 21.6 per row and ≈ SNR + 19.1 integrated — not SNR + 16.5.
 * (2) `WF_FLOOR_PCT`'s median is the whole visible row's, not the noise's, so BAND OCCUPANCY and
 *     PASSBAND TILT both push the EFFECTIVE park past this constant. `flattenRow` below now
 *     removes the TILT term completely — measured per-segment spread across the visible passband
 *     falls 10.61 → 0.45 dB on a 15 dB-tilt rig, 3.51 → 0.45 with 5 dB, 3.12 → 0.44 with the SSB
 *     skirt in view. THE OCCUPANCY TERM SURVIVES AND IS NOT FIXED: flattening removes shape, not
 *     population, so a heavily occupied row still drags `WF_FLOOR_PCT`'s median up. Mean
 *     effective park after flattening, against station count (and the visible-band occupancy it
 *     produces): 0 → **1.91 dB**, 10 → 2.44, 20 → 3.00, 30 → 3.59, 45 → **4.97**. Monotone, so it
 *     is the occupancy and not sampling noise — though past ~30 stations the band is 98% full and
 *     those last figures rest on 4-8 noise-only bins, where the first three rest on 339, 210 and
 *     78. On a contest band the black point therefore still sits ~3 dB above what this constant
 *     says. The honest fix is to park against the flattener's own occupancy-robust p10 anchors
 *     instead of re-measuring a median — a change to the AGC's contract rather than to a
 *     constant, and deliberately NOT made here.
 *
 * ⚠️ THE SECOND "REAL FIX" ONCE NAMED HERE — a SOFT KNEE below the park instead of the hard
 * clamp, so a sub-park column accumulates contrast over the ~105 rows an over lasts — WAS BUILT
 * AND MEASURED TWICE AND IS REFUTED (2026-08-05). It is written down at length because it is the
 * obvious idea, the operator asked for it by name, and it will be had again.
 *
 * The PREMISE IS TRUE: the clamp does destroy sub-park structure. Everything below the black
 * point renders bit-identical to the background, and an unclamped map scores 6.2 LUT of column
 * separation against the clamp's 4.3. What is false is that a knee buys that back at a price
 * worth paying — and the second measurement, on the honest 8-FSK scene with an exactly paired
 * null control (see `waterfall.test.ts`), refuted it more widely than the first.
 *
 * A knee reserving `K` palette indices below the park, decaying with `tau` dB, was swept over
 * K ∈ {4,6,8,12} × tau ∈ {2,3,4} against plain clamps at park ∈ {2.0,1.7,1.4,1.0,0.5}, on a dead
 * band, a 20-station band and a 15 dB-tilt band. EVERY KNEE IS DOMINATED BY A PLAIN CLAMP AT A
 * LOWER PARK, on BOTH background axes at once — the grain p95 the eye reads as boil, and the
 * mean L* of the field — and the gap WIDENS the more the knee spends. On the dead band, matched
 * on mean background L* (MDS = the SNR at which a station crosses 8 LUT of paired column
 * separation; more negative is better):
 *
 *   L* 1.96   clamp park 1.4  MDS -20.81   |  knee K=4 tau=3   MDS -20.43
 *   L* 2.65   clamp park 1.0  MDS -21.37   |  knee K=8 tau=3   MDS -20.68
 *   L* 3.76   clamp park 0.5  MDS -22.04   |  knee K=12 tau=4  MDS -20.78
 *
 * WHY, and this is the mechanism, not a fitted result. The marginal value of one palette index
 * of slope at level `d` relative to the parked floor — separation bought per unit of background
 * brightness paid, `[P(sig>d) - P(no-sig>d)] / P(background>d)` — rises monotonically:
 *
 *   d = -5 dB  0.035    d = -1 dB  0.132    d = +2 dB  0.885    d = +5 dB  1.602
 *   d = -2 dB  0.074    d =  0 dB  0.221    d = +4 dB  1.457    d = +6 dB  2.318
 *
 * Below the park is the WORST place on the axis to spend an index: 55% of every background pixel
 * lives below -2 dB and almost none of the signal's distinguishing mass does.
 *
 * ⚠️ AND THERE IS A SECOND, INDEPENDENT REASON, WHICH IS A CONFLICT INSIDE THIS CODEBASE. The
 * waterfall palette was DELIBERATELY given a long dark bottom (`colormaps.ts`: turbo's canonical
 * low stop replaced by black plus two dark stops `#0a0c22`/`#1d2060`) for the express purpose of
 * making the parked grain read black. Measured on that palette, L* per index averages 0.231 over
 * indices 0-6 and 0.899 over indices 20-40 — the bottom of the ramp is 3.9x flatter than the
 * body BY DESIGN. A knee writes its whole output into indices 0-6, i.e. into exactly the region
 * the palette exists to erase. THE KNEE AND THE PALETTE ARE THE SAME LEVER POINTED IN OPPOSITE
 * DIRECTIONS, and the palette wins: the entire 6-index knee spans ΔL* 1.38, about one JND for a
 * large uniform patch and less than that on a moving grainy field. The information it preserves
 * is real in the Shannon sense and below the threshold of the eye that has to read it.
 *
 * So a knee is a REPARAMETRISATION OF THE PARK, not a new degree of freedom, and a worse-valued
 * one. If weak-signal margin is wanted, MOVE THIS CONSTANT — that is the efficient lever and its
 * frontier is measured (honest scene, 20-station band, paired column separation of a -21 dB
 * station / % of the field black): park 2.0 -> 2.9 LUT / 89.6%, park 1.4 -> 4.0 / 84.4%,
 * park 1.0 -> 5.0 / 80.4%, park 0.5 -> 6.4 / 74.6%. It is a straight trade against the operator's
 * own "the back is dark and not over noisy", which is why it is left at 2 rather than chosen here.
 *
 * ⚠️ ONE EARLIER CLAIM IN THIS COMMENT WAS ALSO WRONG AND IS CORRECTED: that `WF_MIN_WINDOW_DB`
 * (palette indices per dB) is "the lever that DOES move weak-signal separation, nearly free in
 * blackness". It only moves anything ON A BAND WHERE IT BINDS, and it binds only on a quiet one.
 * Measured raw parked window: dead band 3.8 dB (clamp BINDS), 20 stations 24.6 dB, 15 dB tilt
 * 24.5 dB (does NOT bind — the p99.5 ceiling is already wider than the minimum). Sweeping it
 * 24 -> 20 -> 16 dB on the 20-station band changes the separations by nothing at all, to three
 * significant figures. The earlier number was measured on a quiet scene and generalised.
 */
export const WF_PARK_DB = 2

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
 * Segments `flattenRow` splits the row into to estimate the baseline. 16 over a 512-bin
 * 0–4000 Hz row is 32 bins / 250 Hz each: short enough to track a filter's curvature, long
 * enough that the 10th percentile of a segment is a noise statistic (WSJT-X uses 10 over its
 * ~2000-px `swide`, a comparable width).
 *
 * ⚠️ The segment must stay MUCH WIDER THAN A SIGNAL. At 250 Hz it is 5 FT8 signals wide, so a
 * station cannot pull its own segment's baseline up. Raise this (narrower segments) and the
 * baseline starts following signals and subtracting them from themselves.
 */
export const WF_FLATTEN_SEGMENTS = 16

/**
 * Percentile of each segment taken as that segment's noise level. WSJT-X's `npct`
 * (`flat4.f90:13`), and the whole reason a baseline can be fitted through a band full of
 * stations: a rank statistic this low is unmoved until a segment is >90% occupied, where a mean
 * (or a median) is dragged up by every carrier in it.
 */
export const WF_FLATTEN_PCT = 0.1

/**
 * Maximum dB the baseline may depart from its own median — the whole difference between
 * flattening a rig's passband and eating the rig's FILTER.
 *
 * A segment further BELOW the median than this is not tilt, it is a dead segment: the SSB/DATA
 * filter's ~40 dB stopband above ~3.3 kHz, which is ~17% of every 0–4000 Hz row. Such a segment
 * is HELD at its nearest live neighbour rather than anchoring the baseline, so the cliff passes
 * through the flattener intact (measured: 32.8 dB deep before, 32.7 after) instead of being
 * lifted into something that reads like live band.
 *
 * ⚠️ THIS IS WHERE A FAITHFUL `flat4` PORT FAILS ON OUR ROW, and it is measured, not assumed. A
 * single low-order polynomial fitted across the whole row cannot ignore a 40 dB cliff inside its
 * own domain: the quartic bends to chase it and the wiggle contaminates the passband. Measured
 * on the same five scenes, a faithful port leaves the effective park spread over −20…+9 dB —
 * WORSE than no flattening at all — and lifts the stopband to 13 dB deep. Locality is what fixes
 * that, so the baseline here is piecewise-linear between segment anchors rather than a global
 * fit. The `flat4` IDEA (per-segment low percentile, low-order baseline, subtract) is kept
 * whole; only the global polynomial is dropped, because our row has something in it that
 * WSJT-X's `swide` does not carry into the fit the same way.
 *
 * The cap also bounds the damage to a legitimately sloped spectrum: nothing broadband can lose
 * more than 20 dB of its own shape, whatever it is.
 */
export const WF_FLATTEN_MAX_DB = 10

/**
 * Remove the row's smooth spectral SHAPE — the rig's passband tilt and filter curvature — while
 * leaving every narrow feature, and its overall level, alone. `out` is a caller-owned scratch of
 * the row's length; nothing else is allocated per row beyond three small fixed arrays.
 *
 * WHY, and it is the headline number of the parked black point. `parkFloor` puts the black point
 * `WF_PARK_DB` above `WF_FLOOR_PCT`'s median — but that is the median of the WHOLE VISIBLE ROW,
 * not of the noise, so any tilt in the row shows up as a black point that is too high at the
 * quiet end of the band and too low at the loud end. It is the same picture from both ends of the
 * operator's sentence: the loud end keeps a bright noisy field, and at the quiet end signals stop
 * being drawn at all. Measured on the five scenes below, the EFFECTIVE park (dB from the LOCAL
 * noise median up to the black point, which is the excess a signal there needs to be drawn):
 *
 * | scene       | before (p05/mean/p95) | after            |
 * |-------------|-----------------------|------------------|
 * | dead        |  1.8 / 2.0 / 2.3      | 1.8 / 2.1 / 2.4  |
 * | 20 stations |  2.3 / 2.5 / 2.8      | 1.9 / 2.4 / 2.8  |
 * | contest     |  2.6 / 2.9 / 3.3      | 2.3 / 2.8 / 3.1  |
 * | +5 dB tilt  | −0.0 / 3.5 / 5.0      | 2.0 / 2.4 / 2.8  |
 * | +15 dB tilt | −5.0 / 5.6 / 9.9      | 1.5 / 2.4 / 3.0  |
 *
 * A 15 dB-tilt rig went from a 15 dB SPREAD in what a signal must be to survive — decided by
 * where it sits in the passband rather than by its SNR — to 1.5 dB. That ordering inversion is
 * the real bug: a +2 dB signal at the hot end outdrew a +6 dB signal mid-band.
 *
 * HOW (WSJT-X's `flat4.f90`, whose structure this keeps and whose global polynomial it does not
 * — see `WF_FLATTEN_MAX_DB`):
 *  1. split the row into `WF_FLATTEN_SEGMENTS` equal segments;
 *  2. each segment's `WF_FLATTEN_PCT` percentile is its anchor — the rank statistic is what stops
 *     stations dragging the baseline up (`flat4` then keeps every point at-or-below that value
 *     and least-squares a quartic through them; the percentile itself is the same estimator with
 *     the fit's smoothing supplied by the piecewise-linear interpolation instead);
 *  3. a segment more than `WF_FLATTEN_MAX_DB` below the anchor median is DEAD (filter stopband)
 *     and inherits its nearest live neighbour, so the cliff is preserved, not ramped across;
 *  4. the baseline is linear between segment CENTERS and flat outside the outermost two;
 *  5. subtract it, holding the anchor median fixed — so this removes SHAPE, never LEVEL, and the
 *     dB axis keeps the absolute reference `power_to_display` gave it.
 *
 * ⚠️ EACH ANCHOR IS ESTIMATED OVER TWICE ITS SEGMENT'S WIDTH — but only into LIVE neighbours —
 * and that overlap is not tidiness, it is weak-signal margin. A percentile is a noisy estimator
 * and its error is CORRELATED across the whole segment it lifts, so a segment that happened to
 * draw low gets its grain brightened as one block. Measured against a null control (the same
 * scene with the decode-floor station removed, same noise draws, 1500 rows): no flattening
 * separates the station from its own absence by 2.9 LUT on a dead band, DISJOINT segments only
 * 1.4 — the flattener was spending half the margin on its own estimator noise — and the ×2
 * window 2.3. ×3 buys nothing further (2.2).
 *
 * The live-clip is why this takes two passes. A window that simply straddles the cliff reports
 * the STOPBAND, so the segments beside it get a baseline ~10 dB too low and the band edge stops
 * being flattened: with an unclipped ×2 window the 15 dB-tilt scene's park p05 fell to −0.3 dB
 * (the bottom 300 Hz kept a bright field), against 1.7 clipped. Pass 1 is disjoint and decides
 * live/dead; pass 2 widens only where the neighbour is live.
 *
 * NO STATE, no EMA — and the EMA was tried, not assumed. Smoothing the anchors across rows
 * (α 0.25 and 0.10) leaves that separation at 1.4 and 0.6, i.e. it does not help and on a tilted
 * band it hurts: the per-row baseline error is correlated with the row's own noise draw, so the
 * AGC — which re-measures that same row's median — already cancels part of it, and an EMA breaks
 * the cancellation while keeping the variance. Widening the estimator's window attacks the
 * variance itself, which is the term that actually costs.
 *
 * ⚠️ DISPLAY PATH ONLY, and specifically only the FT waterfall's. A flattened row is no longer a
 * calibrated absolute spectrum, and two consumers need one: PhoneScope and MiniSpectrum draw a
 * TRACE whose shape IS the rig's passband ("is my audio alive"), and `tuneSnap.ts::detectSignal`
 * — which MOVES THE RADIO — thresholds a click against a percentile of the row it is handed.
 * Neither is on this path: `flattenRow` is called from `Waterfall.tsx` and nowhere else, and
 * `detectSignal` is reached only through `clickTuneTarget`, imported only by `PhoneScope.tsx`.
 * Putting this in the producer (`tempo_core::spectrum`) would have reached all of them.
 *
 * Non-finite bins are ignored when estimating and passed through unchanged; a row too short to
 * segment, or one with no live segment at all, is copied through untouched.
 */
export function flattenRow(row: ArrayLike<number>, out: Float32Array): void {
  const n = Math.min(row.length, out.length)
  const nseg = WF_FLATTEN_SEGMENTS
  const copy = () => {
    for (let i = 0; i < n; i++) out[i] = row[i]
  }
  // Too short to estimate 16 percentiles worth believing (8 samples each) — pass it through.
  if (n < nseg * 8) {
    copy()
    return
  }
  const segLen = n / nseg
  const anchors = new Float64Array(nseg)
  const centers = new Float64Array(nseg)
  const scratch = new Float64Array(Math.ceil(2 * segLen) + 2)
  const live: boolean[] = new Array(nseg).fill(false)
  const segLo = (g: number) => Math.round(g * segLen)
  const segHi = (g: number) => (g === nseg - 1 ? n : Math.round((g + 1) * segLen))
  /** The `WF_FLATTEN_PCT` percentile of the finite bins in `[ia, ib)`, or NaN if there are none. */
  const anchorOver = (ia: number, ib: number): number => {
    let m = 0
    for (let i = ia; i < ib; i++) {
      const v = row[i]
      if (Number.isFinite(v)) scratch[m++] = v
    }
    if (m === 0) return NaN
    const seg = scratch.subarray(0, m)
    seg.sort()
    return seg[Math.min(m - 1, Math.round(WF_FLATTEN_PCT * (m - 1)))]
  }
  // PASS 1 — a disjoint per-segment percentile, whose only job is to decide which segments
  // are live. It must be disjoint: a window that reaches into the stopband reports the
  // stopband, and then the segment beside the cliff is misjudged.
  for (let g = 0; g < nseg; g++) {
    centers[g] = (segLo(g) + segHi(g) - 1) / 2
    const a = anchorOver(segLo(g), segHi(g))
    if (Number.isFinite(a)) {
      anchors[g] = a
      live[g] = true
    }
  }
  // Median of the live anchors — the level the flattened row is held at.
  const finite: number[] = []
  for (let g = 0; g < nseg; g++) if (live[g]) finite.push(anchors[g])
  if (finite.length === 0) {
    copy()
    return
  }
  finite.sort((a, b) => a - b)
  const med = percentile(finite, 0.5)
  const lim = dbToSpan(WF_FLATTEN_MAX_DB)
  // A segment far BELOW the median is dead (the filter's stopband), not tilt: it must not
  // anchor the baseline, or the ramp toward it eats the cliff and the passband edge with it.
  for (let g = 0; g < nseg; g++) if (live[g] && anchors[g] < med - lim) live[g] = false
  let anyLive = false
  for (let g = 0; g < nseg; g++) anyLive = anyLive || live[g]
  if (!anyLive) {
    copy()
    return
  }
  // PASS 2 — re-estimate each live anchor over a window widened half a segment into each
  // LIVE neighbour. That halves the estimator's variance in the interior (see the overlap
  // note above) while never letting a dead segment's bins into anybody's anchor.
  for (let g = 0; g < nseg; g++) {
    if (!live[g]) continue
    const ia = g > 0 && live[g - 1] ? Math.round((g - 0.5) * segLen) : segLo(g)
    const ib = g < nseg - 1 && live[g + 1] ? Math.min(n, Math.round((g + 1.5) * segLen)) : segHi(g)
    const a = anchorOver(ia, ib)
    if (Number.isFinite(a)) anchors[g] = a
  }
  for (let g = 0; g < nseg; g++) {
    if (live[g]) continue
    for (let d = 1; d < nseg; d++) {
      if (g - d >= 0 && live[g - d]) {
        anchors[g] = anchors[g - d]
        break
      }
      if (g + d < nseg && live[g + d]) {
        anchors[g] = anchors[g + d]
        break
      }
    }
  }
  // The LEVEL the flattened row is held at is the MEAN of the anchors, not `med`.
  // `med` is the right statistic for the two THRESHOLDS above — a rank statistic is what
  // makes them robust — but it is the wrong one to subtract, because a single order
  // statistic hops between neighbouring anchors from row to row: measured, using `med` as
  // the level shifted the whole row by up to ±1.5 dB per row on a tilted band. That is a
  // level that breathes, which is exactly the failure the absolute dB axis was introduced
  // to end. Averaging 16 anchors cuts that jitter by four and costs one loop.
  let level = 0
  for (let g = 0; g < nseg; g++) {
    if (anchors[g] > med + lim) anchors[g] = med + lim
    else if (anchors[g] < med - lim) anchors[g] = med - lim
    level += anchors[g]
  }
  level /= nseg
  // Piecewise-linear between segment centers, held flat outside the outermost two.
  let g = 0
  for (let i = 0; i < n; i++) {
    let base: number
    if (i <= centers[0]) base = anchors[0]
    else if (i >= centers[nseg - 1]) base = anchors[nseg - 1]
    else {
      while (g < nseg - 2 && i > centers[g + 1]) g++
      const f = (i - centers[g]) / (centers[g + 1] - centers[g])
      base = anchors[g] * (1 - f) + anchors[g + 1] * f
    }
    const v = row[i]
    out[i] = Number.isFinite(v) ? clamp01(v - base + level) : v
  }
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
