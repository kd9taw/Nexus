// THE APRS MAP'S PURE HALF — what the APRS cockpit shows on its map and how, with no canvas and no
// DOM, so each rule is unit-testable (aprsMap.test.ts). components/AprsCockpit.tsx owns the
// choices and the filter; components/MapView.tsx draws the trails and the labels from these.
//
// One filter for the list, the counts and the map: the cockpit runs `filterAprsStations` once and
// hands the result to all three, so they can never disagree about which stations exist.
import type { AprsStation, AprsTrackPoint } from '../api'
import { resolveSymbol, symbolCategory, type SymbolCategory } from '../aprsSymbols'
import { haversineKm } from '../grid'

/** The age choices, minutes. 0 = everything the station store still keeps (its own age window,
 *  Settings ▸ APRS). */
export const APRS_AGE_CHOICES = [0, 10, 30, 60, 120] as const
export type AprsAgeMin = (typeof APRS_AGE_CHOICES)[number]

/** The kind-of-station choices: every symbol family, or all of them. */
export const APRS_TYPE_CHOICES = ['all', 'mobile', 'fixed', 'wx', 'infra', 'object', 'air', 'marine', 'other'] as const
export type AprsTypeFilter = 'all' | SymbolCategory

/** The maps the APRS map can draw: the 2-D maps and Street (no WebGL globe: it has no APRS layer). */
export const APRS_MAP_PICKS = ['globe', 'world', 'aeqd', 'street'] as const
export type AprsMapPick = (typeof APRS_MAP_PICKS)[number]

export interface AprsFilter {
  /** Show stations only the internet feed reports (the header's existing toggle). */
  showInet: boolean
  ageMin: number
  type: AprsTypeFilter
}

/** The stations the operator asked to see, in the roster's order. */
export function filterAprsStations(
  stations: readonly AprsStation[],
  f: AprsFilter,
  nowSec: number,
): AprsStation[] {
  const cutoff = f.ageMin > 0 ? nowSec - f.ageMin * 60 : -Infinity
  return stations.filter(
    (st) =>
      (f.showInet || st.sourceKind !== 'inet') &&
      st.lastHeardUnix >= cutoff &&
      (f.type === 'all' || symbolCategory(resolveSymbol(st.symbolTable, st.symbolCode).glyph) === f.type),
  )
}

/** The part of a station's track to draw: the points inside the age filter, oldest first — or
 *  nothing when fewer than two are left, since one point is a position, not a trail. */
export function aprsTrail(st: AprsStation, nowSec: number, ageMin: number): AprsTrackPoint[] {
  const track = st.track ?? []
  const cutoff = ageMin > 0 ? nowSec - ageMin * 60 : -Infinity
  const kept = track.filter((p) => p.atUnix >= cutoff)
  return kept.length >= 2 ? kept : []
}

/** How far a station has travelled along its track and over how long, for its card. */
export function trackSummary(track: readonly AprsTrackPoint[] | undefined): { km: number; secs: number } | null {
  if (!track || track.length < 2) return null
  let km = 0
  for (let i = 1; i < track.length; i++) km += haversineKm(track[i - 1], track[i])
  return { km, secs: track[track.length - 1].atUnix - track[0].atUnix }
}

const moving = (st: AprsStation) => st.speedKnots != null && st.speedKnots > 1

/** Which stations get a label, in the order they claim space: the selection first, then anything
 *  moving, then the most recently heard. With labels off only the selection and the moving
 *  stations are named (the map's behaviour before labels could be turned on). */
export function aprsLabelOrder(stations: readonly AprsStation[], selected: string | null, all: boolean): string[] {
  const sel = selected?.toUpperCase() ?? null
  const rank = (st: AprsStation) => (st.call.toUpperCase() === sel ? 0 : moving(st) ? 1 : 2)
  return stations
    .filter((st) => all || rank(st) < 2)
    .slice()
    .sort((a, b) => rank(a) - rank(b) || b.lastHeardUnix - a.lastHeardUnix || a.call.localeCompare(b.call))
    .map((st) => st.call)
}

