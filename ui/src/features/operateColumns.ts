// OPERATE'S COLUMNS (layout L5). Operate keeps its own two grids (styles.css `.cockpit-lower`,
// not the pane grid), and this is the geometry the dividers between their columns read and write.
//
//   · CLASSIC: Band Activity | the Rx Frequency column (Rx Frequency over Tx1–Tx6) | the Stations
//     rail — three fr tracks and a divider between each pair. A divider moves only its own two
//     columns: it paints the pair at the pair's own total (PaneSeam `scale`), so the third column
//     keeps its width. (Painted as shares summing to 2, the first step of the Rx Frequency column /
//     Stations divider narrowed Band Activity: L1's leftover.) Stored as each column's FRACTION of
//     the grid, ×2 so it rides the record's share range: `cols.a` = Band Activity, `cols.b` =
//     Stations, the Rx Frequency column the rest. A record from an earlier build (share.txmsgs /
//     share.stations, those two columns as fr, Band Activity at the sheet's 1.15) opens exactly as
//     that build painted it, until a divider is moved.
//   · ROSTER: the Call Roster | the side rail — two tracks and one divider. The Call Roster's
//     share of the pair is `share.callRoster` and the rail's is the rest, the way RTTY's text/log
//     divider stores only its transcript's: the rail is no single pane.
//
// Neither changes a pane's column or its place in the tree; a divider only paints tokens the
// sheet's templates read (operate-classic-grid.test.ts pins the templates to these defaults).
import type { CSSProperties } from 'react'
import type { OperatePanelId, PanelLayout } from './panelState'

/** The sheet's own Classic widths, in its fr (`.cockpit-lower.classic`): Band Activity, the Rx
 *  Frequency column, Stations. */
export const CLASSIC_FR = [1.15, 0.95, 0.72] as const
/** The sheet's own Roster widths (`.cockpit-lower.roster`): the Call Roster, the side rail. */
export const ROSTER_FR = [1.4, 1] as const

/** The grid tokens of Classic's three columns, in the same order. */
export const CLASSIC_VARS = ['--op-col-ba', '--op-col-a', '--op-col-b'] as const
/** Roster's two. */
export const ROSTER_VARS = ['--op-roster-a', '--op-roster-b'] as const

/** A stored Band Activity or Stations width never goes past these (fractions of the grid) — the
 *  record's share range, halved — and the Rx Frequency column never below the last. The sheet
 *  floors the two narrow columns in px as well (300 and 260), so these bind only at the far end
 *  of a drag, where the px floors already hold the columns on screen. */
const EDGE_MIN = 0.075
const EDGE_MAX = 0.925
const MIDDLE_MIN = 0.005

export type ClassicTriple = [number, number, number]

/** Classic's three widths as the grid paints them NOW, in that paint's own units, and where they
 *  come from: the dividers (`cols`), an earlier build's pair (`legacy`), or the sheet (`stock`). */
export interface ClassicWidths {
  fr: ClassicTriple
  source: 'cols' | 'legacy' | 'stock'
}

export function classicWidths(layout: PanelLayout<OperatePanelId>): ClassicWidths {
  const a = layout.cols?.a
  const b = layout.cols?.b
  if (a != null && b != null) {
    const ba = a / 2
    const st = b / 2
    // A pair no divider could have written (the two edge columns filling the grid) is not a
    // layout; the earlier layers stand instead.
    if (1 - ba - st >= MIDDLE_MIN) return { fr: [ba, 1 - ba - st, st], source: 'cols' }
  }
  const t = layout.share.txmsgs
  const s = layout.share.stations
  if (t != null || s != null) return { fr: [CLASSIC_FR[0], t ?? CLASSIC_FR[1], s ?? CLASSIC_FR[2]], source: 'legacy' }
  return { fr: [...CLASSIC_FR], source: 'stock' }
}

/** The grid's inline tokens: none for the sheet's own widths, an earlier build's pair exactly as it
 *  painted them (Band Activity left to the sheet), or all three. */
export function classicStyle(layout: PanelLayout<OperatePanelId>): CSSProperties | undefined {
  const w = classicWidths(layout)
  if (w.source === 'stock') return undefined
  const out: Record<string, string> = {}
  if (w.source === 'legacy') {
    if (layout.share.txmsgs != null) out[CLASSIC_VARS[1]] = `${layout.share.txmsgs}fr`
    if (layout.share.stations != null) out[CLASSIC_VARS[2]] = `${layout.share.stations}fr`
  } else {
    CLASSIC_VARS.forEach((v, i) => (out[v] = `${w.fr[i]}fr`))
  }
  return out as CSSProperties
}

/** What a divider between columns `i` and `j` paints a share of 1 as: the pair's mean, so the
 *  pair keeps its total and the third column its width. */
export function classicScale(w: ClassicWidths, i: number, j: number): number {
  return (w.fr[i] + w.fr[j]) / 2
}

/** The widths to store: fractions ×2, the edge columns held in the record's range and the Rx
 *  Frequency column left positive. */
function classicCols(fr: ClassicTriple): { a: number; b: number } {
  const total = fr[0] + fr[1] + fr[2]
  let ba = Math.min(EDGE_MAX, Math.max(EDGE_MIN, fr[0] / total))
  let st = Math.min(EDGE_MAX, Math.max(EDGE_MIN, fr[2] / total))
  const over = ba + st - (1 - MIDDLE_MIN)
  if (over > 0) {
    if (ba >= st) ba -= over
    else st -= over
  }
  return { a: 2 * ba, b: 2 * st }
}

/** A divider between columns `i` and `j` committed the pair's shares (seamShares, summing to 2):
 *  the new widths to store. The third column keeps its width. */
export function classicCommit(w: ClassicWidths, i: number, j: number, av: number, bv: number): { a: number; b: number } {
  const scale = classicScale(w, i, j)
  const fr: ClassicTriple = [...w.fr]
  fr[i] = av * scale
  fr[j] = bv * scale
  return classicCols(fr)
}

/** A divider between columns `i` and `j` reset: that pair back to the sheet's own ratio within
 *  its current total; the third column keeps its width. `null` when the whole grid is then the
 *  sheet's own again (nothing to store). */
export function classicReset(w: ClassicWidths, i: number, j: number): { a: number; b: number } | null {
  const fr: ClassicTriple = [...w.fr]
  const sum = fr[i] + fr[j]
  fr[i] = (sum * CLASSIC_FR[i]) / (CLASSIC_FR[i] + CLASSIC_FR[j])
  fr[j] = sum - fr[i]
  const total = fr[0] + fr[1] + fr[2]
  const stockTotal = CLASSIC_FR[0] + CLASSIC_FR[1] + CLASSIC_FR[2]
  if (fr.every((v, k) => Math.abs(v / total - CLASSIC_FR[k] / stockTotal) < 1e-9)) return null
  return classicCols(fr)
}

/** Roster's inline tokens: none for the sheet's own widths, else the Call Roster's share and the
 *  rail's (the rest of 2). */
export function rosterStyle(layout: PanelLayout<OperatePanelId>): CSSProperties | undefined {
  const r = layout.share.callRoster
  if (r == null) return undefined
  return { [ROSTER_VARS[0]]: `${r}fr`, [ROSTER_VARS[1]]: `${2 - r}fr` } as CSSProperties
}
