import { useCallback, useEffect, useRef, useState } from 'react'
import { surfaceGet, surfaceSet, surfaceId, surfacePanel } from './features/windowScope'

/** Global UI scale (percent). Applied as the `--ui-zoom` factor on <html>; CSS
 * `.app { zoom: var(--ui-zoom) }` scales the whole interface crisply. */
export type Scale = 65 | 70 | 75 | 80 | 85 | 90 | 100 | 110 | 125 | 150 | 175
/** A fine ladder: 5% steps where shrinking matters (65–90), 10–25% up top. Fine
 * enough to feel continuous, discrete enough that the waterfall canvas doesn't
 * re-raster on every resize pixel and click-to-tune stays predictable. */
export const SCALE_STEPS: Scale[] = [65, 70, 75, 80, 85, 90, 100, 110, 125, 150, 175]

/** Scale mode: 'auto' fits the UI to the window; a number pins it. */
export type ScaleMode = 'auto' | Scale

// New keys (the old numeric `tempo-ui-scale` was auto-derived on every change, so
// reading it back would wrongly look like a deliberate pin — start clean so every
// existing user gets the new auto-fit default).
// PER-SURFACE: 'auto' vs a pinned step is a statement about THIS window's size — a pin
// that fits a 4K main window is wrong on a 1080p second monitor.
const MODE_KEY = 'nexus-ui-scale-mode'
// SHARED (stays app-global): the cap is an eyesight preference set once in Settings
// ("never shrink below this"), not a measurement of any one window.
const CAP_KEY = 'nexus-ui-scale-cap'
// SHARED: which auto-fit rule the MAIN window uses on a fractionally scaled display — see
// `scaledFitScale`. Absent = never asked (the scaled rule applies and the one-time prompt may
// show); 'classic' = the operator chose the old size; anything else = the scaled rule, asked.
const FIT_RULE_KEY = 'nexus-ui-scale-fit-rule'

/**
 * A surface's NATURAL FOOTPRINT: the smallest unzoomed CSS-px box in which that
 * surface's content is whole. It is the denominator of the fit — "how much of my
 * content does this window hold?".
 *
 * NOT the size the window opens at. `open_panel_window` (src-tauri) declares an OPENING
 * size — "give this panel plenty of room" — while the fit needs a CONTENT MINIMUM. For
 * the band map the two differ by more than 2×: it opens 780 tall, but its track is
 * `flex: 1; min-height: 240px`, so height is elastic. Conflating them is the bug this
 * type exists to prevent.
 */
export interface Natural {
  readonly w: number
  readonly h: number
}

/**
 * Nexus is data-dense: a cockpit stacks waterfall + decodes + roster + QSO + TX at
 * once. Rather than hide panes on small/short windows, we scale the whole UI DOWN so
 * everything stays visible (operator directive: "even if harder to read, scale it").
 *
 * `MAIN_NATURAL` = the densest cockpit's UNZOOMED target footprint in CSS px (the
 * comfortable size at which FT8-classic shows everything). If the densest view fits,
 * every view fits — so the MAIN window fits globally to it (stable across view switches,
 * no per-view rescale jump). Auto never upscales (see DEFAULT_CAP) so 1080p full-screen =
 * 100%; the height only governs how gently SMALLER windows taper down — roomy near 1080,
 * dropping to the 65% floor only for genuinely small windows (1200×720 default ~80%,
 * 1366×768 ~85%). A lower natural height keeps small windows roomier (more scroll);
 * higher shrinks them sooner.
 *
 * WHY THE FOOTPRINT IS PER SURFACE. Fitting a torn-off 420-px band-map strip against
 * this box asks whether the whole FT8 cockpit fits inside it: 420/1200 = 0.35, below
 * every step, so the width term wins the `min()` and the window pins to the 65% floor at
 * EVERY size it can legally have — permanent, not a resize transient. That was the "the
 * font is very, very small when opening the CW band map" field report. A pop-out is a
 * small window the app sized for ITS content, so it gets ITS own question; the main
 * window's arithmetic is untouched (the parameter defaults to this box). Do not
 * "simplify" the parameter away.
 */
