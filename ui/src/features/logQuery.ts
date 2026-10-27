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
import { filterKey, type LogFilter, type LogSortTerm } from './logFilter'
import { adifNumber, contestExchange, extraTag, uploadMark, uploadOf } from './logColumns'

/** Sortable columns. `band` sorts by frequency (more meaningful than the label string). */
export type LogSortKey =
  | 'call'
  | 'country'
  | 'band'
  | 'freq'
  | 'mode'
  | 'sent'
  | 'rcvd'
  | 'time'
  | 'park'
  | 'qsl'
  | 'grid'
  | 'state'
  | 'iota'
  | 'contest'
  | 'county'
  // Every other column the Logbook shows. Each sorts as its cell reads, and in every column a row
  // with nothing in it comes after every row with something, whichever way it is sorted
  // (`sortVal` null).
  | 'end'
  | 'myGrid'
  | 'myRig'
  | 'name'
  | 'qth'
  | 'power'
  | 'operator'
  | 'cqz'
  | 'ituz'
  | 'cont'
  | 'distance'
  | 'qslSentDate'
  | 'qslRcvdDate'
  | 'lotwRcvdDate'
  | 'eqslRcvdDate'
  | 'upLotw'
  | 'upEqsl'
  | 'upQrz'
  | 'upClublog'
  | 'exchSent'
  | 'exchRcvd'
  | 'notes'

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
  /** The keys after `sort`, in order (shift-click on a header; `features/logFilter`). Absent or
   *  empty: `sort` alone, as before. */
  then?: LogSortTerm[]
  /** The field filters (`features/logFilter`). They run in the engine, in SQL, and this module's
   *  reference order below does not apply them: no window holds the log to filter it. */
  filter?: LogFilter
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

/** Text as a column sorts it: trimmed and upper-cased; null when blank. */
const textOrNull = (s: string | null | undefined): string | null => {
  const v = (s ?? '').trim()
  return v ? v.toUpperCase() : null
}

/** The longest run of digits {@link naturalKey} tells apart by length (the engine's
 *  `NATURAL_MAX_RUN`); longer runs compare as runs of this length do. */
const NATURAL_MAX_RUN = 0xcfcf

/** An exchange as its column sorts it: {@link textOrNull}, with every run of digits written so that
 *  text order is number order — a marker, the run's length without its leading zeros, then those
 *  digits. Serial 9 then sorts before 10 and 100, `05` ties `5`, and the text around the numbers
 *  orders as before. The marker is `0`, which no other character of the key can be, so a number
 *  meets a number; it sorts where a digit does, before the letters. The engine makes the same key
 *  (`query.rs` `natural_or_empty`). */
const naturalKey = (s: string | null | undefined): string | null => {
  const v = textOrNull(s)
  return v === null
    ? null
    : v.replace(/[0-9]+/g, (run) => {
        const digits = run.replace(/^0+/, '') || '0'
        return '0' + String.fromCharCode(0x30 + Math.min(digits.length, NATURAL_MAX_RUN)) + digits
      })
}

/** A number as an added column sorts it; null when there is none. */
const numOrNull = (n: number | null | undefined): number | null => (n != null && Number.isFinite(n) ? n : null)

/** A passthrough tag that holds a number (a zone, the distance): its value, or — as the cell
 *  prints it as it came — its text, which sorts after every number; null when there is none. */
const tagNum = (q: LoggedQso, tag: string): number | string | null => {
  const v = extraTag(q, tag)
  return v === null ? null : (adifNumber(v) ?? v.toUpperCase())
}

/** How much of the notes the sort reads: the first characters, so an order over a log of long
 *  notes holds a short key per contact. Contacts that agree that far keep the time order. The
 *  engine cuts at the same place (`query.rs` `NOTES_KEY_CHARS`). */
const NOTES_KEY_CHARS = 64

