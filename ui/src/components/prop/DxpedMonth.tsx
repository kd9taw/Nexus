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
// Deliberately QUIET. The old view carried saturated yellow/orange/red on every
// row, which "really dominates the whole screen" — colour was doing work that
// position and weight do better. Here the grid is neutral, TODAY is the one strong
// accent, and a bar only takes colour when the operator is chasing it.
import type { CalendarEntry } from '../../types'

const WEEKDAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun']

/** UTC midnight for a day offset from a given day-start. */
function dayStart(unix: number): number {
  const d = new Date(unix * 1000)
  return Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate()) / 1000
}

/** The Monday on or before `unix` — grids read Mon–Sun here to match the ham
 * contest week, and because weekend operations should not be split across rows. */
function weekStart(unix: number): number {
  const d = new Date(unix * 1000)
  const dow = (d.getUTCDay() + 6) % 7 // 0 = Monday
  return dayStart(unix) - dow * 86_400
}

/** Rows of 7 UTC-midnight day stamps covering every entry, from this week on. */
function buildWeeks(entries: CalendarEntry[], todayUnix: number, weeks: number): number[][] {
  const first = weekStart(todayUnix)
  // Extend to cover the last entry that ends in the future, capped so a single
  // year-out announcement cannot stretch the grid to 50 rows.
  const lastEnd = entries.reduce((m, e) => Math.max(m, e.endUnix), todayUnix)
  const needed = Math.ceil((lastEnd - first) / (7 * 86_400)) + 1
  const rows = Math.min(Math.max(needed, 2), weeks)
  return Array.from({ length: rows }, (_, r) =>
    Array.from({ length: 7 }, (_, c) => first + (r * 7 + c) * 86_400),
  )
}

/** Entries touching a given UTC day, longest-running first so the bars stack
 * predictably instead of reordering as the week advances. */
function entriesOn(entries: CalendarEntry[], day: number): CalendarEntry[] {
  const end = day + 86_400
  return entries
    .filter((e) => e.startUnix < end && e.endUnix > day)
    .sort((a, b) => b.endUnix - b.startUnix - (a.endUnix - a.startUnix))
}

export function DxpedMonth({
  entries,
  chasing,
  onSelect,
  nowUnix,
  maxWeeks = 10,
}: {
  entries: CalendarEntry[]
  chasing?: Set<string>
  /** Clicking a bar opens that operation — the list view scrolls to it. */
  onSelect?: (call: string) => void
  /** Injectable for tests; defaults to the wall clock. */
  nowUnix?: number
  maxWeeks?: number
}) {
  const now = nowUnix ?? Math.floor(Date.now() / 1000)
  const today = dayStart(now)
  if (entries.length === 0) return null
  const weeks = buildWeeks(entries, now, maxWeeks)

  return (
    <div className="dxm" aria-label="DXpedition month grid">
      <div className="dxm-head" role="row">
        {WEEKDAYS.map((w) => (
          <div key={w} className="dxm-wd" role="columnheader">
            {w}
          </div>
        ))}
      </div>
      {weeks.map((week) => (
        <div className="dxm-week" key={week[0]} role="row">
          {week.map((day) => {
            const isToday = day === today
            const past = day < today
            const d = new Date(day * 1000)
            const onAir = entriesOn(entries, day)
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
                className={`dxm-day${isToday ? ' today' : ''}${past ? ' past' : ''}`}
              >
                <div className="dxm-dom">
                  {label}
                  {isToday && <span className="dxm-todaytag">TODAY</span>}
                </div>
                {onAir.map((e) => {
                  const call = e.call.toUpperCase()
                  const isChased = chasing?.has(call) ?? false
                  // Bars are continuous across days: only the first day of a run
                  // carries the callsign, the rest are unlabelled continuations.
                  const startsHere = e.startUnix >= day && e.startUnix < day + 86_400
                  const endsHere = e.endUnix > day && e.endUnix <= day + 86_400
                  const edge = `${startsHere ? ' starts' : ''}${endsHere ? ' ends' : ''}`
                  return (
                    <button
                      key={`${call}-${e.startUnix}`}
                      type="button"
                      className={`dxm-bar${isChased ? ' chased' : ''}${edge}`}
                      onClick={() => onSelect?.(e.call)}
                      title={`${e.call} — ${e.entity}${e.best ? ` · ${e.best}` : ''}`}
                    >
                      {startsHere ? (
                        <span className="dxm-barcall">
                          {isChased && <span aria-hidden="true">★ </span>}
                          {e.call}
                        </span>
                      ) : (
                        <span className="dxm-barcont" aria-hidden="true" />
                      )}
                    </button>
                  )
                })}
              </div>
            )
          })}
        </div>
      ))}
    </div>
  )
}
