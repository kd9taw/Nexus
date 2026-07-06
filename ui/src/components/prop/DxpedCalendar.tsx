// The forward DXpedition planning calendar: each announced operation with its
// dates/bands/modes, best-band headline, and the band×UTC-hour likelihood
// heatmap so the operator can plan when to chase it. When the P.533 windows
// command has data for a call, its headline + grid replace the heuristic ones
// (badged), and the ★ lets the operator chase before the expedition even starts.
import type { CalendarEntry, DxpedWindow } from '../../types'
import { LikelihoodHeatmap } from './LikelihoodHeatmap'

function daysUntil(startUnix: number): string {
  const d = Math.round((startUnix - Date.now() / 1000) / 86400)
  return d <= 0 ? 'on the air' : `T-${d}d`
}

export function DxpedCalendar({
  entries,
  windows,
  chasing,
  onToggleChase,
}: {
  entries: CalendarEntry[]
  /** Modelled windows by call (get_dxped_windows) — preferred over the entry's
   * built-in heuristic outlook when present. */
  windows?: Map<string, DxpedWindow>
  chasing?: Set<string>
  onToggleChase?: (call: string) => void
}) {
  if (entries.length === 0) return null
  return (
    <section className="dxped-calendar panel" aria-label="DXpedition calendar">
      <h2>DXpedition calendar — when to plan your chase</h2>
      <div className="cal-list">
        {entries.map((e) => {
          const w = windows?.get(e.call.toUpperCase())
          const isChased = chasing?.has(e.call.toUpperCase()) ?? false
          return (
            <div className="cal-entry" key={`${e.call}-${e.startUnix}`}>
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
              </div>
              {(e.bands.length > 0 || e.modes.length > 0) && (
                <div className="cal-meta">
                  {e.bands.join(' ')} {e.modes.length > 0 && <span className="cal-modes">· {e.modes.join('/')}</span>}
                </div>
              )}
              <LikelihoodHeatmap outlook={w?.outlook ?? e.outlook} />
            </div>
          )
        })}
      </div>
    </section>
  )
}
