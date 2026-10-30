// The selected station's path ribbon, as polylines both maps draw: the short path and the long
// path (the rest of the same great circle, leaving on the reciprocal bearing), each with the
// stretches the outlook's geometry flags this hour (inside the polar cap / on the grey line) and a
// bearing label for its QTH end. Pure geodesy — the 2-D map projects the points, the globe lifts
// them onto the sphere; neither re-derives the route.
import { haversineKm, type LatLon } from '../grid'
import { destinationPoint } from '../mapGeo'
import type { PathPrediction } from '../types'
import { ribbonRoutes, type RibbonInk, type RibbonRoute, type RouteName } from './pathRoute'

/** The on-air abbreviations — invariant tokens, like the map path card's buttons. */
export const ROUTE_SP = 'SP'
export const ROUTE_LP = 'LP'

/** Earth circumference on the sphere `destinationPoint` uses (radius 6371 km). */
const CIRC_KM = 2 * Math.PI * 6371
/** Below this the long path is the whole circle and has no direction: not drawn. */
const MIN_SPAN_KM = 10
/** Where along each route its bearing label sits (km from the QTH, at most a quarter of it). */
const LABEL_KM = 2000

export interface RibbonLine {
  route: RouteName
  /** The model's reliability ink for the hinted band; null = no figure (draw it neutral). */
  ink: RibbonInk | null
  /** Me → them along this route. */
  points: LatLon[]
  stretches: { kind: 'polar' | 'grey'; points: LatLon[] }[]
  label: string
  labelAt: LatLon
}

/** Exact initial bearing (degrees): `grid.bearingDeg` rounds to the whole degree, which walks a
 * 16 000 km route ~14 km off the station. */
function exactBearing(a: LatLon, b: LatLon): number {
  const r = Math.PI / 180
  const dLon = (b.lon - a.lon) * r
  const y = Math.sin(dLon) * Math.cos(b.lat * r)
  const x =
    Math.cos(a.lat * r) * Math.sin(b.lat * r) -
    Math.sin(a.lat * r) * Math.cos(b.lat * r) * Math.cos(dLon)
  return (Math.atan2(y, x) / r + 360) % 360
}

function sample(me: LatLon, brg: number, km: number, from: number, to: number, n: number): LatLon[] {
  const steps = Math.max(1, Math.round(n * (to - from)))
  return Array.from({ length: steps + 1 }, (_, i) =>
    destinationPoint(me, brg, km * (from + ((to - from) * i) / steps)),
  )
}

function line(
  route: RouteName,
  me: LatLon,
  brg: number,
  km: number,
  n: number,
  model: RibbonRoute | null | undefined,
): RibbonLine {
  return {
    route,
    ink: model?.ink ?? null,
    points: sample(me, brg, km, 0, 1, n),
    stretches: (model?.segments ?? []).map((s) => ({
      kind: s.kind,
      points: sample(me, brg, km, s.from, s.to, n),
    })),
    label: `${route === 'short' ? ROUTE_SP : ROUTE_LP} ${Math.round(brg) % 360}°`,
    labelAt: destinationPoint(me, brg, Math.min(LABEL_KM, km / 4)),
  }
}

/** Both routes from `me` to `dx`, short first. `model` (features/pathRoute `ribbonRoutes`) adds
 * the inks and stretches; without it both routes are plain geometry. */
export function ribbonLines(
  me: LatLon,
  dx: LatLon,
  model?: { short: RibbonRoute; long: RibbonRoute | null } | null,
): RibbonLine[] {
  const km = haversineKm(me, dx)
  if (km < MIN_SPAN_KM) return []
  const brg = exactBearing(me, dx)
  return [
    line('short', me, brg, km, 64, model?.short),
    line('long', me, (brg + 180) % 360, CIRC_KM - km, 128, model?.long),
  ]
}

/** The ribbon for the selected station from the per-path outlook the host holds. The outlook is
 * keyed by grid and arrives after a selection changes, so one whose short path is not this pair's
 * (another station's, still in flight) colours nothing: the routes are then drawn plain. */
export function ribbonFor(
  me: LatLon,
  dx: LatLon,
  outlook: PathPrediction | null | undefined,
  focusBand?: string | null,
): { lines: RibbonLine[]; favoured: RouteName | null } {
  const km = haversineKm(me, dx)
  const g = outlook?.geometry
  const mine = g != null && Math.abs(g.distKm - km) <= 20 + km * 0.01
  const model = mine ? ribbonRoutes(outlook, focusBand) : null
  return { lines: ribbonLines(me, dx, model), favoured: model?.favoured ?? null }
}
