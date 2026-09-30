// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The index names (SFI,
// Kp, A, X-ray, SW) and `UTC` are technical tokens, named below as constants; every word of prose
// comes from the catalog.
//
// THE DASHBOARD BAR — the line across the top of the Connect pop-out (the dashboard window) and
// the TV page: the station, a big UTC clock beside the local time, and the day's space-weather
// indices, in the order a wall-display clock reads them (call, clocks, SFI / K / SSN / A / solar
// wind). The look is Nexus's own tokens, and the data is the propagation snapshot the surface
// already polls, with one exception below.
//
// TWO SEAMS, on purpose:
//   · `clock` — a clock to draw instead of `DashClock`, today's UTC + local readout. Connect's
//     Clock box is a whole box with no one-line form, so `DashClock` stays the bar's clock.
//   · `barIndices` — the list the indices come from. SSN sits between Kp and A, from NOAA's
//     daily solar indices, which are their own fetch, not a snapshot field (the snapshot's
//     sunspot input is the model's smoothed R12, a different quantity, and never reaches the UI).
//     The bar asks for that file itself, through the Space Wx box's own `useSolarIndices`
//     (cached an hour by the command), and shows the newest day that has a count, which is
//     normally yesterday's, as that day's. The solar-wind speed reads the Wind gauge's own
//     no-data rule (propViz `windSpeedKms`). DashboardBar.seams.test.tsx holds the bar and the
//     box to the same numbers.
//
// OFFLINE HONESTY, Connect's rule (connect/panes.tsx): an offline snapshot is non-null and
// carries MODELLED values (SFI 120 …), so the bar draws a dash for every index then and says NO
// LIVE DATA. A cached or partial one keeps its last real numbers and says so, in the panes' words
// on a chip of the bar's own (styles.css `.dash-prov`: the panes' chip letters its warning in a
// colour that reads under 4.5:1 on this bar in the light theme).
import { useEffect, useState, type ReactNode } from 'react'
import type { DailySolarIndices, PropagationSnapshot, SpaceWxView } from '../types'
import { aImpact, kpImpact, sfiImpact, windSpeedKms, xrayImpact } from '../propViz'
import { provLabel } from './connect/paneFormat'
import { dayLabel, newest, trendsCaption, useSolarIndices } from './prop/SolarTrends'
import { getWindowBehind, setWindowBehind, type WindowBehind } from '../api'
import { withErrorToast } from '../toast'
import { t } from '../i18n'

/** Tokens, not prose: the same on every ham's screen in every language. */
const UTC = 'UTC'
const NAME = { sfi: 'SFI', kp: 'Kp', ssn: 'SSN', a: 'A', xray: 'X-ray', sw: 'SW' } as const
/** An index with nothing honest to show. */
const DASH = '—'

/** One index on the bar: its token name, the value drawn, and what it means for HF (hover). */
export interface BarIndex {
  key: string
  value: string
  /** The day a daily count is from (SSN), drawn after its value; absent for the snapshot's own. */
  day?: string
  title: string
}

/**
 * The indices from a live or cached snapshot, rounded exactly as the Space Wx gauges round them
 * (whole numbers, the X-ray class without "-class"), so the bar and the box never disagree. The
 * hover words are the gauges' own impact lines. Order: SFI, K, SSN, A, as a wall clock reads
 * them, then X-ray and the solar-wind speed, which is a dash while DSCOVR's plasma feed is out.
 * SSN is the newest day of NOAA's daily file that has a count (`daily`), dated as that day and
 * hovered with the Space Wx box's own caption for the file; a dash while there is none.
 */
export function barIndices(wx: SpaceWxView, daily?: DailySolarIndices | null): BarIndex[] {
  const ssn = daily && daily.days.length > 0 ? newest(daily.days, (d) => d.ssn) : null
  const windKms = windSpeedKms(wx, Date.now())
  return [
    { key: NAME.sfi, value: wx.sfi.toFixed(0), title: sfiImpact(wx.sfi).text },
    { key: NAME.kp, value: wx.kp.toFixed(0), title: kpImpact(wx.kp).text },
    daily && ssn
      ? { key: NAME.ssn, value: ssn.value.toFixed(0), day: dayLabel(ssn.dayUnix), title: trendsCaption(daily.days).text }
      : { key: NAME.ssn, value: DASH, title: '' },
    { key: NAME.a, value: wx.aIndex.toFixed(0), title: aImpact(wx.aIndex).text },
    { key: NAME.xray, value: wx.xrayClass.replace('-class', ''), title: xrayImpact(wx.xrayClass).text },
    { key: NAME.sw, value: windKms != null ? windKms.toFixed(0) : DASH, title: t('dash.index.sw.title') },
  ]
}

