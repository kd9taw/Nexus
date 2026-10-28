// BULK EDIT — set or clear one or more fields on many contacts at once: the ones ticked in the
// list, or every one the list's search and filters show. The operator's own station details
// (MY_GRIDSQUARE, MY_RIG, OPERATOR, STATION_CALLSIGN, TX_PWR), the propagation mode and satellite,
// and their own park or summit. Never the worked call, and nothing is sent from here. A changed
// STATION_CALLSIGN is the call a contact is uploaded under, so its upload marks are cleared and the
// next upload sends it under the new call, and the LoTW, eQSL and QRZ confirmations matched under
// the old call are cleared (a paper card stays); the preview counts both, before the confirm. A
// filled or cleared STATION_CALLSIGN clears nothing, but a contact naming none is uploaded under the
// call in Settings, so it can send contacts to a service not yet sent them under another call: the
// confirm names those, and the change waits for the operator to tick that they understand.
//
// Preview first: how many contacts it would change and how many are already so. The button then
// names that count, and asks once more, in the dialog, before it changes anything. The change is
// made on exactly the contacts the preview counted (their ids, handed back with the count
// confirmed and the moves ticked for, which the station checks), and a change of the contacts chosen, the filter or a field
// makes the preview stale. The change is one change the toast's Undo takes back.
//
// A park re-tag (MY_POTA_REF set or cleared — a forgotten Start, the wrong park, a two-fer as a
// comma list) is this same edit. Its preview also names the contacts already sent to a service,
// which keep the park they were sent with, and the activation files already exported that the
// change puts out of date: nothing changes a stamp, and nothing is sent.
//
// An award's status (Submitted / Granted, by the award's ADIF Credit code) is set here too, on as
// many contacts as the edit names: the same preview, confirm and one-run Undo.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Field names are ADIF
// tokens, shown as they are; an award status is named by its award.
import { useEffect, useRef, useState } from 'react'
import {
  bulkEditApply,
  bulkEditPreview,
  type BulkPreview,
  type BulkTarget,
  type CleanupField,
  type FieldChange,
} from '../api'
import { t } from '../i18n'
import { withErrorToast } from '../toast'
import type { LogQuery } from '../features/logQuery'
import { changesRefusedHere, sayRun } from '../features/logCleanup'
import { STATUS_AWARDS, statusAwardOf } from '../features/awardStatus'

/** The fields a bulk edit offers, in the order the menu lists them. */
export const BULK_FIELDS: readonly CleanupField[] = [
  'MY_GRIDSQUARE',
  'STATION_CALLSIGN',
  'OPERATOR',
  'MY_RIG',
  'TX_PWR',
  'PROP_MODE',
  'SAT_NAME',
  'MY_POTA_REF',
  'MY_SOTA_REF',
  // Each award's status: Submitted or Granted (one clears the other), or cleared for neither —
  // that award's one ADIF Credit code, and nothing else, moves (features/awardStatus.ts).
  ...STATUS_AWARDS.map((a) => a.field),
]

/** A row's starting value for `field`: an award status starts at Submitted, the others blank. */
const startValue = (field: CleanupField): string => (statusAwardOf(field) ? 'SUBMITTED' : '')

/** A field as the menu names it: its ADIF token, or the award whose status it sets. */
function fieldLabel(field: CleanupField): string {
  const award = statusAwardOf(field)
  return award ? t('logbook.tools.bulk.awardStatus', { award: award.name }) : field
}

interface Row {
  field: CleanupField
  clear: boolean
  value: string
}

