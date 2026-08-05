// The forward DXpedition planning calendar: each announced operation with its
// dates/bands/modes, best-band headline, and the band×UTC-hour likelihood
// heatmap so the operator can plan when to chase it. When the P.533 windows
// command has data for a call, its headline + grid replace the heuristic ones
// (badged), and the ★ lets the operator chase before the expedition even starts.
import { useState } from 'react'
import type { CalendarEntry, DxpedDayBest, DxpedWindow } from '../../types'
import { LikelihoodHeatmap } from './LikelihoodHeatmap'
import { DxpedMonth } from './DxpedMonth'
import { DxpedDigest } from './DxpedDigest'
import { dxpedColorIndex } from './dxpedLanes'
import { dxpedLink, dxpedLinkTitle } from './dxpedLink'
import { openDxpedPage } from '../../api'
import { withErrorToast } from '../../toast'

function daysUntil(startUnix: number): string {
  const d = Math.round((startUnix - Date.now() / 1000) / 86400)
  return d <= 0 ? 'on the air' : `T-${d}d`
}

const WEEKDAY = ['Su', 'Mo', 'Tu', 'We', 'Th', 'Fr', 'Sa']

/** The week-planner strip: one chip per modelled day, colored by that day's best
 * score — dimmed when the expedition isn't on the air that day (dates are the
 * announcement's; the model runs regardless). Thresholds mirror isOpenNow's
 * Fair boundary (0.3) with 0.55 ≈ Good. */
function WeekStrip({ days, entry }: { days: DxpedDayBest[]; entry: CalendarEntry }) {
  return (
    <div
      className="cal-week"
      title="Your modelled best shot for each of the next 7 days — plan the chase"
    >
      {days.map((d) => {
        const onAir = d.dayUnix < entry.endUnix && d.dayUnix + 86_400 > entry.startUnix
        const cls = !onAir ? 'off' : d.score >= 0.55 ? 'good' : d.score >= 0.3 ? 'fair' : 'poor'
        const wd = WEEKDAY[new Date(d.dayUnix * 1000).getUTCDay()]
        return (
          <span
            key={d.dayUnix}
            className={`cal-day ${cls}`}
            title={onAir ? `${wd}: ${d.best || 'no modelled path'}` : `${wd}: not on the air`}
          >
            {wd}
          </span>
        )
      })}
    </div>
  )
}

/** Lead-time choices for the wake-me alarm (minutes before the window opens). */
const LEADS = [5, 15, 30, 60]

