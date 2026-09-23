// User-defined watch list — "tell me loudly when THIS shows up." Generalizes the
// DXpedition chase-star to arbitrary operator-defined targets: a specific call, a
// wildcard/prefix (VP8*, *ABC), or a whole DXCC entity, optionally gated by CQ-only
// and a minimum SNR. A match fires the loudest alert tier (it's what the operator asked
// to be told about), reusing the existing alert dedupe/toast plumbing.
//
// The same list MARKS the station afterwards (maintainer, 2026-09-23): the Call Roster, the
// Stations list and Spots put a WATCH tile on every row it names. All three ask ONE matcher
// (`useWatchMatch` → `watchedEntry`), so they can never disagree about a station.
//
// Persisted in localStorage (like the chase star) so there's no backend/settings change;
// the matcher is pure so it's fully unit-tested.

import { useEffect, useMemo, useState } from 'react'
import type { DecodeRow } from './types'
import { durableGet, durableSet } from './features/durableStore'

export type WatchKind = 'call' | 'dxcc' | 'grid'

export interface WatchFilter {
  /** Stable id for list keys + removal. */
  id: string
  kind: WatchKind
  /** For `call`: an exact call or a `*`-wildcard (e.g. `VP8*`, `*ABC`, `3Y0*`). For
   * `dxcc`: a country/entity name matched case-insensitively against the decode's country.
   * For `grid`: an exact 4-char square (`FN31`) or a `*`-wildcard (`EM7*`, `EM*`) — FT8/FT4
   * frames carry 4-char grids, so a 2-char field wants the star. */
  value: string
  /** Only alert on a CQ call (not mid-QSO chatter). Default false. */
  cqOnly?: boolean
  /** Only alert when SNR ≥ this (dB). Null/undefined = any signal. */
  minSnr?: number | null
  /** Optional friendly label shown in the alert (e.g. "Bouvet DXpedition"). */
  label?: string
}

const STORAGE_KEY = 'nexus.watchlist'
/** Dispatched by the Settings manager (`WatchlistPanel`) after every edit; App's alert path and
 *  `useWatchMatch` both re-read the list on it. */
const WATCHLIST_CHANGED = 'nexus:watchlist-changed'

