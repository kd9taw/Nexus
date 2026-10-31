// A solar flare on the map, as NOAA describes it — the pure half of the flare layer (MapView and
// Globe3D draw it; the chip words it).
//
// TRUTH AND GAP-FILLER. NOAA's D-RAP grid (`drap`, sent by the shack only while the flux is M1+)
// is the authoritative footprint while it is current (≤ DRAP_FRESH_SECS old). Otherwise the
// analytic D-RAP field from the GOES flux (mapGeo `flareHafMhz`, cos χ^0.75) stands in, and every
// surface says "estimated from GOES". Simulated flux (the Preview button, the dev override) never
// takes NOAA's grid or event: a simulation drawn with NOAA's name on it would be a lie.
//
// PER-BAND RINGS. Under the D-RAP law band f loses ≥ A dB wherever the local HAF ≥ f·A^(2/3), so
// each band's 1 dB and 10 dB lines are small circles round the sub-solar point: closed form for
// the analytic field, and traced through NOAA's grid (bearing by bearing) when the grid is the
// footprint. Both hand back [lon, lat] rings, so the 2-D map and the globe draw the same lines.
//
// PHASES. NOAA's event list says began / peak / ended in NOAA's words. When it is silent or stale,
// or its event is a previous flare (the flux is now above that event's peak, or GOES's own trace
// has turned up again since it ended), the phase falls back to the direction of the fast lane's
// GOES trace, marked estimated, and to no word at all without a current trace. That trace is one
// point per five minutes, each bucket's PEAK, so its last point can be up to ~4 minutes old and
// higher than the flux now: the GOES sample the chip shows is its newest point (`withNewest`).
// Never the shack's 3-hour trend: that one compares now with the oldest sample in three hours, so
// it says "rising" all through an ordinary flare's decay.
//
// EXTENSION POINT. A later overlay (the proton cap, the aurora edge) adds its own footprint
// function beside `flareFootprint` and hands its rings to the same `WxRing` drawing on both
// surfaces; nothing here is flare-only except the physics.

import type { DrapGrid, FlareEvent } from './types'
import {
  destinationPoint,
  flareField,
  flareHafMhz,
  flareRScale,
  flareRecoveryMin,
  solarElevationDeg,
  subsolarPoint,
  type FlareSample,
} from './mapGeo'

const DEG = Math.PI / 180
const KM_PER_DEG = 111.195
/** 10 dB at f needs a local HAF of f·10^(2/3). */
const TEN_DB = Math.pow(10, 2 / 3)

/** NOAA updates the grid every minute; older than this it is not the picture of now. */
export const DRAP_FRESH_SECS = 180
/** The ten HF bands the rings are drawn for, at their FT8 watering holes (MHz). 6 m is not
 *  among them: no flare's HAF reaches 50 MHz (X20 is ~38). */
export const HF_BANDS: ReadonlyArray<readonly [string, number]> = [
  ['160m', 1.84],
  ['80m', 3.573],
  ['60m', 5.357],
  ['40m', 7.074],
  ['30m', 10.136],
  ['20m', 14.074],
  ['17m', 18.1],
  ['15m', 21.074],
  ['12m', 24.915],
  ['10m', 28.074],
]

/** Where the footprint came from: NOAA's grid, or the analytic field from the GOES flux. */
export type FootprintSource = 'noaa' | 'goes'

export interface FlareFootprint {
  source: FootprintSource
  /** NOAA's grid valid time when `source` is 'noaa'. */
  validAt: number | null
  /** HAF (MHz) at the sub-solar point now. */
  hafSub: number
  /** |NOAA grid − analytic| at the sub-solar point (MHz), when the grid is the footprint. */
  gapMhz: number | null
  /** Local HAF (MHz) at a point; 0 on the night side. */
  hafAt: (lat: number, lon: number) => number
  /** The field sampled on the map's lattice (sunlit, ≥ 2 MHz). */
  field: FlareSample[]
}

/** Whether NOAA's grid is current enough to be the footprint at `nowMs`. */
export function drapFresh(grid: DrapGrid | null | undefined, nowMs: number): grid is DrapGrid {
  if (!grid) return false
  const age = nowMs / 1000 - grid.validAt
  return age < DRAP_FRESH_SECS && age > -120
}

/** NOAA's grid at a point, bilinear (the TS mirror of `DrapGrid::at`): outer rows beyond the
 *  edges, longitudes wrapping the dateline. */