/** A label to place: its station's symbol at (x, y), the text's measured box (w × h) and how far
 *  from the symbol's centre the text must start (`clear`). All in canvas layout px. */
export interface LabelCandidate {
  key: string
  x: number
  y: number
  w: number
  h: number
  clear: number
}
/** A placed label: draw left-aligned at x, with y the middle line (textBaseline "middle"). */
export interface PlacedLabel {
  key: string
  x: number
  y: number
}

/**
 * LABELS THAT DO NOT COLLIDE. Greedy, in the candidates' order (the priority from
 * `aprsLabelOrder`): each label tries the right of its symbol, then the left, above and below, and
 * takes the first spot that stays inside the map and overlaps no label already placed and no other
 * station's symbol. A label with no free spot is left out — a busy net shows the names it has room
 * for instead of a pile of text, and the hover card and the list still name every station.
 */
export function placeAprsLabels(
  cands: readonly LabelCandidate[],
  bounds: { w: number; h: number },
  symbols: ReadonlyArray<{ x: number; y: number; r: number }>,
): PlacedLabel[] {
  type Rect = { x0: number; x1: number; y0: number; y1: number }
  const taken: Rect[] = []
  const hits = (a: Rect, b: Rect) => a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1
  const out: PlacedLabel[] = []
  for (const c of cands) {
    const spots: Array<[number, number]> = [
      [c.x + c.clear, c.y],
      [c.x - c.clear - c.w, c.y],
      [c.x - c.w / 2, c.y - c.clear - c.h / 2],
      [c.x - c.w / 2, c.y + c.clear + c.h / 2],
    ]
    for (const [x, y] of spots) {
      const r: Rect = { x0: x, x1: x + c.w, y0: y - c.h / 2, y1: y + c.h / 2 }
      if (r.x0 < 0 || r.y0 < 0 || r.x1 > bounds.w || r.y1 > bounds.h) continue
      if (taken.some((t) => hits(r, t))) continue
      const onSymbol = symbols.some(
        (s) =>
          Math.hypot(s.x - c.x, s.y - c.y) > 0.5 &&
          hits(r, { x0: s.x - s.r, x1: s.x + s.r, y0: s.y - s.r, y1: s.y + s.r }),
      )
      if (onSymbol) continue
      taken.push(r)
      out.push({ key: c.key, x, y })
      break
    }
  }
  return out
}

/** What the operator chose for the APRS map, remembered per window (`nexus.aprs.map`). */
export interface AprsMapPrefs {
  pick: AprsMapPick
  ageMin: AprsAgeMin
  type: AprsTypeFilter
  tracks: boolean
  labels: boolean
}

export const APRS_MAP_DEFAULTS: AprsMapPrefs = { pick: 'globe', ageMin: 0, type: 'all', tracks: true, labels: true }

/** A stored record, each field checked on its own: anything unknown falls back to its default
 *  without discarding the rest. */
export function parseAprsMapPrefs(raw: string | null): AprsMapPrefs {
  let v: unknown = null
  try {
    v = raw ? JSON.parse(raw) : null
  } catch {
    v = null
  }
  if (typeof v !== 'object' || v === null) return { ...APRS_MAP_DEFAULTS }
  const r = v as Record<string, unknown>
  const d = APRS_MAP_DEFAULTS
  return {
    pick: (APRS_MAP_PICKS as readonly unknown[]).includes(r.pick) ? (r.pick as AprsMapPick) : d.pick,
    ageMin: (APRS_AGE_CHOICES as readonly unknown[]).includes(r.ageMin) ? (r.ageMin as AprsAgeMin) : d.ageMin,
    type: (APRS_TYPE_CHOICES as readonly unknown[]).includes(r.type) ? (r.type as AprsTypeFilter) : d.type,
    tracks: typeof r.tracks === 'boolean' ? r.tracks : d.tracks,
    labels: typeof r.labels === 'boolean' ? r.labels : d.labels,
  }
}

export function aprsMapPrefsValue(p: AprsMapPrefs): string {
  return JSON.stringify(p)
}
