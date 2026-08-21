// Connect pane formatting — the shared helpers (moved verbatim from ConnectView) plus
// the Basic-view projections. CRITICAL: each *Line reads ONLY the same DTO fields its
// Expert render consumes, so Basic is a pure projection of the same data, never a
// second heuristic. Pure (no JSX) → node-testable.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every line here
// used to be assembled from fragments; each is ONE catalog sentence now, with the data
// interpolated. What stays in the code is the data itself — band and mode names, bearings,
// distances, MUF and beacon frequencies, octants, SFI/Kp, hours in Z — and every word the
// BACKEND supplies: the advisory headline, the workability, the dual-state word, the Kp
// impact sentence, the insight text and the getting-out direction summary. Those are values
// here and are translated in phase 3, at their source.
import { modeClassOf } from '../../features/needs'
import { t } from '../../i18n'
import { beaconsNow, beaconHeard } from '../../features/beacons'
import { isEsSeason } from '../../features/es'
import { nextTerminatorMs } from '../../mapGeo'
import {
  gridToLatLon,
  haversineKm,
  bearingDeg,
  azimuthLabel,
  azimuthTo,
  backendAzimuth,
  type Azimuth,
} from '../../grid'
import { dualStateLabel, kpImpact, sortInsights } from '../../propViz'
import { buildChaseTargets, chaseSummaryLine } from '../../features/chase'
import { buildChaseFeed, chaseFeedLine as feedSummary } from '../../features/chaseFeed'
import { getoutSummary } from '../../features/getout'
import type { PropagationSnapshot } from '../../types'
import type { PaneContext } from './paneContext'

/** Need tag → the chip vocabulary (canonical record in features/needVisuals). */
export { NEED_CHIP } from '../../features/needVisuals'

export function provLabel(
  source: PropagationSnapshot['source'],
  asOf: number,
): { label: string; cls: string } {
  if (source === 'live') return { label: t('connect.prov.live'), cls: 'live' }
  if (source === 'partial') return { label: t('connect.prov.partial'), cls: 'partial' }
  if (source === 'cached') {
    const m = Math.max(0, Math.round((Date.now() / 1000 - asOf) / 60))
    return { label: t('connect.prov.cached', { mins: m }), cls: 'cached' }
  }
  return { label: t('connect.prov.offline'), cls: 'offline' }
}

/** A DXpedition's announced modes → its work-routing mode (CW-only → CW, voice-only →
 *  SSB, mixed/unknown → null = digital default). Mirrors MapView's rule. */
export function dxpedWorkMode(modes?: string[]): string | null {
  if (!modes || modes.length === 0) return null
  const classes = new Set(modes.map((m) => modeClassOf(m)))
  if (classes.size === 1) {
    if (classes.has('CW')) return 'CW'
    if (classes.has('Phone')) return 'SSB'
  }
  return null
}

// ---- Basic sentence projections (one plain operator line; null-safe = empty/loading) ----

export function advisoryLine(c: PaneContext): string {
  if (!c.prop) return t('connect.basic.loading')
  // offline = an honest no-live-data snapshot (no/invalid call OR feeds unreachable); the
  // remedy isn't always "set your callsign", so keep it neutral + always accurate.
  if (c.prop.source === 'offline') return t('connect.basic.offline')
  return c.prop.advisory.headline // headline IS the plain verdict
}

/** Chase — how many needed stations are being heard, and how many are workable right now. */
export function chaseLine(c: PaneContext): string {
  return chaseSummaryLine(buildChaseTargets(c.needAlerts, c.bandOutlook, Date.now()))
}

/** Chase Feed — the ranked fusion of heard needs + on-air expeditions. */
export function chaseFeedLine(c: PaneContext): string {
  return feedSummary(
    buildChaseFeed(
      c.needAlerts,
      c.bandOutlook,
      c.prop && c.prop.source !== 'offline' ? c.prop.dxpeditions : null,
      c.dxpedWindows,
      Date.now(),
    ),
  )
}

