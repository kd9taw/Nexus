// The path "why" line — one plain sentence saying how a signal is modelled to travel to a
// station right now and what may get in its way, built from the per-path outlook the shack
// already computes (`get_path_outlook`: the engine's per-hour hops/layer/take-off, and the
// model-free geometry beside it).
//
// Two kinds of fact, and the sentence keeps them apart:
// - the MODEL (band open now, hops, layer, take-off): P.533 is a monthly median — the typical
//   day of the month, not a measurement of today. Beyond 7000 km the hops and take-off are the
//   geometry the long-path method assumes, and are worded as an estimate;
// - the OVERLAY (auroral zone, polar cap, auroral oval, Kp): space weather laid over the model,
//   which does not include storms. A Kp the outlook predicts is said to be a forecast, and its
//   effect "may" happen — never stated as something observed. With no space-weather data the
//   Kp is unknown: it is said so, and nothing that depends on Kp is claimed.
//
// Every number in the sentence comes from a field; nothing here is prose-only.

import { t } from '../i18n'
import type { BandOutlook, PathGeoHour, PathHour, PathPrediction } from '../types'

/** A band counts as open now from this hourly score (the heatmap's Fair threshold). */
const OPEN_NOW = 0.3
/** Kp from which a forecast storm is worth a warning: NOAA G1 (minor storm) starts at Kp 5. */
const STORM_KP = 5
/** Geomagnetic latitude from which a crossing is the polar cap proper. The cap edge the
 * geometry uses (66° − 1.2·Kp) sits near 64° at quiet Kp, inside the auroral zone, so a path
 * that only gets between that edge and here crosses the auroral zone, not the cap. */
const POLAR_CAP_DEG = 70
/** The furthest equatorward that edge ever comes (its floor at Kp 9): a path that never
 * reaches it is out of the auroral zone at any Kp, so an unknown Kp changes nothing for it. */
const EDGE_FLOOR_DEG = 55

export interface PathWhyFacts {
  /** Best band open now (highest current-hour score ≥ Fair), or null if none is. */
  band: string | null
  /** That band's modelled mode this hour (P.533 only). */
  hop: PathHour | null
  /** The path is poleward of the cap edge at this hour's Kp (false when Kp is unknown). */
  polarNow: boolean
  /** ...and reaches the polar cap proper, not only the auroral zone. */
  capNow: boolean
  /** Kp is unknown this hour, and the path reaches latitudes where it would matter. */
  kpUnknown: boolean
  /** The path crosses the live auroral oval (only when an oval is held). */
  ovalNow: boolean
  /** Grey line at the ends right now. */
  grey: 'both' | 'me' | 'dx' | null
  /** The first hour in the next 24 h whose FORECAST Kp reaches a storm and puts the cap edge
   * over this path. */
  storm: { kp: number; unix: number } | null
  /** This hour's geometry (for the detail rows). */
  now: PathGeoHour
}

function bestBandNow(bands: BandOutlook[], hour: number): BandOutlook | null {
  let best: BandOutlook | null = null
  for (const b of bands) {
    const s = b.hourly[hour] ?? 0
    if (s >= OPEN_NOW && (!best || s > (best.hourly[hour] ?? 0))) best = b
  }
  return best
}

/** The facts behind the why line, or null when the outlook carries no geometry (operator grid
 * unknown, or an older shack). */
export function pathWhyFacts(p: PathPrediction | null | undefined): PathWhyFacts | null {
  const g = p?.geometry
  if (!p || !g || g.hours.length !== 24) return null
  const h = g.nowHour
  const now = g.hours[h]
  const band = bestBandNow(p.bands, h)
  const polarNow = (now.polarFrac ?? 0) > 0
  let storm: PathWhyFacts['storm'] = null
  for (let k = 0; k < 24 && !storm; k++) {
    const x = g.hours[(h + k) % 24]
    if (x.kpSource === 'forecast' && x.kp !== null && x.kp >= STORM_KP && (x.polarFrac ?? 0) > 0) {
      storm = { kp: x.kp, unix: x.unix }
    }
  }
  return {
    band: band?.band ?? null,
    hop: band?.detail?.[h] ?? null,
    polarNow,
    capNow: polarNow && g.maxGmlatDeg >= POLAR_CAP_DEG,
    kpUnknown: now.kpSource === 'unknown' && g.maxGmlatDeg >= EDGE_FLOOR_DEG,
    ovalNow: g.ovalNow === true,
    grey: now.greyMe && now.greyDx ? 'both' : now.greyMe ? 'me' : now.greyDx ? 'dx' : null,
    storm,
    now,
  }
}

