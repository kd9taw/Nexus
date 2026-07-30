// Geo/projection helpers for the Map surface — pure, offline, Canvas2D-oriented.
// Default projection is azimuthal-equidistant (AEQD) centered on the operator's
// grid: every operator→point great circle is a straight radial (= true beam
// heading) and concentric range rings are exact great-circle distance. A
// secondary equirectangular "world" projection reuses the same renderer/data.
// Basemap is the bundled world-atlas 110m TopoJSON — no tiles, no network, no key.
import {
  geoAzimuthalEquidistant,
  geoEquirectangular,
  geoOrthographic,
  geoCircle,
  geoGraticule,
  type GeoProjection,
  type GeoPermissibleObjects,
} from 'd3-geo'
import { feature, mesh } from 'topojson-client'
import countriesTopo from 'world-atlas/countries-50m.json'
import statesTopo from 'us-atlas/states-10m.json'
import type { LatLon } from './grid'

export type Projection = 'globe' | 'aeqd' | 'world'

/** Interactive view controls: zoom (scale multiplier), orthographic rotation
 * `[λ, φ]` in degrees (Globe only; null = centered on the operator), and a screen
 * pan offset. Drag rotates the globe / pans the flat maps; the wheel zooms. */
export interface MapView3 {
  zoom: number
  rotate: [number, number] | null
  panX: number
  panY: number
}

const KM_PER_DEG = 111.195 // great-circle km per degree

/** Bundled 110m countries as a GeoJSON FeatureCollection (decoded once). */
let basemapCache: GeoPermissibleObjects | null = null
export function basemap(): GeoPermissibleObjects {
  if (!basemapCache) {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const topo = countriesTopo as any
    basemapCache = feature(topo, topo.objects.countries) as unknown as GeoPermissibleObjects
  }
  return basemapCache
}

/** US state boundaries (us-atlas 10m, lon/lat) as a single-line mesh — shared borders
 * drawn once (no doubled strokes). A core operating layer: an op needs to read which
 * STATE a spot/QTH is in, not just the coastline. Decoded once. */
let statesCache: GeoPermissibleObjects | null = null
export function usStateBorders(): GeoPermissibleObjects {
  if (!statesCache) {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const topo = statesTopo as any
    statesCache = mesh(topo, topo.objects.states) as unknown as GeoPermissibleObjects
  }
  return statesCache
}

/** A 20°×10° graticule (Maidenhead field boundaries) as a GeoJSON object.
 * Cached like its three siblings above — it was the one basemap geometry
 * rebuilt (2,519 coordinates) inside the draw effect on every call. */
let graticuleCache: GeoPermissibleObjects | null = null
export function graticule(): GeoPermissibleObjects {
  if (!graticuleCache) {
    graticuleCache = geoGraticule().step([20, 10])() as unknown as GeoPermissibleObjects
  }
  return graticuleCache
}

/** Zoom limits for the interactive view (wheel). The ceiling has to clear
 * [`APRS_HOME_ZOOM`] — a local packet picture needs far more magnification than a
 * propagation map, and the old ceiling of 10 could not reach it. */
export const MIN_ZOOM = 0.5
export const MAX_ZOOM = 200

/** Opening zoom for the APRS section's embedded map.
 *
 * APRS is a LOCAL, terrestrial mode: 2 m simplex plus a digipeater or two reaches
 * tens of km, not thousands. On the shared globe at `zoom: 1` the Earth's radius
 * maps to the canvas radius — about 23 km per pixel — so a station 40 km away
 * plotted 1.7 px from the operator, underneath their own QTH marker. Every local
 * station drew as one indistinguishable smear and the map read as empty.
 *
 * 25 puts that same station 43 px out and reaches roughly 275 km in every
 * direction — far enough that WIDE2-2 digipeated traffic is still on the map,
 * close enough that a local net spreads across it instead of stacking up. */
export const APRS_HOME_ZOOM = 25

