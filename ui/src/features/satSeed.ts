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
// `birds` proves elements + status + transmitter + class, `passes` is the
// backend's own 24 h scan over the operator's grid (cached 10 min). No extra
// command, no extra fetch — the seed is free.
//
// AND THE TEN SPAN WHAT CAN BE WORKED. Geometry alone is not enough, and the
// operator found out on the air: "FO-29 was not in favorites, but it's
// active." It is alive, amateur and holds current elements — it just loses the
// pass-count race more often than not. FO-29 flies 13.53 rev/day in an
// 800 × 1320 km ellipse; a 480 km cubesat flies 15.3, so it shows more passes
// over any grid on any day, and by orbital rate FO-29 is 349th of the 367
// birds in the shipped catalog that carry elements (RS-44 352nd, AO-7 353rd).
//
// MEASURED, rather than argued — the app's own pass predictor over the shipped
// catalog, 24 h windows from six grids at four start times (2026-08-02 UTC):
// FO-29 lands between 2nd and 81st of the ~332 alive amateur birds that fly a
// workable pass, and made the pure-geometry ten in 6 of those 24 runs. Some
// linear bird made it in 14 of 24. So the geometry rank does not bury the
// linear birds reliably — it decides them by the luck of the day, which is
// worse: the operator cannot tell a missing bird from an unlucky one.
//
// And what the ten is spent ON is not close: 305 of those 367 candidates are
// BEACON-ONLY telemetry cubesats that cannot be worked at all, 18 of the top
// 20 by orbital rate are among them, and 3 to 9 of the pure-geometry ten (6.5
// on average across the 24 runs) were beacon-only. A seed that only rescued
// the linear birds would still spend most of its stars on birds with no
// uplink. So the selection is stratified by what the operator works the bird
// WITH — see [`rankSatSeedCandidates`].
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
 * really clears the operator's horizon above one that does. */
const WORKABLE_EL_DEG = 10

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

/** The classes the seed rotates through, RAREST-WORKABLE FIRST, and the whole
 * quota in one line: fill the ten by rotating Linear → FM voice →
 * Digital/packet, each class contributing its own best bird under the
 * geometry rank below, and spend a slot on a beacon-only or unclassified bird
 * only once those three are exhausted.
 *
 * `beacon` is deliberately NOT in the rotation. It is not a way to work a
 * bird — it is a downlink and nothing else — so a beacon bird is the last
 * call on a star, not a rotation partner. Unclassified birds share that last
 * bucket: an old payload tells us nothing, and nothing is not a claim.
 *
 * Today's catalog yields 4 linear / 3 FM / 3 digital from this. The numbers
 * are not written down anywhere on purpose — they are whatever the operator's
 * own sky supplies, and the rotation is what stays true when it changes. */
const SEED_CLASS_ROTATION = ['linear', 'fm', 'digital'] as const

export interface SatSeedCandidate {
  name: string
  norad: number | null
  /** Workable passes (≥ WORKABLE_EL_DEG) in the snapshot's window. */
  passes: number
  bestElDeg: number
  /** What the catalog says this bird is worked WITH (`SatView.birds[].classes`).
   *
   * Absent (never classified) and `[]` (classified, nothing workable) are
   * different answers and are kept apart on the WIRE — that is why the field
   * is optional-and-nullable here and `Option<Vec<String>>` in the catalog:
   * so no consumer is forced to guess which one it is looking at.
   *
   * The RANK deliberately acts on both the same way: neither offers a class
   * the rotation can place, so both fall through to the residual in geometry
   * order. That is the right answer for each of them separately — an
   * unclassified bird must degrade to exactly the pre-classification
   * behaviour, and a bird with nothing left to work has no claim on a slot
   * ahead of one that has something — so the seed never needs to tell them
   * apart, and does not. */
  classes?: string[] | null
}

/**
 * The seed's ten from one `get_satellites` snapshot.
 *
 * ELIGIBILITY is unchanged and non-negotiable: alive + amateur + elements +
 * at least one workable pass over the grid.
 *
 * The GEOMETRY RANK is unchanged too — most passes first, best peak elevation
 * breaking the tie, name last so the order is deterministic across runs. It
 * was validated on the air and it is right about which of two comparable
 * birds is the better star: it is only wrong about which birds are
 * comparable.
 *
 * What is new is the SELECTION on top of it: the ten are drawn by rotating
 * through [`SEED_CLASS_ROTATION`], each class offering its own best remaining
 * bird under that same rank, and everything the rotation did not place —
 * beacon-only birds, and birds no catalog ever classified — filling what is
 * left in rank order. With no classification anywhere the rotation places
 * nothing and the result is exactly the pre-classification top ten.
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
  const ranked = view.birds
    .filter((b) => b.status === 'alive' && b.amateur === true)
    .map((b) => ({
      name: b.name,
      norad: b.norad ?? null,
      classes: b.classes,
      ...(byName.get(b.name.toUpperCase()) ?? { passes: 0, bestElDeg: 0 }),
    }))
    .filter((c) => c.passes > 0)
    .sort(
      (a, b) =>
        b.passes - a.passes || b.bestElDeg - a.bestElDeg || a.name.localeCompare(b.name),
    )

  const chosen = new Set<SatSeedCandidate>()
  // One bird per class per lap, so the classes take turns rather than the
  // lowest-flying one draining the cap.
  while (chosen.size < cap) {
    const before = chosen.size
    for (const cls of SEED_CLASS_ROTATION) {
      if (chosen.size >= cap) break
      const next = ranked.find((c) => !chosen.has(c) && c.classes?.includes(cls))
      if (next) chosen.add(next)
    }
    // A lap that placed nothing means every workable class is exhausted —
    // stop, and let the residual below finish the set.
    if (chosen.size === before) break
  }
  // The residual: beacon-only and never-classified birds, plus any workable
  // bird the rotation ran out of room for — all in the same geometry rank.
  for (const c of ranked) {
    if (chosen.size >= cap) break
    chosen.add(c)
  }
  // Returned in RANK order, not selection order: which ten is the decision
  // here, "best first" is how the operator reads a list.
  return ranked.filter((c) => chosen.has(c))
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
