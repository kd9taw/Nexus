// Directional "getting out" — turn the who-hears-me reports into an 8-octant coverage
// picture: where the operator's signal actually lands, and how far, at a glance. Pure +
// testable (mirrors features/needs.ts); the compass rose + summary render it. Answers the
// elite-op question the flat receiver list doesn't: "which way am I getting out, and which
// way am I deaf?".
import type { HeardMe, PropagationSnapshot } from '../types'
import { fmtDistanceKm, type Units } from '../units'

/** Compass octants clockwise from North — the vocabulary HeardMe.octant uses. */
export const OCTANTS = ['N', 'NE', 'E', 'SE', 'S', 'SW', 'W', 'NW'] as const
export type Octant = (typeof OCTANTS)[number]

/** Screen bearing (deg clockwise from up/North) for each octant — drives the rose geometry. */
export const OCTANT_DEG: Record<Octant, number> = {
  N: 0,
  NE: 45,
  E: 90,
  SE: 135,
  S: 180,
  SW: 225,
  W: 270,
  NW: 315,
}

export interface OctantCoverage {
  octant: Octant
  count: number
  /** Farthest receiver in this octant (km). */
  maxKm: number
}

/** Aggregate the reports into all 8 octants (0-count octants included, so the rose can
 * show a *gap* where you're not getting out). */
export function octantCoverage(reports: HeardMe[]): OctantCoverage[] {
  const acc = new Map<Octant, { count: number; maxKm: number }>()
  for (const o of OCTANTS) acc.set(o, { count: 0, maxKm: 0 })
  for (const r of reports) {
    const e = acc.get(r.octant as Octant)
    if (!e) continue // unknown octant string — skip rather than mis-place
    e.count += 1
    e.maxKm = Math.max(e.maxKm, r.km)
  }
  return OCTANTS.map((o) => ({ octant: o, ...acc.get(o)! }))
}

/** One plain sentence: strongest direction + which way you're deaf. '' when no reports.
 * The distance is in the operator's units (Settings ▸ Units, resolved). */
export function getoutSummary(reports: HeardMe[], units: Units): string {
  const live = octantCoverage(reports).filter((c) => c.count > 0)
  if (live.length === 0) return ''
  const top = live.reduce((a, b) => (b.maxKm > a.maxKm ? b : a))
  const covered = new Set(live.map((c) => c.octant))
  const dead = OCTANTS.filter((o) => !covered.has(o))
  const strong = `strongest toward ${top.octant} (~${fmtDistanceKm(top.maxKm, units)})`
  // Only call out dead directions when coverage is genuinely lopsided (some, but not all).
  if (dead.length > 0 && dead.length < OCTANTS.length - 1) {
    return `${strong}; little/nothing to the ${dead.join('/')}`
  }
  return strong
}

/** The map's spots with every "heard me" report from before the Start fresh time left out, so its
 * "hearing you" dots count what the Getting Out box counts. It goes by each spot's own report time,
 * as the box does — never `asOf` less the age: a cached snapshot keeps its `asOf` while its spots are
 * rebuilt on every poll. The snapshot keeps the newest "heard me" report per station, so a station
 * heard again after the start stays. Other spots are untouched. `since` null → the snapshot as is. */
export function heardMeSince(prop: PropagationSnapshot | null, since: number | null): PropagationSnapshot | null {
  if (!prop?.spots || since == null) return prop
  const spots = prop.spots.filter((s) => !s.heardMe || s.time >= since)
  return spots.length === prop.spots.length ? prop : { ...prop, spots }
}

/** The Start fresh time as the station clock shows it, to the second (a report earlier in the same
 *  minute is left out): HH:MM:SS UTC. */
export function fmtSinceUtc(secs: number): string {
  const d = new Date(secs * 1000)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${p(d.getUTCHours())}:${p(d.getUTCMinutes())}:${p(d.getUTCSeconds())} UTC`
}