/**
 * Where the APRS map should be centred: the operator's QTH when their grid is set,
 * otherwise the middle of the traffic they can actually hear.
 *
 * The fallback exists because [`MapView`] skips the whole draw without a centre —
 * with no grid configured the APRS section showed a blank box while its station
 * list filled up. This map is about OTHER stations, so it does not need to know
 * where the operator is. Returns null only when there is neither.
 *
 * Longitudes are averaged as unit vectors, not arithmetically: a net straddling the
 * antimeridian (NZ/Fiji) would otherwise average to lon 0 and centre the map on the
 * Gulf of Guinea — the Null-Island bug this codebase has met before.
 */
export function aprsMapCenter(
  operator: LatLon | null,
  heard: ReadonlyArray<{ lat?: number | null; lon?: number | null }>,
): LatLon | null {
  if (operator) return operator
  let n = 0
  let lat = 0
  let x = 0
  let y = 0
  for (const h of heard) {
    if (h.lat == null || h.lon == null || !Number.isFinite(h.lat) || !Number.isFinite(h.lon)) {
      continue
    }
    const rad = (h.lon * Math.PI) / 180
    lat += h.lat
    x += Math.cos(rad)
    y += Math.sin(rad)
    n += 1
  }
  if (n === 0) return null
  return { lat: lat / n, lon: (Math.atan2(y / n, x / n) * 180) / Math.PI }
}

/**
 * Build a d3 projection for the chosen view. Globe = orthographic (a real 3-D
 * sphere you can spin); AEQD = azimuthal-equidistant disc rotated to put `center`
 * at the screen centre (true beam headings + range rings); World = equirectangular.
 * `view` applies zoom (scale), Globe rotation, and a pan offset.
 */
export function makeProjection(
  kind: Projection,
  center: LatLon | null,
  width: number,
  height: number,
  view?: MapView3,
): GeoProjection {
  const zoom = view?.zoom ?? 1
  const panX = view?.panX ?? 0
  const panY = view?.panY ?? 0
  const c = center ?? { lat: 0, lon: 0 }

  if (kind === 'globe') {
    // A spinnable 3-D globe. Default orientation centers the operator; dragging
    // sets an explicit rotation. clipAngle(90) hides the far hemisphere.
    const radius = (Math.min(width, height) / 2) * 0.92 * zoom
    const rot = view?.rotate ?? [-c.lon, -c.lat]
    return geoOrthographic()
      .rotate(rot)
      .clipAngle(90)
      .translate([width / 2 + panX, height / 2 + panY])
      .scale(radius)
  }

  if (kind === 'world') {
    const p = geoEquirectangular().fitSize([width, height], { type: 'Sphere' })
    if (zoom !== 1 || panX || panY) {
      const s0 = p.scale()
      const [tx, ty] = p.translate()
      p.scale(s0 * zoom)
      // Zoom about the canvas centre, then apply pan.
      p.translate([width / 2 + (tx - width / 2) * zoom + panX, height / 2 + (ty - height / 2) * zoom + panY])
    }
    return p
  }

  // AEQD beam map.
  const radius = (Math.min(width, height) / 2) * 0.94 * zoom
  return geoAzimuthalEquidistant()
    .rotate([-c.lon, -c.lat])
    .clipAngle(180)
    .translate([width / 2 + panX, height / 2 + panY])
    .scale(radius / Math.PI) // antipode (π rad) → disc rim
}

/** Project a lat/lon to screen `[x, y]`, or null if clipped/invalid. */
export function project(proj: GeoProjection, ll: LatLon): [number, number] | null {
  const p = proj([ll.lon, ll.lat])
  if (!p || !Number.isFinite(p[0]) || !Number.isFinite(p[1])) return null
  return [p[0], p[1]]
}

/** A range-ring (great-circle circle) of `km` around `center` as a GeoJSON polygon. */
export function rangeRing(center: LatLon, km: number): GeoPermissibleObjects {
  return geoCircle()
    .center([center.lon, center.lat])
    .radius(km / KM_PER_DEG)() as unknown as GeoPermissibleObjects
}