export const MAIN_NATURAL: Natural = { w: 1200, h: 900 }

/**
 * An un-listed pop-out. Deliberately NOT `open_panel_window`'s generic 760×660 opening
 * size (see the `Natural` note): it is the LARGEST box the 65% floor can still show at
 * the generic 420×360 `min_inner_size` — 420/0.65 × 360/0.65 = 646×554, rounded down.
 * Largest = most conservative, because too LARGE a scale clips silently while too small
 * only reads small. `popout-natural.test.ts` machine-checks that relation for every
 * openable panel.
 */
const POPOUT_NATURAL: Natural = { w: 640, h: 520 }

/**
 * Per-panel content minimums. A `Map`, not an object literal, so `?panel=constructor`
 * cannot resolve through the prototype. Paired with `panel_default_inner` /
 * `panel_min_inner` in src-tauri/src/lib.rs — `popout-natural.test.ts` parses those and
 * enforces `natural ≤ default_inner` and `natural ≤ min_inner / 0.65`, per axis.
 */
export const PANEL_NATURAL = new Map<string, Natural>([
  // The SAME dense cockpit as the main window — 1200×900 genuinely IS its question, and
  // it is the one surface whose natural exceeds the window it opens in (1140×760 → 80%,
  // exactly as before). Keep the asymmetry; it is not an oversight.
  ['operate', MAIN_NATURAL],
  // Band map: `.bandmap` padding + `.bandstrip-head` + the SpotLegend's wrapping chip
  // rows + `.bandmap-track`'s hard `min-height: 240px` floor. Height stays ELASTIC above
  // that (the track is `flex: 1`), so this is well under the 780 the window opens at —
  // which is what lets the default open at 100% while a squashed window still tapers.
  ['bandmapPhone', { w: 380, h: 520 }],
  ['bandmapCw', { w: 380, h: 520 }],
  // Waterfall strip: the control row + a usable canvas. NOT a spectral-resolution
  // number — Waterfall.tsx sizes its backing store from `devicePixelContentBoxSize`
  // (device px, correct under any zoom × dpr), so zoom buys no resolution. The real
  // constraint is `.waterfall-wrap > .panel-header` (`flex-wrap: wrap`, and its own
  // comment says "Wide/popped-out: never wraps"): once the control cluster wraps into
  // rows it squeezes `.wf-stage` toward zero.
  ['waterfall', { w: 560, h: 200 }],
  // Field Day scoreboard: event banner + header + score tiles + bonuses + the log head.
  // `.fd-log-scroll` is the scroller below that, so height is elastic.
  ['fieldday', { w: 560, h: 540 }],
  // POTA/SOTA hunter board: the fixed chrome is the header, the hunt/activation
  // rows and the program + refresh + band/mode filter chip rows; the spot-card
  // list below scrolls, so height is elastic. 620 keeps the program tabs and the
  // refresh/timestamp cluster on one row (the chip rows wrap gracefully below
  // it) — under the generic 760×660 default inner and the 646×553 ceiling the
  // generic 420×360 min_inner allows at the 65% floor.
  ['pota', { w: 620, h: 540 }],
  // Club band board: the fixed chrome is the header (label + sync chip + host
  // line + club counters) and the six column heads; the position rows below it
  // scroll, so height is elastic. The numbers grew with the type: the torn-off
  // copy is set at the glance size (FdClubSection's `detached`), because this
  // window is watched from the operating position rather than read at the
  // keyboard, and a natural still declaring the DOCKED footprint would have had
  // auto-fit shrink the bigger type straight back out. 820 is what keeps the six
  // columns — position, band, mode, operator, QSOs, rate — readable at that size
  // without the position and operator cells collapsing; `panel_min_inner` gained
  // an `fdclub` arm (560×400) to keep it under the 65%-floor ceiling.
  ['fdclub', { w: 820, h: 420 }],
  // The contest logger: the entry (the call, the exchange boxes and Log, which wrap to a second
  // row rather than shrink), the dupe warning and the newest contact; the screen below scrolls
  // inside its own panel, so height is elastic. A box this size is what the window's 420×320
  // floor shows at the 65% zoom floor, so a window dragged that small still holds the entry whole,
  // and anything from 640×480 up lays out at 100% and reflows (`browser-probe --desktop contestlog`).
  ['contestlog', { w: 640, h: 480 }],
  // The contest screen window: the same screen without the log line over it, so the logger's width
  // for the header strip, and its height less the line: the banner, the controls and a look at the
  // board, the rest scrolling inside the screen's own panel.
  ['contestscreen', { w: 900, h: 480 }],
  // The Spots board: its nine columns (age, call, entity, state, band, frequency, mode, spotter,
  // comment) floor at a fixed width, so width is the hard axis; the list under the head and the
  // filter bar scrolls, so height is elastic past the first rows. Measured in Chrome at 100%
  // (`browser-probe --desktop spots`): the columns, the row's Spot button with them, fit from 735 px,
  // and at 740 a busy day's filter bar (81 chips, forty of them spotted-from countries, with the
  // scrollbar gutter it keeps), the head, the column headings and three rows fit from 455 px. A
  // filter bar taller than that scrolls inside itself rather than squeezing the list.
  // `panel_min_inner` gained a `spots` arm to keep it under the 65%-floor ceiling.
  ['spots', { w: 740, h: 460 }],
])

