// The DXpedition MONTH GRID — a traditional calendar with the announced
// operations laid across it as bars, so "when is this on, and what else overlaps
// it" is answered by looking rather than by reading a list of countdowns.
//
// Why this exists (operator, 2026-07-29): the forward list gives you one entry at
// a time and a T-minus number. It cannot show that two entities are on the air the
// same weekend, or that the one you want starts the day after you travel. A grid
// can, and it is the format every other calendar in the world already taught the
// operator to read.
//
// SPAN BARS (operator, 2026-07-29, second pass): the first cut stacked one chip
// per day INSIDE each day cell, which never read as a continuous operation — the
// cell padding broke the run visually, the per-day sort let a bar change lanes
// mid-run, and continuation days were anonymous grey strips. Now the WEEK is the
// grid: day cells span its full height as the background, and one bar element per
// week row is placed `grid-column: start / span n`. A spanning grid item covers
// the gutters between the tracks it spans, so the bar is genuinely continuous and
// paints over the intermediate cell borders. Geometry, lanes and colour live in
// dxpedLanes.ts (pure, tested) — this file only renders them.
//
// Still deliberately QUIET. The old view carried saturated yellow/orange/red on
// every row, which "really dominates the whole screen". Colour here is IDENTITY,
// not severity: a solid spine plus a low tint, text stays neutral. TODAY is still
// the one strong accent, and a chased operation still gets the loud treatment.
import { useState } from 'react'
import type { CalendarEntry } from '../../types'
import {
  BANDS_MIN_SPAN,
  buildWeeks,
  compactBands,
  dayStart,
  dxpedColorIndex,
  layoutCalendar,
} from './dxpedLanes'
import { dxpedLink, dxpedLinkTitle } from './dxpedLink'
import { azimuthLabel, backendAzimuth } from '../../grid'
import { t } from '../../i18n'

// Weekday and month names are DATE FORMATTING, not catalog prose — they stay with the rest
// of the date handling in this file (`toLocaleDateString` below) rather than becoming keys.
const WEEKDAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun']

const DATE_FMT: Intl.DateTimeFormatOptions = { day: 'numeric', month: 'short', timeZone: 'UTC' }

/** The announced run, inclusive — `endUnix` is exclusive, so the last day on the
 * air is the second before it. Printing the exclusive end claims a day too many. */
function runDates(e: CalendarEntry): string {
  const from = new Date(e.startUnix * 1000).toLocaleDateString(undefined, DATE_FMT)
  const to = new Date((e.endUnix - 1) * 1000).toLocaleDateString(undefined, DATE_FMT)
  return from === to ? from : `${from} – ${to}`
}

/** Everything the bar cannot fit: the full band list, the modes, the headline. */
function barDetail(e: CalendarEntry): string {
  // The heading rides with the entity name here too. A month bar is the one place
  // the entity appears with no room beside it, so the detail line is where it goes —
  // leaving it out would make this the one dxped view that never states a beam.
  const az = backendAzimuth(e.bearingDeg, e.distanceKm)
  const parts = [`${e.call} — ${e.entity}${az ? ` ${azimuthLabel(az)}` : ''}`, runDates(e)]
  if (e.bands.length > 0) parts.push(e.bands.join(' '))
  if (e.modes.length > 0) parts.push(e.modes.join('/'))
  if (e.best) parts.push(e.best)
  return parts.join(' · ')
}

