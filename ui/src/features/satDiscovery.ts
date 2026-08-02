// The DISCOVERY band under the favorites schedule: which NON-★ birds are
// worth a look, computed entirely from the `get_satellites` snapshot the
// section already polls at 60 s. Zero new IPC, zero per-bird fetches — the
// all-bird pass array was sitting in `view.passes`, unread by this section.
// Also home of `passAdmission`, the ONE admission rule this band shares with
// the Next/Best strip (2026-08 ruling) — never duplicated per surface.
//
// Two deliberate refusals, labelled here because both will read as gaps:
//  - NO `earn` on discovery rows. `earn` is stamped only by
//    `get_sat_pass_needs` (a footprint-grid sweep per pass + a SatNOGS
//    snapshot per bird); paying that for ~300 non-★ birds behind a collapsed
//    band is a new network shape for data mostly never seen. A ★ row answers
//    "what will this WIN me"; a non-★ row answers "is this bird worth
//    STARRING" — geometry, liveness, mode fit, rhythm. Starring promotes the
//    bird into the schedule above, where earn (and the ⏰ alarm) live.
//  - NO alarm from a discovery row. The alarm map is NAME-keyed through the
//    schedule commands' alias resolution; `SatView.passes` carries the
//    element file's CURRENT name, so an alarm armed here on a renamed bird
//    would be filed under a key the firing tick cannot resolve — it would
//    silently never fire, the worst failure available in this section.
import type { SatPass, SatView } from '../types'
import { WORKABLE_EL_DEG } from './satSeed'

/** THE VOLUME LAW (stated where the rollup lives, per the design ruling):
 * the discovery band is ONE ROW PER BIRD, keyed on NORAD — never one row per
 * pass. The 24 h window over the shipped catalog holds ~1,800 passes but only
 * ~300 birds, and the operator's word was "birds", not "passes"; the rollup
 * is also the only cut that survives a rideshare deployment flying in
 * formation (the measured worst hour holds 137 AOS events — as birds, ~75).
 * The band renders at most this many rows of the WORTH-SORTED list — a cap on
 * a sorted list can never hide the best thing — and the remainder is stated
 * ("show all N"), never silently dropped. The bound is a ROW COUNT, never a
 * pixel height: a max-height here would be a second scroll owner inside the
 * schedule scroller. */
export const DISCOVERY_ROW_CAP = 12

/** How many workable passes per day can buy rank. LOAD-BEARING: rideshare
 * clumps fly up to 16 near-identical passes/day and would outrank a 70°
 * linear bird on volume alone — the measured live distortion in the seed's
 * name-keyed ranking (OBJECT H/D/E claiming 16/16/13 passes), not a
 * hypothetical. Four chances today beats one; seventeen buys nothing more. */
export const WORTH_PASS_COUNT_CAP = 4

/** Upstream placeholder catalog names ("OBJECT D") — fresh deployments filed
 * before a real name lands. On the shipped catalog 75 element rows carry one,
 * and 18 of those NAMES are shared by up to six distinct satellites. The ★
 * set is name-keyed on its wire, so starring one "OBJECT D" lights six rows
 * and schedules a first-match sibling — the band excludes them instead of
 * shipping a star that collides (they stay findable in the Birds list). */
const PLACEHOLDER_NAME = /^OBJECT\s/i

export interface DiscoveryBird {
  name: string
  norad: number | null
  /** The bird's BEST workable pass still open in the window (geometry rank). */
  best: SatPass
  /** Workable passes (peak ≥ WORKABLE_EL_DEG) in the window. */
  workable: number
  /** Live altitude (km) from the position snapshot, when the bird has one. */
  altKm: number | null
  /** Per-bird mode class when the catalog can say — null = no claim. */
  birdType: string | null
}