export function bandAdvisorLine(c: PaneContext): string {
  if (c.prop?.source === 'offline') return t('connect.basic.bandAdvisor.offline')
  const bands = c.prop?.advisory.bands ?? []
  // Project through the SAME dual-state word the BandAdvisor shows (modeled-open beats a
  // silent observed tier), so Basic never reads "Quiet"/"Closed" while the advisor says
  // "Open" — the false-dead-band bug the A1 dual-state work fixed.
  const isOpen = (x: (typeof bands)[number]) => dualStateLabel(x.modeled, x.tier).word !== 'Closed'
  const b = bands.find(isOpen) ?? bands[0]
  if (!b) return t('connect.basic.bandAdvisor.none')
  const word = dualStateLabel(b.modeled, b.tier).word
  // Two whole sentences: naming the region it is best TO is a different statement.
  return b.bestRegion
    ? t('connect.basic.bandAdvisor.bestRegion', {
        band: b.band,
        region: b.bestRegion.region,
        word,
      })
    : t('connect.basic.bandAdvisor.best', { band: b.band, word })
}

/** The entity name the selection headlines with — spot, then DXpedition, then the
 * heard station's country. One definition so the card and this module's text mirror
 * can never name different places (and so the heading below matches whichever it is). */
export function selectionEntity(c: PaneContext): string | null {
  return c.selSpot?.entity ?? c.selDxped?.entity ?? c.selStation?.country ?? null
}

/**
 * The beam heading for whatever is selected, or null.
 *
 * Three sources in the same precedence as the name above, because the three kinds of
 * selection know different things:
 *  · a MAP SPOT already carries resolved coordinates, and its own `approx` flag
 *    records whether they came from a real grid or from the entity centre — more
 *    accurate than re-deciding that here,
 *  · a DXPEDITION carries a bearing the backend measured from the announced grid,
 *  · a heard STATION has a grid when it sent one, else its entity's centre.
 */
export function selectionAzimuth(c: PaneContext): Azimuth | null {
  if (c.selSpot) {
    const me = gridToLatLon(c.myGrid)
    if (!me) return null
    return {
      deg: bearingDeg(me, { lat: c.selSpot.lat, lon: c.selSpot.lon }),
      approx: c.selSpot.approx,
    }
  }
  if (c.selDxped) return backendAzimuth(c.selDxped.bearingDeg, c.selDxped.distanceKm)
  if (c.selStation)
    return azimuthTo(c.myGrid, c.selStation.grid, c.selStation.country, c.entityCentroids)
  return null
}

export function selectionLine(c: PaneContext): string {
  if (!c.selectedCall) return t('connect.basic.selection.none')
  const who = selectionEntity(c) ?? '—'
  const azLabel = azimuthLabel(selectionAzimuth(c))
  const bandLabel = c.selSpot?.band ?? c.selDxped?.band ?? null
  // The heading and the band are known only sometimes, so each is a whole optional clause
  // carrying its own separator; "and is hearing you" changes what the sentence SAYS, so it
  // is inside the message rather than glued after it.
  const az = azLabel ? t('connect.basic.selection.az', { az: azLabel }) : ''
  const band = bandLabel ? t('connect.basic.selection.band', { band: bandLabel }) : ''
  const vals = { call: c.selectedCall, who, az, band }
  return c.selSpot?.heardMe
    ? t('connect.basic.selection.hearing', vals)
    : t('connect.basic.selection', vals)
}

export function outlookLine(c: PaneContext): string {
  const open = c.selectedCall ? c.pathOpen : c.outlookOpen
  const top = open[0]
  if (!top) {
    return c.selectedCall
      ? t('connect.basic.outlook.none.call', { call: c.selectedCall })
      : t('connect.basic.outlook.none.dx')
  }
  return c.selectedCall
    ? t('connect.basic.outlook.path', {
        band: top.band,
        call: c.selectedCall,
        window: top.window,
      })
    : t('connect.basic.outlook.best', {
        band: top.band,
        workability: top.workability.toLowerCase(),
      })
}

