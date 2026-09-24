import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { postSpot } from '../api'
import { pushToast } from '../toast'
import { t } from '../i18n'
import { parseOperatorNumber } from '../numInput'
import { confirmDialog } from '../confirm'
import { useStationControl } from '../stationAccess'
import { sendLogChange, useLogChange, useRemoteOperations } from '../remote-web/operations'

// Compose + post a DX-cluster spot. The backend `post_spot` command validates the callsign,
// checks a cluster is connected, and sanitizes the line — this is just the reviewable popup the
// operator asked for: call + freq + comment, editable, one button to send. Reuses the shared
// `.logconfirm` modal styling.
//
// ⛔ FROM A BROWSER the same press rides the station's `spot` log change, which needs STATION
// CONTROL — it posts publicly from the station's own cluster login, so it is not a logging-grant
// action. Without that grant the Spot button is dead, and every press is confirmed first, naming
// the call and the frequency that will go out, exactly as the self-spot press is. Nothing is
// remembered between presses.
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
  // A browser posts through the station; the station's own cluster login is what goes out.
  const local = useStationControl(), client = useRemoteOperations(), remoteSpot = useLogChange('postSpot')

  // THE KEYBOARD GOES BACK TO WHERE THE DIALOG WAS OPENED FROM when it closes — cancelled, escaped
  // or posted. It was left on the page itself; and a Remote post asks first (confirmDialog), so the
  // question's own control is gone with this dialog and only this dialog knows where to return.
  // The call box is focused here rather than by `autoFocus`, which would move the keyboard before
  // the control that opened the dialog could be noted.
  const callRef = useRef<HTMLInputElement>(null)
  useLayoutEffect(() => {
    if (!open) return
    const active = document.activeElement
    const opener = active instanceof HTMLElement && active !== document.body ? active : null
    callRef.current?.focus()
    return () => {
      const now = document.activeElement
      if (now && now !== document.body && now.isConnected) return // the keyboard is somewhere already
      if (opener?.isConnected) opener.focus({ preventScroll: true })
    }
  }, [open])

  // Re-seed from the current call/dial each time the dialog opens.
  useEffect(() => {
    if (open) {
      setCall(initialCall)
      setFreq(freqMhz > 0 ? freqMhz.toFixed(3) : '')
      setComment(defaultComment)
    }
  }, [open, initialCall, freqMhz, defaultComment])

  if (!open) return null

  // `parseFloat('14,074')` is 14 — a comma-decimal operator (Greek-Windows report, 2026-08)
  // spotted the whole cluster onto 14 MHz, out of band, and the Spot button looked perfectly
  // happy about it. The `canSpot` guard below is what turns an unreadable entry into a
  // greyed-out button instead of a wrong spot in front of everyone.
  const freqNum = parseOperatorNumber(freq)
  const canSpot = call.trim().length > 0 && Number.isFinite(freqNum) && freqNum > 0 && (local || (!!client && remoteSpot))

  const submit = async () => {
    if (!canSpot || busy) return
    setBusy(true)
    try {
      const c = call.trim().toUpperCase()
      if (local) {
        await postSpot(freqNum, c, comment.trim())
      } else {
        // A public post: ask on every press, never remember the answer, and show exactly the
        // callsign and frequency that will be sent.
        if (!client || !(await confirmDialog({
          title: t('spots.post.confirm.title', { call: c }),
          confirmLabel: t('spots.post.confirm.post'),
          body: t('spots.post.confirm.body', { call: c, freq: freqNum.toFixed(4) }),
        }))) return
        // sendLogChange says what happened — a station with no cluster node connected, a refused
        // callsign, a request that never left. Only an applied one is a posted spot.
        const outcome = await sendLogChange(client, { kind: 'spot', call: c, freqMhz: freqNum, comment: comment.trim() })
        if (outcome?.outcome !== 'applied') return
      }
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
            ref={callRef}
            className="settings-input"
            value={call}
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