/** This surface's natural footprint. No panel (the main window) → the cockpit box. */
export function naturalFor(panel: string | null): Natural {
  if (!panel) return MAIN_NATURAL
  return PANEL_NATURAL.get(panel) ?? POPOUT_NATURAL
}

const Z_MIN: Scale = 65
// Auto-fit does NOT upscale by default: 1080p full-screen (and anything bigger) sits at
// exactly 100%, familiar and un-magnified. Only SMALLER windows scale down (gently, via
// the surface's natural height) toward the 65% floor. Operators on big panels who want a
// larger UI raise this cap in Settings (or pin a manual scale).
const DEFAULT_CAP: Scale = 100
/** Relative dead-band: don't change step while the desired scale is within this of the
 * current one — kills flip-flop when the window sits on a step boundary. */
const HYST = 0.03

/** The density a CSS pixel is nominally drawn at. Not a preference — the number the whole
 *  web platform sizes against, so a display at 96 must come out unchanged. */
const REFERENCE_DPI = 96

/**
 * Below this the display needs no help and we must not touch it.
 *
 * NOT a round number and not taste — it clears the densest display that is ALREADY being
 * handled properly. A macOS Retina laptop is a 227-dpi panel at dpr 2, so its CSS pixel sits
 * at ~113 dpi, a factor of 1.18; Windows at 150% on a 141-dpi laptop lands at 0.98; a 27"
 * 1440p desktop monitor is 1.14. All of those are somebody's deliberate choice or the OS
 * doing its job, and none of them wants Nexus second-guessing it. The displays this seed
 * exists for are not marginal — the reported machine is 1.92 and a 4K panel with no scaling
 * is 1.70. 1.2 sits in the empty valley between the two populations.
 *
 * The first step above the default cap is 110, so a factor under 1.10 could never produce a
 * seed anyway; this threshold is the deliberate margin ON TOP of that, and it is the knob to
 * turn if this ever proves too shy or too eager.
 */
const SEED_MIN_FACTOR = 1.2