/** Great-circle destination point `km` from `center` along initial `bearingDeg`. */
export function destinationPoint(center: LatLon, bearingDeg: number, km: number): LatLon {
  const R = 6371
  const d = km / R
  const th = (bearingDeg * Math.PI) / 180
  const la1 = (center.lat * Math.PI) / 180
  const lo1 = (center.lon * Math.PI) / 180
  const la2 = Math.asin(Math.sin(la1) * Math.cos(d) + Math.cos(la1) * Math.sin(d) * Math.cos(th))
  const lo2 =
    lo1 + Math.atan2(Math.sin(th) * Math.sin(d) * Math.cos(la1), Math.cos(d) - Math.sin(la1) * Math.sin(la2))
  return { lat: (la2 * 180) / Math.PI, lon: (((lo2 * 180) / Math.PI + 540) % 360) - 180 }
}

/** Spherical great-circle line `a`→`b` as a GeoJSON LineString (geoPath clips it). */
export function greatCircle(a: LatLon, b: LatLon): GeoPermissibleObjects {
  return {
    type: 'LineString',
    coordinates: [
      [a.lon, a.lat],
      [b.lon, b.lat],
    ],
  } as unknown as GeoPermissibleObjects
}

// ── Day/night terminator (greyline) ───────────────────────────────────────────
// The subsolar point + twilight bands. Formulas MIRROR the Rust source of truth
// in crates/propagation/src/geo.rs (Cooper's declination + equation of time), so
// the map's terminator and the engine's solar elevation never disagree.

const norm180 = (deg: number) => ((deg + 540) % 360) - 180

/** Day-of-year (1–366) in UTC. */
function dayOfYearUtc(ms: number): number {
  const d = new Date(ms)
  const start = Date.UTC(d.getUTCFullYear(), 0, 1)
  return Math.floor((ms - start) / 86_400_000) + 1
}

/**
 * The subsolar point (sun directly overhead) at `nowMs` — latitude = solar
 * declination, longitude where the hour angle is zero. ±~0.5° (plenty for a map).
 * Mirrors geo.rs `solar_declination_deg` + `equation_of_time_min`.
 */
export function subsolarPoint(nowMs: number): LatLon {
  const b = ((2 * Math.PI) / 365) * (dayOfYearUtc(nowMs) - 81)
  const declDeg = 23.45 * Math.sin(b)
  const eqMin = 9.87 * Math.sin(2 * b) - 7.53 * Math.cos(b) - 1.5 * Math.sin(b)
  const d = new Date(nowMs)
  const utcHours = d.getUTCHours() + d.getUTCMinutes() / 60 + d.getUTCSeconds() / 3600
  // solar elevation uses hra = ((utcHours + eq/60 − 12)·15 + lon); subsolar lon
  // is where hra = 0.
  const lon = norm180(-(utcHours + eqMin / 60 - 12) * 15)
  return { lat: declDeg, lon }
}

/** A twilight band of the night side, as a great-circle cap around the
 * antisolar point. A point's solar elevation = 90° − (its angular distance from
 * the subsolar point), so "elevation < e" is the cap of angular radius (90+e)
 * around the ANTIsolar point: 90°=civil/day-line, 84°=−6° nautical, 78°=−12°
 * astronomical, 72°=−18° full night. */
export interface Terminator {
  /** Nested night caps, lightest (day/night line) first → darkest (full night). */
  caps: GeoPermissibleObjects[]
  /** The day/night line itself (90° cap boundary) — the greyline DX window. */
  line: GeoPermissibleObjects
  subsolar: LatLon
}

export function terminator(nowMs: number): Terminator {
  const ss = subsolarPoint(nowMs)
  const anti: [number, number] = [norm180(ss.lon + 180), -ss.lat]
  const cap = (radiusDeg: number) =>
    geoCircle().center(anti).radius(radiusDeg)() as unknown as GeoPermissibleObjects
  return {
    caps: [cap(90), cap(84), cap(78), cap(72)],
    line: cap(90),
    subsolar: ss,
  }
}

