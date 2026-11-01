// THE RECENT CONTACTS PANE — the last few contacts in the log, newest first, under a cockpit's log
// entry (the request: a box under the log form showing the last contacts, each one a click away from
// being corrected). A row opens that contact in the Logbook's own editor, by its id (App's handoff,
// the one editor and the one write path), exactly as a previous contact in the lookup card does.
//
// HOW MANY: `RECENT_QSOS`, five. Enough to reach back past the contact just logged to the run around
// it — a wrong call is noticed a contact or two later — and few enough that the pane stays a short
// strip under the form: five rows of one line each. Anything older is the Logbook's.
//
// WHERE IT STANDS: inside the log pane's own frame, directly under the form, in the pane's scrolling
// body — never beside the log pane in its column or shell. So it can only ever have the room the form
// leaves: where the pane is taller than the form it shows in that surplus, and where it is not it is
// the first thing the pane's scrollbar hides, while the form and its Log button stand exactly where
// they stand with the pane hidden. As its own frame beside the log pane, at exactly its content height,
// it took its whole height out of the log pane instead, and JS8's and RTTY's Log button needed a
// scroll at 1920x1080 (measured in Chrome). A role cannot say "after the form, before the rest": a
// fill sibling splits the column by weight whatever the form needs, and a content-first log pane grows
// past its hidden height and takes it from RTTY's and PSK's receive text (measured too).
//
// FT IS THE ONE EXCEPTION, because it has no log entry form (it logs through its confirm dialog) and so
// no log pane to ride in. There the pane is a member of FT's side rail, FT's log column, at the rail's
// foot (OperateCockpit `railFloor`). The rule above still holds there in its substance: nothing the
// operator needs gives it room. FT keeps its stock layout, so the Tx1–Tx6 machine and its divider stand
// exactly as they do with the pane hidden, and the rail's feeds keep a floor and the rail scrolls
// (cockpit-panes.css `.op-rail-floor`), where without one they gave the pane their height.
//
// READ AS THE LOGBOOK READS ITS LIST: the first page of the order it opens on (newest first by QSO
// time), through the window's log source, following the snapshot's log tick. Nothing here counts the
// log a second way.
//
// ONE HEIGHT, WHATEVER IT SAYS: the pane always draws `RECENT_QSOS` lines, filled from the top, so
// logging a contact into a short log never moves what stands below it.
//
// It shows contacts, sends nothing and hosts no stop control, and its hide ends nothing: under THE
// STOP LINE it is a plain entry, with no note.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Times, calls, bands and modes
// are data.
import { t } from '../i18n'
import { CockpitPaneFrame } from './panes/CockpitPaneFrame'
import { DEFAULT_LOG_QUERY, fmtUtc } from '../features/logQuery'
import { useLogAnswer } from '../features/logSource'

/** How many contacts the pane lists (see HOW MANY, above). */
export const RECENT_QSOS = 5

const QUESTION = { kind: 'page', query: DEFAULT_LOG_QUERY, offset: 0, limit: RECENT_QSOS } as const

/** The pane as every cockpit draws it under its `recent` id, inside the log pane under the form, or at the
 *  foot of FT's side rail (WHERE IT STANDS, above): a content-height frame, its ✕ the cockpit's own ⊞ tick. */
export function RecentQsosFrame({
  close,
  ...pane
}: RecentQsosProps & { close: { onRemove?: () => void; hideNote?: string } }) {
  return (
    <CockpitPaneFrame title={t('recentQsos.pane.title')} paneId="recent" fit="content" {...close}>
      <RecentQsos {...pane} />
    </CockpitPaneFrame>
  )
}

export interface RecentQsosProps {
  /** The snapshot's log tick, which moves on every change to the log: the list follows it. */
  logTick?: number
  /** Whether the cockpit is the view on screen. A cockpit kept mounted behind another view asks the log
   *  nothing: the list is read only while it can be seen. */
  active?: boolean
  /** Open a contact in the Logbook's editor (App's handoff). Absent where the Logbook is switched off:
   *  the rows are then plain lines, never controls that go nowhere. */
  onOpen?: (call: string, id?: string | null) => void
}

export function RecentQsos({ logTick, active = true, onOpen }: RecentQsosProps) {
  const page = useLogAnswer(active ? QUESTION : null, logTick)
  const rows = Array.isArray(page?.rows) ? page.rows.slice(0, RECENT_QSOS) : []
  return (
    <div className="recent-qsos" role="list" aria-label={t('recentQsos.aria', { count: RECENT_QSOS })}>
      {Array.from({ length: RECENT_QSOS }, (_, i) => {
        const q = rows[i]
        if (!q) {
          return (
            <div className="recent-qso-row empty" role="listitem" key={`empty-${i}`}>
              {i === 0 && rows.length === 0 ? t('recentQsos.empty') : null}
            </div>
          )
        }
        // The contact's own id (every contact a 1.14+ station serves has one); without it the row is a
        // plain line, as the lookup card's rows are.
        const id = q.id ?? null
        const cells = (
          <>
            <span className="recent-qso-when mono">{fmtUtc(q.whenUnix)}</span>
            <span className="recent-qso-call mono">{q.call}</span>
            <span className="recent-qso-bm">{[q.band, q.mode].filter(Boolean).join(' ')}</span>
          </>
        )
        return (
          <div className="recent-qso-row" role="listitem" key={id ?? `row-${i}`}>
            {onOpen && id ? (
              <button
                type="button"
                className="recent-qso-open"
                onClick={() => onOpen(q.call, id)}
                title={t('logbook.row.edit', { call: q.call })}
                aria-label={t('logbook.row.edit', { call: q.call })}
              >
                {cells}
              </button>
            ) : (
              <div className="recent-qso-line">{cells}</div>
            )}
          </div>
        )
      })}
    </div>
  )
}