export function drapAt(grid: DrapGrid, lat: number, lon: number): number {
  const rows = grid.lats.length
  const cols = grid.lons.length
  if (rows < 2 || cols < 2 || grid.haf.length !== rows * cols) return 0
  const dlat = grid.lats[0] - grid.lats[1]
  const r = Math.max(0, Math.min(rows - 1, (grid.lats[0] - lat) / dlat))
  const r0 = Math.min(Math.floor(r), rows - 2)
  const fr = r - r0
  const dlon = grid.lons[1] - grid.lons[0]
  const c = (lon - grid.lons[0]) / dlon
  const cf = Math.floor(c)
  const c0 = ((cf % cols) + cols) % cols
  const c1 = (c0 + 1) % cols
  const fc = c - cf
  const v = (rr: number, cc: number) => grid.haf[rr * cols + cc]
  const top = v(r0, c0) * (1 - fc) + v(r0, c1) * fc
  const bot = v(r0 + 1, c0) * (1 - fc) + v(r0 + 1, c1) * fc
  return top * (1 - fr) + bot * fr
}

/** The footprint to draw for `xray` at `nowMs`: NOAA's grid while it is current, else the
 *  analytic field. Null below M1 (the layer's onset, the same as the insight and the toast) and
 *  without a flux. Pass `grid` null for simulated flux. */
export function flareFootprint(
  nowMs: number,
  xray: number | null,
  grid: DrapGrid | null | undefined,
): FlareFootprint | null {
  if (xray == null || !(xray > 0) || flareRScale(xray) < 1) return null
  const ss = subsolarPoint(nowMs)
  const sinSs = Math.sin(ss.lat * DEG)
  const cosSs = Math.cos(ss.lat * DEG)
  // cos χ = the great-circle cosine to the sub-solar point (flareField's hoisted form).
  const cosChi = (lat: number, lon: number) =>
    Math.sin(lat * DEG) * sinSs + Math.cos(lat * DEG) * cosSs * Math.cos((lon - ss.lon) * DEG)
  const analyticSub = flareHafMhz(xray)
  if (drapFresh(grid, nowMs)) {
    const g = grid
    // X-rays are line-of-sight: the flare footprint is the sunlit side of NOAA's grid. (Its night
    // cells are the proton part, which the polar-cap layer draws.)
    const hafAt = (lat: number, lon: number) => (cosChi(lat, lon) > 0 ? drapAt(g, lat, lon) : 0)
    const hafSub = drapAt(g, ss.lat, ss.lon)
    const field: FlareSample[] = []
    for (let lat = -88; lat <= 88; lat += 4) {
      for (let lon = -177.5; lon < 180; lon += 5) {
        const haf = hafAt(lat, lon)
        if (haf >= 2) field.push({ lat, lon, haf })
      }
    }
    return { source: 'noaa', validAt: g.validAt, hafSub, gapMhz: Math.abs(hafSub - analyticSub), hafAt, field }
  }
  const hafAt = (lat: number, lon: number) => {
    const c = cosChi(lat, lon)
    return c > 0 ? analyticSub * Math.pow(c, 0.75) : 0
  }
  return { source: 'goes', validAt: null, hafSub: analyticSub, gapMhz: null, hafAt, field: flareField(nowMs, xray) }
}

/** One band's absorption line: where `band` loses `db` dB, as a closed [lon, lat] ring. */
export interface WxRing {
  band: string
  mhz: number
  db: 1 | 10
  ring: [number, number][]
}

/** The 1 dB and 10 dB rings of every band the flare reaches, widest band first. */
export function bandRings(foot: FlareFootprint, nowMs: number): WxRing[] {
  const ss = subsolarPoint(nowMs)
  const out: WxRing[] = []
  for (const [band, mhz] of HF_BANDS) {
    for (const db of [1, 10] as const) {
      const thr = db === 1 ? mhz : mhz * TEN_DB
      if (!(foot.hafSub >= thr)) continue
      const ring =
        foot.source === 'goes'
          ? circle(ss, Math.acos(Math.pow(thr / foot.hafSub, 4 / 3)) / DEG)
          : traced(foot.hafAt, ss, thr)
      out.push({ band, mhz, db, ring })
    }
  }
  return out
}

/** Bearing step (degrees) round a ring: at a 90° radius a chord sags ~6 km, far under the
 *  globe's line altitude. */
