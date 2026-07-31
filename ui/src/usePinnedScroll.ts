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

export function usePinnedScroll<T extends HTMLElement>(): PinnedScroll<T> {
  const ref = useRef<T>(null)
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
    if (el && pinnedRef.current) snapToBottom(el)
  })

  const onScroll = () => {
    const el = ref.current
    if (!el) return
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight <= PIN_SLOP_PX
    pinnedRef.current = atBottom
    if (atBottom !== pinned) setPinned(atBottom)
  }

  // Stable identity so callers can use it inside effects without dep churn.
  const repin = useCallback(() => {
    pinnedRef.current = true
    setPinned(true)
  }, [])

  return { ref, pinned, onScroll, repin }
}
