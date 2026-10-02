// BANDS FOR YOU — the band advice as tiles that read from across the desk. Pure (no JSX), so every rule
// unit-tests without React; components/prop/BandTiles.tsx draws them.
//
// One tile per band in a rig's band-stack order (160 m → 6 m, then 4 m and 2 m), never best-first: a
// tile stays where the operator's eye left it. Each tile says the SAME thing the map's Band conditions
// list, the band menu and the NOW bar say (propViz `bandConditionCell`, through bandConditions.ts's
// published store): the word (Open / Marginal / Closed) and its colour, which follows the word — a
// band heard now is Open and green, a closed one recedes (no tint, dim letters, never red). On top:
//   · a dot for what is heard — ● active for you now, ◐ some activity, ○ none heard (the word then
//     comes from the model alone);
//   · ★ the advisor's best band right now: the first band of its ranking that is not closed (the
//     Band Advisor's own one-line summary picks the same one, paneFormat `bandAdvisorLine`);
//   · a ring on the band the radio is on;
//   · on 6 m, 4 m and 2 m, the mode of an opening the detector sees there (Es, Tropo, Aurora, F2, MS).
// UNKNOWN IS NEVER GREEN: no snapshot, an offline one, or one older than the band menu's staleness bound
// gives hollow neutral tiles for the standard bands — the band menu's own rule, from its own store.
import type { BandReport, OpeningView, PropagationSnapshot } from '../types'
import { BAND_CONDITIONS_STALE_S } from '../bandConditions'
import { bandConditionCell, modeledVar } from '../propViz'

/** The rig's band stack, low to high. The advisor reports HF and 6 m; 4 m and 2 m appear when it
 *  reports them or the opening detector sees one there. */
export const TILE_BANDS = ['160m', '80m', '60m', '40m', '30m', '20m', '17m', '15m', '12m', '10m', '6m', '4m', '2m'] as const
/** The standard set a box shows with no data: the bands the advisor normally reports. */
const STANDARD_BANDS = TILE_BANDS.slice(0, 11)
const VHF = new Set(['6m', '4m', '2m'])

/** The opening detector's mode label → the short token on a tile. Mode names are technical tokens,
 *  the same in every language. Unknown shows nothing. */
const MODE_TOKEN: Record<string, string> = {
  'Sporadic-E': 'Es',
  Es: 'Es',
  F2: 'F2',
  Aurora: 'Aurora',
  'Meteor scatter': 'MS',
  Tropo: 'Tropo',
}

export type TileState = 'open' | 'marginal' | 'closed' | 'unknown'

export interface BandTile {
  band: string
  state: TileState
  /** The list's own word; empty for unknown (the component says "no data"). */
  word: string
  /** The cell's colour token; null for unknown (a hollow neutral tile). */
  color: string | null
  /** What is heard: active for you now / some activity / none. */
  heard: 'active' | 'some' | 'none'
  best: boolean
  onRig: boolean
  /** A VHF opening's mode token, or null. */
  mode: string | null
  report: BandReport | null
}

/** Whether a snapshot is live enough to colour anything: the band menu's rule. */
export function snapshotUsable(prop: PropagationSnapshot | null, nowS: number): boolean {
  return !!prop && prop.source !== 'offline' && nowS - prop.asOf <= BAND_CONDITIONS_STALE_S
}

function modeOn(band: string, openings: readonly OpeningView[]): string | null {
  if (!VHF.has(band)) return null
  for (const o of openings) if (o.band === band && MODE_TOKEN[o.mode]) return MODE_TOKEN[o.mode]
  return null
}

/** The tiles a box shows for this snapshot, in band-stack order. */
export function bandTiles(prop: PropagationSnapshot | null, rigBand: string | null, nowS: number): BandTile[] {
  const usable = snapshotUsable(prop, nowS)
  const reports = new Map((usable ? (prop!.advisory?.bands ?? []) : []).map((b) => [b.band, b]))
  const openings = usable ? (prop!.openings ?? []) : []
  const ranked = usable ? (prop!.advisory?.bands ?? []) : []
  const best = ranked.find((b) => bandConditionCell(b).word !== 'Closed')?.band ?? null
  const bands = usable
    ? TILE_BANDS.filter((b) => reports.has(b) || modeOn(b, openings) != null)
    : STANDARD_BANDS
  return bands.map((band): BandTile => {
    const report = reports.get(band) ?? null
    const base = { band, onRig: rigBand === band, mode: modeOn(band, openings), report }
    // 4 m or 2 m with no report of their own: an opening the detector sees there IS a band heard
    // open, so the tile says Open in green and names the mode.
    if (!report && base.mode) return { ...base, state: 'open', word: 'Open', color: modeledVar('Open'), heard: 'active', best: false }
    if (!report) return { ...base, state: 'unknown', word: '', color: null, heard: 'none', best: false }
    const cell = bandConditionCell(report)
    const state: TileState = cell.word === 'Closed' ? 'closed' : cell.word === 'Marginal' ? 'marginal' : 'open'
    const heard = report.tier === 'Active' ? 'active' : report.tier === 'Moderate' ? 'some' : 'none'
    return { ...base, state, word: cell.word, color: cell.color, heard, best: best === band }
  })
}