/** The notes as they sort: trimmed, cut to {@link NOTES_KEY_CHARS} characters, upper-cased. */
const notesKey = (s: string | null | undefined): string | null => {
  const v = (s ?? '').trim()
  if (!v) return null
  let out = ''
  let n = 0
  for (const ch of v) {
    if (n++ === NOTES_KEY_CHARS) break
    out += ch
  }
  return out.toUpperCase()
}

/** An upload mark's place: failed, waiting, done; null with no stamp. */
const UPLOAD_RANK = { failed: 0, waiting: 1, done: 2, none: null } as const

/** A row's value in a sorted column. Null is "nothing here", which `logOrder` puts last in either
 *  direction, in every column. The band, frequency, time and QSL mark always hold a value. */
export function sortVal(q: LoggedQso, k: LogSortKey): string | number | null {
  switch (k) {
    case 'call':
      return textOrNull(q.call)
    case 'country':
      // The country the Logbook shows: the DXCC entity, so a QRZ "Germany" and a "Fed. Rep. of
      // Germany" sort together, and the stored text only where the call resolves to none.
      return textOrNull(q.entity ?? q.country)
    case 'band':
    case 'freq':
      return q.freqMhz
    case 'mode':
      return textOrNull(q.mode)
    case 'sent':
      return textOrNull(q.rstSent)
    case 'rcvd':
      return textOrNull(q.rstRcvd)
    case 'time':
      return q.whenUnix
    case 'park':
      return textOrNull(q.ota?.theirRef ?? q.ota?.myRef)
    case 'qsl':
      return q.awardConfirmed ? 2 : q.confirmed ? 1 : 0
    case 'grid':
      return textOrNull(q.grid)
    case 'state':
      return textOrNull(q.state)
    case 'iota':
      return textOrNull(q.ota?.iota)
    case 'contest':
      return textOrNull(q.contest?.contestId)
    case 'county':
      // A tag the log keeps only as passthrough (ADIF `CNTY`), as the import spelled it.
      return textOrNull(q.extra?.find(([k]) => k === 'CNTY')?.[1])
    case 'end':
      return numOrNull(q.timeOffUnix)
    case 'myGrid':
      return textOrNull(q.myGrid)
    case 'myRig':
      return textOrNull(q.myRig)
    case 'name':
      return textOrNull(q.name)
    case 'qth':
      return textOrNull(q.qth)
    case 'power':
      return numOrNull(q.txPower)
    case 'operator':
      return textOrNull(q.operator)
    case 'cqz':
      return tagNum(q, 'CQZ')
    case 'ituz':
      return tagNum(q, 'ITUZ')
    case 'cont':
      return textOrNull(extraTag(q, 'CONT'))
    case 'distance':
      return tagNum(q, 'DISTANCE')
    case 'qslSentDate':
      return q.qslSent?.dateUnix ? q.qslSent.dateUnix : null
    // `YYYYMMDD` orders by date as text does.
    case 'qslRcvdDate':
      return textOrNull(extraTag(q, 'QSLRDATE'))
    case 'lotwRcvdDate':
      return textOrNull(extraTag(q, 'LOTW_QSLRDATE'))
    case 'eqslRcvdDate':
      return textOrNull(extraTag(q, 'EQSL_QSLRDATE'))
    case 'upLotw':
    case 'upEqsl':
    case 'upQrz':
    case 'upClublog':
      return UPLOAD_RANK[uploadMark(uploadOf(q, k))]
    case 'exchSent':
      return naturalKey(contestExchange(q, 'sent'))
    case 'exchRcvd':
      return naturalKey(contestExchange(q, 'rcvd'))
    case 'notes':
      return notesKey(q.comment?.trim() || q.notes)
  }
}

/** Columns that open newest first, as the contact time does. */
const NEWEST_FIRST: ReadonlySet<LogSortKey> = new Set<LogSortKey>(['end', 'qslSentDate', 'qslRcvdDate', 'lotwRcvdDate', 'eqslRcvdDate'])

/** Sensible default direction when switching TO a column: text ascending, the band, frequency,
 *  QSL state and every time and date descending. */
