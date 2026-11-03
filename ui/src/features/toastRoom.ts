// #445: WHERE IN ITS CORNER THE POP-UP COLUMN MAY STAND, so that no toast — and above all no toast's
// own buttons (Work →, ✕, which take pointer events) — ever lies over a transmit control.
//
// The corner alone is not enough. A cockpit's TX dock runs along the bottom of the window (Phone's
// PTT at the bottom left, CW's and PSK's Send at the bottom right), FT's QSO strip and every screen's
// TX strip cross the window in the middle, and the top bar's TX cluster sits at the top right. A
// column pinned 16 px from a corner put a toast's ✕ on Phone's PTT with ONE toast, and a burst grew
// up over Stop TX. So the column takes the free stretch of its own corner's side of the window
// between those surfaces: the lowest one tall enough for a toast in a bottom corner, the highest in
// a top corner, and no more of it than there is (it scrolls inside, as it always has).
//
// The surfaces are the call card's (`TX_AVOID`, features/txSurfaces.ts), measured the same way, plus the
// update banner the column already lifted clear of. Pure, so the rule is tested without a browser.
import type { PopupCorner } from './popupCorner'

export interface Box {
  left: number
  top: number
  right: number
  bottom: number
}

/** Where the column stands, in window px: `inset` from its corner's top or bottom edge, and the
 *  tallest it may be there. */
export interface ToastRoom {
  inset: number
  maxHeight: number
}

/** Below this a stretch is no place for the column: one toast's message and its buttons. */
export const TOAST_MIN_ROOM = 64

/**
 * The column's room in a `view` window for a column `width` wide standing `gap` from the corner's
 * side edge, touching none of `avoid` (kept `gap` clear of each). A bottom corner takes the lowest
 * free stretch at least `TOAST_MIN_ROOM` tall, a top corner the highest; where none is that tall,
 * the tallest there is. Null when the column's side of the window has no free stretch at all: then
 * the stylesheet's own corner stands.
 */
export function toastRoom(
  corner: PopupCorner,
  view: { w: number; h: number },
  width: number,
  avoid: readonly Box[],
  gap: number,
): ToastRoom | null {
  const left = corner.endsWith('left') ? gap : view.w - gap - width
  const right = left + width
  const blocks = avoid
    .filter((b) => b.right > left && b.left < right && b.bottom > b.top)
    .map((b) => [b.top - gap, b.bottom + gap] as const)
    .sort((a, b) => a[0] - b[0])
  const free: [number, number][] = []
  let from = gap
  for (const [top, bottom] of blocks) {
    if (top > from) free.push([from, Math.min(top, view.h - gap)])
    from = Math.max(from, bottom)
  }
  if (from < view.h - gap) free.push([from, view.h - gap])
  const usable = free.filter(([a, b]) => b - a > 0)
  if (usable.length === 0) return null
  const ordered = corner.startsWith('bottom') ? [...usable].reverse() : usable
  const pick = ordered.find(([a, b]) => b - a >= TOAST_MIN_ROOM) ?? [...usable].sort((x, y) => y[1] - y[0] - (x[1] - x[0]))[0]
  const [a, b] = pick
  return { inset: corner.startsWith('bottom') ? view.h - b : a, maxHeight: b - a }
}
