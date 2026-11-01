// THE LOGBOOK'S FIELD FILTERS, ITS SAVED FILTERS AND ITS SORT KEYS — the shapes the window builds
// and the engine answers.
//
// The filters run in the ENGINE, in SQL against the log's store (`tempo_core::logbook::query::
// filter` and `sqlite/filter_sql.rs`); the window never holds the log, so it never filters it. What
// lives here is what the window does with a filter before it asks: build it, tidy it (a field left
// empty is no filter), name it for caches, and keep it in `settings.json` under a name.
//
// The one search box (`LogQuery.search`) is unchanged and combines with these: a row is shown when
// it passes the search, the chip, and every filter set here.
//
// ⚠️ A saved filter comes back from `settings.json`, which an older or newer Nexus may have
// written, so it is read through `cleanFilter`: unknown keys and wrong types are dropped rather
// than sent, because the engine refuses a query with a field it does not know.

import type { LogSortKey } from './logQuery'

/** What a QSL service holds for a contact. */
export type QslState = 'confirmed' | 'sent' | 'notSent'
/** Club Log takes uploads and holds no confirmation, so it is sent or not. */
export type SentState = 'sent' | 'notSent'

export interface QslFilter {
  lotw?: QslState
  eqsl?: QslState
  card?: QslState
  qrz?: QslState
  clublog?: SentState
}

/** The awards a contact can still be needed for on its own band: worked there, not confirmed. */
export type AwardNeed = 'dxcc' | 'was' | 'waz' | 'vucc' | 'ffma'

/** The path a contact took, as the Path column grades it: the long path, or across the polar cap. */
export type PathFilter = 'long' | 'polar'

/** The Logbook's field filters. Every field is optional and they combine with AND. */
export interface LogFilter {
  /** A call prefix (`K1`), or a pattern with `*` and `?` (`K?ABC`, or `*` then `/P` for a portable). */
  call?: string
  band?: string
  mode?: string
  /** UTC instants, seconds: from inclusive, to exclusive. */
  fromUnix?: number
  toUnix?: number
  /** The DXCC entity the Logbook shows for the contact. */
  entity?: string
  continent?: string
  cqZone?: number
  ituZone?: number
  /** A grid prefix (`FN`, `FN31`). */
  grid?: string
  state?: string
  /** A county as ADIF writes it (`MA,MIDDLESEX`), matched from its start. */
  county?: string
  iota?: string
  /** A park, summit or WWFF reference, theirs or yours. */
  reference?: string
  /** One of YOUR references, whole: a contact whose park list (one, or a two-fer's) holds it. With
   *  `fromUnix`, what an activation's contacts are (`features/potaActivation` `activationFilter`). */
  myRef?: string
  qsl?: QslFilter
  contest?: string
  need?: AwardNeed
  /** Words in the comment, the name or the QTH. */
  text?: string
  /** The path the contact's grade names (its own, else the Logbook's display-only grade). */
  path?: PathFilter
}

/** One extra sort key after the first. */
export interface LogSortTerm {
  sort: LogSortKey
  asc: boolean
}

/** How many columns may order the list at once: the first, and two more. */
export const MAX_SORT_KEYS = 3

export const QSL_SERVICES = ['lotw', 'eqsl', 'card', 'qrz', 'clublog'] as const
export type QslService = (typeof QSL_SERVICES)[number]
export const AWARD_NEEDS: readonly AwardNeed[] = ['dxcc', 'was', 'waz', 'vucc', 'ffma']
export const PATH_FILTERS: readonly PathFilter[] = ['long', 'polar']
export const CONTINENTS = ['AF', 'AN', 'AS', 'EU', 'NA', 'OC', 'SA'] as const

const TEXT_FIELDS = ['call', 'band', 'mode', 'entity', 'continent', 'grid', 'state', 'county', 'iota', 'reference', 'myRef', 'contest', 'text'] as const
const QSL_STATES: readonly string[] = ['confirmed', 'sent', 'notSent']
const SENT_STATES: readonly string[] = ['sent', 'notSent']