export function defaultAsc(k: LogSortKey): boolean {
  return k !== 'band' && k !== 'freq' && k !== 'time' && k !== 'qsl' && !NEWEST_FIRST.has(k)
}

/** Whether the Logbook shows `q` under `query` — Logbook.tsx's `matchesSearch`, verbatim. */
export function matchesLogQuery(q: LoggedQso, query: LogQuery): boolean {
  if (query.needsConfirmOnly && q.awardConfirmed) return false
  const t = query.search.trim().toLowerCase()
  if (!t) return true
  return (
    q.call.toLowerCase().includes(t) ||
    (q.country?.toLowerCase().includes(t) ?? false) ||
    // …and the country the Logbook shows, the DXCC entity, so a QRZ "Germany" row is found by
    // "Fed. Rep. of Germany" as well as by its stored text.
    (q.entity?.toLowerCase().includes(t) ?? false) ||
    (q.grid?.toLowerCase().includes(t) ?? false) ||
    q.band.toLowerCase().includes(t) ||
    q.mode.toLowerCase().includes(t) ||
    // …and under the name the operator thinks in. A phone contact carries the sideband
    // it was worked on, so searching "ssb" would otherwise miss every USB/LSB row —
    // Nexus's own phone contacts, and any imported from a logger that spells it that
    // way. The raw spelling above still matches, so "usb" finds the USB rows alone.
    modeKey(q.mode).toLowerCase().includes(t) ||
    // …and in its SUBMODE. The log holds a phone contact as SSB with its sideband as the
    // SUBMODE, so "usb" and "lsb" find those contacts there, as they find a VarAC contact's
    // "VARA HF" under its DYNAMIC mode.
    (q.extra?.find(([k]) => k === 'SUBMODE')?.[1].toLowerCase().includes(t) ?? false) ||
    fmtUtc(q.whenUnix).toLowerCase().includes(t)
  )
}

/** The rows `query` shows, as LOG POSITIONS in display order — the order vector (SPEC-2 v3 §4.3).
 *  Logbook.tsx's filter-then-sort `useMemo`, verbatim, returning positions instead of `{q, i}`. */
export function logOrder(log: readonly LoggedQso[], query: LogQuery): number[] {
  const out = log.map((q, i) => ({ q, i })).filter(({ q }) => matchesLogQuery(q, query))
  const keys = [{ sort: query.sort, asc: query.asc }, ...(query.then ?? [])]
  out.sort((a, b) => {
    // Each key in its own direction; the time breaks a full tie in the FIRST key's direction, so
    // one key alone orders exactly as it always did.
    for (const k of keys) {
      const av = sortVal(a.q, k.sort)
      const bv = sortVal(b.q, k.sort)
      // Nothing in the column: after every value, in either direction.
      if (av === null || bv === null) {
        if (av !== bv) return av === null ? 1 : -1
        continue
      }
      // A column of numbers: a value that is not one after every number, in either direction.
      if (typeof av !== typeof bv) return typeof av === 'number' ? -1 : 1
      const cmp = av < bv ? -1 : av > bv ? 1 : 0
      if (cmp) return k.asc ? cmp : -cmp
    }
    const t = a.q.whenUnix - b.q.whenUnix
    return query.asc ? t : -t
  })
  return out.map(({ i }) => i)
}

/** A stable text key for a query — for caches and for telling one view's answers from another's. */
export function logQueryKey(query: LogQuery): string {
  return `${query.sort}|${query.asc ? 'asc' : 'desc'}|${query.needsConfirmOnly ? 'unconfirmed' : 'all'}|${query.search}${extraKey(query)}`
}

/** The sort keys after the first and the filter, as a key's tail — empty when there are none, so
 *  every key a single-column query had is unchanged. */
export function extraKey(query: LogQuery): string {
  const then = (query.then ?? []).map((t) => `${t.sort}:${t.asc ? 'asc' : 'desc'}`).join(',')
  const f = filterKey(query.filter)
  return then || f ? `|${then}|${f}` : ''
}
