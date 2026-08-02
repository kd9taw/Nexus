// ONE-TIME favorites seeding: a first-run operator opens Satellites and finds
// a working ★ set instead of an empty schedule and a 300-bird list to guess
// from. Operator's ask, verbatim intent: "if we are going to set some
// favorites, they should be set to the most recent active birds, one time,
// then allow users to change from there."
//
// Two properties carry the whole design:
//
//  ONE TIME. The moment a seed runs it writes a marker, and nothing ever
//  seeds again. An operator who unstars every bird gets an empty sky — a
//  resurrection would make the ★ set the app's opinion instead of theirs.
//  The marker is not the only guard: an operator who already has stars, or
//  who ever starred a bird and cleared it, is never seeded over either. That
//  second proof has to work for someone UPGRADING — whose stars were set by a
//  build that knew nothing of any of this — so it is the existence of the ★
//  list key itself, not only the name→NORAD map that ships with this feature.
//
//  ACTIVE, or nothing. A pick must be a bird the catalog positively calls
//  alive, that carries a live amateur transmitter, that we hold elements for,
//  and that actually flies a workable pass over THIS operator's grid. When
//  any of that is missing the seed DEFERS without spending itself — starring
//  blind is worse than starring nothing, and a deferred seed still fires the
//  day the grid is set or the catalog lands.
//
// Ranking runs entirely off the snapshot `get_satellites` already returns:
// `birds` proves elements + status + transmitter, `passes` is the backend's
// own 24 h scan over the operator's grid (cached 10 min). No extra command,
// no extra fetch — the seed is free.
import type { SatView } from '../types'
import {
  satChasingEverSet,
  satChasingNorads,
  satChasingSet,
  toggleSatChasing,
} from './satChase'

/** Presence = seeded, forever. Holds what was starred and when, so the notice
 * can name a number and a support question has an answer. */
const SEED_KEY = 'nexus.sats.seeded'
/** The notice was read and dismissed. Separate key: acknowledging the notice
 * must never look like un-seeding. */
const ACK_KEY = 'nexus.sats.seedAck'

/** How many birds the one-time seed stars. Ten fills a day's schedule
 * (~40–60 passes) without burying the operator, and every one is removable
 * with the ★ it is already rendered with. */
export const SAT_SEED_CAP = 10

/** A pass below this peak elevation is a grazer, not an opportunity: too low
 * and too brief to work, and counting them would rank a bird that never
 * really clears the operator's horizon above one that does. Exported for the
 * discovery band (satDiscovery.ts) — ONE workability threshold in the app,
 * not two. */
export const WORKABLE_EL_DEG = 10

export interface SatSeedRecord {
  /** Unix seconds. */
  at: number
  names: string[]
}

/** The seed marker, or null when no seed has ever run. Corrupt storage reads
 * as null — the same failure mode as every other satChase reader. */
export function satSeedRecord(): SatSeedRecord | null {
  try {
    const raw = localStorage.getItem(SEED_KEY)
    if (!raw) return null
    const obj = JSON.parse(raw)
    if (!obj || typeof obj !== 'object' || !Array.isArray(obj.names)) return null
    return { at: Number(obj.at) || 0, names: obj.names.map((n: unknown) => String(n)) }
  } catch {
    return null
  }
}

/** Should the Satellites section show the "we starred these for you" line? */
export function satSeedNoticeOpen(): boolean {
  if (satSeedRecord() == null) return false
  try {
    return localStorage.getItem(ACK_KEY) !== '1'
  } catch {
    return false
  }
}

export function ackSatSeedNotice(): void {
  try {
    localStorage.setItem(ACK_KEY, '1')
  } catch {
    /* storage blocked — the notice reappears next launch, which is honest */
  }
}

export interface SatSeedCandidate {
  name: string
  norad: number | null
  /** Workable passes (≥ WORKABLE_EL_DEG) in the snapshot's window. */
  passes: number
  bestElDeg: number
}

/**
 * The ranked seed candidates from one `get_satellites` snapshot: alive +
 * amateur + elements + at least one workable pass over the grid, most passes
 * first, best peak elevation breaking the tie, name last so the order is
 * deterministic across runs.
 *
 * Pure — the whole ranking is testable without a DOM or a backend.
 */
export function rankSatSeedCandidates(view: SatView, cap = SAT_SEED_CAP): SatSeedCandidate[] {
  const byName = new Map<string, { passes: number; bestElDeg: number }>()
  for (const p of view.passes) {
    if (p.maxElDeg < WORKABLE_EL_DEG) continue
    const k = p.name.toUpperCase()
    const cur = byName.get(k) ?? { passes: 0, bestElDeg: 0 }
    byName.set(k, {
      passes: cur.passes + 1,
      bestElDeg: Math.max(cur.bestElDeg, p.maxElDeg),
    })
  }
  return view.birds
    .filter((b) => b.status === 'alive' && b.amateur === true)
    .map((b) => ({
      name: b.name,
      norad: b.norad ?? null,
      ...(byName.get(b.name.toUpperCase()) ?? { passes: 0, bestElDeg: 0 }),
    }))
    .filter((c) => c.passes > 0)
    .sort(
      (a, b) =>
        b.passes - a.passes || b.bestElDeg - a.bestElDeg || a.name.localeCompare(b.name),
    )
    .slice(0, cap)
}

/**
 * Run the one-time seed if — and only if — this is a genuine first run with
 * something honest to star. Returns the record written, or null when it did
 * not run (already seeded, the operator has their own ★ set, no grid, no
 * candidate). Idempotent: every later call after a successful seed is null,
 * so a caller may run it on every snapshot without guarding.
 */
export function seedSatFavorites(view: SatView | null, hasGrid: boolean): SatSeedRecord | null {
  if (satSeedRecord() != null) return null // once, ever
  if (satChasingSet().size > 0) return null // the operator already chose
  // …and once chose to CLEAR. Seeding over that is the resurrection, one
  // build late, so BOTH records that can prove it are consulted:
  //  - the ★ list KEY exists (satChasingEverSet) — only a toggle writes it,
  //    so an empty array is a set that was emptied. This is the one that
  //    speaks for an UPGRADING operator, whose stars predate everything else
  //    on this list.
  //  - the name→NORAD map has an entry — it survives unstars, and covers a
  //    star set on THIS build onward (including one cleared in a browser
  //    profile whose ★ key was pruned).
  if (satChasingEverSet()) return null
  if (Object.keys(satChasingNorads()).length > 0) return null
  // Defer, never spend: no marker is written on any of these paths, so the
  // seed still fires the day the grid is set / the catalog lands.
  if (!hasGrid || view == null) return null
  const picks = rankSatSeedCandidates(view)
  if (picks.length === 0) return null
  for (const p of picks) toggleSatChasing(p.name, p.norad)
  const rec: SatSeedRecord = {
    at: Math.floor(Date.now() / 1000),
    names: picks.map((p) => p.name),
  }
  try {
    localStorage.setItem(SEED_KEY, JSON.stringify(rec))
  } catch {
    /* storage blocked — the stars above did not persist either, so the next
       launch is a consistent un-seeded first run, not a double seed */
  }
  return rec
}
