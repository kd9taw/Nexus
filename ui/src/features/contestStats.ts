// ---------------------------------------------------------------------------
// ⭐ THE CONTEST STATS TAB'S NUMBERS — read off the contest log the session already sends, and
// nothing else: no score of its own, no network. The score, the multipliers and the rate meter
// keep their own sources (the engine's totals, the multiplier boards, `contestRate`); this adds
// only what no other surface works out.
//
// WHICH ROWS COUNT. A dupe and a contact in a banned mode are in the log and count for nothing
// (`FieldDayQso.dupe` / `.banned`, the engine's own answers), so the totals, the hourly bars, the
// band, mode, country and distance breakdowns leave them out and add up to the score's contact
// count. Operating time is the exception, by its own rule (`contestOpTime`): the station was on
// the air for every row.
//
// Pure: the caller supplies the clock, the window and the places.
// ---------------------------------------------------------------------------
import type { FieldDayQso } from '../types'
import { gridToLatLon, haversineKm, isValidLoggedGrid, type LatLon } from '../grid'
import { OFF_TIME_MIN_SEC, operatingTime, type OperatingTime } from './contestOpTime'

const HOUR = 3600
const DAY = 86_400
/** A log with no contest window draws at most a week of hours, the newest. */
const MAX_HOURS = 168

/** Where the station places a call (`contest_stats_places`): the country the contest counts it as
 *  and the country file's point for it. */
export interface StatsPlace {
  entity: string
  lat: number
  lon: number
}

export interface Tally {
  key: string
  count: number
}

export interface ContestStats {
  /** The rows that count. */
  contacts: number
  /** On time and breaks, under `minOffSec`; null before the first contact. */
  op: OperatingTime | null
  /** Contacts per hour of on time; null until there is on time to divide by. */
  overallPerHour: number | null
  /** The most contacts inside any 60 minutes, and when that hour began. */
  best60: { count: number; startUnix: number } | null
  lastContactUnix: number | null
  /** Contacts since 00:00 UTC. */
  today: number
  /** One bar per UTC clock hour. */
  hours: { startUnix: number; count: number }[]
  /** Contacts outside the contest window (test contacts before the start), not drawn. */
  outsideWindow: number
  bands: Tally[]
  modes: Tally[]
  /** The distinct four-character squares received, sorted; null when the contest receives none. */
  grids: string[] | null
}

export interface StatsOptions {
  nowUnix: number
  /** The contest window (Unix UTC), when the session has one. */
  windowStart?: number
  windowEnd?: number
  /** The index in `rcvd` of the slot that carries a grid, when the contest receives one. */
  gridSlot?: number | null
  minOffSec?: number
}

/** A row the score counts. */
export const counts = (q: FieldDayQso): boolean => !q.dupe && !q.banned

function stamp(q: FieldDayQso): number | null {
  const t = q.whenUnix
  return typeof t === 'number' && Number.isFinite(t) && t > 0 ? t : null
}

function tally(keys: string[]): Tally[] {
  const m = new Map<string, number>()
  for (const k of keys) m.set(k, (m.get(k) ?? 0) + 1)
  return [...m].map(([key, count]) => ({ key, count })).sort((a, b) => b.count - a.count || a.key.localeCompare(b.key))
}

/** The grid a row's exchange carries, upper-cased, or null. */
function sentGrid(q: FieldDayQso, gridSlot: number | null | undefined): string | null {
  if (gridSlot == null) return null
  const g = q.rcvd?.[gridSlot]?.trim() ?? ''
  return isValidLoggedGrid(g) ? g.toUpperCase() : null
}