export function LogBulkEdit({
  onChanged,
  afar,
  picked,
  query,
  shown,
}: {
  onChanged: () => void
  afar: boolean
  picked: ReadonlySet<string>
  query: LogQuery
  shown: number
}) {
  const [which, setWhich] = useState<'picked' | 'shown'>(picked.size > 0 ? 'picked' : 'shown')
  const [rows, setRows] = useState<Row[]>([{ field: 'MY_GRIDSQUARE', clear: false, value: '' }])
  const [preview, setPreview] = useState<BulkPreview | null>(null)
  const [asking, setAsking] = useState(false)
  const [understood, setUnderstood] = useState(false)
  const [busy, setBusy] = useState(false)
  const inFlight = useRef(false)

  const target = (): BulkTarget => (which === 'picked' ? { ids: [...picked] } : { query })
  const changes = (): FieldChange[] =>
    rows.map((r) => ({ field: r.field, value: r.clear ? null : r.value.trim() }))
  // Anything changed after a preview makes it stale: preview again before the change.
  const edit = (next: Row[]) => { setRows(next); setPreview(null); setAsking(false); setUnderstood(false) }
  const fillable = rows.every((r) => r.clear || r.value.trim() !== '')
  const used = new Set(rows.map((r) => r.field))
  // The contacts the preview was made of changed: preview again.
  const queryKey = JSON.stringify(query)
  const pickedKey = [...picked].join(' ')
  useEffect(() => { setPreview(null); setAsking(false); setUnderstood(false) }, [queryKey, pickedKey])

  const onPreview = async () => {
    setBusy(true)
    const p = await withErrorToast(() => bulkEditPreview(target(), changes()), t('logbook.tools.bulk.previewFailed'))
    setBusy(false)
    setPreview(p)
    setUnderstood(false)
  }

  // Contacts it would send under another call wait for the operator's tick.
  const moves = preview?.moves ?? 0
  const onApply = async () => {
    if (changesRefusedHere() || inFlight.current || !preview || (moves > 0 && !understood)) return
    inFlight.current = true
    setBusy(true)
    setAsking(false)
    const made = await withErrorToast(
      // The moves the operator ticked: the station refuses the change if it now finds others.
      () => bulkEditApply(preview.ids, preview.selected, moves, moves > 0 ? preview.moveTo : null, changes()),
      t('logbook.tools.bulk.failed'),
    )
    inFlight.current = false
    setBusy(false)
    setPreview(null)
    if (!made) return
    sayRun(made, t('logbook.tools.bulk.done', { count: made.changed }), onChanged)
    onChanged()
  }

  return (
    <div className="log-tools-pane">
      <fieldset className="log-tools-row">
        <legend>{t('logbook.tools.bulk.which')}</legend>
        <label>
          <input
            type="radio"
            name="bulk-which"
            checked={which === 'picked'}
            disabled={picked.size === 0}
            onChange={() => { setWhich('picked'); setPreview(null) }}
          />{' '}
          {t('logbook.tools.bulk.picked', { count: picked.size })}
        </label>
        <label>
          <input
            type="radio"
            name="bulk-which"
            checked={which === 'shown'}
            onChange={() => { setWhich('shown'); setPreview(null) }}
          />{' '}
          {t('logbook.tools.bulk.shown', { count: shown })}
        </label>
      </fieldset>
      {rows.map((r, i) => (
        <div key={i} className="log-tools-row">
          <select
            aria-label={t('logbook.tools.bulk.field')}
            value={r.field}
            onChange={(e) => {
              const field = e.target.value as CleanupField
              edit(rows.map((x, k) => (k === i ? { ...x, field, value: startValue(field) } : x)))
            }}
          >
            {BULK_FIELDS.filter((f) => f === r.field || !used.has(f)).map((f) => (
              <option key={f} value={f}>{fieldLabel(f)}</option>
            ))}
          </select>
          <select
            aria-label={t('logbook.tools.bulk.action')}
            value={r.clear ? 'clear' : 'set'}
            onChange={(e) => edit(rows.map((x, k) => (k === i ? { ...x, clear: e.target.value === 'clear' } : x)))}
          >
            <option value="set">{t('logbook.tools.bulk.set')}</option>
            <option value="clear">{t('logbook.tools.bulk.clear')}</option>
          </select>
          {!r.clear && statusAwardOf(r.field) && (
            <select
              aria-label={t('logbook.tools.bulk.value')}
              value={r.value}
              onChange={(e) => edit(rows.map((x, k) => (k === i ? { ...x, value: e.target.value } : x)))}
            >
              <option value="SUBMITTED">{t('contactCard.award.submitted')}</option>
              <option value="GRANTED">{t('contactCard.award.granted')}</option>
            </select>
          )}
          {!r.clear && !statusAwardOf(r.field) && (
            <input
              type="text"
              aria-label={t('logbook.tools.bulk.value')}
              value={r.value}
              onChange={(e) => edit(rows.map((x, k) => (k === i ? { ...x, value: e.target.value } : x)))}
            />
          )}
          {rows.length > 1 && (
            <button type="button" className="export-btn" onClick={() => edit(rows.filter((_, k) => k !== i))}>
              {t('logbook.tools.bulk.remove')}
            </button>
          )}
        </div>
      ))}
      <div className="log-tools-row">
        <button
          type="button"
          className="export-btn"
          disabled={used.size >= BULK_FIELDS.length}
          onClick={() => {
            const next = BULK_FIELDS.find((f) => !used.has(f))
            if (next) edit([...rows, { field: next, clear: false, value: startValue(next) }])
          }}
        >
          {t('logbook.tools.bulk.add')}
        </button>
        <button type="button" className="export-btn" disabled={busy || !fillable} onClick={() => void onPreview()}>
          {t('logbook.tools.bulk.preview')}
        </button>
      </div>
      <p className="qso-detail-bare">{t('logbook.tools.bulk.uploads')}</p>
      {preview && (
        <>
          <p className="qso-detail-bare">
            {t('logbook.tools.bulk.previewed', {
              selected: preview.selected,
              change: preview.change,
              unchanged: preview.unchanged,
              missing: preview.missing,
            })}
          </p>
          {preview.resend > 0 && (
            <p className="qso-detail-bare">{t('logbook.tools.bulk.resend', { count: preview.resend })}</p>
          )}
          {preview.unconfirm > 0 && (
            <p className="qso-detail-bare">{t('logbook.tools.bulk.unconfirm', { count: preview.unconfirm })}</p>
          )}
          {preview.activationUploaded > 0 && (
            <p className="qso-detail-bare">
              {t('logbook.tools.bulk.activationUploaded', { count: preview.activationUploaded })}
            </p>
          )}
          {preview.activationExported > 0 && (
            <p className="qso-detail-bare">
              <strong>
                {t('logbook.tools.bulk.activationExported', {
                  count: preview.activationExported,
                  files: preview.activationFiles.join(', '),
                })}
              </strong>
            </p>
          )}
          <div className="log-tools-row">
            <button
              type="button"
              className="export-btn"
              disabled={busy || afar || preview.change === 0}
              onClick={() => { setAsking(true); setUnderstood(false) }}
            >
              {t('logbook.tools.bulk.apply', { count: preview.change })}
            </button>
            {asking && (
              <span className="log-tools-confirm">
                <span>{t('logbook.tools.bulk.ask', { count: preview.change })}</span>
                {preview.recall > 0 && <span>{t('logbook.tools.bulk.askUndo')}</span>}
                {moves > 0 && (
                  <>
                    <strong>
                      {t('logbook.tools.bulk.moves', {
                        count: moves,
                        to: preview.moveTo ?? '',
                        from: preview.moveFrom.join(', '),
                      })}
                    </strong>
                    <label>
                      <input type="checkbox" checked={understood} onChange={(e) => setUnderstood(e.target.checked)} />{' '}
                      {t('logbook.tools.bulk.movesAck', { to: preview.moveTo ?? '' })}
                    </label>
                  </>
                )}
                <button
                  type="button"
                  className="export-btn danger"
                  disabled={busy || (moves > 0 && !understood)}
                  onClick={() => void onApply()}
                >
                  {t('logbook.tools.bulk.confirm', { count: preview.change })}
                </button>
                <button type="button" className="export-btn" onClick={() => setAsking(false)}>
                  {t('logbook.tools.cancel')}
                </button>
              </span>
            )}
          </div>
        </>
      )}
    </div>
  )
}
