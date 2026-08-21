// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog; a hardcoded one fails CI. What does NOT: the band and mode names
// in the sub-heading and the `—` placeholders, which are data and a glyph, not prose.

import { useState } from 'react'
import type { LoggedQso } from '../types'
import { t } from '../i18n'

interface Props {
  /** The completed contact awaiting confirm-before-log. */
  record: LoggedQso
  /** Log the (possibly edited) record. */
  onConfirm: (record: LoggedQso) => void
  /** Discard the contact without logging. */
  onDiscard: () => void
}

/** WSJT-X "Prompt me to log QSO" — a small confirm popup shown when a QSO
 * completes and the operator has asked to review before logging. Pre-fills the
 * exchanged details; the call/grid/reports stay editable. */
export function LogConfirm({ record, onConfirm, onDiscard }: Props) {
  const [call, setCall] = useState(record.call)
  const [grid, setGrid] = useState(record.grid ?? '')
  const [rstSent, setRstSent] = useState(record.rstSent?.toString() ?? '')
  const [rstRcvd, setRstRcvd] = useState(record.rstRcvd?.toString() ?? '')

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

        <div className="logconfirm-grid">
          <label>
            <span>{t('logPrompt.call.label')}</span>
            <input
              className="mono"
              value={call}
              autoFocus
              onChange={(e) => setCall(e.target.value.toUpperCase())}
            />
          </label>
          <label>
            <span>{t('logPrompt.grid.label')}</span>
            <input
              className="mono"
              value={grid}
              maxLength={6}
              placeholder="—"
              onChange={(e) => setGrid(e.target.value.toUpperCase())}
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
        </div>

        <div className="logconfirm-actions">
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
