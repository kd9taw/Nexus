// Satellite ★ favorites (a.k.a. the chase set — one concept, one storage key):
// which birds the operator cares about. Drives the Passes pane sort, the map
// emphasis (bigger icon + footprint ring), the Satellites section schedule,
// and which birds can carry pass alarms (satAlarm.ts).

import { disarmSatAlarm } from './satAlarm'

const KEY = 'nexus.sats.chasing'
const NORAD_KEY = 'nexus.sats.chasingNorad'

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
  const now = !set.has(key)
  if (now) set.add(key)
  else {
    set.delete(key)
    // Alarms only have a disarm surface on the schedule (favorites) — an
    // unstarred bird's alarm would otherwise fire orphaned forever.
    disarmSatAlarm(key)
  }
  try {
    localStorage.setItem(KEY, JSON.stringify([...set]))
  } catch {
    /* storage blocked — applies this session via the read-back failure mode */
  }
  if (typeof norad === 'number' && Number.isFinite(norad)) {
    try {
      localStorage.setItem(NORAD_KEY, JSON.stringify({ ...satChasingNorads(), [key]: norad }))
    } catch {
      /* storage blocked — degrades exactly like the name set above */
    }
  }
  return now
}
