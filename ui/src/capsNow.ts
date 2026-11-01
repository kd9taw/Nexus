// Caps and ovals on the map — the pure half of the proton cap, the aurora oval and the Kp word
// (MapView and Globe3D draw them; the chips, the dashboard bar and the storm toast word them).
//
// THE POLAR CAP. Two sources, never mixed, each labelled. NOAA's D-RAP grid (sent by the shack
// during a flare or a measured proton event) carries NOAA's own polar-cap absorption with NOAA's
// real proton cutoff: while it is current (flareNow `drapFresh`) it IS the cap. Either source speaks
// only from a HELD proton reading (`protonsHeld`: dated, under 30 min, the shack's proton_event rule);
// a stale or undated one draws no cap and no chip, and the layer row says why. Its proton part is
// what the grid holds beyond the X-ray part at each cell (D-RAP adds the two: A = A_X + A_P, at
// 30 MHz A = (HAF/30)^1.5 dB), poleward of 50° geomagnetic. Otherwise the shack's model (the
// Sauer & Wilkinson D-RAP method over GOES protons) is the cap, and its edge, Nexus's 66° − 1.2·Kp
// line, is drawn dashed and called an approximation — and only from a measured Kp. Beside NOAA's
// grid no approximate edge is drawn: NOAA's shading is its own edge, and a second line would be a
// second, contradicting answer.
//
// THE AURORA OVAL. NOAA OVATION is the picture while it is a forecast for now or later (or at most
// AURORA_STALE_SECS past): its dots and its own equatorward edge (the 10 % line). Past that it is
// stale, or undated from any station (its age cannot be proven), and then NO oval and no edge is drawn at all, by
// the operator's rule: nothing is ever modelled in its place. The layer row and the chip say why
// (out of date, with the time it was a forecast for; or no forecast held). Kp and NOAA's Kp outlook
// stay on screen with their own labels.
//
// KP NOW. NOAA's 3-hourly OBSERVED Kp when its period ended within the last 3 hours ("measured
// 21–00Z"), else the 1-minute ESTIMATED Kp when it may speak for now ("est. 22:36Z"), else none —
// no number, no storm word, no toast. The estimate never displaces a measurement in the WORDS (the
// dashboard bar's number is `kpNow`).
//
// KP TO WARN ON (`kpAlert`). Whether to warn is a different question from which number to show: NOAA
// marks each finished 3-hour period "observed" promptly, so a measured period is almost always held,
// and it lags a storm's onset by up to 3 hours. The storm toast, the forecast heads-up's "already
// stormy" gate and the aurora layer's auto-on warn on the HIGHER of the recent measurement and the
// usable estimate, labelled with which one it is ("Kp 6.3 est. 22:36Z"), so an estimate is still
// never called a measurement. A tie goes to the measurement.

import { useSyncExternalStore } from 'react'
import type { AuroraOval, DrapGrid, KpForecast, PcaView, SpaceWxView, ArrivalOutlook } from './types'
import { drapFresh } from './flareNow'
import { flareHafAt } from './mapGeo'
import { wxUsable } from './propViz'
import { t } from './i18n'

const DEG = Math.PI / 180
const HOUR = 3600

// ───────────────────────────── Kp now ─────────────────────────────

/** How long after its 3-hour period ends NOAA's observed Kp is still the Kp now. */
export const KP_MEASURED_WITHIN_SECS = 3 * HOUR

export interface KpNow {
  kp: number
  source: 'measured' | 'estimated'
  /** Measured: the period's start. Estimated: the 1-minute sample's time (null from an older
   *  station, which sends none). */
  at: number | null
  /** Measured only: the period's end. */
  until?: number
}

/** The Kp that speaks for now (see the header): measured if NOAA has a period that ended within
 *  KP_MEASURED_WITHIN_SECS, else the usable 1-minute estimate, else null. */
export function kpNow(
  wx: SpaceWxView | null | undefined,
  fc: KpForecast | null | undefined,
  nowMs: number,
): KpNow | null {
  return measuredKp(fc, nowMs) ?? estimatedKp(wx, nowMs)
}

/** The Kp to warn on (see the header): the higher of the recent measurement and the usable estimate,
 *  each keeping its own label; a tie goes to the measurement; null when neither is held. */
export function kpAlert(
  wx: SpaceWxView | null | undefined,
  fc: KpForecast | null | undefined,
  nowMs: number,
): KpNow | null {
  const m = measuredKp(fc, nowMs)
  const e = estimatedKp(wx, nowMs)
  if (m && e) return e.kp > m.kp ? e : m
  return m ?? e
}