const isWhole = (v: unknown, lo: number, hi: number): v is number =>
  typeof v === 'number' && Number.isInteger(v) && v >= lo && v <= hi

/**
 * `raw` as a filter the engine will take: unknown keys and wrong types dropped, every text
 * trimmed, empty fields removed. Anything that is not an object reads as no filter.
 */
export function cleanFilter(raw: unknown): LogFilter {
  if (!raw || typeof raw !== 'object') return {}
  const r = raw as Record<string, unknown>
  const out: LogFilter = {}
  for (const k of TEXT_FIELDS) {
    const v = r[k]
    if (typeof v === 'string' && v.trim()) out[k] = v.trim()
  }
  if (out.continent !== undefined && !(CONTINENTS as readonly string[]).includes(out.continent.toUpperCase())) {
    delete out.continent
  }
  if (out.continent) out.continent = out.continent.toUpperCase()
  if (isWhole(r.fromUnix, 0, Number.MAX_SAFE_INTEGER)) out.fromUnix = r.fromUnix
  if (isWhole(r.toUnix, 0, Number.MAX_SAFE_INTEGER)) out.toUnix = r.toUnix
  if (isWhole(r.cqZone, 1, 40)) out.cqZone = r.cqZone
  if (isWhole(r.ituZone, 1, 90)) out.ituZone = r.ituZone
  if (typeof r.need === 'string' && (AWARD_NEEDS as readonly string[]).includes(r.need)) out.need = r.need as AwardNeed
  if (typeof r.path === 'string' && (PATH_FILTERS as readonly string[]).includes(r.path)) out.path = r.path as PathFilter
  if (r.qsl && typeof r.qsl === 'object') {
    const q = r.qsl as Record<string, unknown>
    const qsl: QslFilter = {}
    for (const s of QSL_SERVICES) {
      const v = q[s]
      if (typeof v !== 'string') continue
      if (s === 'clublog') {
        if (SENT_STATES.includes(v)) qsl.clublog = v as SentState
      } else if (QSL_STATES.includes(v)) {
        qsl[s] = v as QslState
      }
    }
    if (Object.keys(qsl).length) out.qsl = qsl
  }
  return out
}

/** How many fields the filter sets — the count the Filters button carries. */
export function activeFilterCount(f: LogFilter): number {
  const c = cleanFilter(f)
  const { qsl, ...rest } = c
  return Object.keys(rest).length + (qsl ? Object.keys(qsl).length : 0)
}

export function isEmptyFilter(f: LogFilter | undefined): boolean {
  return !f || activeFilterCount(f) === 0
}

/** A stable text key for a filter: its cleaned fields in a fixed order, so the same filter typed
 *  in a different order, or echoed back by the engine, names the same cached order. */
export function filterKey(f: LogFilter | undefined): string {
  if (!f) return ''
  const c = cleanFilter(f)
  const parts: string[] = []
  for (const k of Object.keys(c).sort()) {
    if (k === 'qsl') {
      const q = c.qsl as Record<string, string>
      parts.push(`qsl=${Object.keys(q).sort().map((s) => `${s}:${q[s]}`).join(',')}`)
    } else {
      parts.push(`${k}=${String((c as Record<string, unknown>)[k])}`)
    }
  }
  return parts.join(';')
}

/** A `YYYY-MM-DD` date as the UTC instant its day starts, or undefined for anything else. */
export function dayStartUnix(date: string): number | undefined {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(date.trim())
  if (!m) return undefined
  const ms = Date.UTC(Number(m[1]), Number(m[2]) - 1, Number(m[3]))
  return Number.isFinite(ms) ? Math.floor(ms / 1000) : undefined
}

/** The instant as the date inputs show it (`YYYY-MM-DD`, UTC). */
export function unixDay(unix: number | undefined): string {
  if (unix === undefined) return ''
  return new Date(unix * 1000).toISOString().slice(0, 10)
}

// ── saved filters (`settings.json` → `logbookFilters`) ─────────────────────────────────────────

