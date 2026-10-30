// Short path or long path — the heading hint, the polar chip and the map ribbon's inks, built
// from the per-path outlook (`get_path_outlook`: both routes from the P.533 engine, each with its
// model-free geometry, and the engine's per-band route call).
//
// What the words may claim:
// - the route call is the MODEL's opinion (a monthly median), worded "likely" / "possible" /
//   "either way" — never "confirmed", which needs the station's own beam heading;
// - the long path is the same published method run along the other arc, with no reference
//   fixture behind it, so every surface that shows it says "modelled, unvalidated for long path";
// - the hint is advice: nothing here turns a rotator.
//
// Every number comes from a field; nothing is prose-only.

import { t } from '../i18n'
import type { PathGeometry, PathPrediction, RouteCall } from '../types'
import { pathWhyFacts, utcHourLabel } from './pathWhy'

export type RouteName = 'short' | 'long'

/** A route counts as open from this circuit reliability, closed below the other (the engine's
 * `route::OPEN_BCR` / `CLOSED_BCR`). */
const OPEN_BCR = 0.5
const CLOSED_BCR = 0.1

export interface RouteHint {
  band: string
  call: RouteCall
  /** The route to beam; `either` when the model puts them within its margin. */
  route: RouteName | 'either'
  /** True bearing of the favoured route (the short path's for `either`). */
  brgDeg: number
  spBrgDeg: number
  lpBrgDeg: number
}

/** The band the hint and the ribbon talk about: the band in focus when the outlook carries a call
 * for it, else the band whose better route is the most reliable now. Bands the model calls closed
 * both ways are never picked. */
function pickCall(routes: RouteCall[], focusBand?: string | null): RouteCall | null {
  const live = routes.filter((r) => r.verdict !== 'unknown')
  const focused = focusBand ? live.find((r) => r.band === focusBand) : undefined
  if (focused) return focused
  let best: RouteCall | null = null
  for (const r of live) {
    if (!best || Math.max(r.shortBcr, r.longBcr) > Math.max(best.shortBcr, best.longBcr)) best = r
  }
  return best
}

/** The heading hint, or null when the engine gave no long path or nothing is open either way. */
export function routeHint(p: PathPrediction | null | undefined, focusBand?: string | null): RouteHint | null {
  const sp = p?.geometry
  const lp = p?.longPath?.geometry
  if (!p?.routes || !sp || !lp) return null
  const c = pickCall(p.routes, focusBand)
  if (!c) return null
  const route = c.verdict === 'long' ? 'long' : c.verdict === 'short' ? 'short' : 'either'
  return {
    band: c.band,
    call: c,
    route,
    brgDeg: Math.round(route === 'long' ? lp.bearingDeg : sp.bearingDeg),
    spBrgDeg: Math.round(sp.bearingDeg),
    lpBrgDeg: Math.round(lp.bearingDeg),
  }
}

function confWords(c: RouteCall['confidence']): string {
  return c === 'possible' ? t('pathRoute.conf.possible') : t('pathRoute.conf.likely')
}

/** The one-sentence hint ("Beam 80° for 20m: long path (likely) — …"), or null. `short` leaves
 * out the reason clauses (a narrow card shows them on hover). */
export function routeHintLine(h: RouteHint | null, short = false): string | null {
  if (!h) return null
  const { band, call } = h
  if (h.route === 'either') {
    return t('pathWhy.sentence', {
      text: t('pathRoute.hint.either', { band, sp: h.spBrgDeg, lp: h.lpBrgDeg }),
    })
  }
  const conf = confWords(call.confidence)
  let text =
    h.route === 'long'
      ? t('pathRoute.hint.long', { band, brg: h.brgDeg, conf })
      : t('pathRoute.hint.short', { band, brg: h.brgDeg, conf })
  const has = (k: RouteCall['reasons'][number]['kind']) => call.reasons.some((r) => r.kind === k)
  if (!short) {
    if (has('oneClosed')) {
      // The other route is closed: a dB difference against a closed route is not worth printing.
      if (h.route === 'long') text += t('pathRoute.why.shortClosed')
    } else if (has('margin') && call.marginDb !== undefined) {
      text += t('pathRoute.why.margin', { db: Math.round(Math.abs(call.marginDb)) })
    }
    if (call.reasons.some((r) => r.kind === 'greyLine' && r.route === h.route)) {
      text += t('pathRoute.why.grey')
    }
  }
  return t('pathWhy.sentence', { text })
}

