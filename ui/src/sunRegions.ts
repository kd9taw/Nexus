// NOAA's numbered active regions on the sun disc — the pure half (FlareChip draws the disc, the
// map's sun hover lists them).
//
// THE RULE IS THE SHACK'S. The shack (`get_solar_regions`) judges the list on its own clock and
// sends regions only while it is held: its observed day has begun and is under three days old.
// This side draws only while the shack says held AND its own clock is before the shack's `until`
// (so a screen whose polls fail still drops the list on time — `useWakeAt` re-renders at `until`),
// and never a list whose day starts ahead of this clock either. A held flag with no `until`, or no
// observed day, is not a held list. Nothing is ever filled in or rotated forward: the regions are
// drawn where NOAA observed them, and every drawing says which day that was.
//
// THE DISC is drawn as NOAA's images show the sun: solar north up and solar EAST ON THE LEFT (we
// look at the sun from Earth). NOAA's longitudes are positive east of the central meridian. The
// tilt of the sun's axis toward us (under 7.25°) is left out: the dots are a where-on-the-sun
// sketch, not an image.

import type { SolarRegion, SolarRegionsView } from './types'

/** The skew a screen allows a day's start ahead of its own clock (the shack's REGIONS_AHEAD_SECS). */
export const REGIONS_AHEAD_SECS = 120
/** From this far off the central meridian a region is "near the limb". */
export const LIMB_DEG = 60

/** Whether the list was fit to draw: why not, when it is not. */
export type RegionsWhy = 'held' | 'stale' | 'ahead' | 'none' | 'unknown'

/** The regions to draw at `nowMs`, or null when nothing may be drawn (an empty array is a held
 *  list with no numbered region on the visible disc). */
export function regionsShown(v: SolarRegionsView | null | undefined, nowMs: number): SolarRegion[] | null {
  return regionsWhy(v, nowMs) === 'held' ? v!.regions : null
}

/** Why the list is, or is not, drawn at `nowMs`. */
export function regionsWhy(v: SolarRegionsView | null | undefined, nowMs: number): RegionsWhy {
  if (!v || v.why === 'none' || v.observedDay == null) return 'none'
  const now = nowMs / 1000
  if (v.why === 'ahead' || v.observedDay > now + REGIONS_AHEAD_SECS) return 'ahead'
  if (v.why === 'stale') return 'stale'
  if (v.held !== true || v.until == null) return 'unknown'
  return now < v.until ? 'held' : 'stale'
}

/** When a held list stops being drawn, for `useWakeAt`. */
export function regionsUntil(v: SolarRegionsView | null | undefined): number | null {
  return v?.held === true ? (v.until ?? null) : null
}

/** A region's place on the unit disc: x right, y up; solar north up, east on the left. */
export function regionDiscXY(r: SolarRegion): { x: number; y: number } {
  const D = Math.PI / 180
  return { x: -Math.cos(r.latDeg * D) * Math.sin(r.lonDeg * D), y: Math.sin(r.latDeg * D) }
}

/** The limb a region is near (≥ LIMB_DEG from the central meridian), or null. */
export function limbSide(r: SolarRegion): 'east' | 'west' | null {
  return r.lonDeg >= LIMB_DEG ? 'east' : r.lonDeg <= -LIMB_DEG ? 'west' : null
}

/** The observed day as the operator reads it ("29 Oct"), UTC. */
export function regionsDayLabel(unix: number): string {
  return new Date(unix * 1000).toLocaleDateString(undefined, { day: 'numeric', month: 'short', timeZone: 'UTC' })
}