export interface SavedLogFilter {
  name: string
  filter: LogFilter
}

/** The longest a saved filter's name may be. */
export const MAX_FILTER_NAME = 60

/** The saved filters as `settings.json` holds them, made safe to use: each a trimmed, unique,
 *  non-empty name with a cleaned filter; anything else dropped. */
export function cleanSavedFilters(raw: unknown): SavedLogFilter[] {
  if (!Array.isArray(raw)) return []
  const seen = new Set<string>()
  const out: SavedLogFilter[] = []
  for (const item of raw) {
    if (!item || typeof item !== 'object') continue
    const { name, filter } = item as { name?: unknown; filter?: unknown }
    if (typeof name !== 'string') continue
    const n = name.trim().slice(0, MAX_FILTER_NAME)
    if (!n || seen.has(n.toLowerCase())) continue
    seen.add(n.toLowerCase())
    out.push({ name: n, filter: cleanFilter(filter) })
  }
  return out
}

/** The list with `filter` kept under `name`: in place of a filter of the same name (any case),
 *  else at the end. */
export function withSavedFilter(list: readonly SavedLogFilter[], name: string, filter: LogFilter): SavedLogFilter[] {
  const n = name.trim().slice(0, MAX_FILTER_NAME)
  if (!n) return [...list]
  const entry = { name: n, filter: cleanFilter(filter) }
  const at = list.findIndex((s) => s.name.toLowerCase() === n.toLowerCase())
  return at < 0 ? [...list, entry] : list.map((s, i) => (i === at ? entry : s))
}

/** The list with `from` renamed `to`. Refused (the list unchanged) when `to` is empty or names
 *  another saved filter. */
export function withFilterRenamed(list: readonly SavedLogFilter[], from: string, to: string): SavedLogFilter[] {
  const n = to.trim().slice(0, MAX_FILTER_NAME)
  if (!n) return [...list]
  const clash = list.some((s) => s.name !== from && s.name.toLowerCase() === n.toLowerCase())
  if (clash) return [...list]
  return list.map((s) => (s.name === from ? { ...s, name: n } : s))
}

export function withoutFilter(list: readonly SavedLogFilter[], name: string): SavedLogFilter[] {
  return list.filter((s) => s.name !== name)
}

// ── the sort keys ───────────────────────────────────────────────────────────────────────────────

export interface LogSort {
  sort: LogSortKey
  asc: boolean
  /** The keys after the first, in order. */
  then: LogSortTerm[]
}

/**
 * A header clicked. Without Shift it orders by that column alone: the sorted column flips, a new
 * one takes its usual direction. With Shift it adds the column as the next key (up to
 * `MAX_SORT_KEYS`), or flips it when it already orders the list.
 */
export function clickSort(cur: LogSort, k: LogSortKey, shift: boolean, defaultAsc: (k: LogSortKey) => boolean): LogSort {
  if (!shift) {
    if (cur.sort === k && cur.then.length === 0) return { ...cur, asc: !cur.asc }
    return { sort: k, asc: cur.sort === k ? !cur.asc : defaultAsc(k), then: [] }
  }
  if (cur.sort === k) return { ...cur, asc: !cur.asc }
  const at = cur.then.findIndex((t) => t.sort === k)
  if (at >= 0) return { ...cur, then: cur.then.map((t, i) => (i === at ? { ...t, asc: !t.asc } : t)) }
  if (cur.then.length + 1 >= MAX_SORT_KEYS) return cur
  return { ...cur, then: [...cur.then, { sort: k, asc: defaultAsc(k) }] }
}

/** Where `k` stands in the order — 1 for the first key — and its direction; null when it does not
 *  order the list. */
export function sortPlace(cur: LogSort, k: LogSortKey): { place: number; asc: boolean } | null {
  if (cur.sort === k) return { place: 1, asc: cur.asc }
  const at = cur.then.findIndex((t) => t.sort === k)
  return at < 0 ? null : { place: at + 2, asc: cur.then[at].asc }
}
