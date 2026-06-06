import { useEffect, useMemo, useRef } from 'react'
import type { FieldDayQso, FieldDayStatus, ModeRequest } from '../types'

interface Props {
  fieldDay: FieldDayStatus | null
  onSetMode: (mode: ModeRequest) => void
}

interface LogRowMeta {
  qso: FieldDayQso
  /** first appearance of this section in the log = a new multiplier */
  isNewSection: boolean
  /** the same call appears more than once in the log = a dupe */
  isDupe: boolean
}

/**
 * Annotate each log entry with multiplier / dupe state. Sections are marked the
 * first time they appear (scanning oldest -> newest); a call is a dupe if it
 * appears more than once anywhere in the log.
 */
function annotate(log: FieldDayQso[]): LogRowMeta[] {
  const seenSections = new Set<string>()
  const callCounts = new Map<string, number>()
  for (const q of log) callCounts.set(q.call, (callCounts.get(q.call) ?? 0) + 1)
  return log.map((q) => {
    const isNewSection = !seenSections.has(q.section)
    seenSections.add(q.section)
    return {
      qso: q,
      isNewSection,
      isDupe: (callCounts.get(q.call) ?? 0) > 1,
    }
  })
}

export function FieldDayView({ fieldDay, onSetMode }: Props) {
  const scrollRef = useRef<HTMLDivElement>(null)
  const running = fieldDay?.running ?? false
  const log = fieldDay?.log ?? []

  const rows = useMemo(() => annotate(log), [log])

  // keep the newest contact in view as the log grows
  useEffect(() => {
    const el = scrollRef.current
    if (el) el.scrollTop = el.scrollHeight
  }, [log.length])

  return (
    <section className="conversation panel fieldday">
      <div className="panel-header fd-header">
        <div className="fd-ident">
          <h2 className="conv-peer">Field Day</h2>
          <span className="fd-class">
            {fieldDay?.myClass ?? '—'}
            <span className="fd-section"> {fieldDay?.mySection ?? '—'}</span>
          </span>
        </div>
        <div className="fd-role-toggle" role="group" aria-label="Field Day role">
          <button
            type="button"
            className={`fd-role-btn${running ? ' active' : ''}`}
            aria-pressed={running}
            onClick={() => onSetMode('fieldday-run')}
          >
            Running
          </button>
          <button
            type="button"
            className={`fd-role-btn${!running ? ' active' : ''}`}
            aria-pressed={!running}
            onClick={() => onSetMode('fieldday-sp')}
          >
            S&amp;P
          </button>
        </div>
      </div>

      <div className="fd-scoreboard">
        <div className="fd-score">
          <span className="fd-score-val">{fieldDay?.qsoCount ?? 0}</span>
          <span className="fd-score-label">QSOs</span>
        </div>
        <div className="fd-score">
          <span className="fd-score-val">{fieldDay?.sections ?? 0}</span>
          <span className="fd-score-label">Sections</span>
        </div>
        <div className="fd-score accent">
          <span className="fd-score-val">{fieldDay?.points ?? 0}</span>
          <span className="fd-score-label">Points</span>
        </div>
        <div className="fd-state-chip" title="Sequencer state">
          {fieldDay?.state ?? 'Idle'}
        </div>
      </div>

      <div className="fd-log">
        <div className="fd-log-head">
          <span className="fd-col call">Call</span>
          <span className="fd-col cls">Class</span>
          <span className="fd-col sec">Section</span>
          <span className="fd-col band">Band</span>
        </div>
        <div className="fd-log-scroll" ref={scrollRef}>
          {rows.length === 0 && <p className="empty">No contacts logged yet.</p>}
          {rows.map((r, i) => (
            <div
              className={`fd-log-row${r.isNewSection ? ' mult' : ''}${r.isDupe ? ' dupe' : ''}`}
              key={`${r.qso.call}-${i}`}
              title={r.isDupe ? 'Duplicate callsign' : r.isNewSection ? 'New section — multiplier' : undefined}
            >
              <span className="fd-col call mono">{r.qso.call}</span>
              <span className="fd-col cls mono">{r.qso.class}</span>
              <span className="fd-col sec mono">
                {r.qso.section}
                {r.isNewSection && <span className="fd-mult-tag">Mult!</span>}
              </span>
              <span className="fd-col band">{r.qso.band}</span>
            </div>
          ))}
        </div>
      </div>
    </section>
  )
}
