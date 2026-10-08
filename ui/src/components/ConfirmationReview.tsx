import { Fragment, useEffect, useLayoutEffect, useRef, useState } from 'react'
import {
  applyConfirmationCheck,
  cancelConfirmationCheck,
  confirmationCheck,
  startConfirmationCheck,
  type CheckedService,
  type ConfirmationApplied,
  type ConfirmationCheck,
  type ConfirmationLine,
} from '../api'
import { t } from '../i18n'
import { pushToast, withErrorToast } from '../toast'
import { utcDate } from '../features/utcLog'
import { fmtUtc } from '../features/logQuery'

// "CHECK CONFIRMATIONS" — the contacts holding a LoTW, eQSL or QRZ confirmation, or LoTW's upload
// mark, that the service's own records give another contact, or none: up to 1.17.0 a
// confirmation took the OLDEST contact with its station, band and mode on its UTC day (#400), and
// so did an upload mark. When the operator presses Check, the station starts a check and
// downloads each service's whole history (each one that is set up; one that is not says why),
// decides each line (`tempo_core::reconcile::check`) and holds the downloads for this check's
// Apply.
//
// Built as Check park states is (ParkStateReview), with its classes and focus rules. What differs,
// on purpose: nothing downloads until Check, since a whole history takes minutes; each service is
// listed under its own heading as its check lands; a line starts ticked only when the station says
// the evidence decides it, and an unticked one says why on hover; each line carries its evidence;
// Apply also adds the confirmations each service holds for contacts lacking them; and the toast
// names the file holding the changed contacts as they were, which Logbook ▸ Import ADIF puts back.
// A click outside the box does not close it, because that would throw the downloads away. Cancel
// and Escape tell the station to drop the check it holds. It never opens or runs by itself.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Calls, bands, modes,
// LoTW, eQSL, QRZ and the credit codes are invariant tokens, shown as they are.

/** The services checked, in the order they are listed. */
const SERVICES: readonly CheckedService[] = ['lotw', 'eqsl', 'qrz']

/** Each service's name, an invariant token. */
const SERVICE_NAME: Record<CheckedService, string> = { lotw: 'LoTW', eqsl: 'eQSL', qrz: 'QRZ' }

/** The done toast names the file that puts the changes back, so it stays long enough to read. */
const DONE_TOAST_MS = 20_000

/** Where one service's check stands. */
type ServiceCheck =
  | { state: 'checking' }
  | { state: 'checked'; check: ConfirmationCheck }
  | { state: 'failed'; why: string }

/** The service a line is about: an upload mark's is LoTW. */
const serviceOf = (l: ConfirmationLine): CheckedService => (l.mark === 'lotwUpload' ? 'lotw' : l.mark)

/** A line is one contact's confirmation from one service, or its upload mark: a contact can have
 *  several. */
const keyOf = (l: ConfirmationLine) => `${l.id} ${l.mark}`

/** The station's own words for a failure, if it gave any. */
const detailOf = (err: unknown) => (err instanceof Error ? err.message : typeof err === 'string' ? err : '')

/** A time a reason names: HH:MM UTC, with its date when that is not the line's own. */
function timeOf(unix: number, l: ConfirmationLine): string {
  return fmtUtc(unix).slice(utcDate(unix) === utcDate(l.whenUnix) ? 11 : 0, 16)
}

/** What the evidence says, in a phrase. A date-only line names no time, since one it rests on is
 *  not known. */
function reasonOf(l: ConfirmationLine): string {
  if (l.unticked === 'dateOnly') return t('logbook.confirmations.why.dateOnly')
  const service = SERVICE_NAME[serviceOf(l)]
  const upload = l.mark === 'lotwUpload'
  switch (l.class) {
    case 'moved': {
      const time = timeOf(l.siblingUnix ?? l.rowUnix, l)
      return upload
        ? t('logbook.confirmations.why.uploadMoved', { time })
        : t('logbook.confirmations.why.moved', { service, time })
    }
    case 'contradicted':
      return t('logbook.confirmations.why.contradicted', { service })
    case 'orphan': {
      const time = timeOf(l.rowUnix, l)
      return upload
        ? t('logbook.confirmations.why.uploadOrphan', { time })
        : t('logbook.confirmations.why.orphan', { service, time })
    }
  }
}

