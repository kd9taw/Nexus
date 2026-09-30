// The Clock box — big UTC and local time, the date, your grid, and today's sunrise and sunset.
//
// Pure clock + geometry, no feed: it keeps time when every feed is down, and reads the same in
// the pop-out and on the TV page as in the main window. UTC is the station clock (logs, spots and
// FT slots run on it), so it is the biggest thing here. Local time is this computer's own zone —
// on a TV or a browser that is the viewer's, which is what a clock across the room should say.
//
// ⚠️ THE DIGITS ARE SVG TEXT, SIZED BY THE BOX. A clock set in a CSS font size either fits the
// narrowest slot (a 200 px rail) and looks small everywhere else, or overflows that slot at the
// Larger text size. Drawn in an SVG whose viewBox holds the eight characters, the digits follow the
// pane's width, and a height cap in styles.css that rides --text-scale stops them growing past what
// Text size asks for in a wide slot. `textLength` holds the eight characters to the viewBox,
// whatever the platform's monospace face measures.
//
// Sunrise and sunset are TODAY's at the grid in Settings (`sunDay`: the Greyline pane's and the
// map's own geometry), printed in UTC like every time in Connect, with this computer's times on
// hover. With no grid the box says so instead of guessing a place.
import { useEffect, useState } from 'react'
import { gridToLatLon } from '../../grid'
import { sunDay, type SunDay } from '../../mapGeo'
import { t } from '../../i18n'

/** The time standard's name — a token, the same in every language (the top bar's own rule). */
const UTC = 'UTC'

const two = (n: number) => String(n).padStart(2, '0')
/** HH:MM:SS in ASCII digits: a clock reading is an invariant token, never locale-formatted. */
const hms = (h: number, m: number, s: number) => `${two(h)}:${two(m)}:${two(s)}`
/** "11:52Z" — a terminator as the Greyline pane prints one. */
function hhmmZ(ms: number): string {
  const d = new Date(ms)
  return `${two(d.getUTCHours())}:${two(d.getUTCMinutes())}Z`
}
/** "06:52" on this computer's clock. */
function hhmmLocal(ms: number): string {
  const d = new Date(ms)
  return `${two(d.getHours())}:${two(d.getMinutes())}`
}

/** This computer's zone as the platform abbreviates it ("CDT", "GMT+2"), or '' where it gives
 * none. Read per render: the zone can change under a running app (travel, a DST switch). */
function zoneName(d: Date): string {
  try {
    const parts = new Intl.DateTimeFormat(undefined, { timeZoneName: 'short' }).formatToParts(d)
    return parts.find((p) => p.type === 'timeZoneName')?.value ?? ''
  } catch {
    return ''
  }
}

/** The time, re-read on each new second and aligned to the second boundary — a free-running
 * one-second interval lags the wall clock by up to a second, which a clock beside an FT8 slot
 * must not do. */
function useSecond(): number {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    let id = 0
    const tick = () => {
      const at = Date.now()
      setNow(at)
      id = window.setTimeout(tick, 1000 - (at % 1000))
    }
    id = window.setTimeout(tick, 1000 - (Date.now() % 1000))
    return () => window.clearTimeout(id)
  }, [])
  return now
}

function Digits({ text, label }: { text: string; label: string }) {
  return (
    <svg className="clock-digits" viewBox="0 0 480 80" preserveAspectRatio="xMinYMid meet" role="img" aria-label={label}>
      <text x="0" y="76" fontSize="100" textLength="480" lengthAdjust="spacingAndGlyphs">
        {text}
      </text>
    </svg>
  )
}

/** The sunrise/sunset line, and its hover text in this computer's times. */
function sunLine(grid: string, sun: SunDay | null): { text: string; title?: string } {
  if (!sun) return { text: t('connect.clock.noGrid') }
  if (sun.polar === 'up') return { text: `${grid} · ${t('connect.clock.sun.up')}` }
  if (sun.polar === 'down') return { text: `${grid} · ${t('connect.clock.sun.down')}` }
  const none = '—'
  return {
    text: t('connect.clock.sun', {
      grid,
      rise: sun.riseMs == null ? none : hhmmZ(sun.riseMs),
      set: sun.setMs == null ? none : hhmmZ(sun.setMs),
    }),
    title: t('connect.clock.sun.local', {
      rise: sun.riseMs == null ? none : hhmmLocal(sun.riseMs),
      set: sun.setMs == null ? none : hhmmLocal(sun.setMs),
    }),
  }
}

export function ClockPane({ myGrid }: { myGrid: string }) {
  const now = useSecond()
  const d = new Date(now)
  const utc = hms(d.getUTCHours(), d.getUTCMinutes(), d.getUTCSeconds())
  const local = hms(d.getHours(), d.getMinutes(), d.getSeconds())
  const zone = zoneName(d)
  const grid = myGrid.trim()
  const here = grid ? gridToLatLon(grid) : null
  const sun = sunLine(grid, here ? sunDay(here.lat, here.lon, now) : null)
  return (
    <div className="clock-pane">
      <div className="clock-utc">
        <div className="clock-line">
          <span className="clock-label">{UTC}</span>
          <span className="clock-date">
            {d.toLocaleDateString(undefined, {
              weekday: 'short',
              day: 'numeric',
              month: 'short',
              year: 'numeric',
              timeZone: 'UTC',
            })}
          </span>
        </div>
        <Digits text={utc} label={t('connect.clock.utc.aria', { time: utc })} />
      </div>
      <div className="clock-local">
        <div className="clock-line">
          <span className="clock-label">
            {zone ? `${t('topbar.localClock.label')} · ${zone}` : t('topbar.localClock.label')}
          </span>
          <span className="clock-date">
            {d.toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' })}
          </span>
        </div>
        <Digits text={local} label={t('connect.clock.local.aria', { time: local })} />
      </div>
      <p className="clock-sun" title={sun.title}>
        {sun.text}
      </p>
    </div>
  )
}
