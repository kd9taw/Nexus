// #346 — THE ROW MENU OF THE CALL ROSTER AND BAND ACTIVITY: right-click a station's row (or press
// the Menu key, or Shift+F10, on the focused row) for what can be done about that one call:
//   · put it on the watch list, or take it off (the entry that names exactly this call);
//   · hide it on this band, or on every band, for an hour, two hours, the rest of the UTC day or
//     until it is brought back (features/hideCalls: the chip's popover lists each, with a ✕);
//   · hide every station from its country (the country exclusion, whose chip undoes it).
//
// DISPLAY ONLY. Hiding here is the "I don't want to look at it" list, never the block list the
// auto-responder honours (Alt-double-click): nothing in this menu changes who Nexus answers or
// what it transmits. The watch list is the one the Settings manager edits, written the same way.
//
// The menu is Radix's, portalled out of the pane (the pane clips), re-applying --ui-zoom like every
// portalled menu. Its anchor is a zero-size span the row renders only while the menu is open, so a
// closed menu costs a row nothing. React events from inside a portal bubble through the React tree,
// so the anchor stops them: a click or a key in the menu must never reach the row (select / work)
// or the list's roving keys.
//
// ⚠️ Migrated from birth (i18n/hardcoded-strings.test.ts): every word arrives from the catalog; the
// callsign, the band and the country are data.
import * as RM from '@radix-ui/react-dropdown-menu'
import { useCallback, useEffect, useRef, useState } from 'react'
import { t } from '../i18n'
import { Entries, type Js8MenuEntry } from './Js8ListMenu'
import { addCallToWatchlist, exactCallEntry, loadWatchlist, removeWatchEntry } from '../watchlist'
import { endOfUtcDay, hideCall } from '../features/hideCalls'
import { useCountryExclude } from '../features/countryExclude'
import { openBestTimesCard } from './BestTimesCard'

const HOUR_MS = 3600_000

/** True for the keys that open a row's menu from the keyboard: the Menu key and Shift+F10. */
export function isMenuKey(e: { key: string; shiftKey: boolean }): boolean {
  return e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')
}

const stop = (e: { stopPropagation: () => void }) => e.stopPropagation()

/**
 * Where the keyboard goes when a row's menu closes: back to the row, or — when the choice took the
 * row away (a hide) — to the row that now stands in its place, else the last row, else the list
 * itself. Without this, focus fell to <body> after "Hide…" and a keyboard user lost their place.
 *
 * `remember(row)` when the menu OPENS (the row, its list and its place in it — by the time the menu
 * closes a hide may already have taken the row out of the list); `returnFocus()` when it closes. The
 * move happens after the render that follows, when it is known whether the row is still there.
 * `rowSelector` picks the list's rows inside the row's parent; the parent must be focusable
 * (tabIndex -1) for the empty-list case.
 */
export function useMenuFocusReturn(rowSelector: string): { remember: (row: HTMLElement) => void; returnFocus: () => void } {
  const opened = useRef<{ row: HTMLElement; list: HTMLElement | null; index: number } | null>(null)
  const pending = useRef(false)
  const [asked, setAsked] = useState(0)
  useEffect(() => {
    const p = opened.current
    if (!pending.current || !p) return
    pending.current = false
    opened.current = null
    let target: HTMLElement | null = p.row.isConnected ? p.row : null
    if (!target && p.list?.isConnected) {
      const rows = [...p.list.querySelectorAll<HTMLElement>(rowSelector)]
      target = rows[Math.min(p.index, rows.length - 1)] ?? p.list
    }
    target?.focus({ preventScroll: true })
    target?.scrollIntoView?.({ block: 'nearest' })
  }, [asked, rowSelector])
  const remember = useCallback(
    (row: HTMLElement) => {
      const list = row.parentElement
      const index = list ? [...list.querySelectorAll(rowSelector)].indexOf(row) : 0
      opened.current = { row, list, index: Math.max(0, index) }
    },
    [rowSelector],
  )
  const returnFocus = useCallback(() => {
    pending.current = true
    setAsked((n) => n + 1)
  }, [])
  return { remember, returnFocus }
}

