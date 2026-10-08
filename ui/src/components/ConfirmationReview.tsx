import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import {
  applyConfirmationCheck,
  cancelConfirmationCheck,
  confirmationCheck,
  type ConfirmationApplied,
  type ConfirmationCheck,
  type ConfirmationLine,
} from '../api'
import { t } from '../i18n'
import { pushToast, withErrorToast } from '../toast'
import { utcDate } from '../features/utcLog'
import { fmtUtc } from '../features/logQuery'

// "CHECK CONFIRMATIONS" — the contacts holding a LoTW confirmation, or LoTW's upload mark, that
// LoTW's own records give another contact, or none: up to 1.17.0 a confirmation took the OLDEST
// contact with its station, band and mode on its UTC day (#400), and so did an upload mark. The
// station downloads LoTW's whole history when the operator presses Check, decides each line
// (`tempo_core::reconcile::check`) and holds the download for this check's Apply.
//
// Built as Check park states is (ParkStateReview), with its classes and focus rules. What differs,
// on purpose: nothing downloads until Check, since a whole history takes minutes; a line starts
// ticked only when the station says the evidence decides it, and an unticked one says why on
// hover; each line carries its evidence; Apply also adds the confirmations LoTW holds for contacts
// lacking them; and the toast names the file holding the changed contacts as they were, which
// Logbook ▸ Import ADIF puts back. A click outside the box does not close it, because that would
// throw the download away. Cancel and Escape tell the station to drop the check it holds. It
// never opens or runs by itself.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Calls, bands, modes,
// LoTW and the credit codes are invariant tokens, shown as they are.

/** The service the lines are about, an invariant token. eQSL and QRZ join when their checks do. */
const LOTW = 'LoTW'

/** The done toast names the file that puts the changes back, so it stays long enough to read. */
const DONE_TOAST_MS = 20_000

const UNTICKED: Record<NonNullable<ConfirmationLine['unticked']>, () => string> = {
  dateOnly: () => t('logbook.confirmations.unticked.dateOnly'),
  notReplayed: () => t('logbook.confirmations.unticked.notReplayed'),
  tie: () => t('logbook.confirmations.unticked.tie'),
  orphan: () => t('logbook.confirmations.unticked.orphan'),
  insideWindow: () => t('logbook.confirmations.unticked.insideWindow'),
  notToTheMinute: () => t('logbook.confirmations.unticked.notToTheMinute'),
}

/** A line is one contact's confirmation or its upload mark: a contact can have both. */
const keyOf = (l: ConfirmationLine) => `${l.id} ${l.mark}`

/** A time a reason names: HH:MM UTC, with its date when that is not the line's own. */
function timeOf(unix: number, l: ConfirmationLine): string {
  return fmtUtc(unix).slice(utcDate(unix) === utcDate(l.whenUnix) ? 11 : 0, 16)
}

/** What the evidence says, in a phrase. A date-only line names no time, since one it rests on is
 *  not known. */
function reasonOf(l: ConfirmationLine): string {
  if (l.unticked === 'dateOnly') return t('logbook.confirmations.why.dateOnly')
  const upload = l.mark === 'lotwUpload'
  switch (l.class) {
    case 'moved': {
      const time = timeOf(l.siblingUnix ?? l.rowUnix, l)
      return upload
        ? t('logbook.confirmations.why.uploadMoved', { time })
        : t('logbook.confirmations.why.moved', { time })
    }
    case 'contradicted':
      return t('logbook.confirmations.why.contradicted')
    case 'orphan': {
      const time = timeOf(l.rowUnix, l)
      return upload
        ? t('logbook.confirmations.why.uploadOrphan', { time })
        : t('logbook.confirmations.why.orphan', { time })
    }
  }
}

/** Why a line starts unticked; nothing for a decisive one. */
function untickedTitle(l: ConfirmationLine): string | undefined {
  if (!l.unticked) return undefined
  if (l.unticked === 'notReplayed' && l.mark === 'lotwUpload')
    return t('logbook.confirmations.unticked.uploadNotReplayed')
  return UNTICKED[l.unticked]()
}

