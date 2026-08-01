// Satellite ★ favorites (a.k.a. the chase set — one concept, one storage key):
// which birds the operator cares about. Drives the Passes pane sort, the map
// emphasis (bigger icon + footprint ring), the Satellites section schedule,
// and which birds can carry pass alarms (satAlarm.ts).

import { disarmSatAlarm } from './satAlarm'
import { surfaceGet, surfaceSet } from './windowScope'

const KEY = 'nexus.sats.chasing'
const NORAD_KEY = 'nexus.sats.chasingNorad'
const FAV_ONLY_KEY = 'nexus.sats.favOnly'

/** Same-window change event, fired on every star toggle and ★/All chip flip.
 * localStorage has no same-window event, and the three Connect surfaces (pane,
 * 2-D map, 3-D globe) each hold their own React state over the ONE shared key —
 * without this, a chip flip on one surface reached the others only on their
 * next poll (up to 30 s of visible disagreement on the globe). */
export const SAT_CHASE_EVENT = 'nexus:satchase-changed'

function notifyChanged(): void {
  try {
    window.dispatchEvent(new Event(SAT_CHASE_EVENT))
  } catch {
    /* non-DOM env — same-window sync is best-effort, storage stays truthful */
  }
}

/** The persisted chased-bird set (uppercase names). Empty when storage is blocked. */
export function satChasingSet(): Set<string> {
  try {
    const raw = localStorage.getItem(KEY)
    if (!raw) return new Set()
    const arr = JSON.parse(raw)
    return new Set(Array.isArray(arr) ? arr.map((c) => String(c).toUpperCase()) : [])
  } catch {
    return new Set()
  }
}

/** Has this station EVER had a favorites list? Only [`toggleSatChasing`]
 * writes the key, so its mere PRESENCE — even holding `[]` — is an operator
 * who starred something and then cleared it. That is the only durable proof
 * an UPGRADING operator can have: the name→NORAD map ships with the seed
 * itself, so it is empty on every install that predates it, and reading the
 * map alone would re-star ten birds for every operator who had deliberately
 * emptied theirs. Blocked storage reads as "no record". */
export function satChasingEverSet(): boolean {
  try {
    return localStorage.getItem(KEY) != null
  } catch {
    return false
  }
}

/** Recorded NAME → NORAD for starred birds — the UI half of rename survival
 * (phase 4). ADDITIVE, never a key: the chase set above stays name-keyed (a
 * re-key would span alarms, fired keys, dial opt-outs and five command
 * signatures for no added correctness); this map just remembers which catalog
 * number a starred name meant, and entries survive unstars. Empty when
 * storage is blocked/corrupt. */
export function satChasingNorads(): Record<string, number> {
  try {
    const raw = localStorage.getItem(NORAD_KEY)
    if (!raw) return {}
    const obj = JSON.parse(raw)
    if (!obj || typeof obj !== 'object' || Array.isArray(obj)) return {}
    const out: Record<string, number> = {}
    for (const [k, v] of Object.entries(obj as Record<string, unknown>)) {
      if (typeof v === 'number' && Number.isFinite(v)) out[k.toUpperCase()] = v
    }
    return out
  } catch {
    return {}
  }
}

/** Flip the chase flag for a bird; returns the NEW state (true = now chasing).
 * Callers that know the bird's NORAD pass it so the name→NORAD record stays
 * current (see satChasingNorads); callers that don't simply omit it. */
export function toggleSatChasing(name: string, norad?: number | null): boolean {
  const set = satChasingSet()
  const key = name.toUpperCase()
  // Resolve the CURRENT state the way isSatChased does — NORAD first. After an
  // upstream rename the operator's star lives under the OLD name; a name-only
  // toggle would then ADD a second entry instead of unstarring (the stuck-★
  // bug: ★ shows ON via the NORAD match, but clicking it could never turn it
  // off). Matching entries include the clicked name itself plus every starred
  // name whose recorded catalog number is this bird's.
  const n = typeof norad === 'number' && Number.isFinite(norad) ? norad : null
  const norads = satChasingNorads()
  const matches = [...set].filter((s) => s === key || (n != null && norads[s] === n))
  const now = matches.length === 0
  if (now) set.add(key)
  else {
    for (const s of matches) {
      set.delete(s)
      // Alarms only have a disarm surface on the schedule (favorites) — an
      // unstarred bird's alarm would otherwise fire orphaned forever. Disarmed
      // per REMOVED name: post-rename the alarm is armed under the old one.
      disarmSatAlarm(s)
    }
  }
  try {
    localStorage.setItem(KEY, JSON.stringify([...set]))
  } catch {
    /* storage blocked — applies this session via the read-back failure mode */
  }
  if (n != null) {
    try {
      localStorage.setItem(NORAD_KEY, JSON.stringify({ ...norads, [key]: n }))
    } catch {
      /* storage blocked — degrades exactly like the name set above */
    }
  }
  notifyChanged()
  return now
}

/** The ★ set resolved for MATCHING: starred names plus the catalog numbers
 * recorded for names that are starred RIGHT NOW. The NORAD record survives
 * unstars (see satChasingNorads), so it must never be read raw as a match
 * set — that would resurrect every bird the operator ever starred. */
export interface SatChaseKeys {
  names: Set<string>
  norads: Set<number>
}

export function satChaseKeys(): SatChaseKeys {
  const names = satChasingSet()
  const norads = new Set<number>()
  for (const [name, n] of Object.entries(satChasingNorads())) if (names.has(name)) norads.add(n)
  return { names, norads }
}

/** Is this bird starred? NORAD first (upstream renames change names, never
 * catalog numbers), name fallback for norad-less rows. */
export function isSatChased(
  name: string,
  norad: number | null | undefined,
  keys: SatChaseKeys,
): boolean {
  if (typeof norad === 'number' && keys.norads.has(norad)) return true
  return keys.names.has(name.toUpperCase())
}

/** The ★-only view of a bird/pass list. ZERO stars returns the list untouched —
 * a fresh install must never render an empty sky. */
export function filterSatsToChased<T extends { name: string; norad?: number | null }>(
  items: T[],
  keys: SatChaseKeys,
): T[] {
  if (keys.names.size === 0) return items
  return items.filter((it) => isSatChased(it.name, it.norad, keys))
}

/** The ★/All chip choice for the Connect satellite surfaces — ONE surface-scoped
 * key that the Passes pane, the 2-D map layer, and the 3-D globe all read (three
 * keys would let the pane and the sky disagree about which birds exist). Shared
 * WITHIN a surface: a pop-out diverges after its first write — the documented
 * windowScope design, not an accident. Default
 * ON: the operator asked Connect to track the ★ birds; with zero stars the
 * filter is inert (filterSatsToChased shows all), so ON is safe on day one. */
export function satFavOnly(): boolean {
  return surfaceGet(FAV_ONLY_KEY) !== '0'
}

export function setSatFavOnly(on: boolean): void {
  surfaceSet(FAV_ONLY_KEY, on ? '1' : '0')
  notifyChanged()
}
