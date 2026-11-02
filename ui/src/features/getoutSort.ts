// THE GETTING OUT LIST'S SORT — the order of the stations hearing you: Distance (most distant first,
// the order the station sends them and the default), Call A–Z, Newest (the most recent report first,
// by the report's age) and SNR (the strongest first; a report that carries no SNR goes last). Pure
// apart from the stored choice, so every order unit-tests without React.
//
// TIES ARE STABLE, never the station's arrival order: an equal key goes by distance (most distant
// first), then by call — so a live update that reorders the station's answer never shuffles equal
// rows. Distance's own tie goes by call.
//
// THE CHOICE IS KEPT PER BOX AND ACROSS A RESTART: one record per window (features/windowScope, like
// the box's Start fresh time), keyed by the box's place — a Conditions slot (`left1`…), a dashboard rail
// slot (`rail1`…) or a cockpit's box, named by its cockpit and its place there (`operate.box2`,
// `cw.box2`…), so the FT8 cockpit's box2 and CW's keep separate choices. A pop-out starts on the main
// window's record and diverges at its first pick. The record is VALIDATED ON LOAD: a value this build
// does not know, a record that does not parse, or a name the object prototype carries reads as Distance.
import type { HeardMe } from '../types'
import { surfaceGet, surfaceSet } from './windowScope'

/** The orders, as the bar lists them. Distance first: the default. */
export const GETOUT_SORTS = ['distance', 'call', 'newest', 'snr'] as const
export type GetoutSort = (typeof GETOUT_SORTS)[number]

const SORT_KEY = 'nexus.connect.getout.sort'

function isSort(v: unknown): v is GetoutSort {
  return typeof v === 'string' && (GETOUT_SORTS as readonly string[]).includes(v)
}

/** Plain code-point order, the same on every machine and locale (calls are ASCII). */
function byCall(a: HeardMe, b: HeardMe): number {
  return a.call < b.call ? -1 : a.call > b.call ? 1 : 0
}

/** The tie-break every order ends with: most distant first, then call. */
function byDistance(a: HeardMe, b: HeardMe): number {
  return b.km - a.km || byCall(a, b)
}

const COMPARE: Record<GetoutSort, (a: HeardMe, b: HeardMe) => number> = {
  distance: byDistance,
  call: (a, b) => byCall(a, b) || b.km - a.km,
  newest: (a, b) => a.ageSecs - b.ageSecs || byDistance(a, b),
  snr: (a, b) => {
    if (a.snr == null || b.snr == null) {
      if (a.snr != null) return -1
      if (b.snr != null) return 1
      return byDistance(a, b)
    }
    return b.snr - a.snr || byDistance(a, b)
  },
}

/** The reports in `sort`'s order — a new array; the one given is never reordered. */
export function sortGetout(reports: readonly HeardMe[], sort: GetoutSort): HeardMe[] {
  return [...reports].sort(COMPARE[sort])
}

/** This window's record, or an empty one when there is none, it does not parse, or it is not a record. */
function readRecord(): Record<string, unknown> {
  try {
    const v: unknown = JSON.parse(surfaceGet(SORT_KEY) ?? '{}')
    return v != null && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : {}
  } catch {
    return {}
  }
}

/** The order the box at `instance` keeps; Distance when it has none this build knows. */
export function loadGetoutSort(instance: string): GetoutSort {
  const rec = readRecord()
  const v = Object.prototype.hasOwnProperty.call(rec, instance) ? rec[instance] : undefined
  return isSort(v) ? v : 'distance'
}

/** Keep `sort` for the box at `instance`, beside the other boxes' choices. */
export function saveGetoutSort(instance: string, sort: GetoutSort): void {
  surfaceSet(SORT_KEY, JSON.stringify({ ...readRecord(), [instance]: sort }))
}