/** What Apply made, and where the contacts as they were are. */
function doneMessage(made: ConfirmationApplied): string {
  const said: string[] = []
  if (made.confirmations > 0)
    said.push(t('logbook.confirmations.done.confirmations', { count: made.confirmations }))
  if (made.uploads > 0) said.push(t('logbook.confirmations.done.uploads', { count: made.uploads }))
  if (made.newlyConfirmed > 0)
    said.push(t('logbook.confirmations.done.gained', { count: made.newlyConfirmed }))
  const skipped = made.ticked - made.confirmations - made.uploads
  if (skipped > 0) said.push(t('logbook.confirmations.done.skipped', { count: skipped }))
  if (made.beforeFile) said.push(t('logbook.confirmations.done.beforeFile', { path: made.beforeFile }))
  return said.length > 0 ? said.join(' ') : t('logbook.confirmations.done.nothing')
}

export function ConfirmationReview({
  open,
  onClose,
  onApplied,
}: {
  open: boolean
  onClose: () => void
  onApplied: () => void
}) {
  const [found, setFound] = useState<ConfirmationCheck | null>(null)
  const [checking, setChecking] = useState(false)
  const [ticked, setTicked] = useState<ReadonlySet<string>>(new Set())
  const [busy, setBusy] = useState(false)
  // The newest check this dialog started. One that lands after a newer one, or after the dialog
  // closed, is dropped as it lands, its failure too: the station answers a cancelled check with
  // an error, and it must not pop up minutes after Cancel.
  const run = useRef(0)
  // A check of this dialog's that the station may still hold. Closing drops it at the station; a
  // dialog that never checked leaves alone a check another window holds.
  const holds = useRef(false)

  // The keyboard goes into the dialog when it opens and back to where it was opened from when it
  // closes, applied or cancelled (SpotDialog's rule).
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

  const close = () => {
    run.current++
    if (holds.current) {
      holds.current = false
      void cancelConfirmationCheck()
    }
    setFound(null)
    setChecking(false)
    setTicked(new Set())
    setBusy(false)
    onClose()
  }

  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') close()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })

  if (!open) return null

  const toggle = (key: string) =>
    setTicked((was) => {
      const next = new Set(was)
      if (next.has(key)) next.delete(key)
      else next.add(key)
      return next
    })

  const check = async () => {
    const mine = ++run.current
    holds.current = true
    // Check disables while the download runs; the keyboard stays in the dialog.
    boxRef.current?.focus({ preventScroll: true })
    setChecking(true)
    try {
      const got = await confirmationCheck()
      if (run.current !== mine) return
      setFound(got)
      setTicked(new Set(got.lines.filter((l) => l.decisive).map(keyOf)))
    } catch (err) {
      if (run.current !== mine) return
      holds.current = false
      const detail = err instanceof Error ? err.message : typeof err === 'string' ? err : ''
      const failed = t('logbook.confirmations.failed')
      pushToast(detail ? `${failed}: ${detail}` : failed, 'error')
    } finally {
      if (run.current === mine) setChecking(false)
    }
  }

  const chosen = found ? found.lines.filter((l) => ticked.has(keyOf(l))) : []
  // The contacts Apply changes: each ticked one once, and those gaining LoTW's confirmation.
  const changing = new Set(chosen.map((l) => l.id)).size + (found?.gains ?? 0)

  const apply = async () => {
    if (!found) return
    boxRef.current?.focus({ preventScroll: true })
    setBusy(true)
    // The station takes the check it holds for this Apply, whatever comes of it.
    holds.current = false
    const made = await withErrorToast(
      () => applyConfirmationCheck(found.session, chosen.map((l) => ({ id: l.id, mark: l.mark }))),
      t('logbook.confirmations.applyFailed'),
    )
    setBusy(false)
    if (made == null) {
      // What is listed went with the check: it can only be checked again.
      setFound(null)
      return
    }
    // A change to the log, so a notice: it pops up even with pop-ups off (toast.ts).
    pushToast(doneMessage(made), 'info', DONE_TOAST_MS)
    onApplied()
    close()
  }

  // A contact's upload is said once, on its last line.
  const lastLineOf = new Map<string, number>()
  found?.lines.forEach((l, i) => lastLineOf.set(l.id, i))

  return (
    <div
      className="logconfirm-backdrop"
      role="dialog"
      aria-modal="true"
      aria-label={t('logbook.confirmations.title')}
    >
      <div ref={boxRef} tabIndex={-1} className="logconfirm park-review">
        <div className="logconfirm-head">
          <h2>{t('logbook.confirmations.title')}</h2>
        </div>
        <p className="park-review-intro">{t('logbook.confirmations.intro')}</p>
        {found == null ? (
          checking && (
            <p className="park-review-note" role="status">
              {t('logbook.confirmations.checking')}
            </p>
          )
        ) : (
          <>
            {found.lines.length === 0 ? (
              <p className="park-review-note">{t('logbook.confirmations.none')}</p>
            ) : (
              <>
                <h3 className="confirm-review-group">{LOTW}</h3>
                <ul className="park-review-list">
                  {found.lines.map((l, i) => {
                    const key = keyOf(l)
                    const on = ticked.has(key)
                    return (
                      <li key={key} className={`park-review-row${on ? ' ticked' : ''}`}>
                        <label title={untickedTitle(l)}>
                          <input type="checkbox" checked={on} onChange={() => toggle(key)} />
                          <span className="mono park-review-call">{l.call}</span>
                          <span className="mono park-review-when">
                            {l.timeKnown ? fmtUtc(l.whenUnix) : utcDate(l.whenUnix)}
                          </span>
                          <span className="park-review-band">{`${l.band} ${l.mode}`}</span>
                          <span className="mono park-review-change">
                            {l.mark === 'lotwUpload'
                              ? t('logbook.confirmations.changeUpload')
                              : t('logbook.confirmations.change')}
                          </span>
                          <span className="confirm-review-why">
                            <span>{reasonOf(l)}</span>
                            {l.cardHeld && (
                              <span className="confirm-review-badge">{t('logbook.confirmations.badge.card')}</span>
                            )}
                            {l.removeGranted.length > 0 && (
                              <span className="confirm-review-badge">
                                {t('logbook.confirmations.badge.credit', { codes: l.removeGranted.join(', ') })}
                              </span>
                            )}
                            {l.removeSubmitted.length > 0 && (
                              <span className="confirm-review-badge">
                                {t('logbook.confirmations.badge.submitted', { codes: l.removeSubmitted.join(', ') })}
                              </span>
                            )}
                            {l.owedAfter && lastLineOf.get(l.id) === i && (
                              <span className="confirm-review-badge" title={t('logbook.confirmations.badge.owedTitle')}>
                                {t('logbook.confirmations.badge.owed')}
                              </span>
                            )}
                          </span>
                        </label>
                      </li>
                    )
                  })}
                </ul>
              </>
            )}
            <div className="confirm-review-counts">
              {found.gains > 0 && (
                <p className="park-review-note">{t('logbook.confirmations.gains', { count: found.gains })}</p>
              )}
              {found.unreached > 0 && (
                <p className="park-review-note">{t('logbook.confirmations.unreached', { count: found.unreached })}</p>
              )}
              {found.outOfScope > 0 && (
                <p className="park-review-note">{t('logbook.confirmations.outOfScope', { count: found.outOfScope })}</p>
              )}
              {found.uploadsUnreached > 0 && (
                <p className="park-review-note">
                  {t('logbook.confirmations.uploadsUnreached', { count: found.uploadsUnreached })}
                </p>
              )}
              {found.uploadsOutOfScope > 0 && (
                <p className="park-review-note">
                  {t('logbook.confirmations.uploadsOutOfScope', { count: found.uploadsOutOfScope })}
                </p>
              )}
            </div>
          </>
        )}
        <div className="logconfirm-actions">
          <button type="button" className="logconfirm-discard" onClick={close}>
            {t('logbook.confirmations.cancel')}
          </button>
          {found == null ? (
            <button type="button" className="logconfirm-log" disabled={checking} onClick={() => void check()}>
              {t('logbook.confirmations.check')}
            </button>
          ) : (
            <button
              type="button"
              className="logconfirm-log"
              disabled={busy || changing === 0}
              onClick={() => void apply()}
            >
              {t('logbook.confirmations.apply', { count: changing })}
            </button>
          )}
        </div>
      </div>
    </div>
  )
}