// ── MUF / foF2 model (the "modelled" overlay) ─────────────────────────────────
// Solar elevation = 90° − angular distance from the subsolar point. The foF2/MUF
// formula MIRRORS crates/propagation/src/likelihood.rs (PathModel.fof2 defaults)
// + the standard MUF(3000) obliquity factor — it's a glanceable modelled field,
// not a measurement (badge it "modelled"). One source of truth for the *physics*
// stays in Rust; this is the visual twin, like the greyline.
const DEG = Math.PI / 180
const FOF2_A = 4.0
const FOF2_B = 0.04
const FOF2_P = 0.25
const FOF2_FLOOR = 3.0
const M3000 = 3.0 // foF2 → MUF(3000 km) nominal obliquity factor

/** Solar elevation (deg, −90..+90) at a point — >0 day, ~0 greyline. */
export function solarElevationDeg(lat: number, lon: number, nowMs: number): number {
  const ss = subsolarPoint(nowMs)
  const c =
    Math.sin(lat * DEG) * Math.sin(ss.lat * DEG) +
    Math.cos(lat * DEG) * Math.cos(ss.lat * DEG) * Math.cos((lon - ss.lon) * DEG)
  return 90 - Math.acos(Math.max(-1, Math.min(1, c))) / DEG
}

export interface NextTerminator {
  atMs: number
  kind: 'rise' | 'set'
}

/** The next sunrise OR sunset at (lat, lon) strictly after `nowMs` — found by scanning
 *  solar elevation forward for a horizon (0°) crossing in coarse 5-min steps, then
 *  bisecting to the minute. Reuses solarElevationDeg verbatim, so it can never disagree
 *  with the drawn terminator. Polar day/night (no crossing within ~25 h) returns the
 *  horizon time with the kind that would END the current state. */
export function nextTerminatorMs(lat: number, lon: number, nowMs: number): NextTerminator {
  const STEP = 5 * 60 * 1000
  const HORIZON = 25 * 60 * 60 * 1000 // > 24 h so a crossing is always found unless polar
  let t0 = nowMs
  let e0 = solarElevationDeg(lat, lon, t0)
  for (let t = nowMs + STEP; t <= nowMs + HORIZON; t += STEP) {
    const e1 = solarElevationDeg(lat, lon, t)
    if (Math.sign(e1) !== Math.sign(e0)) {
      let lo = t0
      let hi = t
      while (hi - lo > 60 * 1000) {
        const mid = (lo + hi) / 2
        if (Math.sign(solarElevationDeg(lat, lon, mid)) === Math.sign(e0)) lo = mid
        else hi = mid
      }
      return { atMs: Math.round(hi), kind: e0 < 0 ? 'rise' : 'set' }
    }
    t0 = t
    e0 = e1
  }
  return { atMs: nowMs + HORIZON, kind: e0 >= 0 ? 'set' : 'rise' }
}

/** Modelled MUF(3000 km) in MHz at a point, from SFI + solar elevation. */
export function mufMhz(lat: number, lon: number, nowMs: number, sfi: number): number {
  const elev = solarElevationDeg(lat, lon, nowMs)
  const day = elev > 0 ? (FOF2_A + FOF2_B * sfi) * Math.pow(Math.sin(elev * DEG), FOF2_P) : 0
  return Math.max(day, FOF2_FLOOR) * M3000
}

// ── Solar-flare D-region absorption (NOAA D-RAP, X-ray part) ─────────────────
// A flare's X-rays ionize the D-layer on the SUNLIT hemisphere only (flares are
// line-of-sight), absorbing HF strongest under the sun. NOAA's D-RAP model:
// subsolar Highest Affected Frequency HAF = 10·log10(flux) + 65 MHz (GOES
// 0.1–0.8 nm long flux, W/m²; anchors M1→15 MHz, X1→25 MHz), tapering with
// solar zenith angle χ as cos(χ)^0.75 (cos χ = sin(solar elevation)); absorption
// at frequency f is (HAF/f)^1.5 dB. Quiet sun (A/B-class) puts HAF below HF, so
// a HAF-driven overlay draws nothing — event-driven by physics, not by toggles.
// R-scale thresholds MIRROR crates/propagation/src/model.rs `r_scale`.