/** NOAA's observed period that ended within KP_MEASURED_WITHIN_SECS (the newest), or null. */
function measuredKp(fc: KpForecast | null | undefined, nowMs: number): KpNow | null {
  const now = nowMs / 1000
  const points = Array.isArray(fc?.points) ? fc.points : []
  let measured: KpNow | null = null
  for (const p of points) {
    if (p.kind !== 'observed' || !Number.isFinite(p.kp)) continue
    const end = p.timeUnix + 3 * HOUR
    if (end <= now && now - end <= KP_MEASURED_WITHIN_SECS && (!measured || p.timeUnix > (measured.at ?? 0))) {
      measured = { kp: p.kp, source: 'measured', at: p.timeUnix, until: end }
    }
  }
  return measured
}

/** The 1-minute estimate when it may speak for now (the honesty rule's `wxUsable`), or null. */
function estimatedKp(wx: SpaceWxView | null | undefined, nowMs: number): KpNow | null {
  if (wx && Number.isFinite(wx.kp) && wxUsable(wx, 'kp', nowMs)) {
    return { kp: wx.kp, source: 'estimated', at: wx.kpAt ?? null }
  }
  return null
}

/** `HH:MMZ`. */
function hm(unix: number): string {
  const d = new Date(unix * 1000)
  return `${String(d.getUTCHours()).padStart(2, '0')}:${String(d.getUTCMinutes()).padStart(2, '0')}Z`
}
/** `HHZ`, for a 3-hour period's ends. */
function hz(unix: number): string {
  return `${String(new Date(unix * 1000).getUTCHours()).padStart(2, '0')}Z`
}

/** "Kp 6.0 est. 22:36Z" / "Kp 6.0 measured 21–00Z" (`digits` decimals). */
export function kpLabel(k: KpNow, digits = 1): string {
  const kp = k.kp.toFixed(digits)
  if (k.source === 'measured') return t('prop.kpNow.measured', { kp, from: hz(k.at!), to: hz(k.until!) })
  return k.at != null ? t('prop.kpNow.est', { kp, time: hm(k.at) }) : t('prop.kpNow.estUndated', { kp })
}

/** The bar's short word after the Kp value: "est." or the measured period "21–00Z". */
export function kpWord(k: KpNow): string {
  return k.source === 'measured' ? `${hz(k.at!)}–${hz(k.until!)}` : t('dash.kp.est')
}

// ───────────────────────────── the polar cap ─────────────────────────────

const DIPOLE_LAT = 80.65
const DIPOLE_LON = -72.68
/** Centred-dipole geomagnetic latitude — the shack's `geo::geomagnetic_lat_deg`. */
export function geomagLat(lat: number, lon: number): number {
  const s =
    Math.sin(lat * DEG) * Math.sin(DIPOLE_LAT * DEG) +
    Math.cos(lat * DEG) * Math.cos(DIPOLE_LAT * DEG) * Math.cos((lon - DIPOLE_LON) * DEG)
  return Math.asin(Math.max(-1, Math.min(1, s))) / DEG
}

/** Poleward of this geomagnetic latitude a grid cell can hold proton absorption (the lowest cutoff
 *  the model allows, 55°, less its 5° taper). */
const CAP_FROM_GEOMAG = 50
/** Below this 30 MHz absorption the cap is not drawn (the shack model's own floor). */
export const CAP_MIN_DB = 0.5

/** 30 MHz absorption (dB) for a local HAF: the D-RAP frequency law, 1 dB at the HAF. */
const db30 = (haf: number) => (haf > 0 ? Math.pow(haf / 30, 1.5) : 0)

/** NOAA's proton part of a D-RAP grid: per cell poleward of CAP_FROM_GEOMAG, the 30 MHz
 *  absorption beyond what the X-ray flux `xray` gives there (D-RAP's own sum), at ≥ CAP_MIN_DB. */
export function gridCap(grid: DrapGrid, xray: number | null, nowMs: number): { lat: number; lon: number; db30: number }[] {
  const out: { lat: number; lon: number; db30: number }[] = []
  for (let r = 0; r < grid.lats.length; r++) {
    const lat = grid.lats[r]
    for (let c = 0; c < grid.lons.length; c++) {
      const lon = grid.lons[c]
      if (Math.abs(geomagLat(lat, lon)) < CAP_FROM_GEOMAG) continue
      const haf = grid.haf[r * grid.lons.length + c]
      const x = xray != null && xray > 0 ? flareHafAt(lat, lon, nowMs, xray) : 0
      const db = db30(haf) - db30(x)
      if (db >= CAP_MIN_DB) out.push({ lat, lon, db30: db })
    }
  }
  return out
}