function pct(x: number): string {
  return String(Math.round(x * 100))
}

/** The rows behind the hint: both headings and reliabilities, the SNR margin, and the long-path
 * caveat. */
export function routeHintDetail(h: RouteHint | null): string[] {
  if (!h) return []
  const rows = [
    t('pathRoute.detail.now', {
      band: h.band,
      sp: h.spBrgDeg,
      spPct: pct(h.call.shortBcr),
      lp: h.lpBrgDeg,
      lpPct: pct(h.call.longBcr),
    }),
  ]
  if (h.call.marginDb !== undefined) {
    const db = Math.round(h.call.marginDb)
    rows.push(t('pathRoute.detail.margin', { db: db > 0 ? `+${db}` : String(db) }))
  }
  rows.push(t('pathRoute.lpCaveat'))
  return rows
}

/** "Polar path" (· the forecast storm that would deepen it) for one route, or null when that
 * route is clear of the cap now and no forecast storm puts it there. */
export function polarChip(p: PathPrediction | null | undefined, route: RouteName): string | null {
  const one = route === 'long' ? p?.longPath : p
  const f = pathWhyFacts(one)
  if (!f || (!f.polarNow && !f.storm)) return null
  if (f.storm) {
    const kp = Number.isInteger(f.storm.kp) ? String(f.storm.kp) : f.storm.kp.toFixed(1)
    return t('pathRoute.chip.storm', { kp, time: utcHourLabel(f.storm.unix) })
  }
  return t('pathRoute.chip.polar')
}

export type RibbonInk = 'open' | 'marginal' | 'closed'

export interface RibbonRoute {
  /** Modelled reliability on the hint's band this hour; null = the engine gave no figure. */
  ink: RibbonInk | null
  segments: NonNullable<PathGeometry['segmentsNow']>
  brgDeg: number
}

function ink(bcr: number): RibbonInk {
  return bcr >= OPEN_BCR ? 'open' : bcr >= CLOSED_BCR ? 'marginal' : 'closed'
}

/** The route calls at UTC hour `h` from the outlook's day (what the path-over-the-day strip
 * shows for that hour), in the shape the current hour's calls have. Reasons are not carried: the
 * map draws inks, not words. */
function callsAt(p: PathPrediction, h: number): RouteCall[] | null {
  if (!p.routes || !p.day) return null
  return p.day.bands.flatMap((b) => {
    const c = b.cells[h]
    if (!c || c.longPct == null || !c.verdict) return []
    return [{ band: b.band, verdict: c.verdict, confidence: c.confidence, shortBcr: c.shortPct / 100, longBcr: c.longPct / 100, reasons: [] }]
  })
}

/** What the map draws for the selected station: each route's ink and its polar / grey-line
 * stretches. `long` is null when the engine modelled no long path (the map then draws the long
 * route as plain geometry). `hour` (UTC) is the hour the operator scrubbed the day to; null or
 * the current hour draws now. */
export function ribbonRoutes(
  p: PathPrediction | null | undefined,
  focusBand?: string | null,
  hour?: number | null,
): { short: RibbonRoute; long: RibbonRoute | null; favoured: RouteName | null } | null {
  const sp = p?.geometry
  if (!p || !sp) return null
  const lp = p.longPath?.geometry
  const scrubbed = hour != null && hour !== sp.nowHour
  const calls = scrubbed ? callsAt(p, hour) : p.routes
  const c = calls ? pickCall(calls, focusBand) : null
  const segs = (g: PathGeometry) => (scrubbed ? (g.hours[hour]?.segments ?? []) : (g.segmentsNow ?? []))
  return {
    short: { ink: c ? ink(c.shortBcr) : null, segments: segs(sp), brgDeg: Math.round(sp.bearingDeg) },
    long: lp
      ? { ink: c ? ink(c.longBcr) : null, segments: segs(lp), brgDeg: Math.round(lp.bearingDeg) }
      : null,
    favoured: c?.verdict === 'long' ? 'long' : c?.verdict === 'short' ? 'short' : null,
  }
}
