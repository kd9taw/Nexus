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
 * free stretch at least `minRoom` tall (`TOAST_MIN_ROOM` at the window's zoom: a toast scales with it), a top corner the highest; where none is that tall,
 * the tallest there is. Null when the column's side of the window has no free stretch at all: then
 * the stylesheet's own corner stands.
 */
export function toastRoom(
  corner: PopupCorner,
  view: { w: number; h: number },
  width: number,
  avoid: readonly Box[],
  gap: number,
  minRoom = TOAST_MIN_ROOM,
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
  const pick = ordered.find(([a, b]) => b - a >= minRoom) ?? [...usable].sort((x, y) => y[1] - y[0] - (x[1] - x[0]))[0]
  const [a, b] = pick
  return { inset: corner.startsWith('bottom') ? view.h - b : a, maxHeight: b - a }
}

/** Where the column stands, and whether it steps aside for now. */
export interface ToastPlace {
  room: ToastRoom | null
  stepAside: boolean
}

/**
 * WHAT THE OPERATOR IS USING RIGHT NOW comes second only to the transmit surfaces. A toast's buttons take clicks and its
 * body hides what is under it, so a column standing over an open menu covered the menu's items, one standing where a
 * folded dock's macro keys scroll hid the key Tab had just focused, and one over JTTY's refusal hid the refusal (all
 * measured). So, given `inUse` (the boxes of what is open, parked or focused) besides `hard` (the transmit surfaces):
 *   · the column takes a stretch clear of both where one is at least `minRoom` tall;
 *   · where none is, it takes its room clear of the transmit surfaces alone, exactly as without `inUse`, and STEPS
 *     ASIDE (is not drawn) while its `height` there meets anything in use. It never stands over a transmit surface to
 *     make room for anything else, and it comes back as soon as the menu closes or the focus moves on;
 *   · where the transmit surfaces leave its side of the window no room at all, it steps aside too, rather than fall back
 *     to the stylesheet's bare corner, which may be over one.
 * `height` is the column's content height, in window px like everything else here.
 */
export function toastPlace(
  corner: PopupCorner,
  view: { w: number; h: number },
  width: number,
  hard: readonly Box[],
  inUse: readonly Box[],
  gap: number,
  height: number,
  minRoom = TOAST_MIN_ROOM,
): ToastPlace {
  if (inUse.length > 0) {
    const clear = toastRoom(corner, view, width, [...hard, ...inUse], gap, minRoom)
    if (clear && clear.maxHeight >= minRoom) return { room: clear, stepAside: false }
  }
  const room = toastRoom(corner, view, width, hard, gap, minRoom)
  const left = corner.endsWith('left') ? gap : view.w - gap - width
  const h = Math.min(height, room ? room.maxHeight : view.h - 2 * gap)
  const inset = room ? room.inset : gap
  const top = corner.startsWith('bottom') ? view.h - inset - h : inset
  // With no transmit-safe room at all there is nowhere to stand: the stylesheet's own corner may be over a stop.
  const stepAside = (room === null && hard.length > 0) || inUse.some((b) => b.left < left + width && b.right > left && b.top < top + h && b.bottom > top)
  return { room, stepAside }
}

/** A scrolling ancestor of a surface, as `scrollReach` needs it: its box in window px, the zoom from its own px to window
 *  px, and how far it is scrolled and can scroll on each axis (0 on an axis it does not scroll), in its own px. */
export interface Scroller {
  box: Box
  zoom: number
  top: number
  maxTop: number
  left: number
  maxLeft: number
}

/**
 * EVERYWHERE A SURFACE CAN BE BROUGHT BY SCROLLING, not only where it stands. The browser scrolls first and tells the page
 * after (the scroll event, a frame later, and later still for a scroll the compositor runs on its own), so a column placed
 * against where a stop control stood stood over it for a frame while the operator scrolled (CW, Phone and JS8 at 700×800,
 * 175 %, measured). Kept clear of every position its scroller can bring it to, the column is clear at every scroll
 * position, mid-scroll included, whenever it was last placed, and no scroll can make it wrong.
 *
 * Content moves by exactly the scroll and a sticky surface by no more than that, so the reach is the box grown by how far
 * each scroller can still scroll either way (up by what is left to scroll down, down by what has been scrolled), cut to the
 * scroller's own box (nothing in it is seen outside it), and never less than where it stands now.
 */
export function scrollReach(b: Box, scrollers: readonly Scroller[]): Box {
  let r = { ...b }
  for (const s of scrollers) {
    const grown = {
      left: Math.max(r.left - (s.maxLeft - s.left) * s.zoom, s.box.left),
      top: Math.max(r.top - (s.maxTop - s.top) * s.zoom, s.box.top),
      right: Math.min(r.right + s.left * s.zoom, s.box.right),
      bottom: Math.min(r.bottom + s.top * s.zoom, s.box.bottom),
    }
    if (grown.right > grown.left && grown.bottom > grown.top) {
      r = {
        left: Math.min(r.left, grown.left),
        top: Math.min(r.top, grown.top),
        right: Math.max(r.right, grown.right),
        bottom: Math.max(r.bottom, grown.bottom),
      }
    }
  }
  return r
}