/** Subsolar-point Highest Affected Frequency (MHz, ≥0) for a GOES long-band
 * X-ray flux. ~5 MHz at C1, 15 at M1, 25 at X1 — the D-RAP ceiling. */
export function flareHafMhz(xrayLong: number): number {
  if (!(xrayLong > 0)) return 0
  return Math.max(0, 10 * Math.log10(xrayLong) + 65)
}

/** Local Highest Affected Frequency (MHz) at a point: the subsolar HAF tapered
 * by cos(χ)^0.75. Zero on the night side. */
export function flareHafAt(lat: number, lon: number, nowMs: number, xrayLong: number): number {
  const elev = solarElevationDeg(lat, lon, nowMs)
  if (elev <= 0) return 0
  return flareHafMhz(xrayLong) * Math.pow(Math.sin(elev * DEG), 0.75)
}

/** NOAA radio-blackout R-scale (0 = none) from the GOES long X-ray flux —
 * the TS mirror of model.rs `r_scale` (R1 ≥ M1 … R5 ≥ X20). */
export function flareRScale(xrayLong: number): number {
  if (xrayLong >= 2e-3) return 5
  if (xrayLong >= 1e-3) return 4
  if (xrayLong >= 1e-4) return 3
  if (xrayLong >= 5e-5) return 2
  if (xrayLong >= 1e-5) return 1
  return 0
}

/** Flare class label ("M2.3", "X1.0") from the GOES long flux — display only. */
export function flareClass(xrayLong: number): string {
  if (!(xrayLong > 0)) return 'A0.0'
  const bands: Array<[number, string]> = [
    [1e-4, 'X'],
    [1e-5, 'M'],
    [1e-6, 'C'],
    [1e-7, 'B'],
  ]
  for (const [floor, letter] of bands) {
    if (xrayLong >= floor) return `${letter}${(xrayLong / floor).toFixed(1)}`
  }
  return `A${(xrayLong / 1e-8).toFixed(1)}`
}

/** One sampled point of the dayside absorption field. */
export interface FlareSample {
  lat: number
  lon: number
  /** Local Highest Affected Frequency, MHz. */
  haf: number
}

/** The dayside HAF field sampled on a lat/lon grid — flareHafAt's math with the
 * subsolar point HOISTED (calling flareHafAt per point recomputes the subsolar
 * position ~3k×; here the per-point work is one dot product: cos χ =
 * sin(elevation) = the great-circle cosine to the subsolar point). Points below
 * 2 MHz (night side / terminator fringe) are dropped. */
export function flareField(
  nowMs: number,
  xrayLong: number,
  stepLat = 4,
  stepLon = 5,
): FlareSample[] {
  const sub = flareHafMhz(xrayLong)
  if (sub <= 0) return []
  const ss = subsolarPoint(nowMs)
  const sinSs = Math.sin(ss.lat * DEG)
  const cosSs = Math.cos(ss.lat * DEG)
  const out: FlareSample[] = []
  for (let lat = -88; lat <= 88; lat += stepLat) {
    const sinLa = Math.sin(lat * DEG)
    const cosLa = Math.cos(lat * DEG)
    for (let lon = -177.5; lon < 180; lon += stepLon) {
      const cosChi = sinLa * sinSs + cosLa * cosSs * Math.cos((lon - ss.lon) * DEG)
      if (cosChi <= 0) continue
      const haf = sub * Math.pow(cosChi, 0.75)
      if (haf < 2) continue
      out.push({ lat, lon, haf })
    }
  }
  return out
}

/** D-RAP's empirical fade-recovery estimate (minutes) from the CURRENT flux:
 * 32.19·L² + 323.45·L + 837.2 for L = log10(flux). The published fit covers
 * M1–X5 (L −5…−3.3): ≈25 min at M1, ≈60 at X1, capped ≈120 above X5. Below M1
 * there is no published fit → null (the chip just omits the estimate). */