/** Why a line starts unticked; nothing for a decisive one. A card carries its sender's time, and
 *  the other ways a mark arrives differ by service, so those two say which. */
function untickedTitle(l: ConfirmationLine): string | undefined {
  const service = SERVICE_NAME[serviceOf(l)]
  switch (l.unticked) {
    case null:
      return undefined
    case 'dateOnly':
      return t('logbook.confirmations.unticked.dateOnly', { service })
    case 'notReplayed':
      switch (l.mark) {
        case 'lotw':
          return t('logbook.confirmations.unticked.notReplayed')
        case 'lotwUpload':
          return t('logbook.confirmations.unticked.uploadNotReplayed')
        case 'eqsl':
          return t('logbook.confirmations.unticked.notReplayedEqsl')
        case 'qrz':
          return t('logbook.confirmations.unticked.notReplayedQrz')
      }
      break
    case 'tie':
      return t('logbook.confirmations.unticked.tie', { service })
    case 'orphan':
      return t('logbook.confirmations.unticked.orphan', { service })
    case 'insideWindow':
      return l.mark === 'eqsl'
        ? t('logbook.confirmations.unticked.insideWindowEqsl')
        : t('logbook.confirmations.unticked.insideWindow', { service })
    case 'notToTheMinute':
      return t('logbook.confirmations.unticked.notToTheMinute', { service })
  }
}

