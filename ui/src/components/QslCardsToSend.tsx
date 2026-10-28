import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import {
  getSettings,
  isTauri,
  markQslSentById,
  openQslLabels,
  qslCardQueue,
  type KeyedRow,
} from '../api'
import type { LoggedQso } from '../types'
import { t } from '../i18n'
import { pushToast, withErrorToast } from '../toast'
import { utcDate } from '../features/utcLog'
import {
  LABEL_FORMATS,
  clampStart,
  groupLabels,
  labelFormat,
  labelSheetHtml,
  labelsPerPage,
  pagesNeeded,
  qsosPerLabel,
  VIA,
  type LabelQso,
} from '../features/qslLabels'

// CARDS TO SEND — the contacts queued for a paper QSL card (ADIF `QSL_SENT` `Q`, or `R` when the
// other station asked for one; queued from a contact's QSL menu, or imported so), listed for the
// operator to tick, printed as labels on one of three label stocks (features/qslLabels.ts), and
// marked sent.
//
// ⭐ THE GATE: printing marks nothing. A label sheet that jammed, printed off the labels or never
// reached the printer must leave every card queued, and nothing here can see paper. So Print only
// opens the sheet, and the dialog then ASKS whether the labels printed. Only "Yes" marks the
// contacts that were printed sent (`QSL_SENT=Y`, `QSLSDATE` today, the route the operator picks),
// one by one, each only while it is still the contact that was listed; "No" leaves them queued.
// Nothing else about a contact changes.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Calls, bands, modes and
// the label stocks' product names are invariant tokens, shown as they are.

type Phase = 'pick' | 'asking' | 'marking'

/** The contact as a label prints it: its QSL route from the log, its own call, else the station's. */
export function labelQso(q: LoggedQso, myCall: string): LabelQso {
  const field = (name: string) => q.extra?.find(([k]) => k === name)?.[1]?.trim() || null
  return {
    call: q.call,
    via: field('QSL_VIA'),
    whenUnix: q.whenUnix,
    band: q.band,
    mode: q.mode,
    rst: q.rstSent,
    myCall: (q.stationCallsign?.trim() || q.operator?.trim() || myCall).toUpperCase(),
  }
}