export interface CapPicture {
  /** 'noaa': NOAA's D-RAP grid; 'model': the shack's Sauer & Wilkinson model over GOES protons;
   *  'none': no cap, because the proton reading is not held (`why`). */
  source: 'noaa' | 'model' | 'none'
  /** Why there is no cap ('none' only): the reading is 30 min old or more, or carries no time. */
  why?: 'stale' | 'undated'
  /** The proton reading's own time ('none' with `why: 'stale'`, for the layer row). */
  protonsAt?: number | null
  points: { lat: number; lon: number; db30: number }[]
  /** NOAA's grid valid time (noaa only). */
  validAt: number | null
  /** The approximate edge, [north, south] [lon, lat] rings — model only, and only at a measured
   *  Kp; null otherwise. */
  edge: [number, number][][] | null
  /** The strongest absorption drawn (dB at 30 MHz), 0 when nothing is. */
  maxDb: number
}

/** The cap the map draws (see the header). Null without any proton reading. */
export function capPicture(
  pca: PcaView | null | undefined,
  drap: DrapGrid | null | undefined,
  xray: number | null,
  nowMs: number,
): CapPicture | null {
  if (!pca) return null
  // FAIL CLOSED: a proton cap only from a HELD reading, dated and under PROTONS_HELD_SECS old (the
  // shack's `pca::proton_event`). The shack serves its last reading after a failed fetch with no age
  // limit, so a stale or undated one is no proton event now: no cap from either source (not NOAA's
  // grid's polar excess called "proton", not the model's points), no edge and no chip. The layer
  // row says which.
  if (!protonsHeld(pca, nowMs)) {
    const at = pca.protonsAt ?? null
    return { source: 'none', why: at == null ? 'undated' : 'stale', protonsAt: at, points: [], validAt: null, edge: null, maxDb: 0 }
  }
  if (drapFresh(drap, nowMs)) {
    const points = gridCap(drap, xray, nowMs)
    return { source: 'noaa', points, validAt: drap.validAt, edge: null, maxDb: maxOf(points) }
  }
  const edge = pca.cutoffDeg != null && Array.isArray(pca.edge) ? (pca.edge as [number, number][][]) : null
  return { source: 'model', points: pca.points, validAt: null, edge, maxDb: maxOf(pca.points) }
}

const maxOf = (pts: { db30: number }[]) => pts.reduce((m, p) => Math.max(m, p.db30), 0)

/** NOAA's S-scale from J(≥10 MeV) (the shack sends it; an older one does not). */
export function sScale(pca: PcaView): number {
  if (typeof pca.sScale === 'number') return pca.sScale
  const j = pca.j10
  return j >= 1e5 ? 5 : j >= 1e4 ? 4 : j >= 1e3 ? 3 : j >= 100 ? 2 : j >= 10 ? 1 : 0
}

/** Whether there is a cap worth a chip: a proton event (S1+) or any absorption drawn. */
export function capShown(pca: PcaView | null | undefined, pic: CapPicture | null): boolean {
  return !!pca && !!pic && pic.source !== 'none' && (sScale(pca) >= 1 || pic.points.length > 0)
}

/** How old a GOES proton reading may be and still be held: the shack's `pca::PROTON_FRESH_SECS`, the
 *  same limit at which it stops asking NOAA for the grid for a proton event. */
export const PROTONS_HELD_SECS = 30 * 60

/** A proton reading that may speak for now: dated, and under PROTONS_HELD_SECS old. */
export function protonsHeld(pca: PcaView, nowMs: number): boolean {
  const at = pca.protonsAt
  return at != null && nowMs / 1000 - at < PROTONS_HELD_SECS
}

/** How old the GOES proton sample may be before the chip says how old it is and drops the
 *  recovery estimate (the product is every 5 minutes). */
export const PROTONS_STALE_SECS = 15 * 60

// ───────────────────────────── the aurora oval ─────────────────────────────

/** How far past its forecast time an OVATION oval is still the picture: two of NOAA's 5-minute
 *  cycles. OVATION is normally a forecast for 30–90 min ahead, so an oval past this has stopped. */
export const AURORA_STALE_SECS = 10 * 60