export function DxpedCalendar({
  entries,
  windows,
  chasing,
  onToggleChase,
  alarms,
  onToggleAlarm,
  onAlarmLead,
  openPage = (e) => void withErrorToast(() => openDxpedPage(e.call, e.website), `Could not open the page for ${e.call}`),
}: {
  entries: CalendarEntry[]
  /** Modelled windows by call (get_dxped_windows) — preferred over the entry's
   * built-in heuristic outlook when present. */
  windows?: Map<string, DxpedWindow>
  chasing?: Set<string>
  onToggleChase?: (call: string) => void
  /** Armed wake-me alarms by call (features/dxpedAlarm) + their lead minutes. */
  alarms?: Record<string, { leadMin: number }>
  onToggleAlarm?: (call: string) => void
  onAlarmLead?: (call: string, leadMin: number) => void
  /** Opens an operation's page in the system browser. Injectable for tests. */
  openPage?: (entry: CalendarEntry) => void
}) {
  // MONTH GRID IS THE LANDING VIEW (operator, 2026-07-29). The per-entry list is
  // still here and still complete — it carries the heatmaps and the alarm/chase
  // controls — but it is no longer what greets you. "I think we could do better
  // ... the DXpedition calendar could be one section of it shown as a traditional
  // calendar."
  const [view, setView] = useState<'month' | 'list'>('month')
  if (entries.length === 0) return null
  // Clicking a bar or a digest row jumps to that operation in the list, which is
  // where the detail lives. Scrolling beats a modal: the surrounding entries are
  // context the operator wants.
  const openEntry = (call: string) => {
    setView('list')
    // Defer to the paint that mounts the list.
    requestAnimationFrame(() => {
      document
        .querySelector(`[data-cal-call="${call.toUpperCase()}"]`)
        ?.scrollIntoView({ block: 'center', behavior: 'smooth' })
    })
  }
  return (
    <section className="dxped-calendar panel" aria-label="DXpedition calendar">
      <div className="cal-topbar">
        <h2>DXpedition calendar — when to plan your chase</h2>
        <div className="cal-viewtabs" role="tablist" aria-label="Calendar view">
          <button
            type="button"
            role="tab"
            aria-selected={view === 'month'}
            className={view === 'month' ? 'on' : ''}
            onClick={() => setView('month')}
          >
            Calendar
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={view === 'list'}
            className={view === 'list' ? 'on' : ''}
            onClick={() => setView('list')}
          >
            Details
          </button>
        </div>
      </div>
      <DxpedDigest
        entries={entries}
        windows={windows}
        chasing={chasing}
        onSelect={openEntry}
      />
      {view === 'month' && (
        <DxpedMonth
          entries={entries}
          chasing={chasing}
          onSelect={openEntry}
          onOpenPage={openPage}
        />
      )}
      <div className="cal-list" hidden={view !== 'list'}>
        {entries.map((e) => {
          const w = windows?.get(e.call.toUpperCase())
          const isChased = chasing?.has(e.call.toUpperCase()) ?? false
          const alarm = alarms?.[e.call.toUpperCase()]
          const link = dxpedLink(e)
          return (
            <div
              className="cal-entry"
              key={`${e.call}-${e.startUnix}`}
              data-cal-call={e.call.toUpperCase()}
              // The identity colour follows the operation off the grid: a bar you
              // clicked and the entry you landed on carry the same rail.
              data-dxc={dxpedColorIndex(e.call)}
            >
              <div className="cal-head">
                <b className="cal-call">{e.call}</b>
                <span className="cal-entity">{e.entity}</span>
                <span className="cal-when">{daysUntil(e.startUnix)}</span>
                <span className="cal-geo">
                  {e.octant} · {e.region}
                </span>
                {(w?.best || e.best) && (
                  <span className="cal-best">
                    {w?.best ?? e.best}
                    {w && <span className="cp-engine">{w.engine === 'p533' ? 'P.533' : 'modelled'}</span>}
                  </span>
                )}
                {/* The same page the calendar bar opens, reachable from the detail
                    too — and here it is LABELLED, so "website" vs "QRZ fallback"
                    is visible rather than a surprise after the browser opens. */}
                {link && (
                  <button
                    type="button"
                    className={`cal-site${link.kind === 'qrz' ? ' fallback' : ''}`}
                    onClick={() => openPage(e)}
                    title={dxpedLinkTitle(link)}
                  >
                    ↗ {link.label}
                  </button>
                )}
                {onToggleChase && (
                  <button
                    type="button"
                    className={`wn-chase${isChased ? ' active' : ''}`}
                    onClick={() => onToggleChase(e.call)}
                    title={
                      isChased
                        ? 'Chasing — you get an alert when your window opens and they are spotted. Click to stop.'
                        : 'Chase this expedition — alert me when my modelled window opens and live spots confirm them'
                    }
                    aria-pressed={isChased}
                  >
                    {isChased ? '★' : '☆'}
                  </button>
                )}
                {onToggleAlarm && (
                  <button
                    type="button"
                    className={`wn-chase cal-alarm${alarm ? ' active' : ''}`}
                    onClick={() => onToggleAlarm(e.call)}
                    title={
                      alarm
                        ? `Alarm armed — a loud in-app wake-up fires ${alarm.leadMin} min before your modelled window opens. Click to disarm.`
                        : 'Wake me — arm a loud in-app alarm for when your modelled window to this expedition opens'
                    }
                    aria-pressed={!!alarm}
                  >
                    ⏰
                  </button>
                )}
                {alarm && onAlarmLead && (
                  <select
                    className="cal-lead"
                    value={alarm.leadMin}
                    onChange={(ev) => onAlarmLead(e.call, Number(ev.target.value))}
                    title="How long before the window opens to wake you"
                    aria-label="Alarm lead time"
                  >
                    {LEADS.map((m) => (
                      <option key={m} value={m}>
                        {m} min
                      </option>
                    ))}
                  </select>
                )}
              </div>
              {w?.days && w.days.length > 1 && <WeekStrip days={w.days} entry={e} />}
              {(e.bands.length > 0 || e.modes.length > 0) && (
                <div className="cal-meta">
                  {e.bands.join(' ')} {e.modes.length > 0 && <span className="cal-modes">· {e.modes.join('/')}</span>}
                </div>
              )}
              <LikelihoodHeatmap outlook={w?.outlook ?? e.outlook} muted />
            </div>
          )
        })}
      </div>
    </section>
  )
}