export function QslCardsToSend({
  open,
  onClose,
  onChanged,
}: {
  open: boolean
  onClose: () => void
  onChanged: () => void
}) {
  const [rows, setRows] = useState<KeyedRow[] | null>(null)
  const [ticked, setTicked] = useState<ReadonlySet<string>>(new Set())
  const [formatId, setFormatId] = useState<string>(LABEL_FORMATS[0].id)
  const [combine, setCombine] = useState(true)
  const [start, setStart] = useState(1)
  const [via, setVia] = useState<'B' | 'D'>('B')
  const [phase, setPhase] = useState<Phase>('pick')
  // The contacts the last sheet printed: what a "Yes" marks, whatever is ticked by then.
  const [printed, setPrinted] = useState<KeyedRow[]>([])
  const [myCall, setMyCall] = useState('')

  const boxRef = useRef<HTMLDivElement>(null)
  useLayoutEffect(() => {
    if (!open) return
    const active = document.activeElement
    const opener = active instanceof HTMLElement && active !== document.body ? active : null
    boxRef.current?.focus()
    return () => {
      const now = document.activeElement
      if (now && now !== document.body && now.isConnected) return
      if (opener?.isConnected) opener.focus({ preventScroll: true })
    }
  }, [open])

  const load = async () => {
    const found = await withErrorToast(() => qslCardQueue(), t('logbook.qslCards.failed'))
    if (!found) {
      onClose()
      return
    }
    setRows(found)
    setTicked(new Set(found.map((r) => r.row.id ?? '')))
  }

  useEffect(() => {
    if (!open) return
    setRows(null)
    setPhase('pick')
    setPrinted([])
    void load()
    void getSettings()
      .then((s) => setMyCall(s.mycall ?? ''))
      .catch(() => setMyCall(''))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open])

  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && phase === 'pick') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [open, onClose, phase])

  const format = labelFormat(formatId)
  const chosen = useMemo(() => (rows ?? []).filter((r) => ticked.has(r.row.id ?? '')), [rows, ticked])
  const labels = useMemo(
    () => groupLabels(chosen.map((r) => labelQso(r.row, myCall)), qsosPerLabel(format), combine),
    [chosen, myCall, format, combine],
  )

  if (!open) return null

  const toggle = (id: string) =>
    setTicked((was) => {
      const next = new Set(was)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })

  const print = async () => {
    if (labels.length === 0) return
    const html = labelSheetHtml(labels, format, start, {
      title: t('logbook.qslCards.sheet.title'),
      help: t('logbook.qslCards.sheet.help'),
      print: t('logbook.qslCards.sheet.print'),
    })
    let opened = false
    if (isTauri()) {
      opened = (await withErrorToast(() => openQslLabels(html).then(() => true), t('logbook.qslCards.printFailed'))) === true
    } else {
      // A browser (the hosted Remote): a tab of its own, which prints it.
      const url = URL.createObjectURL(new Blob([html], { type: 'text/html' }))
      opened = window.open(url, '_blank') != null
      setTimeout(() => URL.revokeObjectURL(url), 60_000)
      if (!opened) pushToast(t('logbook.qslCards.printFailed'), 'error', 8000)
    }
    if (!opened) return
    setPrinted(chosen)
    setPhase('asking')
  }

  // Only after the operator says the labels printed: each printed contact marked sent, by its id
  // and the key it was listed with — a contact changed since is refused and stays queued.
  const markSent = async () => {
    setPhase('marking')
    let done = 0
    for (const r of printed) {
      if (!r.row.id) continue
      try {
        const answer = await markQslSentById({ id: r.row.id, editKey: r.editKey }, via)
        if (answer.kind === 'applied') done++
      } catch {
        // Counted as not done: it stays queued, and the summary says so.
      }
    }
    if (done === printed.length) pushToast(t('logbook.qslCards.done', { count: done }), 'success')
    else pushToast(t('logbook.qslCards.partial', { done, count: printed.length }), 'error', 10000)
    onChanged()
    setPrinted([])
    setPhase('pick')
    await load()
  }

  const keepQueued = () => {
    setPrinted([])
    setPhase('pick')
  }

  const perPage = labelsPerPage(format)

  return (
    <div
      className="logconfirm-backdrop"
      role="dialog"
      aria-modal="true"
      aria-label={t('logbook.qslCards.title')}
      onClick={() => {
        if (phase === 'pick') onClose()
      }}
    >
      <div ref={boxRef} tabIndex={-1} className="logconfirm park-review" onClick={(e) => e.stopPropagation()}>
        <div className="logconfirm-head">
          <h2>{t('logbook.qslCards.title')}</h2>
        </div>
        <p className="park-review-intro">{t('logbook.qslCards.intro')}</p>
        {phase === 'pick' ? (
          <>
            {rows == null ? (
              <p className="park-review-note">{t('logbook.qslCards.reading')}</p>
            ) : rows.length === 0 ? (
              <p className="park-review-note">{t('logbook.qslCards.none')}</p>
            ) : (
              <>
                <div className="qsl-cards-options">
                  <label>
                    <span>{t('logbook.qslCards.format')}</span>
                    <select className="settings-input" value={format.id} onChange={(e) => setFormatId(e.target.value)}>
                      {LABEL_FORMATS.map((f) => (
                        <option key={f.id} value={f.id}>
                          {f.name}
                        </option>
                      ))}
                    </select>
                  </label>
                  {perPage > 1 && (
                    <label title={t('logbook.qslCards.startTitle')}>
                      <span>{t('logbook.qslCards.start')}</span>
                      <input
                        className="settings-input qsl-cards-start"
                        type="number"
                        min={1}
                        max={perPage}
                        value={clampStart(format, start)}
                        onChange={(e) => setStart(clampStart(format, Number(e.target.value)))}
                      />
                    </label>
                  )}
                  <label className="qsl-cards-check">
                    <input type="checkbox" checked={combine} onChange={(e) => setCombine(e.target.checked)} />
                    <span>{t('logbook.qslCards.combine')}</span>
                  </label>
                  <p className="park-review-note">
                    {t('logbook.qslCards.summary', {
                      labels: labels.length,
                      pages: pagesNeeded(format, labels.length, start),
                    })}
                  </p>
                </div>
                <ul className="park-review-list">
                  {rows.map((r) => {
                    const id = r.row.id ?? ''
                    const q = labelQso(r.row, myCall)
                    return (
                      <li key={id} className={`park-review-row${ticked.has(id) ? ' ticked' : ''}`}>
                        <label>
                          <input type="checkbox" checked={ticked.has(id)} onChange={() => toggle(id)} />
                          <span className="mono park-review-call">{r.row.call}</span>
                          {/* The route as a QSL card writes it: an invariant token (VIA). */}
                          {q.via && <span className="mono">{`${VIA} ${q.via}`}</span>}
                          <span className="mono park-review-when">{utcDate(r.row.whenUnix)}</span>
                          <span className="park-review-band">{`${r.row.band} ${r.row.mode}`}</span>
                          {r.row.rstSent && <span className="mono">{r.row.rstSent}</span>}
                        </label>
                      </li>
                    )
                  })}
                </ul>
              </>
            )}
            <div className="logconfirm-actions">
              <button type="button" className="logconfirm-discard" onClick={onClose}>
                {t('logbook.qslCards.close')}
              </button>
              <button
                type="button"
                className="logconfirm-log"
                disabled={rows == null || labels.length === 0}
                onClick={() => void print()}
              >
                {t('logbook.qslCards.print', { count: labels.length })}
              </button>
            </div>
          </>
        ) : (
          <div role="group" aria-label={t('logbook.qslCards.question')}>
            <p className="park-review-note">{t('logbook.qslCards.question')}</p>
            <div className="qsl-cards-options">
              <label>
                <span>{t('logbook.qslCards.via')}</span>
                <select
                  className="settings-input"
                  value={via}
                  disabled={phase === 'marking'}
                  onChange={(e) => setVia(e.target.value === 'D' ? 'D' : 'B')}
                >
                  {/* The VALUES are the ADIF QSL_SENT_VIA letters; only the labels are prose. */}
                  <option value="B">{t('logbook.row.qslSent.bureau')}</option>
                  <option value="D">{t('logbook.row.qslSent.direct')}</option>
                </select>
              </label>
            </div>
            <div className="logconfirm-actions">
              <button type="button" className="logconfirm-discard" disabled={phase === 'marking'} onClick={keepQueued}>
                {t('logbook.qslCards.no')}
              </button>
              <button
                type="button"
                className="logconfirm-log"
                disabled={phase === 'marking'}
                onClick={() => void markSent()}
              >
                {t('logbook.qslCards.yes', { count: printed.length })}
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
