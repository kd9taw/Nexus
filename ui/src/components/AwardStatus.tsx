// AWARD STATUS — the Awards view's "Ready to submit" panel, and the contact card's per-award
// Submitted / Granted box. See features/awardStatus.ts for where the status lives and why both
// write it through the Bulk edit's own change.
//
// The panel: pick an award, see its entities worked / confirmed / submitted / granted, and the
// list ready to submit — one contact per entity confirmed by that award's own rule and neither
// submitted nor granted, its oldest confirmed contact or its newest. Export the list as CSV, save
// a page to print, and mark the whole list Submitted — only once the operator has confirmed the
// count the station previewed; the change is one run of the history, which the toast's Undo
// takes back.
//
// ⚠️ Every operator-visible string comes from the catalog. Not from it: the award names
// (programme names, invariant tokens), entity names, calls, bands and modes (data).

import { useEffect, useRef, useState } from 'react'
import { ClipboardCheck } from 'lucide-react'
import {
  bulkEditApply,
  bulkEditPreview,
  getAwardStatus,
  saveTextToDownloads,
  type AwardKind,
  type AwardStatus,
  type FieldChange,
} from '../api'
import type { LoggedQso } from '../types'
import { t } from '../i18n'
import { confirmDialog } from '../confirm'
import { pushToast, withErrorToast } from '../toast'
import { utcDate } from '../features/utcLog'
import { changesRefusedHere, sayRun } from '../features/logCleanup'
import { askAgainWhileSaving } from '../features/notAnswered'
import {
  STATUS_AWARDS,
  readyCsv,
  readyFileName,
  readyHtml,
  readyIds,
  statusOf,
  type CreditStatus,
  type StatusAward,
} from '../features/awardStatus'

/** Rows the panel lists before "Show all". */
const READY_SHOWN = 25

/** A confirmation channel as the list names it: the card in words, the services by name. */
function channelName(c: string): string {
  if (c === 'card') return t('qso.detail.qsl.card')
  if (c === 'lotw') return 'LoTW'
  if (c === 'eqsl') return 'eQSL AG'
  return c
}

/** The award's confirmation rule, in words. */
function ruleText(kind: AwardKind): string {
  switch (kind) {
    case 'dxcc':
    case 'was':
      return t('awards.status.rule.arrl')
    case 'waz':
      return t('awards.status.rule.waz')
    case 'iota':
      return t('awards.status.rule.iota')
    case 'usaca':
      return t('awards.status.rule.usaca')
  }
}

/** Mark `ids` Submitted (or Granted, or neither: `status` null) for `award`, through the Bulk
 *  edit's change: preview, ask the operator naming the count it would change, then make it on
 *  exactly the contacts previewed. Resolves with how many changed, or null when nothing was made
 *  (refused here, the operator said no, or the station refused — its toast has said why). */
export async function setAwardStatus(
  award: StatusAward,
  ids: string[],
  status: CreditStatus | null,
  ask: (count: number) => { title: string; body: string; confirmLabel: string },
  onChanged: () => void,
): Promise<number | null> {
  if (changesRefusedHere() || ids.length === 0) return null
  const changes: FieldChange[] = [{ field: award.field, value: status }]
  const preview = await withErrorToast(() => bulkEditPreview({ ids }, changes), t('awards.status.mark.failed'))
  if (!preview) return null
  if (preview.change === 0) {
    pushToast(t('logbook.tools.nothing'), 'info', 5000)
    return 0
  }
  const ok = await confirmDialog({ ...ask(preview.change), cancelLabel: t('logbook.tools.cancel') })
  if (!ok) return null
  const made = await withErrorToast(
    () => bulkEditApply(preview.ids, preview.selected, 0, null, changes),
    t('awards.status.mark.failed'),
  )
  if (!made) return null
  sayRun(made, t('awards.status.mark.done', { count: made.changed, award: award.name }), onChanged)
  onChanged()
  return made.changed
}

