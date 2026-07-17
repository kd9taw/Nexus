// Roving-tabindex keyboard navigation for a list/grid of rows (WAI-ARIA APG
// pattern) — the keyboard path for the decode/roster/needed lists a blind
// operator must reach. ARIA alone adds no behavior; this hook wires it.
//
// The list is ONE Tab stop; ArrowUp/Down (and Home/End) move focus between
// rows without leaving the list; Enter/Space activate the focused row. A row is
// the tab stop when its index === active (all others tabIndex=-1). Focus is
// moved imperatively to the active row's DOM node when the user arrows, so the
// screen reader announces each row as it lands.
//
// Mouse and keyboard stay in sync: clicking a row (or the list re-sorting under
// a stable selection) can set `active` from outside via `setActive`.

import { useCallback, useEffect, useRef, useState } from 'react'

export interface RovingList {
  /** Index that currently holds the tab stop (-1 = none focused yet). */
  active: number
  /** Props for the container element (role + keydown). */
  containerProps: {
    onKeyDown: (e: React.KeyboardEvent) => void
  }
  /** Props for row `i` — tabIndex, ref registration, and pointer sync. */
  rowProps: (i: number) => {
    tabIndex: number
    ref: (el: HTMLElement | null) => void
    onFocus: () => void
    onClick: () => void
  }
  setActive: (i: number) => void
}

/**
 * @param count   number of rows currently rendered
 * @param onActivate called on Enter/Space with the row index + whether Shift/Alt were held
 */
export function useRovingList(
  count: number,
  onActivate: (index: number, mods: { shift: boolean; alt: boolean }) => void,
): RovingList {
  const [active, setActive] = useState(-1)
  const rows = useRef<(HTMLElement | null)[]>([])
  // Set when a keyboard move should also pull DOM focus (a pointer-driven
  // active change must NOT steal focus back to the list).
  const focusWanted = useRef(false)

  // Keep the tab stop valid as the list shrinks/grows.
  useEffect(() => {
    if (active >= count) setActive(count - 1)
  }, [count, active])

  useEffect(() => {
    if (focusWanted.current && active >= 0) {
      rows.current[active]?.focus()
      focusWanted.current = false
    }
  }, [active])

  const move = useCallback(
    (to: number) => {
      const n = Math.max(0, Math.min(count - 1, to))
      focusWanted.current = true
      setActive(n)
    },
    [count],
  )

  const onKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (count === 0) return
      const cur = active < 0 ? 0 : active
      switch (e.key) {
        case 'ArrowDown':
          e.preventDefault()
          move(cur + 1)
          break
        case 'ArrowUp':
          e.preventDefault()
          move(cur - 1)
          break
        case 'Home':
          e.preventDefault()
          move(0)
          break
        case 'End':
          e.preventDefault()
          move(count - 1)
          break
        case 'Enter':
        case ' ':
          if (active >= 0) {
            e.preventDefault()
            onActivate(active, { shift: e.shiftKey, alt: e.altKey })
          }
          break
      }
    },
    [active, count, move, onActivate],
  )

  const rowProps = useCallback(
    (i: number) => ({
      // Exactly one row is tabbable (the active one, or the first when none is
      // yet chosen) so Tab reaches the list in a single stop.
      tabIndex: i === (active < 0 ? 0 : active) ? 0 : -1,
      ref: (el: HTMLElement | null) => {
        rows.current[i] = el
      },
      onFocus: () => {
        if (active !== i) setActive(i)
      },
      onClick: () => setActive(i),
    }),
    [active],
  )

  return { active, containerProps: { onKeyDown }, rowProps, setActive }
}
