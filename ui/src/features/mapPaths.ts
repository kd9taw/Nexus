/**
 * TX/RX path lines — WHICH station-to-QTH relationships earn a line on the map.
 *
 * The operator asked for GridTracker's transmit/receive lines. "Like GridTracker" is a
 * picture, not a specification, so the relationships are pinned down here, in one place,
 * shared by the 2-D map and the 3-D globe (the `features/pulse.ts` pattern — 2D↔3D parity
 * is a stated contract in this tree, and it only holds when the rule has one home):
 *
 *   TX — a station that REPORTED HEARING ME. `MapSpot.heardMe`: the PSK Reporter / RBN
 *        reception reports the backend already merges (`mapspots.rs`, `Side::HeardMe`).
 *        Green everywhere, because `#3ddc6a` already means "heard ME" on both surfaces.
 *   RX — a station I DECODED. The operator's own roster (`Station[]` with a grid).
 *
 * ⚠️ WHY RX IS NOT `MapSpot { heardMe: false }`. That flag is false for BOTH my own
 * decodes and the far↔far cluster/RBN firehose. A line drawn from it would run from MY
 * QTH along a path neither of my antennas was on — a third party's spot redrawn as my
 * own reception. It is also the single biggest source of the spider's web the operator
 * would then have to turn off. The roster is the unambiguous "I heard them" set.
 *
 * LEGIBILITY IS THE FEATURE. A line is far more ink than a dot, so stale ones LEAVE
 * rather than dim forever, and each direction is hard-capped: a contest pileup can put
 * 400 spots on the canvas but never more than `MAX_PATHS` strokes per direction.
 */
import { gridToLatLon, type LatLon } from '../grid'
import type { MapSpot, Station } from '../types'

/** One great-circle path from the operator's QTH to a station. */
export interface MapPath {
  call: string
  ll: LatLon
  /** 'tx' = they reported hearing ME; 'rx' = I decoded them. */
  dir: 'tx' | 'rx'
  /** Recency dimming, 0..1. */
  fade: number
}

/**
 * Strokes per direction. Every path is a radial from ONE point, so the fan reads far
 * better than GridTracker's arbitrary web — but 30 is where a fan stops being a fan.
 * Both directions on at once is a 60-stroke worst case.
 */
export const MAX_PATHS = 30

/** A reception report older than this stops earning a line (it stops being news). */
export const TX_MAX_AGE_SECS = 30 * 60

/** Age fade for a TX path — the same ladder the spot dots use, minus the stale rung
 * (a path that old is dropped above, not drawn at 0.35). */
function txFade(ageSecs: number): number {
  return ageSecs < 10 * 60 ? 1 : 0.6
}

/**
 * Stations that reported hearing ME, freshest first, gated and capped.
 * `spots` is `PropagationSnapshot.spots` — pass it whole; the heard-me filter is here.
 */
export function txPaths(spots: readonly MapSpot[], cap = MAX_PATHS): MapPath[] {
  return spots
    .filter((s) => s.heardMe && s.ageSecs <= TX_MAX_AGE_SECS)
    .slice()
    .sort((a, b) => a.ageSecs - b.ageSecs)
    .slice(0, cap)
    .map((s) => ({
      call: s.call,
      ll: { lat: s.lat, lon: s.lon },
      dir: 'tx' as const,
      fade: txFade(s.ageSecs),
    }))
}

/**
 * Stations I decoded, freshest first, gated and capped.
 *
 * `exclude` is normally the TX set: a call heard BOTH ways gets its green TX line only.
 * Two dotted strokes over identical geometry read as a rendering artifact, and "they
 * hear me" is the scarcer, more valuable half of a two-way path.
 */
export function rxPaths(
  stations: readonly Station[],
  exclude: readonly MapPath[] = [],
  cap = MAX_PATHS,
): MapPath[] {
  const already = new Set(exclude.map((p) => p.call.toUpperCase()))
  const out: MapPath[] = []
  for (const s of stations) {
    // A stale roster row is history, not activity — no line at all.
    if (s.presence === 'stale') continue
    if (already.has(s.call.toUpperCase())) continue
    if (!s.grid) continue
    const ll = gridToLatLon(s.grid)
    if (!ll) continue
    out.push({ call: s.call, ll, dir: 'rx', fade: s.presence === 'active' ? 1 : 0.55 })
  }
  // Freshest first, then cap — `lastHeardSlot` is the roster's own recency clock.
  const slot = new Map(stations.map((s) => [s.call, s.lastHeardSlot]))
  out.sort((a, b) => (slot.get(b.call) ?? 0) - (slot.get(a.call) ?? 0))
  return out.slice(0, cap)
}