const RING_STEP = 4

/** A small circle `radiusDeg` round `c`, closed, as [lon, lat]. */
function circle(c: { lat: number; lon: number }, radiusDeg: number): [number, number][] {
  const ring: [number, number][] = []
  for (let b = 0; b < 360; b += RING_STEP) {
    const p = destinationPoint(c, b, radiusDeg * KM_PER_DEG)
    ring.push([p.lon, p.lat])
  }
  ring.push(ring[0])
  return ring
}

/** The `thr` contour of `hafAt` traced outward from the sub-solar point along each bearing: the
 *  last point still at or above it (1° steps, then bisected to ~0.06°), closed, as [lon, lat]. */
function traced(
  hafAt: (lat: number, lon: number) => number,
  c: { lat: number; lon: number },
  thr: number,
): [number, number][] {
  const inside = (b: number, d: number) => {
    const p = destinationPoint(c, b, d * KM_PER_DEG)
    return hafAt(p.lat, p.lon) >= thr
  }
  const ring: [number, number][] = []
  for (let b = 0; b < 360; b += RING_STEP) {
    let lo = 0
    while (lo < 90 && inside(b, lo + 1)) lo += 1
    let hi = Math.min(90, lo + 1)
    for (let i = 0; i < 4; i++) {
      const mid = (lo + hi) / 2
      if (inside(b, mid)) lo = mid
      else hi = mid
    }
    const p = destinationPoint(c, b, lo * KM_PER_DEG)
    ring.push([p.lon, p.lat])
  }
  ring.push(ring[0])
  return ring
}

/** What the flare does at one place: night side (nothing), else the highest band OUT (≥ 10 dB)
 *  and the highest band WEAKENED (≥ 1 dB), each null when none. */
export interface FlareHere {
  night: boolean
  out: string | null
  weak: string | null
}

export function flareHere(foot: FlareFootprint, lat: number, lon: number, nowMs: number): FlareHere {
  if (solarElevationDeg(lat, lon, nowMs) <= 0) return { night: true, out: null, weak: null }
  return { night: false, ...bandsAt(foot.hafAt(lat, lon)) }
}

/** The highest band at ≥ 10 dB and at ≥ 1 dB for a local HAF. */
function bandsAt(haf: number): { out: string | null; weak: string | null } {
  let out: string | null = null
  let weak: string | null = null
  for (const [band, mhz] of HF_BANDS) {
    if (haf >= mhz * TEN_DB) out = band
    if (haf >= mhz) weak = band
  }
  return { out, weak }
}

/** The highest band out (≥ 10 dB) and weakened (≥ 1 dB) under the sun. */
export function flareUnderSun(foot: FlareFootprint): { out: string | null; weak: string | null } {
  return bandsAt(foot.hafSub)
}

export type FlarePhaseKind = 'rising' | 'peaked' | 'ended' | 'falling'

/** Where the flare is: in NOAA's words (`noaa`) with NOAA's times, or the GOES trace's direction. */
export interface FlarePhase {
  kind: FlarePhaseKind | null
  noaa: boolean
  beginAt: number | null
  peakAt: number | null
  peakClass: string | null
  endAt: number | null
}

/** NOAA's event list is this flare's while it was updated in the last 10 minutes, began in the
 *  last 6 hours, and has not been over for more than an hour. */
export const EVENT_FRESH_SECS = 600

/** A flux this far above an event's peak is a new flare, not that event (GOES's sample and NOAA's
 *  peak differ a little for the same flare). The same margin over the trace's low since NOAA's end
 *  is a new rise, not the decay's wobble. */
const NEW_FLARE_OVER_PEAK = 1.1
/** The trace speaks for now only while its newest sample is this recent (the UI's X-ray age). */
const TRACE_LIVE_SECS = 600
/** The direction word compares the newest sample with the trace's last half hour before it. */
const TRACE_WINDOW_SECS = 30 * 60
/** The flare layer's onset (M1). */
const M1 = 1e-5

const CLASS_BASE: Record<string, number> = { A: 1e-8, B: 1e-7, C: 1e-6, M: 1e-5, X: 1e-4 }

/** The flux a class names ("M5.2" → 5.2e-5); null for anything else. */
function classFlux(cls: string | null): number | null {
  const m = /^([ABCMX])(\d+(?:\.\d+)?)$/.exec(cls?.trim() ?? '')
  return m ? CLASS_BASE[m[1]] * Number(m[2]) : null
}

