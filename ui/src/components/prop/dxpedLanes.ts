// Month-grid geometry for the DXpedition calendar: which week rows exist, where
// each operation's bar starts and how far it spans, which lane it sits in, and
// what colour identifies it. Pure (no JSX, no DOM) → node-testable, because every
// failure mode here is SILENT — a bar one column off, or a lane that shifts
// mid-run, looks like a perfectly good calendar and quietly lies about the dates.
import type { CalendarEntry } from '../../types'

/** UTC midnight for the day containing `unix`. */
export function dayStart(unix: number): number {
  const d = new Date(unix * 1000)
  return Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate()) / 1000
}

/** The Monday on or before `unix` — grids read Mon–Sun here to match the ham
 * contest week, and because weekend operations should not be split across rows. */
export function weekStart(unix: number): number {
  const d = new Date(unix * 1000)
  const dow = (d.getUTCDay() + 6) % 7 // 0 = Monday
  return dayStart(unix) - dow * 86_400
}

/** Rows of 7 UTC-midnight day stamps covering every entry, from this week on. */
export function buildWeeks(
  entries: CalendarEntry[],
  todayUnix: number,
  maxWeeks: number,
): number[][] {
  const first = weekStart(todayUnix)
  // Extend to cover the last entry that ends in the future, capped so a single
  // year-out announcement cannot stretch the grid to 50 rows.
  const lastEnd = entries.reduce((m, e) => Math.max(m, e.endUnix), todayUnix)
  const needed = Math.ceil((lastEnd - first) / (7 * 86_400)) + 1
  const rows = Math.min(Math.max(needed, 2), maxWeeks)
  return Array.from({ length: rows }, (_, r) =>
    Array.from({ length: 7 }, (_, c) => first + (r * 7 + c) * 86_400),
  )
}

/** One operation's run clipped to one week row — the thing that becomes a bar. */
export interface LaneSegment {
  entry: CalendarEntry
  /** Uppercased, so callers do not each re-normalise for the chase set. */
  call: string
  /** Row within the week, 0 = topmost lane. */
  lane: number
  /** 0-based column (0 = Monday) where the bar begins in this week. */
  startCol: number
  /** Columns covered, ≥ 1. */
  span: number
  /** The run carries on past this week row's edge — square, flush edge + cue.
   * The negation is "this is the operation's real start/end": round that edge. */
  contPrev: boolean
  contNext: boolean
}

/** One week row's laid-out bars, plus whatever the lane cap pushed out of sight. */
export interface WeekLanes {
  weekStart: number
  days: number[]
  /** Bars to draw, lane-ordered. */
  segments: LaneSegment[]
  /** Segments the cap hid; empty when the week fits. */
  hidden: LaneSegment[]
  /** Per-column count of hidden segments — the "+N" chips, index 0 = Monday. */
  overflowByCol: number[]
  /** Lane rows the week needs, including the overflow row when there is one.
   * 0 for a week nothing is on the air in — that row stays short. */
  laneRows: number
}

/** Longest-running first among equal starts, then callsign — a total order, so a
 * re-render or a refetch can never reshuffle the lanes under the operator. */
function bySortKey(a: CalendarEntry, b: CalendarEntry): number {
  return (
    a.startUnix - b.startUnix ||
    b.endUnix - b.startUnix - (a.endUnix - a.startUnix) ||
    a.call.localeCompare(b.call)
  )
}

/**
 * Lay one week row out.
 *
 * Lanes are assigned greedily over a sort key that belongs to the ENTRY, not to
 * the week — that is what makes a lane stable across week rows without any
 * cross-week bookkeeping. A run continuing into the next week always has the
 * earliest start among that week's segments, so it sorts first there too and
 * keeps its low lane. Segments whose columns do not overlap share a lane, which
 * is correct packing (two short operations side by side), not a collision.
 *
 * `maxLanes` caps the row: when a week needs more, the last visible row is given
 * over to per-day "+N" chips instead of bars. Pass `Infinity` for an expanded week.
 */