/** Escape a string for literal use inside a RegExp. */
function escapeRegex(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

/** Match a callsign against a pattern that may contain `*` wildcards (glob-style). */
export function matchCallPattern(call: string, pattern: string): boolean {
  const c = call.toUpperCase()
  const p = pattern.toUpperCase().trim()
  if (!p) return false
  if (!p.includes('*')) return c === p
  const re = new RegExp('^' + p.split('*').map(escapeRegex).join('.*') + '$')
  return re.test(c)
}

/** What a watch entry can name about a station. A decode, a roster row and a cluster spot each
 *  carry these under names of their own (`from` or `call`, `country` or `entity`); the caller
 *  maps them. */
export interface WatchSubject {
  call: string | null | undefined
  /** The cty.dat entity name; '' or null when it could not be resolved. */
  entity?: string | null
  grid?: string | null
}

/** Does this entry name this station — its call or prefix, its DXCC entity, or its grid? The
 *  IDENTITY half of every watch match, shared by the alert (`matchWatchlist`) and the lists
 *  (`watchedEntry`), so one entry cannot come to mean two things. `call` arrives uppercased. */
function entryNames(
  f: WatchFilter,
  call: string,
  entity: string | null | undefined,
  grid: string | null | undefined,
): boolean {
  if (f.kind === 'call') return matchCallPattern(call, f.value)
  if (f.kind === 'dxcc') {
    const country = (entity ?? '').toUpperCase().trim()
    return country !== '' && country === f.value.toUpperCase().trim()
  }
  if (f.kind === 'grid') {
    // Only frames that carry a grid can match (CQ + first reply, per protocol) — a
    // grid-less row is "unknown", never a hit. matchCallPattern is a general glob.
    const g = (grid ?? '').toUpperCase().trim()
    return g !== '' && matchCallPattern(g, f.value)
  }
  return false
}

/** Return the FIRST watch filter a decode matches, or null. Pure — no I/O. */
export function matchWatchlist(d: DecodeRow, filters: WatchFilter[]): WatchFilter | null {
  const call = (d.from ?? '').toUpperCase()
  if (!call) return null
  for (const f of filters) {
    if (f.cqOnly && !d.isCq) continue
    if (f.minSnr != null && d.snr < f.minSnr) continue
    if (entryNames(f, call, d.country, d.grid)) return f
  }
  return null
}

/**
 * The entry that puts a station on the watch list, for the lists that MARK it (the Call Roster,
 * the Stations list, Spots): the FIRST entry that names it, or null.
 *
 * IDENTITY ONLY. `cqOnly` and `minSnr` are the alert's gates ("only alert on a CQ call", "only
 * alert when SNR ≥") and they stay in `matchWatchlist`. A station does not leave the watch list
 * when it answers somebody or fades a few dB. Gated here, a row would lose its tile — and drop
 * out of Needed only — every time the station stopped calling CQ, and a cluster spot, which
 * carries neither a CQ flag nor an SNR, could never agree with the roster about the same call.
 */
export function watchedEntry(s: WatchSubject, filters: readonly WatchFilter[]): WatchFilter | null {
  const call = (s.call ?? '').toUpperCase()
  if (!call) return null
  for (const f of filters) if (entryNames(f, call, s.entity, s.grid)) return f
  return null
}

/** How many rows one matcher remembers before starting over. A long contest session hears
 *  thousands of distinct stations; a forgotten answer only costs working it out again. */
const MATCH_MEMO_MAX = 5000
const NO_WATCH = (): WatchFilter | null => null

/**
 * `watchedEntry`, memoised per row for ONE watch list. The roster recomputes on every decode
 * period with dozens of rows, and a row's answer depends only on the list and on the row's call,
 * entity and grid — so it is worked out once per distinct row and read back after that. A
 * changed list gets a new matcher (`useWatchMatch` keys it on the list); no list costs nothing.
 */
export function watchMatcher(filters: readonly WatchFilter[]): (s: WatchSubject) => WatchFilter | null {
  if (filters.length === 0) return NO_WATCH
  const memo = new Map<string, WatchFilter | null>()
  return (s) => {
    const key = `${s.call ?? ''}\n${s.entity ?? ''}\n${s.grid ?? ''}`
    const known = memo.get(key)
    if (known !== undefined) return known
    if (memo.size >= MATCH_MEMO_MAX) memo.clear()
    const found = watchedEntry(s, filters)
    memo.set(key, found)
    return found
  }
}

/**
 * The operator's watch list as a live matcher, for the lists that mark watched stations. It
 * reads the list where the alert path does (`loadWatchlist`) and re-reads it on the event the
 * Settings manager dispatches after every edit, so a tile comes and goes with the list: no
 * remount, no second copy.
 */
export function useWatchMatch(): (s: WatchSubject) => WatchFilter | null {
  const [list, setList] = useState<WatchFilter[]>(loadWatchlist)
  useEffect(() => {
    const resync = () => setList(loadWatchlist())
    window.addEventListener(WATCHLIST_CHANGED, resync)
    return () => window.removeEventListener(WATCHLIST_CHANGED, resync)
  }, [])
  return useMemo(() => watchMatcher(list), [list])
}

/** A short human label for a matched filter, for the alert toast. */
export function watchLabel(f: WatchFilter): string {
  if (f.label?.trim()) return f.label.trim()
  if (f.kind === 'dxcc') return f.value
  if (f.kind === 'grid') return `grid ${f.value.toUpperCase()}`
  return f.value.toUpperCase()
}

/** Load the saved watch list (empty on first run or any parse error). */
export function loadWatchlist(): WatchFilter[] {
  try {
    const raw = durableGet(STORAGE_KEY)
    if (!raw) return []
    const arr = JSON.parse(raw)
    if (!Array.isArray(arr)) return []
    return arr.filter(
      (f): f is WatchFilter =>
        f &&
        typeof f.id === 'string' &&
        (f.kind === 'call' || f.kind === 'dxcc' || f.kind === 'grid') &&
        typeof f.value === 'string',
    )
  } catch {
    return []
  }
}

/** Persist the watch list. */
export function saveWatchlist(filters: WatchFilter[]): void {
  try {
    durableSet(STORAGE_KEY, JSON.stringify(filters))
  } catch {
    // storage full / unavailable — non-fatal; the list just isn't remembered
  }
}

/** Make a new filter with a unique-enough id (no crypto dependency). */
export function newWatchFilter(kind: WatchKind, value: string, extra?: Partial<WatchFilter>): WatchFilter {
  const id = `${kind}-${value}-${Math.random().toString(36).slice(2, 8)}`
  return { id, kind, value: value.trim(), ...extra }
}
