// THE LOGBOOK'S ORDER AND ITS SEARCH — the meaning every source of Logbook pages must reproduce
// exactly (SPEC-2 v2 §2, carried unchanged into v3 §4.3).
//
// Lifted VERBATIM out of Logbook.tsx, where it was the body of a `useMemo` over the window's copy of
// the whole log. It lives on its own so that one copy serves three readers: the Logbook (its header
// click needs `defaultAsc`, its rows `fmtUtc`), the reference answers (`logAnswers.ts`
// `answerFrom`), and the goldens in `__fixtures__/log-query/` that the engine's port
// of this code is held to. The goldens are generated FROM this module, so a change here that alters
// an order fails them — that is the point of them.
//
// What the code guarantees, stated because an SQL rewrite breaks each of these silently:
//   - strings compare as JS compares them: `toUpperCase()` over all of Unicode, then UTF-16 code
//     units (`<`), not a collation and not an ASCII-only fold;
//   - equal keys fall back to the contact time (`whenUnix`), and that tie-break FLIPS with the
//     direction, because the whole comparison is negated for descending;
//   - full ties keep log order: `Array.prototype.sort` is stable (ES2019);
//   - the search lower-cases `fmtUtc(whenUnix)`, whose last character is `Z`, so the search "z"
//     matches every row. A SQL filter that skips the formatted time finds a fraction of them.

import type { LoggedQso } from '../types'
import { modeKey } from './callHistory'

/** Sortable columns. `band` sorts by frequency (more meaningful than the label string). */
export type LogSortKey = 'call' | 'country' | 'band' | 'freq' | 'mode' | 'sent' | 'rcvd' | 'time' | 'park' | 'qsl'

/** What the Logbook list shows: the rows the search and the filter chip let through, in the
 *  order the sorted column gives. The same four fields identify an order everywhere — the view,
 *  a page, a cached order, a golden. */
export interface LogQuery {
  sort: LogSortKey
  asc: boolean
  /** The search box as typed: trimmed and lower-cased HERE, never by the caller. */
  search: string
  /** Only contacts still lacking an award-eligible confirmation (the DX chaser's chip). */
  needsConfirmOnly: boolean
}

/** Newest first, nothing filtered: the view the Logbook opens on. */
export const DEFAULT_LOG_QUERY: LogQuery = { sort: 'time', asc: false, search: '', needsConfirmOnly: false }

export function fmtUtc(whenUnix: number): string {
  const d = new Date(whenUnix * 1000)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getUTCFullYear()}-${p(d.getUTCMonth() + 1)}-${p(d.getUTCDate())} ${p(
    d.getUTCHours(),
  )}:${p(d.getUTCMinutes())}Z`
}

export function sortVal(q: LoggedQso, k: LogSortKey): string | number {
  switch (k) {
    case 'call':
      return q.call.toUpperCase()
    case 'country':
      return (q.country ?? '').toUpperCase()
    case 'band':
    case 'freq':
      return q.freqMhz
    case 'mode':
      return q.mode.toUpperCase()
    case 'sent':
      return (q.rstSent ?? '').toUpperCase()
    case 'rcvd':
      return (q.rstRcvd ?? '').toUpperCase()
    case 'time':
      return q.whenUnix
    case 'park':
      return (q.ota?.theirRef ?? q.ota?.myRef ?? '').toUpperCase()
    case 'qsl':
      return q.awardConfirmed ? 2 : q.confirmed ? 1 : 0
  }
}

/** Sensible default direction when switching TO a column: text ascending, numeric/time descending. */
export function defaultAsc(k: LogSortKey): boolean {
  return k === 'call' || k === 'country' || k === 'mode' || k === 'sent' || k === 'rcvd' || k === 'park'
}

/** Whether the Logbook shows `q` under `query` — Logbook.tsx's `matchesSearch`, verbatim. */
export function matchesLogQuery(q: LoggedQso, query: LogQuery): boolean {
  if (query.needsConfirmOnly && q.awardConfirmed) return false
  const t = query.search.trim().toLowerCase()
  if (!t) return true
  return (
    q.call.toLowerCase().includes(t) ||
    (q.country?.toLowerCase().includes(t) ?? false) ||
    (q.grid?.toLowerCase().includes(t) ?? false) ||
    q.band.toLowerCase().includes(t) ||
    q.mode.toLowerCase().includes(t) ||
    // …and under the name the operator thinks in. A phone contact carries the sideband
    // it was worked on, so searching "ssb" would otherwise miss every USB/LSB row —
    // Nexus's own phone contacts, and any imported from a logger that spells it that
    // way. The raw spelling above still matches, so "usb" finds the USB rows alone.
    modeKey(q.mode).toLowerCase().includes(t) ||
    fmtUtc(q.whenUnix).toLowerCase().includes(t)
  )
}

/** The rows `query` shows, as LOG POSITIONS in display order — the order vector (SPEC-2 v3 §4.3).
 *  Logbook.tsx's filter-then-sort `useMemo`, verbatim, returning positions instead of `{q, i}`. */
export function logOrder(log: readonly LoggedQso[], query: LogQuery): number[] {
  const out = log.map((q, i) => ({ q, i })).filter(({ q }) => matchesLogQuery(q, query))
  out.sort((a, b) => {
    const av = sortVal(a.q, query.sort)
    const bv = sortVal(b.q, query.sort)
    const cmp = av < bv ? -1 : av > bv ? 1 : a.q.whenUnix - b.q.whenUnix
    return query.asc ? cmp : -cmp
  })
  return out.map(({ i }) => i)
}

/** A stable text key for a query — for caches and for telling one view's answers from another's. */
export function logQueryKey(query: LogQuery): string {
  return `${query.sort}|${query.asc ? 'asc' : 'desc'}|${query.needsConfirmOnly ? 'unconfirmed' : 'all'}|${query.search}`
}
