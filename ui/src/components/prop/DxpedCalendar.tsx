// The forward DXpedition planning calendar: each announced operation with its
// dates/bands/modes, best-band headline, and the band×UTC-hour likelihood
// heatmap so the operator can plan when to chase it.
import type { CalendarEntry } from '../../types'
import { LikelihoodHeatmap } from './LikelihoodHeatmap'

function daysUntil(startUnix: number): string {
  const d = Math.round((startUnix - Date.now() / 1000) / 86400)
  return d <= 0 ? 'on the air' : `T-${d}d`
}

export function DxpedCalendar({ entries }: { entries: CalendarEntry[] }) {
  if (entries.length === 0) return null
  return (
    <section className="dxped-calendar panel" aria-label="DXpedition calendar">
      <h2>DXpedition calendar — when to plan your chase</h2>
      <div className="cal-list">
        {entries.map((e) => (
          <div className="cal-entry" key={`${e.call}-${e.startUnix}`}>
            <div className="cal-head">
              <b className="cal-call">{e.call}</b>
              <span className="cal-entity">{e.entity}</span>
              <span className="cal-when">{daysUntil(e.startUnix)}</span>
              <span className="cal-geo">
                {e.octant} · {e.region}
              </span>
              {e.best && <span className="cal-best">{e.best}</span>}
            </div>
            {(e.bands.length > 0 || e.modes.length > 0) && (
              <div className="cal-meta">
                {e.bands.join(' ')} {e.modes.length > 0 && <span className="cal-modes">· {e.modes.join('/')}</span>}
              </div>
            )}
            <LikelihoodHeatmap outlook={e.outlook} />
          </div>
        ))}
      </div>
    </section>
  )
}