export function layoutWeek(
  entries: CalendarEntry[],
  days: number[],
  maxLanes: number,
): WeekLanes {
  const wStart = days[0]
  const wEnd = wStart + 7 * 86_400
  const all: LaneSegment[] = []
  // `laneFreeFrom[l]` = first column in lane l not yet taken. Valid because the
  // sort key yields non-decreasing startCol within the week.
  const laneFreeFrom: number[] = []

  for (const e of entries.filter((x) => x.startUnix < wEnd && x.endUnix > wStart).sort(bySortKey)) {
    const runStart = Math.max(e.startUnix, wStart)
    // An operation ending exactly at a midnight boundary does not touch that day:
    // step back one second before resolving the end column.
    const runEnd = Math.min(e.endUnix, wEnd) - 1
    const startCol = Math.floor((dayStart(runStart) - wStart) / 86_400)
    const endCol = Math.floor((dayStart(runEnd) - wStart) / 86_400)
    if (endCol < startCol) continue // a zero-length run inside one day boundary
    let lane = 0
    while (lane < laneFreeFrom.length && laneFreeFrom[lane] > startCol) lane++
    laneFreeFrom[lane] = endCol + 1
    all.push({
      entry: e,
      call: e.call.toUpperCase(),
      lane,
      startCol,
      span: endCol - startCol + 1,
      contPrev: e.startUnix < wStart,
      contNext: e.endUnix > wEnd,
    })
  }

  const used = laneFreeFrom.length
  // Room for every lane → draw them all. Otherwise surrender the last visible
  // row to the "+N" chips, so the cap never silently swallows an operation.
  const visible = used <= maxLanes ? used : Math.max(maxLanes - 1, 0)
  const segments = all.filter((s) => s.lane < visible)
  const hidden = all.filter((s) => s.lane >= visible)
  const overflowByCol = Array.from({ length: 7 }, (_, c) =>
    hidden.reduce((n, s) => (c >= s.startCol && c < s.startCol + s.span ? n + 1 : n), 0),
  )
  return {
    weekStart: wStart,
    days,
    segments,
    hidden,
    overflowByCol,
    laneRows: hidden.length > 0 ? visible + 1 : visible,
  }
}

/** The whole grid: one laid-out row per week. `expanded` week-starts ignore the cap. */
export function layoutCalendar(
  entries: CalendarEntry[],
  weeks: number[][],
  maxLanes: number,
  expanded?: ReadonlySet<number>,
): WeekLanes[] {
  return weeks.map((days) =>
    layoutWeek(entries, days, expanded?.has(days[0]) ? Infinity : maxLanes),
  )
}

/** Palette slots — see `--dxc-0..9` in styles.css (defined in BOTH themes). */
export const DXPED_COLOR_COUNT = 10

/**
 * A callsign's palette slot. FNV-1a, so it is stable across renders, months and
 * sessions and identical everywhere the operation appears. Deliberately NOT
 * sequential-and-collision-free: stability is what lets the operator learn "the
 * teal one is VK9CM", and sequential assignment would repaint every bar in the
 * grid whenever one operation ends.
 */
export function dxpedColorIndex(call: string): number {
  const s = call.toUpperCase()
  let h = 2166136261
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i)
    h = Math.imul(h, 16777619)
  }
  return (h >>> 0) % DXPED_COLOR_COUNT
}

/**
 * Bands for the bar face. DTO order is preserved on purpose — announcements list
 * low bands first, and "they are bringing 160 and 80" is the interesting fact
 * about a DXpedition, not that they will also be on 20m.
 */
export function compactBands(bands: string[], max = 3): string {
  if (bands.length === 0) return ''
  const head = bands.slice(0, max).join('·')
  return bands.length > max ? `${head}+${bands.length - max}` : head
}

/** Bands ≥ this many columns wide get chips on the bar face; narrower bars would
 * ellipsis the callsign away, and the callsign is the thing you are looking for. */
export const BANDS_MIN_SPAN = 3
