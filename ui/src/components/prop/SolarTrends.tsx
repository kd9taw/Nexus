// The Space Wx box's 30-day trends — NOAA SWPC's daily solar indices: the solar flux and the
// sunspot number over the last thirty days as lines, each with its newest value and that day's
// date. The sunspot number is the box's only SSN.
//
// ⚠️ STALE-HONEST, AND THE FILE CARRIES WHAT MAKES IT SO. Every row is dated, so however long the
// fetch has been failing (the command then serves its last good copy), this block can say how
// old its newest day is. It shows exactly one of three things:
//   • the file is current (its newest day is under STALE_AFTER_DAYS old — normally yesterday):
//     the lines, each index's newest value with its date, and the date range they cover;
//   • the file has stopped arriving (the newest day is STALE_AFTER_DAYS or more old): the same,
//     with the caption saying since when, in the warning colour;
//   • there is no file (never fetched, or a surface that cannot reach it): one line saying so and
//     no chart — an empty or flat chart would read as a quiet Sun.
// A day NOAA published no value for is a GAP in its line, never a point at zero; a newest day
// with no value shows the last day that has one, dated as THAT day.
//
// The sunspot number here is SWPC's DAILY count. The propagation model's sunspot input is the
// smoothed R12, a different quantity, and nothing here reaches it.
//
// The dashboard bar shows the same count (components/DashboardBar.tsx): it reads the file through
// `useSolarIndices`, picks the day with `newest` and words its hover with `trendsCaption`, so the
// bar and this block cannot show two different counts or dates for one file.
import { useEffect, useState } from 'react'
import { getSolarIndices } from '../../api'
import type { DailySolarIndex, DailySolarIndices } from '../../types'
import { t } from '../../i18n'

/** The index names — technical tokens, the same on every ham's screen (SpaceWxGauges' rule). */
const SFI = 'SFI'
const SSN = 'SSN'

/** A newest day this many days before today (UTC) means the file has stopped arriving. SWPC
 * issues it daily with yesterday as its newest row, and just after 00:00 UTC it can sit a day
 * further behind for a few hours; three days is the first age the ordinary schedule never shows. */
export const STALE_AFTER_DAYS = 3

/** Matches the server's one-hour cache; the file changes about once a day. */
const POLL_MS = 3_600_000
const DAY_S = 86_400

/** "Sep 28" — one day of the file. The rows are UTC days, so the label is too. Date formatting
 * at the display edge, as DxpedMonth does: never persisted, never parsed back. */
export function dayLabel(dayUnix: number): string {
  return new Date(dayUnix * 1000).toLocaleDateString(undefined, {
    day: 'numeric',
    month: 'short',
    timeZone: 'UTC',
  })
}

type Pick = (d: DailySolarIndex) => number | null

/** The line over a 100 × 100 box that the SVG stretches to its row: one polyline per run of days
 * with a value, so a missing day breaks the line instead of dragging it to the floor. */
function line(days: DailySolarIndex[], pick: Pick): { runs: string[]; low: number; high: number } | null {
  const vals = days.map(pick)
  const have = vals.filter((v): v is number => v != null)
  if (have.length === 0) return null
  const low = Math.min(...have)
  const high = Math.max(...have)
  const x = (i: number) => (days.length === 1 ? 50 : (i / (days.length - 1)) * 100)
  const y = (v: number) => (high === low ? 50 : 100 - ((v - low) / (high - low)) * 100)
  const runs: string[] = []
  let run: string[] = []
  vals.forEach((v, i) => {
    if (v == null) {
      if (run.length) runs.push(run.join(' '))
      run = []
    } else run.push(`${x(i).toFixed(2)},${y(v).toFixed(2)}`)
  })
  if (run.length) runs.push(run.join(' '))
  return { runs, low, high }
}

/** The newest day that has a value, and that value. */
export function newest(days: DailySolarIndex[], pick: Pick): { value: number; dayUnix: number } | null {
  for (let i = days.length - 1; i >= 0; i--) {
    const value = pick(days[i])
    if (value != null) return { value, dayUnix: days[i].dayUnix }
  }
  return null
}

function TrendRow({ index, days, pick }: { index: string; days: DailySolarIndex[]; pick: Pick }) {
  const drawn = line(days, pick)
  const last = newest(days, pick)
  if (!drawn || !last) return null
  const label = t('connect.solar.trend.aria', {
    index,
    from: dayLabel(days[0].dayUnix),
    to: dayLabel(days[days.length - 1].dayUnix),
    low: drawn.low.toFixed(0),
    high: drawn.high.toFixed(0),
  })
  return (
    <div className="swx-trend" data-index={index}>
      <span className="swx-trend-k">{index}</span>
      <svg className="swx-spark" viewBox="0 0 100 100" preserveAspectRatio="none" role="img" aria-label={label}>
        <title>{label}</title>
        {drawn.runs.map((points, i) => (
          <polyline key={i} points={points} vectorEffect="non-scaling-stroke" />
        ))}
      </svg>
      <span className="swx-trend-v">{last.value.toFixed(0)}</span>
      <span className="swx-trend-d">{dayLabel(last.dayUnix)}</span>
    </div>
  )
}

/** The caption over a file's lines (`days` not empty): the dates it covers, or, once its newest
 * day is STALE_AFTER_DAYS old, since when it has not been updated. */
export function trendsCaption(days: DailySolarIndex[]): { text: string; stale: boolean } {
  const newestDay = days[days.length - 1].dayUnix
  const today = Math.floor(Date.now() / 1000 / DAY_S) * DAY_S
  const stale = (today - newestDay) / DAY_S >= STALE_AFTER_DAYS
  const text = stale
    ? t('connect.solar.stale', { date: dayLabel(newestDay) })
    : t('connect.solar.caption', { from: dayLabel(days[0].dayUnix), to: dayLabel(newestDay) })
  return { text, stale }
}

/** NOAA's daily solar indices, asked for on the server's cadence: undefined while still asking,
 * null when there is nothing to show. A failed refresh keeps what is on screen: its dates already
 * say how old it is. */
export function useSolarIndices(): DailySolarIndices | null | undefined {
  const [ix, setIx] = useState<DailySolarIndices | null | undefined>(undefined)
  useEffect(() => {
    let live = true
    const load = () =>
      getSolarIndices()
        .then((v) => live && setIx(v))
        .catch(() => live && setIx((cur) => cur ?? null))
    load()
    const id = window.setInterval(load, POLL_MS)
    return () => {
      live = false
      window.clearInterval(id)
    }
  }, [])
  return ix
}

export function SolarTrends() {
  // undefined = still asking (nothing drawn yet); null = nothing to show.
  const ix = useSolarIndices()

  if (ix === undefined) return null
  if (!ix || ix.days.length === 0) return <p className="swx-trend-none">{t('connect.solar.unavailable')}</p>

  const days = ix.days
  const caption = trendsCaption(days)
  return (
    <div className="swx-trends">
      <div className={`swx-trend-head${caption.stale ? ' stale' : ''}`}>{caption.text}</div>
      <TrendRow index={SFI} days={days} pick={(d) => d.sfi} />
      <TrendRow index={SSN} days={days} pick={(d) => d.ssn} />
    </div>
  )
}