// Literal keys at every call (no key maps): the catalog guards read call sites, and a computed
// key is one they cannot check.
function greyWords(g: NonNullable<PathWhyFacts['grey']>): string {
  if (g === 'both') return t('pathWhy.grey.both')
  return g === 'me' ? t('pathWhy.grey.me') : t('pathWhy.grey.dx')
}

function kpSourceWords(s: PathGeoHour['kpSource']): string {
  if (s === 'measured') return t('pathWhy.kpSource.measured')
  if (s === 'forecast') return t('pathWhy.kpSource.forecast')
  return s === 'current' ? t('pathWhy.kpSource.current') : t('pathWhy.kpSource.unknown')
}

/** "1800Z" — the clock format of the outlook's windows. */
export function utcHourLabel(unix: number): string {
  const d = new Date(unix * 1000)
  return `${String(d.getUTCHours()).padStart(2, '0')}${String(d.getUTCMinutes()).padStart(2, '0')}Z`
}

/** Kp as printed: whole numbers bare, thirds to one decimal. */
function kpLabel(kp: number): string {
  return Number.isInteger(kp) ? String(kp) : kp.toFixed(1)
}

/** The one-sentence why line, or null when there is nothing worth saying. */
export function pathWhyLine(f: PathWhyFacts | null): string | null {
  if (!f) return null
  const parts: string[] = []
  if (f.hop) {
    const { hops: count, layer } = f.hop
    parts.push(
      f.hop.estimated
        ? t('pathWhy.hopsEstimate', { count, layer })
        : t('pathWhy.hops', { count, layer }),
    )
  }
  if (f.polarNow) parts.push(f.capNow ? t('pathWhy.polar') : t('pathWhy.auroralZone'))
  if (f.kpUnknown) parts.push(t('pathWhy.kpUnknown'))
  if (f.ovalNow) parts.push(t('pathWhy.oval'))
  if (f.grey) parts.push(greyWords(f.grey))
  if (!f.band && parts.length === 0 && !f.storm) return null
  const lead = f.band ? t('pathWhy.lead.band', { band: f.band }) : t('pathWhy.lead')
  let text = [lead, ...parts].join(t('pathWhy.sep'))
  if (f.storm) {
    const kp = kpLabel(f.storm.kp)
    const time = utcHourLabel(f.storm.unix)
    // Already inside the edge: an aurora deepens it. Outside: the forecast pushes it over.
    text +=
      t('pathWhy.sepWarn') +
      (f.polarNow ? t('pathWhy.storm.aurora', { kp, time }) : t('pathWhy.storm.cap', { kp, time }))
  }
  return t('pathWhy.sentence', { text })
}

/** The expert rows behind the line — every number the sentence stands on. */
export function pathWhyDetail(f: PathWhyFacts | null, maxGmlatDeg: number): string[] {
  if (!f) return []
  const pct = (x: number) => String(Math.round(x * 100))
  const rows: string[] = []
  if (f.hop) {
    const mode = `${f.hop.hops}${f.hop.layer}`
    const deg = f.hop.takeoffDeg.toFixed(1)
    rows.push(
      f.hop.estimated
        ? t('pathWhy.detail.modeEstimate', { mode, deg })
        : t('pathWhy.detail.mode', { mode, deg }),
    )
  }
  const { kp, capLatDeg, polarFrac } = f.now
  rows.push(
    kp === null || capLatDeg === null || polarFrac === null
      ? t('pathWhy.detail.polarUnknown')
      : t('pathWhy.detail.polar', {
          pct: pct(polarFrac),
          lat: capLatDeg.toFixed(0),
          kp: kpLabel(kp),
          source: kpSourceWords(f.now.kpSource),
        }),
  )
  rows.push(t('pathWhy.detail.maxLat', { lat: maxGmlatDeg.toFixed(0) }))
  rows.push(
    t('pathWhy.detail.grey', { pct: pct(f.now.greylineFrac), sun: pct(f.now.sunlitFrac) }),
  )
  if (f.ovalNow) rows.push(t('pathWhy.detail.oval'))
  rows.push(t('pathWhy.detail.note', { source: kpSourceWords(f.now.kpSource) }))
  return rows
}