export function openingsLine(c: PaneContext): string {
  const o = c.prop?.openings[0]
  // Round to match the OpeningStrip Expert ("~N km") — same field, same formatting.
  return o
    ? t('connect.basic.openings', {
        band: o.band,
        octant: o.octant,
        km: Math.round(o.maxKm).toLocaleString(),
        stations: o.stations,
      })
    : t('connect.basic.openings.none')
}

export function spaceWxLine(c: PaneContext): string {
  const w = c.prop?.spaceWx
  if (!w || c.prop?.source === 'offline') return t('connect.basic.spaceWx.unavailable')
  // Round (matches the gauges' toFixed(0)) and derive the geomag descriptor from the SAME
  // kpImpact bucketing the SpaceWx Kp gauge uses (no divergent threshold ladder). Append
  // the live SWPC R-scale (radio blackout) when active — it's the "so what" for HF.
  // Four whole sentences, one per combination: a flare and a blackout are each a statement
  // of their own, and a clause glued on cannot be placed by a translation.
  const blackout = c.scales && c.scales.r >= 1
  const vals = {
    sfi: w.sfi.toFixed(0),
    kp: w.kp.toFixed(0),
    impact: kpImpact(w.kp).text,
    xray: w.xrayClass,
    scale: c.scales?.r ?? 0,
  }
  if (w.flare && blackout) return t('connect.basic.spaceWx.flareBlackout', vals)
  if (w.flare) return t('connect.basic.spaceWx.flare', vals)
  if (blackout) return t('connect.basic.spaceWx.blackout', vals)
  return t('connect.basic.spaceWx', vals)
}

export function getoutLine(c: PaneContext): string {
  const g = c.getout
  if (!g || g.count === 0) return t('connect.getout.none')
  const dir = getoutSummary(g.reports)
  return dir
    ? t('connect.basic.getout.dir', { count: g.count, dir })
    : t('connect.basic.getout.furthest', {
        count: g.count,
        km: g.maxKm.toLocaleString(),
      })
}

// ---- B2 Tier-1 pane projections ----

export function bestbandLine(c: PaneContext): string {
  if (!c.prop) return t('connect.basic.loading') // loading, not a settled negative
  if (c.prop.source === 'offline') return t('connect.basic.offline')
  const r = c.prop.bestToRegion?.[0]
  if (!r) return t('connect.basic.bestband.none')
  return t('connect.basic.bestband', {
    region: r.region,
    band: r.band,
    word: dualStateLabel(r.modeled, r.tier).word,
  })
}

export function activityLine(c: PaneContext): string {
  if (!c.prop) return t('connect.basic.loading')
  if (c.prop.source === 'offline') return t('connect.basic.activity.offline')
  const top = [...(c.prop.regionBand ?? [])].sort((a, b) => b.stations - a.stations)[0]
  if (!top) return t('connect.basic.activity.none')
  // `{{count}}` picks the form — the old `stn`/`stns` ternary is unwritable in most languages.
  return t('connect.basic.activity.top', {
    band: top.band,
    region: top.region,
    count: top.stations,
  })
}

export function beaconsLine(c: PaneContext): string {
  // Clock-derived (not prop-derived) — valid even offline; only the heard half degrades.
  // The list is callsigns and band names, built here and interpolated as one token run.
  const slots = beaconsNow(Date.now() / 1000)
  const heard = slots.filter((s) => beaconHeard(s.call, c.prop?.spots))
  if (heard.length) {
    return t('connect.basic.beacons.heard', {
      list: heard.map((h) => `${h.call} (${h.band})`).join(', '),
    })
  }
  return t('connect.basic.beacons.now', {
    list: slots.map((s) => `${s.call} ${s.band}`).join(' · '),
  })
}