/**
 * The auto-fit CEILING this display deserves, or `null` to leave the operator's scale alone.
 *
 * WHY THIS EXISTS. `DEFAULT_CAP` is 100, so auto-fit can shrink the UI for a small window but
 * can never GROW it for a dense one — and on Linux nothing else grows it either. Windows and
 * macOS hand the webview a `devicePixelRatio` that already accounts for the panel, so a CSS
 * pixel arrives near the reference density and everything is sized correctly. X11 hands over
 * 1 whatever the hardware is, and the GNOME setting an operator reaches for (text-scaling-
 * factor → `gtk-xft-dpi`) was measured to move nothing in web content at all. The result is a
 * 4K or a small-and-sharp panel rendering the app at 1:1 physical pixels: everything half the
 * size it is on the same machine booted into Windows. That is a real field report (1.9.0,
 * 1920×1080 on a twelve-inch panel, ~184 dpi).
 *
 * THE ARITHMETIC. `physicalDpi / dpr` is the density of a CSS pixel — how sharp, and so how
 * SMALL, the app's own units land on this glass. Divided by the 96-dpi reference it is
 * exactly the factor the UI is undersized by. It is deliberately the same expression on all
 * three platforms: Windows and macOS choose a `dpr` that makes it come out at ~1, so they
 * take the `null` branch and nothing changes, which is the correct answer rather than a
 * special case.
 *
 * SNAPPED DOWN, NEVER UP, AND NEVER BELOW 100. Down because a UI slightly too small is a
 * squint and a UI too large hides controls; never below 100 because this may only ever RAISE
 * a ceiling — shrinking is auto-fit's job and it is already good at it.
 *
 * Note this returns a CAP, not a scale: auto-fit still takes `min(what fits, this)`, so a
 * window too small to hold the content whole is unaffected. On the reporter's own 1920×1080
 * that fit ceiling is 110, so he gets 110% rather than the 100% he has now; on a 4K panel,
 * where the fit allows far more, the cap is what binds and the change is large.
 */
export function dpiSeedCap(physicalDpi: number | null | undefined, dpr: number): Scale | null {
  if (physicalDpi == null || !Number.isFinite(physicalDpi) || physicalDpi <= 0) return null
  if (!Number.isFinite(dpr) || dpr <= 0) return null
  const factor = physicalDpi / dpr / REFERENCE_DPI
  if (factor < SEED_MIN_FACTOR) return null
  const target = factor * 100
  let seed: Scale | null = null
  for (const s of SCALE_STEPS) if (s <= target && s > DEFAULT_CAP) seed = s
  return seed
}

/**
 * Largest scale step that keeps `nat` whole in `innerW × innerH`, clamped to
 * [Z_MIN, cap] and snapped DOWN so content never overflows. Pure + testable.
 *
 * Oscillation-proof by construction: `innerW`/`innerH` are the raw layout-viewport size
 * and are UNCHANGED by `zoom` (it lives on `.app`, a child), and `nat` is a constant for
 * a window's lifetime (it comes from `?panel=`, which cannot change without a reload) —
 * so the output feeds nothing the inputs depend on. `prev` only adds hysteresis.
 */
export function fitScale(
  innerW: number,
  innerH: number,
  cap: Scale = DEFAULT_CAP,
  prev?: Scale,
  nat: Natural = MAIN_NATURAL,
): Scale {
  const target = Math.min(innerW / nat.w, innerH / nat.h) * 100
  const allowed = SCALE_STEPS.filter((s) => s <= cap)
  let z: Scale = allowed[0] ?? Z_MIN // smallest allowed == the floor
  for (const s of allowed) if (s <= target) z = s
  // Hysteresis: keep the current step if the desired scale is within HYST of it.
  if (prev != null && allowed.includes(prev) && Math.abs(target - prev) <= prev * HYST) {
    return prev
  }
  return z
}

/** FIELD MODE's shrink factor for the natural footprint (POTA field report).
 *
 * 0.85 is not taste — it is the layout contract's boundary. Field mode buys its larger type by
 * shrinking the EFFECTIVE viewport (zoom up = fewer effective pixels), and the supported floor
 * is 1024×768. 0.85 × MAIN_NATURAL (1200×900) = 1020×765: within a rounding error of the
 * floor, so the bump spends exactly the margin the contract allows and no more. A smaller
 * factor would push real windows below the floor; a larger one buys less type. Pinned by
 * `keeps the effective viewport at or above the supported floor` in useScale.test.ts. */