/** The lane's trace with the GOES sample the chip shows (`xray` at `xrayAt`) as its newest point.
 *  The trace is 5-minute bucket PEAKS (propagation `xray_series`), so its last point is the current
 *  bucket's high, not its newest sample: read as "now", a peak early in a bucket says Rising for up
 *  to ~4 minutes while the flux beside it falls. The sample joins only a current trace (its last
 *  point ≤ TRACE_LIVE_SECS old, the readers' own rule) and only when it is later than that point;
 *  otherwise the trace is unchanged, and an old one still gives no word. */
function withNewest(
  series: ReadonlyArray<[number, number]> | undefined,
  xray: number | null,
  xrayAt: number | null | undefined,
  now: number,
): ReadonlyArray<[number, number]> {
  const s = series ?? []
  if (xray == null || xrayAt == null || s.length === 0) return s
  const lastAt = s[s.length - 1][0]
  if (now - lastAt > TRACE_LIVE_SECS || xrayAt <= lastAt) return s
  return [...s, [xrayAt, xray]]
}

/** Whether GOES's trace has turned up again since `since`: its newest sample is recent and more
 *  than the margin above the lowest earlier sample since then. No trace, too few samples or an
 *  old one is "no": without a measurement nothing says it rose. */
function risenSince(series: ReadonlyArray<[number, number]> | undefined, since: number, now: number): boolean {
  const after = (series ?? []).filter(([t]) => t >= since)
  if (after.length < 2) return false
  const [lastAt, last] = after[after.length - 1]
  if (now - lastAt > TRACE_LIVE_SECS) return false
  const low = Math.min(...after.slice(0, -1).map(([, f]) => f))
  return last > low * NEW_FLARE_OVER_PEAK
}

/** The trace's direction now: its newest sample (≤ TRACE_LIVE_SECS old) against the samples of the
 *  half hour before it. Rising when more than the margin above their lowest SINCE the last one above
 *  the newest (checked first: a re-rise during a decay is the news); while none is above it, against
 *  the whole half hour's lowest; when the sample just before it is above it, never (the last
 *  measured step fell). Falling when more than the margin below their highest, else none. Not the
 *  half hour's low: after an impulsive flare the half hour still holds the pre-flare background, and
 *  a flux falling in every sample is not rising. Not the low since their highest: in a double-peaked
 *  flare that is the dip between the peaks, and the second peak's decay is not rising. No trace, too
 *  few samples or an old one is none: no measurement, no word. */
function traceDirection(series: ReadonlyArray<[number, number]> | undefined, now: number): 'rising' | 'falling' | null {
  const s = series ?? []
  if (s.length < 2) return null
  const [lastAt, last] = s[s.length - 1]
  if (now - lastAt > TRACE_LIVE_SECS) return null
  const before = s.slice(0, -1).filter(([t]) => t >= lastAt - TRACE_WINDOW_SECS)
  if (before.length === 0) return null
  const fluxes = before.map(([, f]) => f)
  const high = Math.max(...fluxes)
  let i = fluxes.length - 1
  while (i >= 0 && fluxes[i] <= last) i--
  const sinceAbove = fluxes.slice(i + 1)
  if (sinceAbove.length > 0 && last > Math.min(...sinceAbove) * NEW_FLARE_OVER_PEAK) return 'rising'
  if (last * NEW_FLARE_OVER_PEAK < high) return 'falling'
  return null
}

/** `xray` is the flux now (null when unknown) and `xrayAt` its GOES sample time; `series` is the
 *  fast lane's GOES trace (5-minute bucket peaks), read with that sample as its newest point
 *  (`withNewest`), the same reading the chip shows. An event that has peaked or ended is this
 *  flare's only while the flux is not above its peak and, once
 *  ended, the trace has not turned up again: NOAA lists a flare only after minutes of rise, so
 *  until it does the newest event is the PREVIOUS flare, and its words must not be quoted for this
 *  one. Fails closed: with no peak flux (NOAA's `maxFlux`, else its peak class) a settled event is
 *  not an M1+ flare's. Without a current trace NOAA's "ended" stands — no measurement says the
 *  flux rose, and NOAA's word about its own event is true. */