export function contestStats(log: readonly FieldDayQso[], o: StatsOptions): ContestStats {
  const rows = log.filter(counts)
  const stamps = rows.map(stamp).filter((t): t is number => t !== null).sort((a, b) => a - b)
  const op = operatingTime(log, o.minOffSec ?? OFF_TIME_MIN_SEC)
  const overallPerHour = op && op.onSec > 0 ? (rows.length * HOUR) / op.onSec : null

  // The best 60 minutes: a window [t_i, t_i + 1 h) slid over every contact.
  let best60: ContestStats['best60'] = null
  for (let i = 0, j = 0; j < stamps.length; j++) {
    while (stamps[j] - stamps[i] >= HOUR) i++
    const count = j - i + 1
    if (!best60 || count > best60.count) best60 = { count, startUnix: stamps[i] }
  }

  const today0 = Math.floor(o.nowUnix / DAY) * DAY
  const today = stamps.filter((t) => t >= today0 && t <= o.nowUnix).length

  // The hours drawn: the contest window, or the log's own span.
  const windowed = typeof o.windowStart === 'number' && typeof o.windowEnd === 'number' && o.windowEnd > o.windowStart
  let first: number | null = null
  let end: number | null = null
  if (windowed) {
    first = Math.floor(o.windowStart! / HOUR) * HOUR
    end = o.windowEnd!
  } else if (stamps.length > 0) {
    first = Math.floor(stamps[0] / HOUR) * HOUR
    end = stamps[stamps.length - 1] + 1
    first = Math.max(first, Math.floor((end - 1) / HOUR) * HOUR - (MAX_HOURS - 1) * HOUR)
  }
  const hours: ContestStats['hours'] = []
  let outsideWindow = 0
  if (first !== null && end !== null) {
    for (let h = first; h < end; h += HOUR) hours.push({ startUnix: h, count: 0 })
    for (const t of stamps) {
      const i = Math.floor((t - first) / HOUR)
      if (t < end && i >= 0 && i < hours.length) hours[i].count++
      else if (windowed) outsideWindow++
    }
  }

  const grids =
    o.gridSlot == null
      ? null
      : [...new Set(rows.map((q) => sentGrid(q, o.gridSlot)?.slice(0, 4)).filter((g): g is string => !!g))].sort()

  return {
    contacts: rows.length,
    op,
    overallPerHour,
    best60,
    lastContactUnix: stamps.length ? stamps[stamps.length - 1] : null,
    today,
    hours,
    outsideWindow,
    bands: tally(rows.map((q) => q.band || '?')),
    modes: tally(rows.map((q) => q.submode || q.mode || '?')),
    grids,
  }
}

export interface DistanceStats {
  /** Contacts a distance was measured for. */
  measured: number
  longest: { call: string; entity: string | null; km: number } | null
  shortestKm: number | null
  averageKm: number | null
}

/** Distances from the station to each counting contact: to the grid its exchange sent where it sent
 *  one, else to the country file's point for its call. */
export function distanceStats(
  log: readonly FieldDayQso[],
  places: ReadonlyMap<string, StatsPlace | null>,
  home: LatLon | null,
  gridSlot: number | null,
): DistanceStats {
  if (!home) return { measured: 0, longest: null, averageKm: null, shortestKm: null }
  let measured = 0
  let sum = 0
  let longest: DistanceStats['longest'] = null
  let shortestKm: number | null = null
  for (const q of log) {
    if (!counts(q)) continue
    const place = places.get(q.call) ?? null
    const grid = sentGrid(q, gridSlot)
    const there = (grid && gridToLatLon(grid)) || place
    if (!there) continue
    const km = haversineKm(home, there)
    measured++
    sum += km
    if (!longest || km > longest.km) longest = { call: q.call, entity: place?.entity ?? null, km }
    if (shortestKm === null || km < shortestKm) shortestKm = km
  }
  return { measured, longest, shortestKm, averageKm: measured ? sum / measured : null }
}

export interface EntityStats {
  entities: { entity: string; count: number }[]
  /** Counting contacts outside the station's own country; null when that country is not known. */
  dx: number | null
  /** Counting contacts the country file could not place. */
  unplaced: number
}

export function entityStats(
  log: readonly FieldDayQso[],
  places: ReadonlyMap<string, StatsPlace | null>,
  homeEntity: string | null,
): EntityStats {
  const names: string[] = []
  let unplaced = 0
  for (const q of log) {
    if (!counts(q)) continue
    const p = places.get(q.call)
    if (p) names.push(p.entity)
    else unplaced++
  }
  return {
    entities: tally(names).map(({ key, count }) => ({ entity: key, count })),
    dx: homeEntity === null ? null : names.filter((n) => n !== homeEntity).length,
    unplaced,
  }
}
