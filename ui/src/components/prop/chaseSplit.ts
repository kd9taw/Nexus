// A CHASE ROW'S ENTITY TAKES A LINE OF ITS OWN WHEN ITS FIRST LINE HAS NO ROOM FOR IT (the operator's
// ruling F2, 2026-09-30). A Chase row's first line is the need chip, the call, the ↗ point button, the
// entity with its beam heading (`.chase-where`), and the age. In a Chase box about 300 px wide Chrome
// measured "South Orkney Is." cut to 47 px at 100 %, and to 0 px — gone — at 160 % (a box's own text
// size). Where the entity and its heading do not fit beside the rest, the head is stamped `data-split`
// and styles.css puts them under it; the call, the chip, the ↗ and the age stay where they are. The
// Chase Feed pane shares the row, and this.
//
// A MEASURED ATTRIBUTE, NEVER A QUERY (useRegionCols' reasons: no size-based @media, and no container
// queries). The measure cannot undo its own stamp: it compares the entity's full text width — which its
// nowrap box reports whatever width the box is given — plus its heading, with the room the head's other
// items leave on one line; those items are the same width on one line or two. It runs before paint
// after every render (a box's A+ re-renders its pane) and on every resize of the list.
import { useLayoutEffect, type RefObject } from 'react'

/** Would this head's entity and heading fit beside its other items on one line? */
export function placeFits(head: HTMLElement): boolean {
  const place = head.querySelector<HTMLElement>(':scope > .chase-where')
  if (!place) return true
  const gap = (el: Element) => parseFloat(getComputedStyle(el).columnGap) || 0
  const others = [...head.children].filter((c) => c !== place) as HTMLElement[]
  const beside = others.reduce((w, c) => w + c.offsetWidth, 0) + gap(head) * others.length
  const entity = place.querySelector<HTMLElement>('.chase-entity')
  const az = place.querySelector<HTMLElement>('.chase-az')
  const natural = (entity?.scrollWidth ?? 0) + (az ? gap(place) + az.offsetWidth : 0)
  return natural <= head.clientWidth - beside + 0.5
}

function splitHeads(list: HTMLElement): void {
  for (const head of list.querySelectorAll<HTMLElement>('.chase-head')) head.toggleAttribute('data-split', !placeFits(head))
}

/** Keep every row's `data-split` in step with its room. `shown` is whether the list is rendered. */
export function useChaseSplit(list: RefObject<HTMLElement | null>, shown: boolean): void {
  // After every render: the rows may have changed, or the box's text size.
  useLayoutEffect(() => {
    if (list.current) splitHeads(list.current)
  })
  // And on every resize of the list, which renders nothing.
  useLayoutEffect(() => {
    const el = list.current
    if (!el || typeof ResizeObserver === 'undefined') return
    const ro = new ResizeObserver(() => splitHeads(el))
    ro.observe(el)
    return () => ro.disconnect()
  }, [list, shown])
}