/** The PRIMARY class of a bird's `classes` set, by workability: a bird
 * carrying several transmitter kinds (QO-100 flies a linear transponder,
 * digital segments and a beacon at once) is pilled and tiered by the most
 * workable one — linear > fm > digital > beacon, the same priority the seed's
 * rotation draws in. Absent/empty = no claim (the catalog never classified
 * it, or classified it as carrying nothing live) — null, never a guess. */
export function primaryClass(classes: string[] | null | undefined): string | null {
  if (!classes || classes.length === 0) return null
  const set = new Set(classes.map((c) => c.toLowerCase()))
  for (const c of ['linear', 'fm', 'digital', 'beacon']) if (set.has(c)) return c
  return null
}

/** The mode pill's word for a per-bird class. Unknown or absent = NO pill —
 * never a guess (kindWord's law, applied to the class the same way it is
 * applied to transmitter kinds). Only the four canonical tokens map; a
 * vocabulary this display does not know renders nothing rather than
 * something wrong. */
export function modePillWord(birdType: string | null | undefined): string | null {
  switch ((birdType ?? '').toLowerCase()) {
    case 'fm':
      return 'FM voice'
    case 'linear':
      return 'Linear SSB/CW'
    case 'digital':
      return 'Digital'
    case 'beacon':
      return 'Beacon'
    default:
      return null
  }
}

/** Mode class as a lexicographic TIER, never a blended score term (the
 * design ruling: a gate, not a bonus). Linear outranks FM for the SSB
 * operator this section serves; a bird with NO class sits between the voice
 * fleet and the digital/beacon tail — unknown is never dropped and never
 * promoted (dropping unknowns would make the band exactly as blind as its
 * classifier's coverage). */
function classTier(birdType: string | null): number {
  switch ((birdType ?? '').toLowerCase()) {
    case 'linear':
      return 0
    case 'fm':
      return 1
    case 'digital':
      return 3
    case 'beacon':
      return 4
    default:
      return 2
  }
}

/** Geometry-only pass quality — passScore's shape minus its status terms.
 * The status terms are dropped deliberately: on this surface status is the
 * bundled catalog's, which reads 'alive' for every admitted bird (a constant
 * buys no ranking), and the live SatNOGS overlay is favorites-only. */
function geomScore(p: SatPass): number {
  const durMin = (p.losUnix - p.aosUnix) / 60
  return p.maxElDeg + Math.min(durMin, 15) * 0.8
}

/** The rank number for a discovery row. NEVER RENDERED — passScore's own law
 * ("for RANKING only — the UI never shows a made-up number") holds here;
 * worth reaches the operator as ORDER plus the section's existing words. */
export function birdWorth(b: DiscoveryBird): number {
  return geomScore(b.best) + 3 * Math.min(b.workable, WORTH_PASS_COUNT_CAP)
}

/**
 * The section's ONE pass-admission rule, shared by the discovery band and the
 * Next/Best strip above the schedule (the strip must never invent a second
 * one — a bird this refuses cannot be allowed to surface anywhere the section
 * suggests birds). What it refuses: expired passes, sub-workable peaks (the
 * seed's WORKABLE_EL_DEG floor, imported so the app has ONE workability
 * threshold, not two), placeholder catalog names, names carried by multiple
 * NORADs (every action on these surfaces — star, open, work — is name-keyed
 * and would collide), birds the catalog POSITIVELY calls dead/re-entered/
 * not-yet-launched, and birds it positively says have nothing left to
 * transmit — a SUGGESTION must clear a higher bar than a choice. An ABSENT
 * status/amateur answer is not an exclusion: dropping every bird a thin
 * catalog cannot vouch for would blind these surfaces exactly when the
 * catalog is the thing that failed.
 *
 * Favourite-ness is deliberately NOT part of admission: the band filters ★
 * out (those rows live in the schedule above), the strip keeps them — each
 * surface applies its own ★ policy on top of the one shared rule.
 */
