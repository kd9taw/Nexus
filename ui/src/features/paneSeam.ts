// THE PANE SEAM's pure half (layout L1, 2026-09-28) — what a key does to a divider, and the clamps
// a strip divider declares in the sheet's own units. components/PaneSeam.tsx measures the box and
// paints; everything here is arithmetic, so every rule is unit-tested without a layout engine.
//
// The strip clamps moved here unchanged from Splitter.tsx, which PaneSeam replaced.

export type SeamAxis = 'x' | 'y'

/** Keyboard steps for a divider measured in CSS px (a strip's height, a rail's width) — the
 *  steps Connect's rail handles already use (features/connectRails), so the app's px dividers
 *  move alike under the arrows. */
export const SEAM_STEP_PX = 16
export const SEAM_STEP_PX_BIG = 64
/** …and for a divider that splits two panes, as a fraction of their combined span (Connect's
 *  RailSplitHandle steps). */
export const SEAM_STEP_SPLIT = 0.05
export const SEAM_STEP_SPLIT_BIG = 0.15

/** Where a divider stands, as its keyboard sees it. `value` is the size of the PRIMARY pane (the
 *  one the divider sizes); `grows` says which way that pane grows: +1 when moving the divider
 *  down/right makes it bigger (a strip above its seam, a left rail), −1 when moving it up/left
 *  does (a rail on the right, whose seam is its left edge). */
export interface SeamKeys {
  axis: SeamAxis
  value: number
  min: number
  max: number
  step: number
  bigStep: number
  grows: 1 | -1
}

export interface SeamKeyPress {
  key: string
  shiftKey: boolean
  altKey: boolean
  ctrlKey: boolean
  metaKey: boolean
}

/**
 * What a key press does to a divider: the next value (UNCLAMPED — the caller clamps it against the
 * box as it is now), 'reset' for the default back, or null for a key that is not the divider's.
 *
 * The arrows along the axis move the divider the way they point (WAI-ARIA's window splitter);
 * Shift makes the step big; Home and End go to the smallest and largest the primary pane may be;
 * Backspace resets. A chord with Alt, Ctrl or Meta is never the divider's: those belong to the OS,
 * the webview and the app's own shortcuts, and must reach them untouched.
 */
export function seamKey(e: SeamKeyPress, k: SeamKeys): number | 'reset' | null {
  if (e.altKey || e.ctrlKey || e.metaKey) return null
  if (e.key === 'Backspace') return 'reset'
  if (e.key === 'Home') return k.min
  if (e.key === 'End') return k.max
  const [back, fwd] = k.axis === 'y' ? ['ArrowUp', 'ArrowDown'] : ['ArrowLeft', 'ArrowRight']
  const dir = e.key === fwd ? 1 : e.key === back ? -1 : 0
  if (dir === 0) return null
  return k.value + dir * k.grows * (e.shiftKey ? k.bigStep : k.step)
}

/** The geometry a strip clamp may be expressed against. `fontPx` is the container's computed
 *  font size (what an `em` in the sheet resolves to) and `vhEff` is `--vh-eff` in CSS px
 *  (what the sheet's `calc(f * var(--vh-eff))` caps resolve to). */
export interface SplitGeom {
  fontPx: number
  vhEff: number
}

/** A drag end: CSS px, or a function of the live geometry.
 *
 *  The function form exists because the CLAMPS THAT ACTUALLY BIND ARE IN THE SHEET, and
 *  they are not px. `.phone-cockpit .ph-scope-panel` floors at `8em` and caps at
 *  `calc(0.45 * var(--vh-eff))`; the call sites declared 100 px and 420 px. Both ends of
 *  the drag were therefore DEAD TRAVEL — below 112 px and above 0.45·--vh-eff the
 *  pointer moved the variable and the panel did not move at all, which reads to the
 *  operator as a broken handle. A clamp written in the sheet's own units cannot drift
 *  out of agreement with it; cockpit-shells.test.ts computes both sides and fails when
 *  they do.
 *
 *  ⚠️ A declared clamp can only speak for the strip's OWN rules. The limit a NEIGHBOUR sets —
 *  Phone's scope shrinking against the pane region's 18em floor, measured at 1024×768 as a
 *  ceiling of ~242 CSS px under a declared 406 — is in no rule of the strip's, so PaneSeam
 *  measures the range the layout honours as well and clamps to both (components/PaneSeam.tsx). */
export type SplitClamp = number | ((g: SplitGeom) => number)

export function resolveClamp(c: SplitClamp, g: SplitGeom): number {
  return typeof c === 'function' ? c(g) : c
}

/** The clamps of the CW/Phone band-scope strip, in the sheet's units — kept here, beside
 *  the resolver, because both cockpits declare the identical pair and the sheet rules
 *  (`.cw-cockpit .ph-scope-panel` / `.phone-cockpit .ph-scope-panel`) are their only
 *  other definition. */
export const SCOPE_SPLIT_MIN: SplitClamp = (g) => 8 * g.fontPx
export const SCOPE_SPLIT_MAX: SplitClamp = (g) => 0.45 * g.vhEff

/** The clamps of JS8's waterfall strip (layout L2), in the sheet's units, from its own rule
 *  (`.js8-cockpit .waterfall-wrap`): the YIELDING floor the keyboard cockpits' waterfalls share —
 *  8em where there is room, never more than 28 % of the effective viewport — and the scope's
 *  45 % ceiling. cockpit-shells.test.ts computes both sides at three geometries. */
export const WATERFALL_SPLIT_MIN: SplitClamp = (g) => Math.min(8 * g.fontPx, 0.28 * g.vhEff)
export const WATERFALL_SPLIT_MAX: SplitClamp = (g) => 0.45 * g.vhEff

/** A split percentage as stored by any build (Splitter wrote the same key and format): NaN-safe,
 *  and the impossible ends — 0, 100 and outside — read as "never set". */
export function parseSplitPct(raw: string | null): number | null {
  const v = parseFloat(raw ?? '')
  return Number.isFinite(v) && v > 0 && v < 100 ? v : null
}

/** Clamp a split percentage against a container span, ALL in CSS px: the panel stays
 *  within [minPx, min(maxPx, 90% of span)] — the drag's own bounds — so a stored %
 *  from another geometry re-enters range at apply time. span ≤ 0 (hidden container)
 *  returns the input unchanged. */
export function clampSplitPct(pct: number, spanCss: number, minPx: number, maxPx: number): number {
  if (!(spanCss > 0)) return pct
  const px = (pct / 100) * spanCss
  const clamped = Math.min(Math.min(maxPx, spanCss * 0.9), Math.max(minPx, px))
  return (clamped / spanCss) * 100
}
