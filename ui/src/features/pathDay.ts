// A path over the day, as a strip draws it — pure, so every rule unit-tests without React.
//
// The data is the shack's (propagation::pathday): per band and UTC hour, each route's MODELLED
// circuit reliability and the route the model favours that hour (the same call the Path card makes
// for now), and per hour the Kp with its own source and the polar-cap crossings. Nothing here
// derives a route of its own: a cell shows the route the model named, or none ("either way", "both
// closed"). The strip can show the better route ("best"), or the short or the long path alone.
//
// What a cell means is fixed vocabulary, so a strip can never overclaim: a figure is the model's
// monthly median (never a measurement), a long-path figure is unvalidated (the surface says so),
// and a Kp is measured, forecast or the current value carried forward, each drawn differently.
import { bandRangeForLabel } from '../band'
import type { PathDay, PathDayCell, PathDayHour, RegionPathDay } from '../types'

/** Which route the strip colours by. */
export type DayMode = 'best' | 'short' | 'long'

/** The route marks — invariant glyphs, explained by the strip's legend. */
export const ROUTE_GLYPH = { short: '▸', long: '◂' } as const

/** A route counts as open from this modelled reliability (propagation::route OPEN_BCR). */
export const OPEN_PCT = 50

export interface CellView {
  /** The figure the cell is coloured by (0..100), or null where the mode has none (no long path). */
  pct: number | null
  /** The route that figure is for; null when the model names neither (either way, both closed). */
  route: 'short' | 'long' | null
  glyph: string
  /** The route shown crosses the polar cap at that hour's Kp. */
  polar: boolean
}

/** What one cell shows in `mode`. */
export function cellView(c: PathDayCell, h: PathDayHour | undefined, mode: DayMode): CellView {
  const view = (pct: number | null, route: 'short' | 'long' | null, polarRoute: 'short' | 'long' | null): CellView => ({
    pct,
    route,
    glyph: route ? ROUTE_GLYPH[route] : '',
    polar: polarRoute === 'long' ? h?.polarLong === true : polarRoute === 'short' ? h?.polarShort === true : false,
  })
  if (mode === 'short') return view(c.shortPct, 'short', 'short')
  if (mode === 'long') return c.longPct == null ? view(null, null, null) : view(c.longPct, 'long', 'long')
  if (c.longPct == null || c.verdict == null) return view(c.shortPct, 'short', 'short')
  if (c.verdict === 'short') return view(c.shortPct, 'short', 'short')
  if (c.verdict === 'long') return view(c.longPct, 'long', 'long')
  // Either way / both closed: the better figure, and no route claimed.
  const longer = c.longPct > c.shortPct
  return view(Math.max(c.shortPct, c.longPct), null, longer ? 'long' : 'short')
}

/** Per UTC hour, the row index of the highest band at or under the MUF of the route shown (the
 * MUF ceiling the strip draws), or null when the MUF is under every band. Rows run highest band
 * first. Best mode takes the higher of the two routes' MUF. */
export function ceilingRow(day: PathDay, mode: DayMode): (number | null)[] {
  const lo = day.bands.map((b) => bandRangeForLabel(b.band)?.lo ?? Infinity)
  return Array.from({ length: 24 }, (_, h) => {
    const s = day.mufShort[h] ?? 0
    const l = day.mufLong?.[h]
    const muf = mode === 'short' ? s : mode === 'long' ? l : Math.max(s, l ?? 0)
    if (muf == null) return null
    const i = lo.findIndex((f) => f <= muf)
    return i < 0 ? null : i
  })
}

export interface KpCellView {
  /** measured = solid, forecast = hollow, current = the current value carried forward (no outlook
   * sample covers the hour) — drawn faint and dashed, never as a forecast. */
  fill: PathDayHour['kpSource']
  tone: 'good' | 'warn' | 'bad'
}