export type AuroraPicture =
  | { mode: 'ovation'; forecastAt: number; edge: [number, number][][] }
  | { mode: 'none'; why: 'nofeed' }
  | { mode: 'none'; why: 'undated' }
  | { mode: 'none'; why: 'stale'; forecastAt: number }

/** The oval the map draws (see the header). */
export function auroraPicture(oval: AuroraOval | null | undefined, nowMs: number): AuroraPicture {
  const now = nowMs / 1000
  const f = oval?.forecastAt ?? null
  // FAIL CLOSED, whatever the source (an older station's oval included): only an oval whose
  // forecast time proves it current is drawn. An undated one draws nothing, as a stale one does.
  if (oval && f != null && now - f <= AURORA_STALE_SECS) {
    return { mode: 'ovation', forecastAt: f, edge: [...edgeLines(oval.edge?.north), ...edgeLines(oval.edge?.south)] }
  }
  if (oval && f != null) return { mode: 'none', why: 'stale', forecastAt: f }
  if (oval && oval.points.length > 0) return { mode: 'none', why: 'undated' }
  return { mode: 'none', why: 'nofeed' }
}

/** Longitude of OVATION edge bin `i` (the shack's `oval::edge_bin_lon`). */
const binLon = (i: number) => -180 + 15 * (i + 0.5)

/** OVATION's per-15° edge as [lon, lat] polylines: consecutive bins joined, broken where a bin has
 *  no edge (never bridged), and joined across the dateline when both ends have one. */
export function edgeLines(lats: (number | null)[] | null | undefined): [number, number][][] {
  if (!Array.isArray(lats) || lats.length === 0) return []
  const n = lats.length
  const gap = lats.findIndex((v) => v == null)
  if (gap < 0) {
    const ring = lats.map((lat, i) => [binLon(i), lat as number] as [number, number])
    return [[...ring, ring[0]]]
  }
  const lines: [number, number][][] = []
  let cur: [number, number][] = []
  for (let k = 1; k <= n; k++) {
    const i = (gap + k) % n
    const lat = lats[i]
    if (lat == null) {
      if (cur.length > 1) lines.push(cur)
      cur = []
    } else cur.push([binLon(i), lat])
  }
  if (cur.length > 1) lines.push(cur)
  return lines
}

/** A storm that turns the aurora layer on by itself: Kp ≥ 5 now (`kpAlert`: the higher of the recent
 *  measurement and the usable estimate), or a NOAA G watch in force. */
export function auroraStorm(k: KpNow | null, outlook: ArrivalOutlook | null | undefined): boolean {
  return (k != null && k.kp >= 5) || (outlook?.watch?.maxG ?? 0) >= 1
}

// ───────────────────────────── the storm's untick ─────────────────────────────
//
// ONE UNTICK FOR THE WHOLE STORM, ON EVERY MAP. An operator who unticks the aurora layer while a storm
// has it on means "not this storm": it must hold on the flat map, the 2-D globe and the 3-D globe, on
// every screen that hosts them, and across leaving and coming back. So the dismissal is one value for
// the window, here, not state inside a map that a remount forgets. It is never saved (a restart is a
// new look at the sky), and it is cleared when the storm ends: by App on every propagation poll,
// whether or not a map is on screen, and by a mounted map as soon as it sees the storm end. A storm
// that ends while nothing watches can therefore never carry an old untick into the next one.

let dismissed = false
const listeners = new Set<() => void>()

/** Whether the operator has unticked the aurora layer during the current storm. */
export function auroraDismissal(): boolean {
  return dismissed
}

/** Set (an untick during a storm) or clear (the storm is over) the storm's untick. */
export function setAuroraDismissal(v: boolean): void {
  if (dismissed === v) return
  dismissed = v
  for (const l of listeners) l()
}

/** The storm's untick, re-rendering the caller when any surface changes it. */
export function useAuroraDismissal(): boolean {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l)
      return () => {
        listeners.delete(l)
      }
    },
    () => dismissed,
  )
}

/** Tests: no untick carried from one test to the next (src/test-setup.ts). */
;(globalThis as { __nexusTestResets?: Set<() => void> }).__nexusTestResets?.add(() => {
  dismissed = false
})

/** Why the layer is on for the storm, for the chip: the Kp, or NOAA's watch. */
export function auroraStormWhy(k: KpNow | null, outlook: ArrivalOutlook | null | undefined): string {
  if (k != null && k.kp >= 5) return kpLabel(k)
  return t('map.aurora.why.watch', { g: outlook?.watch?.maxG ?? 1 })
}
