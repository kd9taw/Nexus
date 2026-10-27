// THE ACTIVATION PANE — the activator's place in the app: a pane and a pop-out, while the POTA/SOTA
// view's strip keeps Start, Stop, Spot me and the grid. While an activation runs it shows:
//   · each park's contacts today, "7 of 10", with how many to go or "valid", and park to park — the
//     strip's own words over the station's own count (`Activation.parks`), so the two can never
//     disagree: nothing here counts a contact;
//   · a park's day under more than one call, said plainly: POTA counts each call separately;
//   · how long the activation has run, and how long until 0000Z starts the counts again;
//   · the rate, read as the contest meter reads it (features/contestRate) over this activation's
//     contacts;
//   · this activation's contacts, newest first: the Logbook's own answer to its activation filter
//     (features/potaActivation `activationFilter`), through the window's log source.
//
// THE COUNTS FIRST: in a short box (the rail's four at 768 tall) they are what shows before any scroll.
//
// WHERE IT STANDS: a Conditions box (connect/panes `activation`), so it goes wherever a box goes — a
// cockpit's box, the dashboard rail, Conditions — through the cockpit's pane frame with the shared
// list's role (a list: it fills), and in a window of its own (DetachedPanel `activation`).
//
// It sends nothing, hosts no control that starts or stops a transmission, and its hide ends nothing:
// under THE STOP LINE it is a plain entry, with no note.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Times, calls, bands, modes,
// references and the programmes' names are data.
import { useEffect, useMemo, useState } from 'react'
import { t } from '../i18n'
import type { Activation } from '../types'
import { DEFAULT_LOG_QUERY } from '../features/logQuery'
import { useLogAnswer } from '../features/logSource'
import { contestRate } from '../features/contestRate'
import {
  activationFilter,
  activationParks,
  mixedCallsNote,
  parkProgress,
  parkRefs,
  spanLabel,
  toUtcMidnight,
} from '../features/potaActivation'

/** How many of this activation's contacts the pane reads: enough for the rate's 100-contact window
 *  and a busy hour; the rest are the Logbook's. */
export const ACTIVATION_ROWS = 200

/** What a window lends the pane: the activation off its snapshot, and the snapshot's log tick. */
export interface ActivationBoard {
  act: Activation | null
  logTick?: number
}

/** "14:02Z": a contact's time, as the pane lists it. */
function hhmm(unix: number): string {
  const d = new Date(unix * 1000)
  return `${String(d.getUTCHours()).padStart(2, '0')}:${String(d.getUTCMinutes()).padStart(2, '0')}Z`
}

/** The time, in Unix seconds, moving on every half minute: the spans and the rate decay with it. */
function useNowUnix(fixed?: number): number {
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000))
  useEffect(() => {
    if (fixed !== undefined) return
    const id = window.setInterval(() => setNow(Math.floor(Date.now() / 1000)), 30_000)
    return () => window.clearInterval(id)
  }, [fixed])
  return fixed ?? now
}

export function ActivationPane({ act, logTick, nowUnix }: ActivationBoard & { nowUnix?: number }) {
  const now = useNowUnix(nowUnix)
  const parks = activationParks(act)
  const filter = activationFilter(act)
  const myRef = filter?.myRef
  const fromUnix = filter?.fromUnix
  const question = useMemo(
    () =>
      myRef
        ? ({
            kind: 'page',
            query: { ...DEFAULT_LOG_QUERY, filter: { myRef, fromUnix } },
            offset: 0,
            limit: ACTIVATION_ROWS,
          } as const)
        : null,
    [myRef, fromUnix],
  )
  const page = useLogAnswer(question, logTick)
  if (!act || parks.length === 0) {
    return <p className="pane-basic">{t('activation.pane.none')}</p>
  }
  const rows = Array.isArray(page?.rows) ? page.rows : []
  const total = typeof page?.total === 'number' ? page.total : rows.length
  const rate = contestRate(rows, now)
  const tenRate = rate.windows[0]
  return (
    <div className="act-pane">
      <ul className="act-pane-parks" aria-label={t('activation.pane.parks')}>
        {(act.parks?.length ? act.parks : parks.map((reference) => ({ reference, count: act.qsoCount ?? 0, p2p: 0 }))).map(
          (p) => {
            const g = parkProgress(p.count)
            return (
              <li key={p.reference} className={`act-pane-park${g.valid ? ' valid' : ''}`} title={t('ota.activation.progress.title', { reference: p.reference })}>
                <span className="act-pane-count mono">{t('ota.activation.progress', { reference: p.reference, count: p.count })}</span>
                <span className="act-pane-togo">{g.valid ? t('ota.activation.valid') : t('ota.activation.toGo', { count: g.toGo })}</span>
                <span className="act-pane-p2p" title={t('ota.activation.p2p.title')}>
                  {t('ota.activation.p2p', { count: p.p2p })}
                </span>
              </li>
            )
          },
        )}
      </ul>
      {(act.parks ?? []).map((p) => {
        const note = mixedCallsNote(p)
        return note ? (
          <p key={p.reference} className="act-pane-calls">
            {note}
          </p>
        ) : null
      })}
      <div className="act-pane-head">
        <span className="act-pane-ref mono">{[act.program, act.reference].filter(Boolean).join(' ')}</span>
        {act.startedAt ? (
          <span className="act-pane-time">{t('activation.pane.running', { span: spanLabel(now - act.startedAt) })}</span>
        ) : null}
        <span className="act-pane-time" title={t('activation.pane.midnight.title')}>
          {t('activation.pane.midnight', { span: spanLabel(toUtcMidnight(now)) })}
        </span>
      </div>
      {act.restored && <p className="act-pane-paused">{t('activation.pane.paused')}</p>}
      <div className="act-pane-rate" title={t('activation.pane.rate.title')}>
        {tenRate.perHour !== null
          ? t('activation.pane.rate', { rate: Math.round(tenRate.perHour), count: tenRate.sample })
          : t('activation.pane.rate.none')}
        {' · '}
        {t('activation.pane.lastHour', { count: rate.lastHour })}
      </div>
      <div className="act-pane-list" role="list" aria-label={t('activation.pane.list')}>
        {rows.length === 0 ? (
          <div className="act-pane-row empty" role="listitem">
            {page ? t('activation.pane.empty') : t('activation.pane.reading')}
          </div>
        ) : (
          rows.map((q, i) => {
            const theirs = (q.ota?.theirProgram ?? '').toUpperCase() === 'POTA' ? parkRefs(q.ota?.theirRef) : []
            return (
              <div className="act-pane-row" role="listitem" key={q.id ?? `row-${i}`}>
                <span className="act-pane-when mono">{hhmm(q.whenUnix)}</span>
                <span className="act-pane-call mono">{q.call}</span>
                <span className="act-pane-bm">{[q.band, q.mode].filter(Boolean).join(' ')}</span>
                <span className="act-pane-their mono">{theirs.join(' ')}</span>
              </div>
            )
          })
        )}
      </div>
      {total > rows.length && (
        <p className="act-pane-more">{t('activation.pane.more', { shown: rows.length, total })}</p>
      )}
    </div>
  )
}
