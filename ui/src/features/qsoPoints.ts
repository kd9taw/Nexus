// Shared QSO → map-point reduction for the Logbook's globe (3-D) and flat map (2-D), so the two
// views plot the SAME dots. Dedupe to unique 4-char Maidenhead squares — what keeps a 50k-QSO
// FT8 log at ~a thousand points instead of 50k — carrying each square's QSO count (dot
// brightness) and its most-recent QSO's band (dot colour). Grid-less QSOs are skipped.
import { gridToLatLon } from '../grid'
import type { LoggedQso } from '../types'

export interface QsoPoint {
  lat: number
  lng: number
  /** QSOs logged in this square. */
  n: number
  /** Band of the square's most-recent QSO — the dot's colour (the app's band palette). */
  band: string
}

/** One worked 4-char square before it is placed on the globe: what the log can answer on its own
 *  (the square, its QSO count, its most-recent QSO's band), with the latitude/longitude left to the
 *  UI. The Logbook's globe asks for these through `LogSource` (SPEC-2 v2 §4 `grid_points`). */
export interface QsoGridCount {
  /** The 4-char Maidenhead square, upper-cased. */
  grid: string
  n: number
  band: string
}

/**
 * Reduce `qsos` to per-square points. `band` filters by band ('all' = every band pooled) —
 * grids are a PER-BAND achievement (VUCC), so a selected band shows only ITS squares.
 */
export function qsoGridPoints(qsos: LoggedQso[], band: string): QsoPoint[] {
  return gridCountsToPoints(qsoGridCounts(qsos, band))
}

/** The squares `qsoGridPoints` places, in first-seen order — its reduction, without the placing. */
export function qsoGridCounts(qsos: LoggedQso[], band: string): QsoGridCount[] {
  const acc = new Map<string, { n: number; band: string; when: number }>()
  for (const q of qsos) {
    if (band !== 'all' && q.band !== band) continue
    const gr = (q.grid ?? '').trim().toUpperCase()
    if (gr.length < 4) continue
    const key = gr.slice(0, 4)
    const cur = acc.get(key)
    if (cur) {
      cur.n += 1
      if (q.whenUnix >= cur.when) {
        cur.when = q.whenUnix
        cur.band = q.band
      }
    } else {
      acc.set(key, { n: 1, band: q.band, when: q.whenUnix })
    }
  }
  const counts: QsoGridCount[] = []
  acc.forEach((v, grid) => counts.push({ grid, n: v.n, band: v.band }))
  return counts
}

/** Place counted squares on the globe; a square the grid parser cannot place is skipped. */
export function gridCountsToPoints(counts: readonly QsoGridCount[]): QsoPoint[] {
  const pts: QsoPoint[] = []
  for (const c of counts) {
    const ll = gridToLatLon(c.grid)
    if (ll) pts.push({ lat: ll.lat, lng: ll.lon, n: c.n, band: c.band })
  }
  return pts
}
