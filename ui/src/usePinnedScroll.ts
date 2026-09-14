// Bottom-pinned auto-scroll for an append-only feed (the WSJT-X Band Activity
// flow), extracted from OperateDecodes so every transcript shares ONE pin
// discipline: pinned = follow appended content; scrolled up past the slop =
// the operator is READING and the view is never yanked; scroll back to within
// PIN_SLOP_PX of the bottom to resume. Five hand-rolled copies of the
// unconditional `scrollTop = scrollHeight` snap existed before this hook —
// only OperateDecodes checked first, and the other four made scroll-back
// physically impossible while copy was flowing (which is exactly when the
// operator wants to re-read a callsign).
//
// Wiring: put `ref` and `onScroll` on the scroll container; `pinned` can drive
// a "▲ reviewing" hint; call `repin()` on an operator-initiated wipe
// (Erase/Clear) or a scope change (band/tier) so an emptied pane follows again.
//
// THE TOP EDGE (#276, Band Activity "newest on top"): `usePinnedScroll('top')` is the same
// discipline mirrored — pinned follows scrollTop 0, and scrolling DOWN past the slop is reading.
// A top-growing feed needs one thing the bottom mode does not: new rows land ABOVE the reader,
// so a view left alone slides the row being read down the screen. While reading, the hook
// therefore holds the first visible row still, found by its `data-pin-key` attribute (put one on
// each row). It anchors on the ROW, never on the height change: the pane's render window trims
// the oldest rows off the bottom in the same render, so at the cap the height delta is zero while
// the row being read still moved. The bottom mode is untouched.

import { useCallback, useLayoutEffect, useRef, useState } from 'react'

/** Stay auto-scrolled while within this many px of the bottom (scroll up
 * further than this to pause and read; scroll back down to resume). */
export const PIN_SLOP_PX = 40

export interface PinnedScroll<T extends HTMLElement> {
  /** Attach to the scroll container. */
  ref: React.RefObject<T>
  /** False while the operator has scrolled up to read (drive a hint from it). */
  pinned: boolean
  /** Attach to the scroll container's onScroll. */
  onScroll: () => void
  /** Force-follow again (wipe / scope change): pins and snaps on the next render. */
  repin: () => void
}

/** Which edge the newest content grows at. */
export type PinEdge = 'bottom' | 'top'

/** Instant jump to the top — the top-edge twin of `snapToBottom`, same `instant` reason. */
function snapToTop(el: HTMLElement) {
  if (typeof el.scrollTo === 'function') {
    el.scrollTo({ top: 0, behavior: 'instant' })
  } else {
    el.scrollTop = 0
  }
}

interface Anchor {
  key: string
  /** The row's top, relative to the container's top edge, when it was captured. */
  offset: number
}

/** The first row whose bottom is inside the viewport, and where it sits. Rects, not offsetTop,
 *  so the answer does not depend on which ancestor is the offsetParent. */
function captureAnchor(el: HTMLElement): Anchor | null {
  const top = el.getBoundingClientRect().top
  for (const row of Array.from(el.querySelectorAll<HTMLElement>('[data-pin-key]'))) {
    const r = row.getBoundingClientRect()
    if (r.bottom > top) return { key: row.getAttribute('data-pin-key') ?? '', offset: r.top - top }
  }
  return null
}

/** Put the anchored row back where the reader left it, if it is still in the feed. */
function restoreAnchor(el: HTMLElement, anchor: Anchor) {
  for (const row of Array.from(el.querySelectorAll<HTMLElement>('[data-pin-key]'))) {
    if (row.getAttribute('data-pin-key') !== anchor.key) continue
    const moved = row.getBoundingClientRect().top - el.getBoundingClientRect().top - anchor.offset
    if (moved !== 0) el.scrollTop += moved
    return
  }
}

/** Instant jump to the bottom. scrollTo({ behavior: 'instant' }) overrides any
 * CSS `scroll-behavior: smooth` on the container (.message-scroll carries one):
 * a smooth pin animates, and a wheel gesture cancels the animation mid-flight —
 * silently dropping the very pin that fired. jsdom has no Element.scrollTo,
 * hence the direct-assignment fallback (equivalent where no smooth CSS applies). */
function snapToBottom(el: HTMLElement) {
  if (typeof el.scrollTo === 'function') {
    el.scrollTo({ top: el.scrollHeight, behavior: 'instant' })
  } else {
    el.scrollTop = el.scrollHeight
  }
}

export function usePinnedScroll<T extends HTMLElement>(edge: PinEdge = 'bottom'): PinnedScroll<T> {
  const ref = useRef<T>(null)
  // Top edge only: the row the operator was reading, re-found after each render.
  const anchorRef = useRef<Anchor | null>(null)
  const edgeRef = useRef<PinEdge>(edge)
  // pinnedRef is the live value the layout effect reads; the mirrored state
  // drives the caller's "reviewing" hint.
  const pinnedRef = useRef(true)
  const [pinned, setPinned] = useState(true)

  // After EVERY render: if pinned, snap to the bottom so the newest content is
  // in view. While the operator has scrolled up, do nothing — no view yank.
  //
  // NO dependency array — deliberate and LOAD-BEARING (assessment V17): inside
  // a keep-alive host (`display:none`) scrollHeight/clientHeight read 0 and
  // this snap is a no-op, so a pinned pane must re-snap on the renders that
  // happen AFTER the host is shown again — which only an every-render effect
  // does. It also cannot be keyed on content: the hook has no knowledge of the
  // caller's data shape. Adding a dep array reintroduces the "feed frozen
  // mid-list after a view switch" bug.
  useLayoutEffect(() => {
    const el = ref.current
    if (edgeRef.current !== edge) {
      // The feed flipped direction: an anchor measured in the other order means nothing.
      edgeRef.current = edge
      anchorRef.current = null
    }
    if (!el) return
    if (edge === 'bottom') {
      if (pinnedRef.current) snapToBottom(el)
      return
    }
    if (pinnedRef.current) {
      snapToTop(el)
      return
    }
    if (anchorRef.current) restoreAnchor(el, anchorRef.current)
    anchorRef.current = captureAnchor(el)
  })

  const onScroll = () => {
    const el = ref.current
    if (!el) return
    const atEdge =
      edge === 'top'
        ? el.scrollTop <= PIN_SLOP_PX
        : el.scrollHeight - el.scrollTop - el.clientHeight <= PIN_SLOP_PX
    pinnedRef.current = atEdge
    // Reading from a top-edge feed: remember which row is under the reader's eyes.
    if (edge === 'top') anchorRef.current = atEdge ? null : captureAnchor(el)
    if (atEdge !== pinned) setPinned(atEdge)
  }

  // Stable identity so callers can use it inside effects without dep churn.
  const repin = useCallback(() => {
    pinnedRef.current = true
    setPinned(true)
  }, [])

  return { ref, pinned, onScroll, repin }
}