export function AwardStatusPanel({ readOnly = false }: { readOnly?: boolean }) {
  const [kind, setKind] = useState<AwardKind>('dxcc')
  const [newest, setNewest] = useState(false)
  const [status, setStatus] = useState<AwardStatus | null>(null)
  const [failed, setFailed] = useState(false)
  const [all, setAll] = useState(false)
  const [busy, setBusy] = useState(false)
  const [tick, setTick] = useState(0)
  const live = useRef(true)
  const award = STATUS_AWARDS.find((a) => a.kind === kind) ?? STATUS_AWARDS[0]

  useEffect(() => {
    live.current = true
    return () => {
      live.current = false
    }
  }, [])
  useEffect(() => {
    // Asked again while a change just made is still being saved (features/notAnswered).
    let wanted = true
    setStatus(null)
    setFailed(false)
    askAgainWhileSaving(() => getAwardStatus(kind, newest), () => wanted)
      .then((s) => wanted && setStatus(s))
      .catch(() => wanted && setFailed(true))
    return () => {
      wanted = false
    }
  }, [kind, newest, tick])

  const reload = () => setTick((n) => n + 1)
  const rows = status?.ready ?? []
  const shown = all ? rows : rows.slice(0, READY_SHOWN)
  const now = () => Math.floor(Date.now() / 1000)

  const save = (name: string, text: string) =>
    void saveTextToDownloads(name, text)
      .then((path) => pushToast(t('awards.status.saved', { path }), 'success', 6000))
      .catch((e) => pushToast(String(e), 'error'))

  const exportCsv = () => save(readyFileName(kind, 'csv', now()), readyCsv(rows))
  const printable = () =>
    save(
      readyFileName(kind, 'html', now()),
      readyHtml(rows, {
        title: t('awards.status.print.title', { award: award.name }),
        summary: t('awards.status.print.summary', { count: rows.length, date: utcDate(now()) }),
        entity: t('awards.status.col.entity'),
        call: t('awards.status.col.call'),
        date: t('awards.status.col.date'),
        band: t('awards.status.col.band'),
        mode: t('awards.status.col.mode'),
        confirmedBy: t('awards.status.col.confirmedBy'),
        channel: channelName,
      }),
    )

  const mark = async () => {
    if (readOnly || busy || !status) return
    setBusy(true)
    await setAwardStatus(
      award,
      readyIds(status),
      'SUBMITTED',
      (count) => ({
        title: t('awards.status.mark.title', { count, award: award.name }),
        body: t('awards.status.mark.body', { award: award.name }),
        confirmLabel: t('awards.status.mark.confirm'),
      }),
      reload,
    )
    if (live.current) setBusy(false)
  }

  return (
    <div className="aw-panel aw-status-panel">
      <h3>
        <ClipboardCheck size={14} aria-hidden="true" /> {t('awards.status.head')}
      </h3>
      <div className="aw-status-controls">
        <select aria-label={t('awards.status.award')} value={kind} onChange={(e) => { setKind(e.target.value as AwardKind); setAll(false) }}>
          {STATUS_AWARDS.map((a) => (
            <option key={a.kind} value={a.kind}>
              {a.name}
            </option>
          ))}
        </select>
        <select
          aria-label={t('awards.status.pick')}
          value={newest ? 'newest' : 'oldest'}
          onChange={(e) => setNewest(e.target.value === 'newest')}
        >
          <option value="oldest">{t('awards.status.oldest')}</option>
          <option value="newest">{t('awards.status.newest')}</option>
        </select>
      </div>
      <p className="aw-empty">{ruleText(kind)}</p>
      {failed ? (
        <p className="aw-empty">{t('awards.status.error')}</p>
      ) : !status ? (
        <p className="aw-empty">{t('awards.chart.loading')}</p>
      ) : (
        <>
          <p className="aw-empty" data-testid="award-status-counts">
            {t('awards.status.counts', {
              worked: status.worked,
              confirmed: status.confirmed,
              submitted: status.submitted,
              granted: status.granted,
            })}
          </p>
          <h4>{t('awards.status.readyHead', { count: rows.length })}</h4>
          {rows.length === 0 ? (
            <p className="aw-empty">{t('awards.status.empty')}</p>
          ) : (
            <>
              <ul className="aw-needed aw-status-ready">
                {shown.map((r) => (
                  <li key={`${r.entity}-${r.id}`}>
                    <span className="aw-entity">
                      {r.entity} · {r.call} · {utcDate(r.whenUnix)} · {r.band} {r.mode}
                    </span>
                    <span className="aw-needbands">
                      {r.confirmedBy.map((c) => (
                        <span className="aw-chip" key={c}>
                          {channelName(c)}
                        </span>
                      ))}
                    </span>
                  </li>
                ))}
              </ul>
              {!all && rows.length > READY_SHOWN && (
                <button type="button" className="aw-needsort" onClick={() => setAll(true)}>
                  {t('awards.status.showAll', { count: rows.length })}
                </button>
              )}
              <div className="aw-status-actions">
                <button type="button" className="aw-needsort" onClick={exportCsv}>
                  {t('awards.status.csv')}
                </button>
                <button type="button" className="aw-needsort" onClick={printable}>
                  {t('awards.status.printButton')}
                </button>
                {!readOnly && (
                  <button type="button" className="aw-needsort" disabled={busy} onClick={() => void mark()}>
                    {t('awards.status.markButton', { count: rows.length })}
                  </button>
                )}
              </div>
            </>
          )}
        </>
      )}
    </div>
  )
}

/** The contact card's award status: one choice per award — not submitted, Submitted, Granted —
 *  each made at once through [`setAwardStatus`], which asks before it changes anything. */
export function CardAwardStatus({ q, canSave, onChanged }: { q: LoggedQso; canSave: boolean; onChanged: () => void }) {
  const [busy, setBusy] = useState(false)
  if (!q.id) return null
  const id = q.id
  const choose = async (award: StatusAward, value: string) => {
    if (busy) return
    const status = value === 'GRANTED' || value === 'SUBMITTED' ? value : null
    setBusy(true)
    const word = status === 'GRANTED' ? t('contactCard.award.granted') : status === 'SUBMITTED' ? t('contactCard.award.submitted') : t('contactCard.award.none')
    await setAwardStatus(
      award,
      [id],
      status,
      () => ({
        title: t('contactCard.award.ask', { award: award.name, call: q.call, status: word }),
        body: t('contactCard.award.body', { award: award.name }),
        confirmLabel: t('contactCard.award.confirm'),
      }),
      onChanged,
    )
    setBusy(false)
  }
  return (
    <fieldset className="contact-card-awards">
      <legend>{t('contactCard.award.head')}</legend>
      {STATUS_AWARDS.map((a) => (
        <label className="logbook-field" key={a.kind}>
          <span>{a.name}</span>
          <select
            className="settings-input"
            value={statusOf(q, a.code) ?? ''}
            disabled={!canSave || busy}
            onChange={(e) => void choose(a, e.target.value)}
          >
            <option value="">{t('contactCard.award.none')}</option>
            <option value="SUBMITTED">{t('contactCard.award.submitted')}</option>
            <option value="GRANTED">{t('contactCard.award.granted')}</option>
          </select>
        </label>
      ))}
    </fieldset>
  )
}