export function DxpedMonth({
  entries,
  chasing,
  onSelect,
  onOpenPage,
  nowUnix,
  maxWeeks = 10,
  maxLanes = 4,
}: {
  entries: CalendarEntry[]
  chasing?: Set<string>
  /** Clicking a bar selects that operation — the list view scrolls to it. */
  onSelect?: (call: string) => void
  /** …and opens its webpage, which is what the operator asked a bar click to do.
   * BOTH fire: the browser gets the page, and the Details rail is already on that
   * operation when they come back, so the click costs nothing it used to do. */
  onOpenPage?: (entry: CalendarEntry) => void
  /** Injectable for tests; defaults to the wall clock. */
  nowUnix?: number
  maxWeeks?: number
  /** Lane rows a week may spend before the rest collapse into "+N" chips. */
  maxLanes?: number
}) {
  // Weeks the operator opened past the lane cap. Keyed by week-start so the set
  // survives the grid sliding forward a row at UTC midnight.
  const [expanded, setExpanded] = useState<ReadonlySet<number>>(() => new Set<number>())
  const now = nowUnix ?? Math.floor(Date.now() / 1000)
  const today = dayStart(now)
  if (entries.length === 0) return null
  const rows = layoutCalendar(entries, buildWeeks(entries, now, maxWeeks), maxLanes, expanded)

  const toggleWeek = (weekStart: number) =>
    setExpanded((prev) => {
      const next = new Set(prev)
      if (!next.delete(weekStart)) next.add(weekStart)
      return next
    })

  return (
    <div className="dxm" aria-label={t('dxped.month.aria')}>
      <div className="dxm-head" role="row">
        {WEEKDAYS.map((w) => (
          <div key={w} className="dxm-wd" role="columnheader">
            {w}
          </div>
        ))}
      </div>
      {rows.map((row) => {
        const isOpen = expanded.has(row.weekStart)
        // An opened week earns one extra row for the control that closes it again.
        const extraRow = isOpen && row.laneRows > maxLanes
        const trackCount = row.laneRows + (extraRow ? 1 : 0)
        // Day number, one track per lane, then a spacer that eats the slack. The
        // spacer is load-bearing: the day cell's min-height has to come from
        // SOMEWHERE, and without a trailing 1fr it stretches the auto header row
        // instead, leaving a quiet week's single bar floating near the bottom of
        // the cell instead of sitting under the date.
        const lanes = trackCount > 0 ? `repeat(${trackCount}, var(--dxm-lane)) ` : ''
        return (
          <div
            className="dxm-week"
            key={row.weekStart}
            role="row"
            style={{ gridTemplateRows: `auto ${lanes}1fr` }}
          >
            {row.days.map((day, col) => {
              const isToday = day === today
              const d = new Date(day * 1000)
              // First of the month earns its name, so a grid spanning a boundary
              // does not silently roll over.
              const dom = d.getUTCDate()
              const label =
                dom === 1
                  ? `${d.toLocaleString(undefined, { month: 'short', timeZone: 'UTC' })} 1`
                  : String(dom)
              return (
                <div
                  key={day}
                  role="gridcell"
                  className={`dxm-day${isToday ? ' today' : ''}${day < today ? ' past' : ''}`}
                  style={{ gridColumn: col + 1 }}
                >
                  <div className="dxm-dom">
                    {label}
                    {isToday && <span className="dxm-todaytag">{t('dxped.month.today')}</span>}
                  </div>
                </div>
              )
            })}
            {row.segments.map((seg) => {
              const e = seg.entry
              const isChased = chasing?.has(seg.call) ?? false
              // Bands only where the bar is wide enough to hold them without
              // ellipsing the callsign away — the callsign is what you scan for.
              const bands = seg.span >= BANDS_MIN_SPAN ? compactBands(e.bands) : ''
              const edge = `${seg.contPrev ? ' cont-prev' : ''}${seg.contNext ? ' cont-next' : ''}`
              const link = dxpedLink(e)
              return (
                <button
                  key={`${seg.call}-${e.startUnix}`}
                  type="button"
                  className={`dxm-bar${isChased ? ' chased' : ''}${edge}`}
                  data-dxc={dxpedColorIndex(seg.call)}
                  style={{
                    gridColumn: `${seg.startCol + 1} / span ${seg.span}`,
                    gridRow: seg.lane + 2,
                  }}
                  onClick={() => {
                    onSelect?.(e.call)
                    onOpenPage?.(e)
                  }}
                  title={`${barDetail(e)}${link ? `\n${dxpedLinkTitle(link)}` : ''}`}
                  aria-label={`${barDetail(e)}${isChased ? t('dxped.month.bar.chasing') : ''}${
                    link ? t('dxped.month.bar.opens', { url: link.url }) : ''
                  }`}
                >
                  <span className="dxm-barcall">
                    {isChased && <span aria-hidden="true">★ </span>}
                    {e.call}
                  </span>
                  {bands && (
                    <span className="dxm-barbands" aria-hidden="true">
                      {bands}
                    </span>
                  )}
                  {seg.contNext && (
                    <span className="dxm-barmore" aria-hidden="true">
                      ›
                    </span>
                  )}
                </button>
              )
            })}
            {row.overflowByCol.map((n, col) =>
              n > 0 ? (
                <button
                  key={`more-${col}`}
                  type="button"
                  className="dxm-more"
                  style={{ gridColumn: col + 1, gridRow: row.laneRows + 1 }}
                  onClick={() => toggleWeek(row.weekStart)}
                  title={t('dxped.month.more.title', { count: n })}
                >
                  +{n}
                </button>
              ) : null,
            )}
            {extraRow && (
              <button
                type="button"
                className="dxm-more dxm-less"
                style={{ gridColumn: '1 / -1', gridRow: trackCount + 1 }}
                onClick={() => toggleWeek(row.weekStart)}
              >
                {t('dxped.month.showFewer')}
              </button>
            )}
          </div>
        )
      })}
    </div>
  )
}