export const FIELD_FIT = 0.85

/** [`fitScale`] with field mode's inputs: the natural shrunk by [`FIELD_FIT`] and the cap
 *  raised to the ladder top. A swap of the pure function's INPUTS, deliberately — nothing is
 *  written to the scale keys, so switching field off restores the previous scale exactly,
 *  with no snapshot to manage and no macro trap (the operator's other settings were never
 *  touched). Auto mode only; a Manual pin stays verbatim (the pin doctrine above). */
export function fieldFitScale(
  innerW: number,
  innerH: number,
  prev?: Scale,
  nat: Natural = MAIN_NATURAL,
): Scale {
  const shrunk: Natural = { w: nat.w * FIELD_FIT, h: nat.h * FIELD_FIT }
  return fitScale(innerW, innerH, MAX_STEP, prev, shrunk)
}

/**
 * THE MAIN WINDOW'S FOOTPRINT ON A FRACTIONALLY SCALED DISPLAY (Windows 125/150/175%, Linux
 * fractional scaling).
 *
 * The OS scales the CSS pixel there, so a 1080p panel at 125% hands the webview a 1536×824
 * window. Fitted against `MAIN_NATURAL` that is 90%: an effective 1707×916 viewport, just under
 * the ~1800–1920 px where the top bar, the cockpit headers and the RTTY/PSK docks stop wrapping
 * (measured in real Chrome, 2026-11-01: at 90% the FT top bar is two rows and the CW decode
 * pane four lines; at 80% one row and 292 px). This box asks for that unwrapped width instead,
 * and for about the height a 1080p panel gives a maximized window at 100% scaling.
 *
 * A scaled CSS pixel is already physically bigger than an unscaled one, so shrinking it costs
 * no readability against the same panel at 100% — and `SCALED_FLOOR` makes sure it never costs
 * more than that.
 */
export const SCALED_NATURAL: Natural = { w: 1900, h: 1000 }

/** The devicePixelRatio range treated as fractional scaling. The bottom is Windows' 125% with
 *  room for a float that reports 1.2499…; the top stops short of 2 because an INTEGER ratio is
 *  the HiDPI design (a Retina Mac, Windows at 200%), where the panel is ~220 dpi and the OS
 *  already sized the CSS pixel for it: there the old fit is right, and this footprint would
 *  shrink a 1512-wide MacBook window to 75%. */
const SCALED_DPR_MIN = 1.24
const SCALED_DPR_MAX = 1.95

export function isFractionalScaling(dpr: number): boolean {
  return Number.isFinite(dpr) && dpr >= SCALED_DPR_MIN && dpr < SCALED_DPR_MAX
}

/**
 * The main window's auto-fit on a fractionally scaled display. Three bounds, in order:
 *
 *  1. fit against `SCALED_NATURAL` (the unwrapped width and the 1080p height);
 *  2. but never below `100 / dpr` snapped UP to a step — one unscaled CSS pixel per device
 *     pixel, i.e. never smaller than the same panel at 100% OS scaling (125% → 80, 150% → 70);
 *  3. and never ABOVE the classic fit — this rule only ever makes the UI smaller, so a small
 *     window on a scaled display keeps exactly what it had.
 *
 * At any ratio outside `isFractionalScaling` it IS the classic fit, so unscaled screens are
 * untouched — and so it is under a RAISED cap: an operator who set the ceiling above the default
 * (Settings ▸ Appearance, or the DPI seed) asked for a bigger UI, and this rule never argues with
 * that. Pure + testable; `prev` is the same hysteresis as `fitScale`.
 */
export function scaledFitScale(
  innerW: number,
  innerH: number,
  dpr: number,
  cap: Scale = DEFAULT_CAP,
  prev?: Scale,
): Scale {
  const classic = fitScale(innerW, innerH, cap, prev)
  if (!isFractionalScaling(dpr) || cap > DEFAULT_CAP) return classic
  const fit = fitScale(innerW, innerH, cap, prev, SCALED_NATURAL)
  const floor = SCALE_STEPS.find((s) => s >= 100 / dpr - 1e-6) ?? Z_MIN
  return Math.min(classic, Math.max(fit, floor)) as Scale
}