export function CallRowMenu({
  call,
  band,
  country,
  onClose,
}: {
  call: string
  /** The band the row was heard on; unknown = no "on this band" choices. */
  band?: string | null
  /** The station's DXCC entity, for "hide its country"; unknown = no such choice. */
  country?: string | null
  /** The menu closed (chosen, Escape, or a click away). The host clears its open row and puts the
   *  keyboard back on the row. */
  onClose: () => void
}) {
  const countries = useCountryExclude()
  const anchor = useRef<HTMLSpanElement>(null)
  const entries = (): Js8MenuEntry[] => {
    // Each end is worked out when the item is CHOSEN, not when the menu opened.
    const durations = (scope: string | null): Js8MenuEntry[] => [
      { kind: 'item', label: t('callMenu.hide.hour'), onSelect: () => hideCall(call, scope, Date.now() + HOUR_MS) },
      { kind: 'item', label: t('callMenu.hide.twoHours'), onSelect: () => hideCall(call, scope, Date.now() + 2 * HOUR_MS) },
      { kind: 'item', label: t('callMenu.hide.today'), onSelect: () => hideCall(call, scope, endOfUtcDay(Date.now())) },
      { kind: 'item', label: t('callMenu.hide.always'), onSelect: () => hideCall(call, scope, null) },
    ]
    const exact = exactCallEntry(loadWatchlist(), call)
    // The row the menu was opened on, for the card to stand beside (the anchor sits inside it).
    const row = () => anchor.current?.parentElement ?? document.body
    const out: Js8MenuEntry[] = [
      { kind: 'heading', label: call },
      // Best times first, on every call menu: it opens a card beside the row and selects, tunes and
      // works nothing (the anchor below keeps the choice from reaching the row).
      { kind: 'item', label: t('callMenu.bestTimes'), onSelect: () => openBestTimesCard(call, row()) },
      { kind: 'sep' },
      exact
        ? { kind: 'item', label: t('callMenu.watch.remove'), onSelect: () => removeWatchEntry(exact.id) }
        : { kind: 'item', label: t('callMenu.watch.add'), onSelect: () => addCallToWatchlist(call) },
      { kind: 'sep' },
    ]
    if (band) out.push({ kind: 'sub', label: t('callMenu.hide.band', { band }), entries: durations(band) })
    out.push({ kind: 'sub', label: t('callMenu.hide.allBands'), entries: durations(null) })
    // A country already in the exclusion (shown only because the exclusion is paused, or because a
    // need outranks it) gets no item: the country picker's toggle would take it OUT again.
    if (country && !countries.hidden.has(country) && !countries.entities.has(country))
      out.push({ kind: 'item', label: t('callMenu.hide.country', { country }), onSelect: () => countries.toggleEntity(country) })
    return out
  }
  return (
    <span ref={anchor} className="call-row-menu-anchor" onClick={stop} onDoubleClick={stop} onKeyDown={stop} onContextMenu={stop}>
      <RM.Root open onOpenChange={(open) => !open && onClose()}>
        <RM.Trigger asChild>
          <span aria-hidden="true" />
        </RM.Trigger>
        <RM.Portal>
          <RM.Content
            className="ui-menu box-menu call-row-menu"
            aria-label={t('callMenu.aria', { call })}
            sideOffset={2}
            align="start"
            collisionPadding={8}
            // The anchor is not focusable; the host puts the keyboard back on the row.
            onCloseAutoFocus={(e) => e.preventDefault()}
          >
            <div style={{ zoom: 'var(--ui-zoom, 1)' }}>
              <Entries entries={entries()} />
            </div>
          </RM.Content>
        </RM.Portal>
      </RM.Root>
    </span>
  )
}
