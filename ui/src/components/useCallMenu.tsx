// ONE CALL MENU EVERYWHERE: right-click a call (or press the Menu key, or Shift+F10, on it) on any list
// that draws calls, and the same menu opens as on the Call Roster and Band Activity (CallRowMenu): Best
// times first, then watch and hide. A list with no menu of its own takes this hook:
//
//     const menu = useCallMenu()
//     <tr {...menu.on(key, call, { band, country })}> <td>{menu.at(key)}{call}</td> … </tr>
//
// `on` gives the row its right-click and menu keys; `at` draws the menu's anchor, inside the element
// the card should stand beside, only while that row's menu is open. Opening it never selects, tunes
// or works the row: the right-click is the menu's alone (its default and its propagation stop here),
// and the anchor stops every event from inside the menu (CallRowMenu). On close the keyboard goes back
// to the row it was opened from.
import { useCallback, useRef, useState, type KeyboardEvent, type MouseEvent, type ReactNode } from 'react'
import { CallRowMenu, isMenuKey } from './CallRowMenu'
import { lookupCall } from '../callsign'

interface Open {
  key: string
  call: string
  band?: string | null
  country?: string | null
  from: HTMLElement
}

export interface CallMenu {
  /** The row's right-click and menu keys. `extraKeyDown` is the row's own key handler, run first. */
  on: (
    key: string,
    call: string,
    opts?: { band?: string | null; country?: string | null; extraKeyDown?: (e: KeyboardEvent<HTMLElement>) => void },
  ) => { onContextMenu: (e: MouseEvent<HTMLElement>) => void; onKeyDown: (e: KeyboardEvent<HTMLElement>) => void }
  /** The menu's anchor for row `key`, while its menu is open; else nothing. */
  at: (key: string) => ReactNode
  /** Open a row's menu from code (a ⋯ button). */
  open: (key: string, call: string, from: HTMLElement, opts?: { band?: string | null; country?: string | null }) => void
}

export function useCallMenu(): CallMenu {
  const [open, setOpen] = useState<Open | null>(null)
  /** The row the open menu came from, for the keyboard to go back to. */
  const from = useRef<HTMLElement | null>(null)
  const start = useCallback((key: string, call: string, el: HTMLElement, opts?: { band?: string | null; country?: string | null }) => {
    const cu = call.trim().toUpperCase()
    // An unresolved hash names no station: nothing to offer.
    if (!lookupCall(cu)) return
    from.current = el
    setOpen({ key, call: cu, band: opts?.band, country: opts?.country, from: el })
  }, [])
  const close = useCallback(() => {
    const row = from.current
    from.current = null
    setOpen(null)
    // The project's rule for programmatic focus: never scroll the page to it, then bring the row
    // into view only as far as it needs (a list may have scrolled while the menu was open).
    if (row?.isConnected) {
      row.focus({ preventScroll: true })
      row.scrollIntoView?.({ block: 'nearest' })
    }
  }, [])
  const on: CallMenu['on'] = (key, call, opts) => ({
    onContextMenu: (e) => {
      e.preventDefault()
      e.stopPropagation()
      start(key, call, e.currentTarget, opts)
    },
    onKeyDown: (e) => {
      opts?.extraKeyDown?.(e)
      if (e.defaultPrevented || !isMenuKey(e)) return
      e.preventDefault()
      e.stopPropagation()
      start(key, call, e.currentTarget, opts)
    },
  })
  const at = (key: string) =>
    open && open.key === key ? <CallRowMenu call={open.call} band={open.band} country={open.country} onClose={close} /> : null
  return { on, at, open: start }
}
