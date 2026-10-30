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
import { kpSourceWords, pathWhyFacts, utcHourLabel } from './pathWhy'

export type RouteName = 'short' | 'long'

/** A route counts as open from this circuit reliability, closed below the other (the engine's
 * `route::OPEN_BCR` / `CLOSED_BCR`). */
const OPEN_BCR = 0.5
const CLOSED_BCR = 0.1
/** Below this long-minus-short distance (km) the stations are near-antipodal and no route is
 * called (the engine's `route::NEAR_ANTIPODE_KM`). */
const NEAR_ANTIPODE_KM = 500

/** The stations are near-antipodal: every heading is about the same distance, so neither route
 * is a choice. */
export function nearAntipode(p: PathPrediction | null | undefined): boolean {
  const sp = p?.geometry
  const lp = p?.longPath?.geometry
  return !!sp && !!lp && lp.distKm - sp.distKm < NEAR_ANTIPODE_KM
}

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
  if (!p?.routes || !sp || !lp || nearAntipode(p)) return null
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

/** The long path is never more than "possible" until it is validated — held here too, so a call
 * that somehow came without a confidence cannot read "likely". */
function confWords(c: RouteCall): string {
  return c.verdict === 'long' || c.confidence === 'possible'
    ? t('pathRoute.conf.possible')
    : t('pathRoute.conf.likely')
}

/** The one-sentence hint ("Beam 80° for 20m: long path (likely) — …"), or null. `short` leaves
 * out the reason clauses (a narrow card shows them on hover). */
export function routeHintLine(h: RouteHint | null, short = false): string | null {
  if (!h) return null
  const { band, call } = h
  if (h.route === 'either') {
    // "Within 6 dB" only when the margin said so; an either the grey line made from a margin
    // call names both reasons instead.
    const grey = call.reasons.find((r) => r.kind === 'greyLine')
    const [sp, lp] = [h.spBrgDeg, h.lpBrgDeg]
    let text: string
    if (call.reasons.some((r) => r.kind === 'withinMargin')) text = t('pathRoute.hint.either', { band, sp, lp })
    else if (grey?.kind === 'greyLine' && grey.route === 'long') text = t('pathRoute.hint.eitherGreyLong', { band, sp, lp })
    else if (grey?.kind === 'greyLine') text = t('pathRoute.hint.eitherGreyShort', { band, sp, lp })
    else return null
    return t('pathWhy.sentence', { text })
  }
  const conf = confWords(call)
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

function chipFacts(p: PathPrediction | null | undefined, route: RouteName) {
  const f = pathWhyFacts(route === 'long' ? p?.longPath : p)
  // An unknown Kp has no edge: `polarNow` is false and no forecast storm is held, so no chip.
  if (!f || (!f.polarNow && !f.storm)) return null
  const storm = f.storm && {
    kp: Number.isInteger(f.storm.kp) ? String(f.storm.kp) : f.storm.kp.toFixed(1),
    time: utcHourLabel(f.storm.unix),
  }
  return { ...f, storm }
}

/** The chip for one route, named as the why line names it: "Polar path" in the cap proper,
 * "Auroral zone" past the edge but short of it, each with the forecast storm that would deepen
 * it; a route clear of both now that a forecast storm would reach gets an "if". Null when the
 * route is clear and no storm is forecast over it, or Kp is unknown. */
export function polarChip(p: PathPrediction | null | undefined, route: RouteName): string | null {
  const f = chipFacts(p, route)
  if (!f) return null
  const s = f.storm
  if (!f.polarNow) return s && t('pathRoute.chip.stormIf', { kp: s.kp, time: s.time })
  if (f.capNow) return s ? t('pathRoute.chip.storm', { kp: s.kp, time: s.time }) : t('pathRoute.chip.polar')
  return s ? t('pathRoute.chip.zoneStorm', { kp: s.kp, time: s.time }) : t('pathRoute.chip.zone')
}

/** The chip's hover: what it means, and where this hour's Kp came from (or, for the "if", that
 * it is a forecast). */
export function polarChipTitle(p: PathPrediction | null | undefined, route: RouteName): string | null {
  const f = chipFacts(p, route)
  if (!f) return null
  const s = f.storm
  if (!f.polarNow) return s && t('pathRoute.chip.title.stormIf', { kp: s.kp, time: s.time })
  const source = kpSourceWords(f.now.kpSource)
  return f.capNow ? t('pathRoute.chip.title.cap', { source }) : t('pathRoute.chip.title.zone', { source })
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

/** What the map draws for the selected station: each route's ink and its polar / grey-line
 * stretches. `long` is null when the engine modelled no long path (the map then draws the long
 * route as plain geometry). */
export function ribbonRoutes(
  p: PathPrediction | null | undefined,
  focusBand?: string | null,
): { short: RibbonRoute; long: RibbonRoute | null; favoured: RouteName | null } | null {
  const sp = p?.geometry
  if (!p || !sp) return null
  const lp = p.longPath?.geometry
  const c = p.routes ? pickCall(p.routes, focusBand) : null
  return {
    short: { ink: c ? ink(c.shortBcr) : null, segments: sp.segmentsNow ?? [], brgDeg: Math.round(sp.bearingDeg) },
    long: lp
      ? { ink: c ? ink(c.longBcr) : null, segments: lp.segmentsNow ?? [], brgDeg: Math.round(lp.bearingDeg) }
      : null,
    favoured: c?.verdict === 'long' ? 'long' : c?.verdict === 'short' ? 'short' : null,
  }
}