/** The same names with nothing claimed: waiting for the first snapshot, or no live data. */
const NO_INDICES: BarIndex[] = Object.values(NAME).map((key) => ({ key, value: DASH, title: '' }))

const two = (n: number) => String(n).padStart(2, '0')
const hms = (h: number, m: number, s: number) => `${two(h)}:${two(m)}:${two(s)}`

/**
 * Today's clock, the seam's default: UTC, big, and this computer's local time, both to the
 * second — the TopBar clock's two readings side by side. It ticks in its own component, so the
 * second re-renders the clock and not the bar around it.
 */
export function DashClock() {
  const [now, setNow] = useState(() => new Date())
  useEffect(() => {
    const id = window.setInterval(() => setNow(new Date()), 1000)
    return () => window.clearInterval(id)
  }, [])
  return (
    <span className="dash-clock">
      <span className="dash-time dash-utc" title={t('topbar.utc.title')}>
        <span className="dash-time-v">{hms(now.getUTCHours(), now.getUTCMinutes(), now.getUTCSeconds())}</span>
        <span className="dash-time-k">{UTC}</span>
      </span>
      <span className="dash-time dash-local" title={t('topbar.localClock.title')}>
        <span className="dash-time-v">{hms(now.getHours(), now.getMinutes(), now.getSeconds())}</span>
        <span className="dash-time-k">{t('topbar.localClock.label')}</span>
      </span>
    </span>
  )
}

interface Props {
  /** The station's call and grid, as the surface knows them ('' while it does not). */
  call: string
  grid: string
  /** The propagation snapshot the surface already polls; null until the first one lands. */
  prop: PropagationSnapshot | null
  /** THE CLOCK SEAM: a clock to draw instead of `DashClock` (Connect's clock box). */
  clock?: ReactNode
  /** The surface's own controls and chips, drawn last (the TV's read-only and link chips, the
   *  pop-out's Stay behind toggle). */
  children?: ReactNode
}

export function DashboardBar({ call, grid, prop, clock, children }: Props) {
  const daily = useSolarIndices()
  // `spaceWx` is read defensively: the TV page's LAN transport hands over whatever the station
  // serves, and a missing block is "no live data", never a throw that blanks the wall.
  const wx = prop?.spaceWx
  const indices = prop && wx && prop.source !== 'offline' ? barIndices(wx, daily) : NO_INDICES
  // Live is the normal state and gets no chip; anything else says what it is.
  const prov = prop && prop.source !== 'live' ? provLabel(prop.source, prop.asOf) : null
  return (
    <header className="dash-bar" aria-label={t('dash.bar.aria')}>
      <span className="dash-station">
        <span className="dash-call">{call || DASH}</span>
        {grid && <span className="dash-grid">{grid}</span>}
      </span>
      {clock ?? <DashClock />}
      <ul className="dash-indices" aria-label={t('prop.spaceWx.aria')}>
        {indices.map((i) => (
          <li key={i.key} className="dash-index" title={i.title || undefined}>
            <span className="dash-index-k">{i.key}</span>
            <span className="dash-index-v">{i.value}</span>
            {i.day && <span className="dash-index-d">{i.day}</span>}
          </li>
        ))}
      </ul>
      {prov && (
        <span className="dash-prov" data-prov={prov.cls} title={t('connect.prov.title')}>
          {prov.label}
        </span>
      )}
      {children != null && <span className="dash-end">{children}</span>}
    </header>
  )
}

/**
 * The pop-out's "Stay behind" toggle: keep this window behind the others even when it is
 * clicked, so it can fill the screen behind Nexus as a wall display does. Drawn only
 * where the shell offers it for this window (`get_window_behind` — Windows, the Connect window),
 * and it shows what the window really is: a refused change leaves it as it was.
 */
export function StayBehindToggle() {
  const [state, setState] = useState<WindowBehind | null>(null)
  useEffect(() => {
    let live = true
    getWindowBehind()
      .then((s) => live && setState(s))
      .catch(() => {})
    return () => {
      live = false
    }
  }, [])
  if (!state?.supported) return null
  const toggle = () => {
    void withErrorToast(() => setWindowBehind(!state.on), t('dash.behind.failed')).then((s) => {
      if (s) setState(s)
    })
  }
  return (
    <button
      type="button"
      className={`dash-behind${state.on ? ' active' : ''}`}
      aria-pressed={state.on}
      title={t('dash.behind.title')}
      onClick={toggle}
    >
      {t('dash.behind.label')}
    </button>
  )
}
