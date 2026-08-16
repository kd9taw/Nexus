// User-defined watch list — "tell me loudly when THIS shows up." Generalizes the
// DXpedition chase-star to arbitrary operator-defined targets: a specific call, a
// wildcard/prefix (VP8*, *ABC), or a whole DXCC entity, optionally gated by CQ-only
// and a minimum SNR. A match fires the loudest alert tier (it's what the operator asked
// to be told about), reusing the existing alert dedupe/toast plumbing.
//
// Persisted in localStorage (like the chase star) so there's no backend/settings change;
// the matcher is pure so it's fully unit-tested.

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

/** Return the FIRST watch filter a decode matches, or null. Pure — no I/O. */
export function matchWatchlist(d: DecodeRow, filters: WatchFilter[]): WatchFilter | null {
  const call = (d.from ?? '').toUpperCase()
  if (!call) return null
  for (const f of filters) {
    if (f.cqOnly && !d.isCq) continue
    if (f.minSnr != null && d.snr < f.minSnr) continue
    let hit = false
    if (f.kind === 'call') {
      hit = matchCallPattern(call, f.value)
    } else if (f.kind === 'dxcc') {
      const country = (d.country ?? '').toUpperCase().trim()
      hit = country !== '' && country === f.value.toUpperCase().trim()
    } else if (f.kind === 'grid') {
      // Only frames that carry a grid can match (CQ + first reply, per protocol) — a
      // grid-less row is "unknown", never a hit. matchCallPattern is a general glob.
      const grid = (d.grid ?? '').toUpperCase().trim()
      hit = grid !== '' && matchCallPattern(grid, f.value)
    }
    if (hit) return f
  }
  return null
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
