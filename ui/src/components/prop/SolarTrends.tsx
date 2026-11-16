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
// TODAY SO FAR. The file's days are finished UTC days, so its newest count is yesterday's. The
// station also reads NOAA's observed-SSN file, which carries a running count for today that NOAA
// revises during the day (`DailySolarIndices.today`). It gets a line of its own under the SSN
// line, value first: "SSN today 95 · 21:04Z (so far)", the fetch time in UTC as the space-weather
// chips write theirs. The value and time never shorten; only the trailing note gives way to an
// ellipsis, so the line is one line at any width and the caption above keeps its own words (a
// stuck file's warning keeps its date). It shows only while it is the current UTC day's and newer
// than the file's newest day (`todaySoFar`), and leaves the screen at 00:00 UTC on its own;
// otherwise the box shows yesterday's count alone, with no notice — a missing running count is
// ordinary just after 00:00 UTC.
//
// The dashboard bar shows the same count (components/DashboardBar.tsx): it reads the file through
// `useSolarIndices`, picks the day with `newest` and words its hover with `trendsCaption`, so the
// bar and this block cannot show two different counts or dates for one file.
import { useEffect, useState } from 'react'
import type { DailySolarIndex, DailySolarIndices, SsnToday } from '../../types'
import { SOLAR_INDICES, useFeed } from '../../features/connectFeeds'
import { t } from '../../i18n'

/** The index names — technical tokens, the same on every ham's screen (SpaceWxGauges' rule). */
const SFI = 'SFI'
const SSN = 'SSN'

/** A newest day this many days before today (UTC) means the file has stopped arriving. SWPC
 * issues it daily with yesterday as its newest row, and just after 00:00 UTC it can sit a day
 * further behind for a few hours; three days is the first age the ordinary schedule never shows. */
export const STALE_AFTER_DAYS = 3

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

/** Today's running count while it is still today's (the current UTC day) and newer than the
 * file's newest finished day; null otherwise, and the box shows yesterday's count alone. */
export function todaySoFar(ix: DailySolarIndices, nowMs = Date.now()): SsnToday | null {
  const today = Math.floor(nowMs / 1000 / DAY_S) * DAY_S
  const t0 = ix.today
  if (!t0 || t0.dayUnix !== today) return null
  const newestDay = ix.days.length > 0 ? ix.days[ix.days.length - 1].dayUnix : -Infinity
  return t0.dayUnix > newestDay ? t0 : null
}

/** `HH:MMZ`, as the space-weather chips beside this box write a time (FlareChip, CapsChips). */
function hmZ(unix: number): string {
  const d = new Date(unix * 1000)
  return `${String(d.getUTCHours()).padStart(2, '0')}:${String(d.getUTCMinutes()).padStart(2, '0')}Z`
}

/** The running count's words: the value and fetch time ("SSN today 95 · 21:04Z"), the note that
 * may shorten ("(so far)"), and the hover. */
export function todayWords(today: SsnToday): { text: string; note: string; title: string } {
  const time = hmZ(today.fetchedAt)
  return {
    text: t('connect.solar.today', { index: SSN, value: today.ssn.toFixed(0), time }),
    note: t('connect.solar.today.note'),
    title: t('connect.solar.today.title', { time }),
  }
}

/** NOAA's daily solar indices: undefined while still asking, null when there is nothing to show.
 * The window's one poll of the file (features/connectFeeds, on the server's one-hour cache), so
 * this box in Connect, in the dashboard rail and in the pop-out shares it with the dashboard bar.
 * A failed refresh keeps what is on screen: its dates already say how old it is. */
export function useSolarIndices(): DailySolarIndices | null | undefined {
  const held = useFeed(SOLAR_INDICES)
  return held.settled ? (held.value ?? null) : undefined
}

export function SolarTrends() {
  // undefined = still asking (nothing drawn yet); null = nothing to show.
  const ix = useSolarIndices()
  // Today's count leaves at 00:00 UTC even when no fetch or snapshot re-renders the box then.
  const shownDay = ix ? (todaySoFar(ix)?.dayUnix ?? null) : null
  const [, rerender] = useState(0)
  useEffect(() => {
    if (shownDay == null) return
    const id = window.setTimeout(() => rerender((n) => n + 1), (shownDay + DAY_S) * 1000 - Date.now())
    return () => window.clearTimeout(id)
  }, [shownDay])

  if (ix === undefined) return null
  if (!ix || ix.days.length === 0) return <p className="swx-trend-none">{t('connect.solar.unavailable')}</p>

  const days = ix.days
  const caption = trendsCaption(days)
  const today = todaySoFar(ix)
  const words = today ? todayWords(today) : null
  return (
    <div className="swx-trends">
      <div className={`swx-trend-head${caption.stale ? ' stale' : ''}`}>{caption.text}</div>
      <TrendRow index={SFI} days={days} pick={(d) => d.sfi} />
      <TrendRow index={SSN} days={days} pick={(d) => d.ssn} />
      {words && (
        <div className="swx-trend-today" title={words.title}>
          <span className="swx-trend-today-v">{words.text}</span>
          <span className="swx-trend-today-k">{words.note}</span>
        </div>
      )}
    </div>
  )
}
