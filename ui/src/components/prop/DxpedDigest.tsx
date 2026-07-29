// "What should I chase, and when?" — the one-glance answer, above the calendar.
//
// Operator, 2026-07-29: "it would be great to give a more summarized and direct
// set of feedback on windows to chase these DXpeditions ... we're missing some of
// those key insights if someone wants to go to this quickly and see what they need
// to chase and when."
//
// Nothing here is new intelligence. Every number already existed — the modelled
// headline (`DxpedWindow.best`, P.533 or heuristic) and the 7-day planner
// (`DxpedWindow.days`) were already fetched and already rendered, just spread
// across per-entry rows where the operator had to assemble the comparison
// themselves. This states the conclusion instead of the evidence.
import type { CalendarEntry, DxpedWindow } from '../../types'
import { dxpedColorIndex } from './dxpedLanes'

const WEEKDAY = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat']

/** On the air right now, by the announcement's own dates. */
function isOnAir(e: CalendarEntry, now: number): boolean {
  return e.startUnix <= now && e.endUnix > now
}

function startsIn(e: CalendarEntry, now: number): string {
  const d = Math.round((e.startUnix - now) / 86_400)
  if (d <= 0) return 'on the air now'
  if (d === 1) return 'starts tomorrow'
  return `starts in ${d} days`
}

/** The best day(s) from the modelled week, named rather than numbered. A day only
 * qualifies if it beats the Fair boundary (0.3) that the rest of the prop UI uses
 * — "your best day is a bad one" is worth saying plainly, not dressing up. */
function bestDays(w: DxpedWindow | undefined, e: CalendarEntry): string | null {
  const days = w?.days
  if (!days || days.length === 0) return null
  const onAir = days.filter(
    (d) => d.dayUnix < e.endUnix && d.dayUnix + 86_400 > e.startUnix && d.score >= 0.3,
  )
  if (onAir.length === 0) return null
  const top = Math.max(...onAir.map((d) => d.score))
  const picks = onAir
    .filter((d) => d.score >= top - 0.05) // ties, not a single arbitrary winner
    .slice(0, 2)
    .map((d) => WEEKDAY[new Date(d.dayUnix * 1000).getUTCDay()])
  return picks.length ? picks.join(' + ') : null
}

export function DxpedDigest({
  entries,
  windows,
  chasing,
  onSelect,
  nowUnix,
  limit = 4,
}: {
  entries: CalendarEntry[]
  windows?: Map<string, DxpedWindow>
  chasing?: Set<string>
  onSelect?: (call: string) => void
  nowUnix?: number
  limit?: number
}) {
  const now = nowUnix ?? Math.floor(Date.now() / 1000)
  if (entries.length === 0) return null

  // Order: on the air first, then whatever starts soonest. Chased entities are
  // pulled to the front of their group — the operator already told us those
  // matter, so burying them under an alphabetically-luckier call is wrong.
  const ranked = [...entries]
    .filter((e) => e.endUnix > now)
    .sort((a, b) => {
      const ca = chasing?.has(a.call.toUpperCase()) ? 0 : 1
      const cb = chasing?.has(b.call.toUpperCase()) ? 0 : 1
      const la = isOnAir(a, now) ? 0 : 1
      const lb = isOnAir(b, now) ? 0 : 1
      return la - lb || ca - cb || a.startUnix - b.startUnix
    })
    .slice(0, limit)

  if (ranked.length === 0) return null

  return (
    <section className="dxd" aria-label="What to chase">
      <h3 className="dxd-h">What to chase</h3>
      <ul className="dxd-list">
        {ranked.map((e) => {
          const call = e.call.toUpperCase()
          const w = windows?.get(call)
          const headline = w?.best || e.best
          const days = bestDays(w, e)
          const live = isOnAir(e, now)
          return (
            <li key={`${call}-${e.startUnix}`} className={`dxd-row${live ? ' live' : ''}`}>
              {/* Same slot the operation's calendar bar uses — the row and the bar
                  are recognisably one thing without reading either callsign. */}
              <span className="dxd-dot" data-dxc={dxpedColorIndex(call)} aria-hidden="true" />
              <button type="button" className="dxd-call" onClick={() => onSelect?.(e.call)}>
                {chasing?.has(call) && <span aria-hidden="true">★ </span>}
                {e.call}
              </button>
              <span className="dxd-entity">{e.entity}</span>
              <span className="dxd-when">{live ? 'ON THE AIR' : startsIn(e, now)}</span>
              {headline && <span className="dxd-best">{headline}</span>}
              {days && <span className="dxd-days">best {days}</span>}
            </li>
          )
        })}
      </ul>
    </section>
  )
}
