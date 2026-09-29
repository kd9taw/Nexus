// THE OWN-GRID VIEWS' COLUMN SPLIT (layout L7) — the pure half. Satellites and Awards lay their two
// columns out as fr tracks in styles.css, and one divider between them (a PaneSeam split in column
// mode) sets the pair's shares. They have no panel record, so what is stored — per window, under
// the view's own key — is the first column's share of 2 (panelState.seamShares, the shares every
// split divider commits), and the tracks read it as two fr tokens.
//
// A SHARE NEEDS NO CLAMP AGAINST THE WINDOW, only against its own range: it is a proportion, and
// each template floors both tracks at `min(260px, 40%)`, which the divider also stops at
// (`VIEW_COLUMN_FLOOR`), so a split stored on a wide window can never squeeze a column past its
// floor on a narrow one, and the floors can never add up to more than the grid.
import type { CSSProperties } from 'react'
import { MIN_SHARE } from './panelState'

/** Each column's floor, CSS px — the templates' `min(260px, 40%)`, which is 260 px at every width
 *  where these views have two columns (both stack below the md tier). */
export const VIEW_COLUMN_FLOOR = 260

/** A stored share, or null for "never set" (and for anything that is not a share). Clamped into the
 *  range every split divider keeps, so a hand-edited value cannot collapse a column. */
export function parseColumnShare(raw: string | null): number | null {
  if (raw == null || raw.trim() === '') return null
  const v = Number(raw)
  if (!Number.isFinite(v)) return null
  return Math.min(2 - MIN_SHARE, Math.max(MIN_SHARE, v))
}

/** The two fr tokens for a stored share, or nothing at all (the sheet's stock split). */
export function columnShareStyle(share: number | null, vars: readonly [string, string]): CSSProperties | undefined {
  if (share == null) return undefined
  return { [vars[0]]: `${share}fr`, [vars[1]]: `${2 - share}fr` } as CSSProperties
}