export function flarePhase(
  event: FlareEvent | null | undefined,
  nowMs: number,
  xray: number | null,
  series?: ReadonlyArray<[number, number]>,
  xrayAt?: number | null,
): FlarePhase {
  const now = nowMs / 1000
  const trace = withNewest(series, xray, xrayAt, now)
  const settled = event != null && (event.endAt != null || event.maxAt != null)
  const peak = event ? (event.maxFlux ?? classFlux(event.maxClass)) : null
  const abovePeak = xray != null && (peak != null ? xray > peak * NEW_FLARE_OVER_PEAK : xray >= M1)
  const risen = event?.endAt != null && risenSince(trace, event.endAt, now)
  const newFlare = settled && (abovePeak || risen)
  const ours =
    event != null &&
    !newFlare &&
    now - event.at <= EVENT_FRESH_SECS &&
    now - event.beginAt <= 6 * 3600 &&
    event.beginAt <= now + 60 &&
    (event.endAt == null || now - event.endAt <= 3600)
  if (event && ours) {
    return {
      kind: event.endAt != null ? 'ended' : event.maxAt != null ? 'peaked' : 'rising',
      noaa: true,
      beginAt: event.beginAt,
      peakAt: event.maxAt,
      peakClass: event.maxClass,
      endAt: event.endAt,
    }
  }
  return {
    // Measured from the trace's last half hour, or nothing.
    kind: traceDirection(trace, now),
    noaa: false,
    beginAt: null,
    peakAt: null,
    peakClass: null,
    endAt: null,
  }
}

/** How long until HF recovers: NOAA's own recovery time from a current grid, else NOAA's
 *  empirical fit on the current flux. Minutes, with which one it is; null when neither has one. */
export function flareRecovery(
  grid: DrapGrid | null | undefined,
  xray: number,
  nowMs: number,
): { mins: number; noaa: boolean } | null {
  const now = nowMs / 1000
  if (drapFresh(grid, nowMs) && grid.recoveryAt != null && grid.recoveryAt > now) {
    return { mins: Math.round((grid.recoveryAt - now) / 60), noaa: true }
  }
  const fit = flareRecoveryMin(xray)
  return fit == null ? null : { mins: Math.round(fit), noaa: false }
}

/** What NOAA says beside the flux, for the map surfaces: the six-hour trace (5-minute bucket
 *  peaks, so its last point is not the flux now), the event list and the D-RAP grid, and the GOES
 *  sample's own time. Null for simulated flux (Preview, the dev override), which is drawn with the
 *  analytic field alone. */
export interface FlareNoaa {
  xrayAt: number
  series: [number, number][]
  event: FlareEvent | null
  drap: DrapGrid | null
}

/** The fast-lane answer as the map takes it; null when there is none or the flux is simulated. */
export function flareNoaaOf(
  lane: { asOf: number; series?: [number, number][]; event?: FlareEvent | null; drap?: DrapGrid | null } | null,
  simulated: boolean,
): FlareNoaa | null {
  if (!lane || simulated) return null
  return { xrayAt: lane.asOf, series: lane.series ?? [], event: lane.event ?? null, drap: lane.drap ?? null }
}

/** GOES samples older than this freeze the footprint (no breathing) and the chip says how old. */
export const XRAY_LIVE_SECS = 5 * 60

/** How warm the sun disc is below the flare gate: 0 up to C1, rising on the log scale to 1 at M1
 *  (where the flare layer's own sun takes over). A C-class flare shows on the disc, and only there. */
export function sunWarmth(xray: number | null | undefined): number {
  if (xray == null || !(xray > 0)) return 0
  return Math.max(0, Math.min(1, Math.log10(xray) + 6)) // C1 = 1e-6 → 0, M1 = 1e-5 → 1
}

/** The flare layer's warm accent (the chip's border, the fx canvas's rays). */
const FLARE_ORANGE: [number, number, number] = [0xff, 0x8c, 0x3a]

/** The theme's sun colour (`#rrggbb`) mixed toward flare orange by `warmth` (0–1). A colour that is
 *  not `#rrggbb` comes back unchanged. */
export function warmSunInk(base: string, warmth: number): string {
  const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(base.trim())
  if (!m || !(warmth > 0)) return base
  const w = Math.min(1, warmth)
  return (
    '#' +
    [1, 2, 3]
      .map((i) => {
        const from = parseInt(m[i], 16)
        return Math.round(from + (FLARE_ORANGE[i - 1] - from) * w)
          .toString(16)
          .padStart(2, '0')
      })
      .join('')
  )
}
