import { useEffect, useState } from 'react'
import { postSpot } from '../api'
import { pushToast } from '../toast'
import { t } from '../i18n'

// Compose + post a DX-cluster spot. The backend `post_spot` command validates the callsign,
// checks a cluster is connected, and sanitizes the line — this is just the reviewable popup the
// operator asked for: call + freq + comment, editable, one button to send. Reuses the shared
// `.logconfirm` modal styling.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The callsign and the
// frequency the operator types are wire values that go to the cluster untouched — the dialog
// never formats either, and `.toFixed(3)` is the only thing it does to the dial.
export function SpotDialog({
  open,
  onClose,
  initialCall,
  freqMhz,
  defaultComment,
}: {
  open: boolean
  onClose: () => void
  initialCall: string
  freqMhz: number
  defaultComment: string
}) {
  const [call, setCall] = useState(initialCall)
  const [freq, setFreq] = useState('')
  const [comment, setComment] = useState(defaultComment)
  const [busy, setBusy] = useState(false)

  // Re-seed from the current call/dial each time the dialog opens.
  useEffect(() => {
    if (open) {
      setCall(initialCall)
      setFreq(freqMhz > 0 ? freqMhz.toFixed(3) : '')
      setComment(defaultComment)
    }
  }, [open, initialCall, freqMhz, defaultComment])

  if (!open) return null

  const freqNum = parseFloat(freq)
  const canSpot = call.trim().length > 0 && Number.isFinite(freqNum) && freqNum > 0

  const submit = async () => {
    if (!canSpot || busy) return
    setBusy(true)
    try {
      const c = call.trim().toUpperCase()
      await postSpot(freqNum, c, comment.trim())
      pushToast(t('spots.post.done', { call: c }), 'success', 2500)
      onClose()
    } catch (e) {
      pushToast(typeof e === 'string' ? e : t('spots.post.failed'), 'error', 3500)
    } finally {
      setBusy(false)
    }
  }

  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') void submit()
    if (e.key === 'Escape') onClose()
  }

  return (
    <div
      className="logconfirm-backdrop"
      role="dialog"
      aria-modal="true"
      aria-label={t('spots.post.aria')}
      onClick={onClose}
    >
      <div className="logconfirm spot-dialog" onClick={(e) => e.stopPropagation()}>
        <div className="logconfirm-head">
          <h2>{t('spots.post.title')}</h2>
        </div>
        <label className="settings-field">
          <span className="settings-label">{t('spots.post.call.label')}</span>
          <input
            className="settings-input"
            value={call}
            autoFocus
            onChange={(e) => setCall(e.target.value)}
            onKeyDown={onKey}
            autoComplete="off"
            spellCheck={false}
          />
        </label>
        <label className="settings-field">
          <span className="settings-label">{t('spots.post.freq.label')}</span>
          <input
            className="settings-input"
            value={freq}
            inputMode="decimal"
            onChange={(e) => setFreq(e.target.value)}
            onKeyDown={onKey}
            autoComplete="off"
          />
        </label>
        <label className="settings-field">
          <span className="settings-label">{t('spots.post.comment.label')}</span>
          <input
            className="settings-input"
            value={comment}
            maxLength={30}
            placeholder={t('spots.post.comment.placeholder')}
            onChange={(e) => setComment(e.target.value)}
            onKeyDown={onKey}
            autoComplete="off"
          />
        </label>
        <div className="logconfirm-actions">
          <button type="button" className="logconfirm-discard" onClick={onClose}>
            {t('spots.post.cancel')}
          </button>
          <button type="button" className="logconfirm-log" onClick={submit} disabled={!canSpot || busy}>
            {busy ? t('spots.post.busy') : t('spots.post.submit')}
          </button>
        </div>
      </div>
    </div>
  )
}
