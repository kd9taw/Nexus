// THE GRID COCKPITS' COLUMN DIVIDERS (layout L2) — the pure half. Phone, CW and JS8 lay their pane
// region out as two or three columns (cockpit-panes.css): a divider between the two feed columns
// splits them (their fr shares, the record's `cols.a` / `cols.b`), and a divider on the log
// column's left edge sets its width (`cols.log`, CSS px). This module is what they write and the
// range the log divider moves through; components/panes/RegionColumnSeams.tsx measures and paints.
//
// THE LOG WIDTH IS CLAMPED BY THE LAYOUT ITSELF. What the region carries is `min(<px>px, 50%)`,
// and the template floors the track at 24em, so the width shown is the operator's clamped to 24em
// … half the region on load, on every resize and on every zoom change — before any script runs,
// and without the stored preference ever being rewritten (a bigger window gets it back, the
// connectRails discipline). The divider's own range below is the same pair of numbers, measured,
// so the keys and the drag stop exactly where the column does.
import type { PanelCols } from './panelState'

/** The log column's floor, in the region's em — the template's own `minmax(24em, …)`. */
export const LOG_COL_MIN_EM = 24
/** The widest a dragged log column may be, as a share of the region. */
export const LOG_COL_MAX_SHARE = 0.5

/** `--cockpit-col-log` for a width the operator set: capped in the sheet at half the region. */
export function logColValue(px: number): string {
  return `min(${Math.round(px)}px, ${LOG_COL_MAX_SHARE * 100}%)`
}

/** The tokens a pane region carries for the record's columns — only the ones the operator set,
 *  so a region nobody has divided renders exactly the sheet's defaults. The two- and three-track
 *  templates consume them; the stacking tier's one track ignores them. */
export function regionColsStyle(cols: PanelCols | undefined): Record<string, string> {
  const s: Record<string, string> = {}
  if (cols?.a != null) s['--cockpit-col-a'] = `${cols.a}fr`
  if (cols?.b != null) s['--cockpit-col-b'] = `${cols.b}fr`
  if (cols?.log != null) s['--cockpit-col-log'] = logColValue(cols.log)
  return s
}

/** The range the log divider moves through, in CSS px, for a region `contentW` CSS px wide whose
 *  font is `fontPx`: the template's 24em floor up to half the region. The floor wins a
 *  disagreement, which only a region below the two-column tier could produce. */
export function logColRange(contentW: number, fontPx: number): { min: number; max: number } {
  const min = LOG_COL_MIN_EM * fontPx
  return { min, max: Math.max(min, LOG_COL_MAX_SHARE * contentW) }
}

// ── THE LEFT SIDE (operator's pick, 2026-10-03; Phone) ─────────────────────────────────────────────
// A full-height column beside the scope that ⊞ Panels ▸ Arrange fills (features/panelPlace). Its width
// is the operator's (the record's `cols.leftSide`, CSS px, written on the side as `--cockpit-left-w`),
// and the SHEET clamps it — `clamp(<floor>em, the width, <share>)` of the row (cockpit-panes.css
// `.cockpit-left`) — so a width stored on a wide window is clamped by the layout itself on load, on
// every resize and on every zoom change, before any script runs, and the preference is never rewritten.
// The divider's range below is the same pair of numbers, measured.

/** The side shows only on a window at least this wide, in EFFECTIVE CSS px (`--vw-eff`): the
 *  operator's "about 1280 px". Below it its panes stand in their usual columns. The supported floor
 *  window (1024×768, about 1205 px at its own zoom) is under it, so every cockpit there is unchanged. */
export const LEFT_SIDE_MIN_VW = 1280
/** The side's floor, in its own em — the sheet's `clamp()` floor. */
export const LEFT_SIDE_MIN_EM = 16
/** The widest a dragged side may be, as a share of the row it shares with the scope and the panes. */
export const LEFT_SIDE_MAX_SHARE = 0.4

/** `--cockpit-left-w` for a width the operator set (the sheet clamps it). */
export function leftSideValue(px: number): string {
  return `${Math.round(px)}px`
}

/** The range the side's divider moves through, in CSS px, for a row `rowW` CSS px wide whose font is
 *  `fontPx`: the sheet's em floor up to its share of the row. The floor wins a disagreement. */
export function leftSideRange(rowW: number, fontPx: number): { min: number; max: number } {
  const min = LEFT_SIDE_MIN_EM * fontPx
  return { min, max: Math.max(min, LEFT_SIDE_MAX_SHARE * rowW) }
}