export function flareRecoveryMin(xrayLong: number): number | null {
  if (!(xrayLong >= 1e-5)) return null
  const l = Math.min(-3.3, Math.log10(xrayLong))
  return 32.19 * l * l + 323.45 * l + 837.2
}

/** A lat/lon grid cell for the MUF heatmap (geometry is static; color recomputed). */
export interface MufCell {
  center: LatLon
  poly: GeoPermissibleObjects
}

/** Build the static MUF grid cells (default 10°×15°). */
export function mufCells(stepLat = 10, stepLon = 15): MufCell[] {
  const cells: MufCell[] = []
  for (let lat = -90; lat < 90; lat += stepLat) {
    for (let lon = -180; lon < 180; lon += stepLon) {
      const poly = {
        type: 'Polygon',
        coordinates: [
          [
            [lon, lat],
            [lon + stepLon, lat],
            [lon + stepLon, lat + stepLat],
            [lon, lat + stepLat],
            [lon, lat],
          ],
        ],
      } as unknown as GeoPermissibleObjects
      cells.push({ center: { lat: lat + stepLat / 2, lon: lon + stepLon / 2 }, poly })
    }
  }
  return cells
}

/**
 * The ±22.5° opening wedge as a CLOSED ring of [lon, lat], subdivided so every edge is short
 * enough to hug the sphere.
 *
 * ⚠️ WHY THE SUBDIVISION IS THE WHOLE POINT (operator report: green spikes tearing through the
 * 3-D globe, while the 2-D map was clean). Nothing here draws great circles: `syncLines` maps
 * each vertex to a 3-D point and draws a STRAIGHT chord between them, and globe.gl's polygon
 * layer triangulates flat between vertices too. A chord between two points `d` km apart sags
 * `R(1 - cos(d/2R))` BELOW the surface at its midpoint — for a 3000 km radial that is ~78 km,
 * against a layer altitude of only ~0.006 R (~38 km). So the geometry dived through the globe
 * and poked back out: the spikes.
 *
 * The far arc was never the problem (16 steps across 45° puts its vertices ~2.8° apart). The
 * two RADIAL edges were each a SINGLE segment spanning the full `maxKm`. So walk out along one
 * radial, across the arc, and back down the other — every edge short.
 *
 * 2-D is unaffected because it projects to a plane, where a straight edge is straight.
 *
 * Shared by the fill and the outline deliberately: they drew the same wedge from two copies of
 * the geometry, so a fix to one silently left the other torn.
 */
export function sectorRing(
  qth: { lat: number; lon: number },
  bearingDeg: number,
  maxKm: number,
): [number, number][] {
  // ~200 km per step keeps the worst-case sag well under a kilometre; clamped so a short
  // opening still reads as a wedge and a very long one can't explode the vertex count.
  const radialSteps = Math.max(4, Math.min(48, Math.ceil(maxKm / 200)))
  const ARC_STEPS = 16
  const left = bearingDeg - 22.5
  const right = bearingDeg + 22.5
  const ring: [number, number][] = []
  // Out along the left radial (skip 0 km — that IS the QTH, added last when we close).
  for (let i = 1; i <= radialSteps; i++) {
    const d = destinationPoint(qth, left, (maxKm * i) / radialSteps)
    ring.push([d.lon, d.lat])
  }
  // Across the far arc, left → right.
  for (let i = 1; i < ARC_STEPS; i++) {
    const d = destinationPoint(qth, left + (45 * i) / ARC_STEPS, maxKm)
    ring.push([d.lon, d.lat])
  }
  // Back down the right radial to the QTH.
  for (let i = radialSteps; i >= 1; i--) {
    const d = destinationPoint(qth, right, (maxKm * i) / radialSteps)
    ring.push([d.lon, d.lat])
  }
  ring.push([qth.lon, qth.lat])
  ring.push(ring[0]) // close
  return ring
}