/** How the Kp ribbon draws one hour (the Kp outlook pane's tones: storm from 5, active from 4). */
export function kpCell(h: PathDayHour): KpCellView {
  return { fill: h.kpSource, tone: h.kp >= 5 ? 'bad' : h.kp >= 4 ? 'warn' : 'good' }
}

export interface OpenRun {
  band: string
  /** UTC hours; `to` is the hour the run ends (exclusive), so 12..16 is 1200–1600Z. */
  from: number
  to: number
  /** The route shown through the run; null when the model names neither (either way). */
  route: 'short' | 'long' | null
}

/** A band's runs of open hours that keep one route (wrapping past midnight): where the route the
 * model favours changes, a new run starts, so "open all day" reads as the two halves it is. */
function openRuns(cells: PathDayCell[], hours: PathDayHour[], mode: DayMode): { from: number; len: number; route: OpenRun['route'] }[] {
  const v = cells.map((c, h) => cellView(c, hours[h], mode))
  const open = v.map((x) => x.pct != null && x.pct >= OPEN_PCT)
  const joins = (a: number, b: number) => open[a] && open[b] && v[a].route === v[b].route
  if (open.every(Boolean) && v.every((x) => x.route === v[0].route)) return [{ from: 0, len: 24, route: v[0].route }]
  const runs: { from: number; len: number; route: OpenRun['route'] }[] = []
  for (let h = 0; h < 24; h++) {
    // A run starts at an open hour that does not continue the previous hour's run.
    if (!open[h] || joins((h + 23) % 24, h)) continue
    let len = 1
    while (len < 24 && joins((h + len - 1) % 24, (h + len) % 24)) len++
    runs.push({ from: h, len, route: v[h].route })
  }
  return runs
}

/** The day in one line: the longest open runs (two by default), each with its band, its hours
 * and the route the model favours through it (null: either way, or no route named). */
export function daySummary(day: PathDay, mode: DayMode, max = 2): OpenRun[] {
  return day.bands
    .flatMap((b, i) => openRuns(b.cells, day.hours, mode).map((r) => ({ b, i, r })))
    .sort((a, b) => b.r.len - a.r.len || a.i - b.i || a.r.from - b.r.from)
    .slice(0, max)
    .map(({ b, r }) => ({ band: b.band, from: r.from, to: (r.from + r.len) % 24, route: r.route }))
}

/** The band a collapsed region row shows: open the most hours by the mode shown, then the
 * highest day total. Null for a day with no bands. */
export function bestBand(day: PathDay, mode: DayMode): PathDay['bands'][number] | null {
  let best: { b: PathDay['bands'][number]; open: number; sum: number } | null = null
  for (const b of day.bands) {
    const v = b.cells.map((c, h) => cellView(c, day.hours[h], mode).pct ?? 0)
    const open = v.filter((p) => p >= OPEN_PCT).length
    const sum = v.reduce((a, p) => a + p, 0)
    if (!best || open > best.open || (open === best.open && sum > best.sum)) best = { b, open, sum }
  }
  return best?.b ?? null
}

/** The regions in the order the operator cares about: most needed stations heard first, then the
 * most logged contacts, then the fixed order (so an empty log reads in the default order). Both
 * are counted by CQ zone, which the needs and the log statistics both carry. */
export function orderRegions(
  regions: readonly RegionPathDay[],
  needs: readonly { zone: number }[],
  logged: readonly { zone: number; qsos: number }[],
): RegionPathDay[] {
  const count = (r: RegionPathDay) => ({
    need: needs.filter((n) => r.zones.includes(n.zone)).length,
    qsos: logged.reduce((a, z) => a + (r.zones.includes(z.zone) ? z.qsos : 0), 0),
  })
  return regions
    .map((r, i) => ({ r, i, ...count(r) }))
    .sort((a, b) => b.need - a.need || b.qsos - a.qsos || a.i - b.i)
    .map((x) => x.r)
}