export function insightsLine(c: PaneContext): string {
  if (!c.prop) return t('connect.basic.loading')
  if (c.prop.source === 'offline') return t('connect.basic.offline')
  return sortInsights(c.prop.insights ?? [])[0]?.plain ?? t('connect.basic.insights.none')
}

// ---- B3 Tier-2 pane projections (no-network panes) ----

export function greylineLine(c: PaneContext): string {
  // Clock-derived from the operator grid — valid even when prop is null (like beacons).
  const ll = c.myGrid ? gridToLatLon(c.myGrid) : null
  if (!ll) return t('connect.basic.greyline.noGrid')
  const now = Date.now()
  const next = nextTerminatorMs(ll.lat, ll.lon, now)
  const mins = Math.max(0, Math.round((next.atMs - now) / 60000))
  const when =
    mins >= 60
      ? t('connect.basic.greyline.in.hours', { hours: Math.floor(mins / 60), mins: mins % 60 })
      : t('connect.basic.greyline.in.mins', { mins })
  // Sunrise and sunset are two whole sentences: which one it is, is the point of the line.
  return next.kind === 'rise'
    ? t('connect.basic.greyline.sunrise', { when })
    : t('connect.basic.greyline.sunset', { when })
}

export function bandHoursLine(c: PaneContext): string {
  const bo = c.bandOutlook
  if (!bo) return t('connect.basic.loading')
  // Peak (band, hour) over the SAME 24h reliability grid the LikelihoodHeatmap renders —
  // a strict subset of the Expert view (which shows bands[].hourly, not mufHourly).
  let best = { band: '', hour: 0, p: 0 }
  for (const b of bo.bands) {
    ;(b.hourly ?? []).forEach((p, h) => {
      if (p > best.p) best = { band: b.band, hour: h, p }
    })
  }
  if (!best.band) return t('connect.basic.bandHours.none')
  return t('connect.basic.bandHours.peak', {
    band: best.band,
    hour: best.hour,
    pct: Math.round(best.p * 100),
  })
}

export function measuredMufLine(c: PaneContext): string {
  const sts = c.muf ?? []
  // No stations = no live feed; a grid only RANKS existing stations, so don't nudge to
  // "set your grid" here (that wouldn't make ionosonde data appear).
  if (!sts.length) return t('connect.basic.muf.noData')
  const ll = c.myGrid ? gridToLatLon(c.myGrid) : null
  // Nearest station that actually reported a MUF.
  let best = sts.find((s) => s.mufMhz != null)
  if (ll) {
    let bestD = Infinity
    for (const s of sts) {
      if (s.mufMhz == null) continue
      const d = haversineKm(ll, { lat: s.lat, lon: s.lon })
      if (d < bestD) {
        bestD = d
        best = s
      }
    }
  }
  if (!best || best.mufMhz == null) return t('connect.basic.muf.noneNearby')
  return t('connect.basic.muf.nearby', {
    mhz: Math.round(best.mufMhz),
    mins: Math.round(best.ageSecs / 60),
  })
}

const ES_BANDS = new Set(['6m', '4m', '2m'])

/** The 6 m Es calling frequency — a dial reading, so it lives here and is interpolated. */
const ES_CALLING_MHZ = '50.313'

export function esNowcastLine(c: PaneContext): string {
  // Real status from the spot-evidenced VHF openings; season is only a soft pre-opening
  // prior (never declares an opening on its own).
  const top = (c.prop?.openings ?? []).find((o) => ES_BANDS.has(o.band))
  if (top) {
    return t('connect.basic.es.open', {
      band: top.band,
      octant: top.octant,
      km: Math.round(top.maxKm).toLocaleString(),
      mode: top.mode,
      count: top.stations,
    })
  }
  if (isEsSeason(Date.now())) return t('connect.basic.es.season', { freq: ES_CALLING_MHZ })
  return t('connect.basic.es.quiet')
}