/** Which auto-fit rule the main window uses on a fractionally scaled display. */
export type FitRule = 'scaled' | 'classic'

/** The stored answer, or `null` when the operator was never asked. A value that is not
 *  'classic' — including a corrupt one — reads as 'scaled', the default, so a damaged key can
 *  neither re-ask nor shrink anything the default would not. Storage failure: not asked, which
 *  only means the default rule applies (the prompt itself is gated on the write landing). */
export function readFitRule(): FitRule | null {
  try {
    const raw = localStorage.getItem(FIT_RULE_KEY)
    if (raw === null) return null
    return raw === 'classic' ? 'classic' : 'scaled'
  } catch {
    return null
  }
}

/** The one-time offer: the scaled rule moved the main window off the classic size. */
export interface FitOffer {
  scaled: Scale
  classic: Scale
}

/**
 * Synchronous first-paint seed (no DOM measurement, no flash): fit against the default
 * cap using the current window. Kept as a named export because tests + `readInitial`
 * use it. Replaces the old coarse width-band table.
 */
export function pickInitialZoom(
  w: number = typeof window !== 'undefined' ? window.innerWidth : 1280,
  h: number = typeof window !== 'undefined' ? window.innerHeight : 800,
  nat: Natural = MAIN_NATURAL,
): Scale {
  return fitScale(w, h, DEFAULT_CAP, undefined, nat)
}

function readMode(): ScaleMode {
  const raw = surfaceGet(MODE_KEY)
  if (raw === 'auto' || raw == null) return 'auto'
  const n = Number(raw)
  return (SCALE_STEPS as number[]).includes(n) ? (n as Scale) : 'auto'
}

const MAX_STEP: Scale = SCALE_STEPS[SCALE_STEPS.length - 1]

/**
 * A pinned step as APPLIED on this surface. In the MAIN window a pin is an explicit
 * operator choice — honoured verbatim, never silently overridden (Settings is always
 * reachable there to change it). A NON-main surface (pop-out) has no scale UI, and its
 * pin is usually INHERITED via windowScope's bare-key fallback — so a pin above what
 * that window can even fit (a 175 pin in a 380×180-min waterfall window → 217×103 CSS
 * px effective) leaves no in-window recovery path. Cap it at the window's own fit
 * ceiling — the same `autoCeil = fitScale(w, h, MAX_STEP)` rule SettingsPanel uses to
 * disable dead cap chips. The stored pin itself is never rewritten. Pure + testable.
 *
 * The ceiling is measured against THIS surface's natural, so the pin half of the
 * band-map bug dies with the auto half: the ceiling in a 420×780 band map was the 65
 * floor, which crushed every inherited pin. It is now the pin honoured as far as this
 * window can actually take it — still bounded, never rewritten.
 */
export function capPinnedScale(
  pin: Scale,
  mainSurface: boolean,
  innerW: number,
  innerH: number,
  nat: Natural = MAIN_NATURAL,
): Scale {
  if (mainSurface) return pin
  const ceil = fitScale(innerW, innerH, MAX_STEP, undefined, nat)
  return pin <= ceil ? pin : ceil
}

/** True when nobody has ever chosen an auto-fit ceiling on this install.
 *
 * THE FIRST-LAUNCH GATE, and the whole of it: the DPI seed writes this key, so this is false
 * for the rest of the install's life. An upgrade cannot re-seed, and a scale the operator
 * chose — including choosing to leave it at 100 — is never moved under them. Absence of the
 * key is the only signal used; a stored value is treated as an answer even when it equals the
 * default, because it IS one. A storage failure returns false, so the seed does nothing rather
 * than firing on every launch. */
export function capNeverChosen(): boolean {
  try {
    return localStorage.getItem(CAP_KEY) === null
  } catch {
    return false
  }
}

