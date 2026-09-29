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