export function passAdmission(
  view: SatView,
  nowSecs: number,
): {
  admits: (p: SatPass) => boolean
  metaFor: (p: SatPass) => SatView['birds'][number] | undefined
} {
  type Meta = SatView['birds'][number]
  const metaByNorad = new Map<number, Meta>()
  const metaByName = new Map<string, Meta>()
  for (const b of view.birds) {
    if (b.norad != null) metaByNorad.set(b.norad, b)
    metaByName.set(b.name.toUpperCase(), b)
  }
  // Duplicate-name census across the pass set: a name carried by more than
  // one NORAD cannot be starred without lighting its siblings (see
  // PLACEHOLDER_NAME) — those names are excluded outright.
  const noradsByName = new Map<string, Set<number>>()
  for (const p of view.passes) {
    if (p.norad == null) continue
    const k = p.name.toUpperCase()
    const set = noradsByName.get(k) ?? new Set<number>()
    set.add(p.norad)
    noradsByName.set(k, set)
  }
  const metaFor = (p: SatPass): Meta | undefined =>
    (p.norad != null ? metaByNorad.get(p.norad) : undefined) ??
    metaByName.get(p.name.toUpperCase())
  const admits = (p: SatPass): boolean => {
    if (p.losUnix <= nowSecs) return false // expired — nothing to act on
    if (p.maxElDeg < WORKABLE_EL_DEG) return false // a grazer, not an opportunity
    if (PLACEHOLDER_NAME.test(p.name)) return false
    if ((noradsByName.get(p.name.toUpperCase())?.size ?? 0) > 1) return false
    const meta = metaFor(p)
    const status = meta?.status ?? p.status ?? null
    if (status === 'dead' || status === 're-entered' || status === 'future') return false
    // `amateur` is a real answer ONLY beside a present status (SatBird's own
    // doc: catalog_marks returns (None, false) for every bird it was never
    // ASKED about, and the bool rides the wire unconditionally). Believe
    // "positively nothing left to work" only when the catalog actually
    // answered — satHealth's rule (status absent/'' = never asked, say
    // nothing). Gating on the bare false blinded the band on every
    // catalog-less wire shape: imported TLEs, Celestrak-leg birds,
    // pre-catalog snapshots.
    const asked = meta?.status != null && meta.status !== ''
    if (asked && meta?.amateur === false) return false
    return true
  }
  return { admits, metaFor }
}

/**
 * One row per non-★ bird with at least one workable pass still open in the
 * snapshot's window, worth-sorted (class tier, then worth, soonest AOS and
 * name breaking ties — deterministic across runs). Pure: the caller passes
 * its own ★ test so this module owns no storage. Admission is passAdmission
 * (the shared rule, documented there), plus the band's own ★ exclusion.
 */
export function rollPassesToBirds(
  view: SatView,
  nowSecs: number,
  isFav: (name: string, norad?: number | null) => boolean,
): DiscoveryBird[] {
  const { admits, metaFor } = passAdmission(view, nowSecs)
  const rows = new Map<string, DiscoveryBird>()
  for (const p of view.passes) {
    if (!admits(p)) continue
    if (isFav(p.name, p.norad)) continue // ★ rows live in the schedule above
    const meta = metaFor(p)
    const key = p.norad != null ? `n${p.norad}` : p.name.toUpperCase()
    const cur = rows.get(key)
    if (cur == null) {
      rows.set(key, {
        name: p.name,
        norad: p.norad ?? null,
        best: p,
        workable: 1,
        altKm: meta?.altKm ?? null,
        birdType: primaryClass(meta?.classes),
      })
    } else {
      cur.workable += 1
      if (geomScore(p) > geomScore(cur.best)) cur.best = p
    }
  }
  return [...rows.values()].sort(
    (a, b) =>
      classTier(a.birdType) - classTier(b.birdType) ||
      birdWorth(b) - birdWorth(a) ||
      a.best.aosUnix - b.best.aosUnix ||
      a.name.localeCompare(b.name),
  )
}