function readCap(): Scale {
  try {
    const n = Number(localStorage.getItem(CAP_KEY))
    return (SCALE_STEPS as number[]).includes(n) ? (n as Scale) : DEFAULT_CAP
  } catch {
    return DEFAULT_CAP
  }
}

export interface ScaleControl {
  /** The effective numeric scale currently applied (drives `--ui-zoom`). */
  scale: Scale
  /** 'auto' (fit to window) or a pinned step. */
  mode: ScaleMode
  /** Max step auto-fit may reach (the raised cap; only meaningful in 'auto'). */
  cap: Scale
  /** Set 'auto' or pin a specific step. */
  setMode: (m: ScaleMode) => void
  /** Set the auto-fit max cap. */
  setCap: (c: Scale) => void
  /** Set while the main window, never asked, sits at a different auto-fit size than the
   *  classic rule would give it (a fractionally scaled display) — the one-time prompt. */
  fitOffer: FitOffer | null
  /** Record the operator's answer (and with it, that they were asked). */
  setFitRule: (r: FitRule) => void
}

export function useScale(field = false): ScaleControl {
  const [mode, setModeState] = useState<ScaleMode>(readMode)
  const [cap, setCapState] = useState<Scale>(readCap)
  const [fitRule, setFitRuleState] = useState<FitRule | null>(readFitRule)
  const [fitOffer, setFitOffer] = useState<FitOffer | null>(null)
  // The natural footprint is read from the URL HERE, not at module scope: a pop-out
  // knows which panel it is only from `?panel=`, and a module-level read would freeze
  // whatever URL loaded the module first (it also makes the hook testable per surface).
  // DetachedPanel therefore needs no change — every caller of useScale() gets the right
  // denominator for free, which is the point: a future pop-out branch cannot forget to
  // pass it.
  const [scale, setScaleState] = useState<Scale>(() => {
    const nat = naturalFor(surfacePanel())
    return mode === 'auto'
      ? field
        ? fieldFitScale(window.innerWidth, window.innerHeight, undefined, nat)
        : scaledRuleApplies(fitRule)
          ? scaledFitScale(window.innerWidth, window.innerHeight, window.devicePixelRatio, DEFAULT_CAP)
          : pickInitialZoom(window.innerWidth, window.innerHeight, nat)
      : capPinnedScale(mode, surfaceId() === 'main', window.innerWidth, window.innerHeight, nat)
  })
  // Latest applied scale, for hysteresis — read inside the resize handler without
  // making it an effect dependency (that would re-subscribe every fit).
  const scaleRef = useRef(scale)
  scaleRef.current = scale

  // Publish the effective scale as `--ui-zoom`.
  useEffect(() => {
    document.documentElement.style.setProperty('--ui-zoom', String(scale / 100))
  }, [scale])

  // Auto-fit: recompute on resize. Deps are [mode, cap] only — NOT scale — so our own
  // scale change never re-subscribes, and (since zoom doesn't move innerHeight) never
  // re-fires the listener. rAF-debounced, mirroring useViewport.
  useEffect(() => {
    // Constant for this window's lifetime (see the note on the initialiser above), so it
    // is not an effect dependency — the oscillation-proof property is unaffected.
    const nat = naturalFor(surfacePanel())
    if (mode !== 'auto') {
      const applyPin = () =>
        setScaleState(
          capPinnedScale(mode, surfaceId() === 'main', window.innerWidth, window.innerHeight, nat),
        )
      applyPin()
      // A MAIN-window pin is verbatim (capPinnedScale is the identity there) — nothing
      // to re-evaluate. A POP-OUT's cap depends on the window box, and sampling it only
      // here re-created the very stranding the cap removes: a strip opened at its
      // 380×180 minimum capped an inherited 175 pin to 65 and STAYED 65 after the
      // operator maximized; opened large, a shrink kept the oversized pin. Re-run the
      // cap on resize, rAF-debounced like the auto fit below (review 2026-07-31).
      if (surfaceId() === 'main') return
      let pinRaf = 0
      const onPinResize = () => {
        cancelAnimationFrame(pinRaf)
        pinRaf = requestAnimationFrame(applyPin)
      }
      window.addEventListener('resize', onPinResize)
      return () => {
        window.removeEventListener('resize', onPinResize)
        cancelAnimationFrame(pinRaf)
      }
    }
    let raf = 0
    const scaled = !field && scaledRuleApplies(fitRule)
    const apply = () => {
      const w = window.innerWidth
      const h = window.innerHeight
      // Field mode swaps the fit's INPUTS (smaller natural, top cap) — see fieldFitScale. The
      // scaled rule reads the ratio LIVE: it changes when the window crosses to a monitor with
      // a different scaling, and the answer must follow it.
      const z = field
        ? fieldFitScale(w, h, scaleRef.current, nat)
        : scaled
          ? scaledFitScale(w, h, window.devicePixelRatio, cap, scaleRef.current)
          : fitScale(w, h, cap, scaleRef.current, nat)
      if (z !== scaleRef.current) setScaleState(z)
      // Never asked, and the scaled rule moved us off the classic size: offer the choice. Not
      // sticky — a window resized to where both rules agree has nothing to ask about.
      const classic = fitScale(w, h, cap, undefined, nat)
      const offer = scaled && fitRule === null && z !== classic ? { scaled: z, classic } : null
      setFitOffer((o) => (o?.scaled === offer?.scaled && o?.classic === offer?.classic ? o : offer))
    }
    const onResize = () => {
      cancelAnimationFrame(raf)
      raf = requestAnimationFrame(apply)
    }
    raf = requestAnimationFrame(apply)
    window.addEventListener('resize', onResize)
    // A move to a monitor with a different scaling need not fire `resize`: Windows keeps the
    // window's logical size across a DPI change, so innerWidth/innerHeight can stay put while
    // the ratio moves. The resolution media query is the platform's own signal for it; it
    // matches one ratio, so it is re-armed at the new one after each change.
    let dprQuery: MediaQueryList | null = null
    const onDpr = () => {
      armDpr()
      onResize()
    }
    const armDpr = () => {
      dprQuery?.removeEventListener('change', onDpr)
      dprQuery =
        scaled && typeof window.matchMedia === 'function'
          ? window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`)
          : null
      dprQuery?.addEventListener('change', onDpr)
    }
    armDpr()
    return () => {
      window.removeEventListener('resize', onResize)
      dprQuery?.removeEventListener('change', onDpr)
      cancelAnimationFrame(raf)
    }
  }, [mode, cap, field, fitRule])

  const setMode = useCallback((m: ScaleMode) => {
    setModeState(m)
    surfaceSet(MODE_KEY, String(m))
  }, [])

  const setCap = useCallback((c: Scale) => {
    setCapState(c)
    try {
      localStorage.setItem(CAP_KEY, String(c))
    } catch {
      /* storage blocked */
    }
  }, [])

  const setFitRule = useCallback((r: FitRule) => {
    setFitRuleState(r)
    try {
      localStorage.setItem(FIT_RULE_KEY, r)
    } catch {
      /* storage blocked */
    }
  }, [])

  // A pinned scale is the operator's own choice: nothing to offer while one is set.
  return { scale, mode, cap, setMode, setCap, fitOffer: mode === 'auto' ? fitOffer : null, setFitRule }
}

/** The scaled rule is the MAIN window's (a pop-out keeps its own natural, untouched) and the
 *  desktop app's: in a browser (Remote) the ratio is also the browser's own zoom, and an
 *  operator who zoomed in to read must not be shrunk back out. The desktop app is Tauri's
 *  bridge, which Tauri puts in each of its webviews before the page's first script (the same
 *  test as useTheme.ts; not api.ts's `isTauri`, so this module stays free of the bridge).
 *  'classic' is the operator's no. */
function scaledRuleApplies(rule: FitRule | null): boolean {
  return rule !== 'classic' && surfaceId() === 'main' && window.__TAURI_INTERNALS__ !== undefined
}
