// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog; a hardcoded one fails CI. What does NOT: the band and mode names
// in the sub-heading and the `—` placeholders, which are data and a glyph, not prose.

import { useState } from 'react'
import type { LoggedQso } from '../types'
import { t } from '../i18n'
import { FtStopControl } from './FtStopControl'
import { upperTyped } from '../typedInput'

interface Props {
  /** The completed contact awaiting confirm-before-log. */
  record: LoggedQso
  /** Log the (possibly edited) record. */
  onConfirm: (record: LoggedQso) => void
  /** Discard the contact without logging. */
  onDiscard: () => void
  /** How many MORE completed contacts are held behind this one (operator ruling 2026-09-19:
   *  queue them). 0 = this is the only one, and nothing is said about a queue. */
  waiting?: number
  /** How many contacts the queue logged WITHOUT review since this popup was last answered.
   *  0 = nothing to report, and the warning is absent. */
  autoLogged?: number
  onStop?: () => void
}

/** WSJT-X "Prompt me to log QSO" — a small confirm popup shown when a QSO
 * completes and the operator has asked to review before logging. Pre-fills the
 * exchanged details; the call/grid/reports and the comment stay editable. */
export function LogConfirm({ record, onConfirm, onDiscard, waiting = 0, autoLogged = 0, onStop }: Props) {
  const [call, setCall] = useState(record.call)
  const [grid, setGrid] = useState(record.grid ?? '')
  const [rstSent, setRstSent] = useState(record.rstSent?.toString() ?? '')
  const [rstRcvd, setRstRcvd] = useState(record.rstRcvd?.toString() ?? '')
  // #371: the contact's COMMENT, as held (our dB-reports one when that option is on), so an
  // untouched field logs exactly what it would have; the engine logs what is sent here.
  const [comment, setComment] = useState(record.comment ?? '')

  // RST is a free string now (CW "599" / phone "59" / digital "-12"); just trim.
  const parseRst = (v: string): string | null => {
    const t = v.trim()
    return t === '' ? null : t
  }

  const confirm = () => {
    if (!call.trim()) return
    onConfirm({
      ...record,
      call: call.trim().toUpperCase(),
      grid: grid.trim() ? grid.trim().toUpperCase() : null,
      rstSent: parseRst(rstSent),
      rstRcvd: parseRst(rstRcvd),
      comment: comment.trim() || null,
    })
  }

  return (
    <div className="logconfirm-backdrop" role="dialog" aria-modal="true" aria-label={t('logPrompt.aria')}>
      <div className="logconfirm">
        <div className="logconfirm-head">
          <h2>{t('logPrompt.title')}</h2>
          <span className="logconfirm-sub">
            {record.band} · {record.mode}
          </span>
        </div>

        {/* The queue behind this contact. An operator who logs one and sees another appear
            should have been told it was coming. */}
        {waiting > 0 && (
          <p className="logconfirm-waiting" role="status">
            {t('logPrompt.waiting', { count: waiting })}
          </p>
        )}

        {/* A contact the queue logged at its cap, unreviewed. It is in the log, the connectors
            will upload it and LoTW will carry it under the operator's certificate, so this is a
            WARNING, not a note — and it belongs here rather than only in the Connections log,
            because this popup is where the operator comes back to. */}
        {autoLogged > 0 && (
          <p className="logconfirm-autologged" role="alert">
            {t('logPrompt.autoLogged', { count: autoLogged })}
          </p>
        )}

        <div className="logconfirm-grid">
          <label>
            <span>{t('logPrompt.call.label')}</span>
            <input
              className="mono"
              value={call}
              autoFocus
              onChange={(e) => setCall(upperTyped(e.target))}
            />
          </label>
          <label>
            <span>{t('logPrompt.grid.label')}</span>
            <input
              className="mono"
              value={grid}
              maxLength={6}
              placeholder="—"
              onChange={(e) => setGrid(upperTyped(e.target))}
            />
          </label>
          <label>
            <span>{t('logPrompt.rstSent.label')}</span>
            <input
              className="mono"
              value={rstSent}
              placeholder="—"
              onChange={(e) => setRstSent(e.target.value)}
            />
          </label>
          <label>
            <span>{t('logPrompt.rstRcvd.label')}</span>
            <input
              className="mono"
              value={rstRcvd}
              placeholder="—"
              onChange={(e) => setRstRcvd(e.target.value)}
            />
          </label>
          <label className="logconfirm-comment">
            <span>{t('logPrompt.comment.label')}</span>
            <input
              value={comment}
              placeholder="—"
              onChange={(e) => setComment(e.target.value)}
            />
          </label>
        </div>

        <div className="logconfirm-actions">
          {onStop && <FtStopControl onHaltTx={onStop} />}
          <button type="button" className="logconfirm-discard" onClick={onDiscard}>
            {t('logPrompt.discard')}
          </button>
          <button type="button" className="logconfirm-log" onClick={confirm} disabled={!call.trim()}>
            {t('logPrompt.log')}
          </button>
        </div>
      </div>
    </div>
  )
}