/** What Apply made, service by service, and where the contacts as they were are. */
function doneMessage(made: ConfirmationApplied): string {
  const said: string[] = []
  for (const s of made.services)
    if (s.confirmations > 0)
      said.push(
        t('logbook.confirmations.done.confirmations', { count: s.confirmations, service: SERVICE_NAME[s.service] }),
      )
  if (made.uploads > 0) said.push(t('logbook.confirmations.done.uploads', { count: made.uploads }))
  for (const s of made.services)
    if (s.gained > 0)
      said.push(t('logbook.confirmations.done.gained', { count: s.gained, service: SERVICE_NAME[s.service] }))
  const applied = made.services.reduce((n, s) => n + s.confirmations, made.uploads)
  const skipped = made.ticked - applied
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
  // Each service's check, from the moment the station starts one; null before Check is pressed.
  const [found, setFound] = useState<Record<CheckedService, ServiceCheck> | null>(null)
  const [session, setSession] = useState<number | null>(null)
  const [checking, setChecking] = useState(false)
  const [ticked, setTicked] = useState<ReadonlySet<string>>(new Set())
  const [busy, setBusy] = useState(false)
  // The newest check this dialog started. A service that lands after a newer check, or after the
  // dialog closed, is dropped as it lands, its failure too: the station answers a cancelled check
  // with an error, and it must not pop up minutes after Cancel.
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
    setSession(null)
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
    // Check disables while the downloads run; the keyboard stays in the dialog.
    boxRef.current?.focus({ preventScroll: true })
    setChecking(true)
    setTicked(new Set())
    let started: number
    try {
      started = await startConfirmationCheck()
    } catch (err) {
      if (run.current !== mine) return
      holds.current = false
      setChecking(false)
      const detail = detailOf(err)
      const failed = t('logbook.confirmations.failed')
      pushToast(detail ? `${failed}: ${detail}` : failed, 'error')
      return
    }
    if (run.current !== mine) return
    setSession(started)
    setFound(
      Object.fromEntries(SERVICES.map((s) => [s, { state: 'checking' }])) as Record<CheckedService, ServiceCheck>,
    )
    // Every service at once, each listed as it lands: LoTW's whole history can take minutes.
    await Promise.all(
      SERVICES.map(async (service) => {
        let now: ServiceCheck
        try {
          const got = await confirmationCheck(started, service)
          now = { state: 'checked', check: got }
          if (run.current === mine)
            setTicked((was) => new Set([...was, ...got.lines.filter((l) => l.decisive).map(keyOf)]))
        } catch (err) {
          now = { state: 'failed', why: detailOf(err) }
        }
        if (run.current === mine) setFound((was) => was && { ...was, [service]: now })
      }),
    )
    if (run.current === mine) setChecking(false)
  }

  const checks = found
    ? SERVICES.flatMap((s) => {
        const f = found[s]
        return f.state === 'checked' ? [f.check] : []
      })
    : []
  const lines = checks.flatMap((c) => c.lines)
  const chosen = lines.filter((l) => ticked.has(keyOf(l)))
  // The contacts Apply changes, each once: those ticked, and those gaining a service's confirmation.
  const changing = new Set([...chosen.map((l) => l.id), ...checks.flatMap((c) => c.gainIds)]).size

  const apply = async () => {
    if (session == null) return
    boxRef.current?.focus({ preventScroll: true })
    setBusy(true)
    // The station takes the check it holds for this Apply, whatever comes of it.
    holds.current = false
    const made = await withErrorToast(
      () => applyConfirmationCheck(session, chosen.map((l) => ({ id: l.id, mark: l.mark }))),
      t('logbook.confirmations.applyFailed'),
    )
    setBusy(false)
    if (made == null) {
      // What is listed went with the check: it can only be checked again.
      setFound(null)
      setSession(null)
      return
    }
    // A change to the log, so a notice: it pops up even with pop-ups off (toast.ts).
    pushToast(doneMessage(made), 'info', DONE_TOAST_MS)
    onApplied()
    close()
  }

  // A contact's upload is said once, on the last line that says it.
  const owedOn = new Map<string, string>()
  for (const l of lines) if (l.owedAfter) owedOn.set(l.id, keyOf(l))

  const row = (l: ConfirmationLine) => {
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
              : t('logbook.confirmations.change', { service: SERVICE_NAME[l.mark] })}
          </span>
          <span className="confirm-review-why">
            <span>{reasonOf(l)}</span>
            {/* Only LoTW's confirmation gives award credit, so only its line can keep it by a card. */}
            {l.cardHeld && l.mark === 'lotw' && (
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
            {owedOn.get(l.id) === key && (
              <span className="confirm-review-badge" title={t('logbook.confirmations.badge.owedTitle')}>
                {t('logbook.confirmations.badge.owed')}
              </span>
            )}
          </span>
        </label>
      </li>
    )
  }

  const listed = (service: CheckedService, got: ConfirmationCheck) => {
    const name = SERVICE_NAME[service]
    return (
      <>
        {got.lines.length === 0 ? (
          <p className="park-review-note">
            {service === 'lotw'
              ? t('logbook.confirmations.none')
              : t('logbook.confirmations.noneService', { service: name })}
          </p>
        ) : (
          <ul className="park-review-list">{got.lines.map(row)}</ul>
        )}
        <div className="confirm-review-counts">
          {got.gainIds.length > 0 && (
            <p className="park-review-note">
              {t('logbook.confirmations.gains', { count: got.gainIds.length, service: name })}
            </p>
          )}
          {got.unreached > 0 && (
            <p className="park-review-note">
              {t('logbook.confirmations.unreached', { count: got.unreached, service: name })}
            </p>
          )}
          {got.outOfScope > 0 && (
            <p className="park-review-note">
              {t('logbook.confirmations.outOfScope', { count: got.outOfScope, service: name })}
            </p>
          )}
          {got.uploadsUnreached > 0 && (
            <p className="park-review-note">
              {t('logbook.confirmations.uploadsUnreached', { count: got.uploadsUnreached })}
            </p>
          )}
          {got.uploadsOutOfScope > 0 && (
            <p className="park-review-note">
              {t('logbook.confirmations.uploadsOutOfScope', { count: got.uploadsOutOfScope })}
            </p>
          )}
        </div>
      </>
    )
  }

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
        {found != null &&
          SERVICES.map((service) => {
            const f = found[service]
            return (
              <Fragment key={service}>
                <h3 className="confirm-review-group">{SERVICE_NAME[service]}</h3>
                {f.state === 'checking' && (
                  <p className="park-review-note" role="status">
                    {t('logbook.confirmations.checking', { service: SERVICE_NAME[service] })}
                  </p>
                )}
                {f.state === 'failed' && (
                  <p className="park-review-note">{t('logbook.confirmations.notChecked', { reason: f.why })}</p>
                )}
                {f.state === 'checked' && listed(service, f.check)}
              </Fragment>
            )
          })}
        <div className="logconfirm-actions">
          <button type="button" className="logconfirm-discard" onClick={close}>
            {t('logbook.confirmations.cancel')}
          </button>
          {/* Check until a service has been checked; then Apply. A check every service failed
              offers Check again. */}
          {checking || checks.length === 0 ? (
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
