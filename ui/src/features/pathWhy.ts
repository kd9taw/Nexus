// The path "why" line — one plain sentence saying how a signal is modelled to travel to a
// station right now and what may get in its way, built from the per-path outlook the shack
// already computes (`get_path_outlook`: the engine's per-hour hops/layer/take-off, and the
// model-free geometry beside it).
//
// Two kinds of fact, and the sentence keeps them apart:
// - the MODEL (band open now, hops, layer, take-off): P.533 is a monthly median — the typical
//   day of the month, not a measurement of today;
// - the OVERLAY (polar cap, auroral oval, Kp): space weather laid over the model, which does not
//   include storms. A Kp the outlook predicts is said to be a forecast, and its effect "may"
//   happen — never stated as something observed.
//
// Every number in the sentence comes from a field; nothing here is prose-only.

import { t } from '../i18n'
import type { BandOutlook, PathGeoHour, PathHour, PathPrediction } from '../types'

/** A band counts as open now from this hourly score (the heatmap's Fair threshold). */
const OPEN_NOW = 0.3
/** Kp from which a forecast storm is worth a warning: NOAA G1 (minor storm) starts at Kp 5. */
const STORM_KP = 5

export interface PathWhyFacts {
  /** Best band open now (highest current-hour score ≥ Fair), or null if none is. */
  band: string | null
  /** That band's modelled mode this hour (P.533 only). */
  hop: PathHour | null
  /** The path crosses the polar cap at this hour's Kp. */
  polarNow: boolean
  /** The path crosses the live auroral oval (only when an oval is held). */
  ovalNow: boolean
  /** Grey line at the ends right now. */
  grey: 'both' | 'me' | 'dx' | null
  /** The first hour in the next 24 h whose FORECAST Kp reaches a storm and puts the polar cap
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
  let storm: PathWhyFacts['storm'] = null
  for (let k = 0; k < 24 && !storm; k++) {
    const x = g.hours[(h + k) % 24]
    if (x.kpSource === 'forecast' && x.kp >= STORM_KP && x.polarFrac > 0) {
      storm = { kp: x.kp, unix: x.unix }
    }
  }
  return {
    band: band?.band ?? null,
    hop: band?.detail?.[h] ?? null,
    polarNow: now.polarFrac > 0,
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
  return s === 'forecast' ? t('pathWhy.kpSource.forecast') : t('pathWhy.kpSource.current')
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
  if (f.hop) parts.push(t('pathWhy.hops', { count: f.hop.hops, layer: f.hop.layer }))
  if (f.polarNow) parts.push(t('pathWhy.polar'))
  if (f.ovalNow) parts.push(t('pathWhy.oval'))
  if (f.grey) parts.push(greyWords(f.grey))
  if (!f.band && parts.length === 0 && !f.storm) return null
  const lead = f.band ? t('pathWhy.lead.band', { band: f.band }) : t('pathWhy.lead')
  let text = [lead, ...parts].join(t('pathWhy.sep'))
  if (f.storm) {
    const kp = kpLabel(f.storm.kp)
    const time = utcHourLabel(f.storm.unix)
    // Already inside the cap: an aurora deepens it. Outside: the forecast grows the cap over it.
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
    rows.push(
      t('pathWhy.detail.mode', {
        mode: `${f.hop.hops}${f.hop.layer}`,
        deg: f.hop.takeoffDeg.toFixed(1),
      }),
    )
  }
  rows.push(
    t('pathWhy.detail.polar', {
      pct: pct(f.now.polarFrac),
      lat: f.now.capLatDeg.toFixed(0),
      kp: kpLabel(f.now.kp),
      source: kpSourceWords(f.now.kpSource),
    }),
  )
  rows.push(t('pathWhy.detail.maxLat', { lat: maxGmlatDeg.toFixed(0) }))
  rows.push(
    t('pathWhy.detail.grey', { pct: pct(f.now.greylineFrac), sun: pct(f.now.sunlitFrac) }),
  )
  if (f.ovalNow) rows.push(t('pathWhy.detail.oval'))
  rows.push(t('pathWhy.detail.note'))
  return rows
}
