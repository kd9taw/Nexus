import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { clearEqslSentMarks, restoreEqslSentMarks, type EqslSentCleared, type EqslSentMark } from '../api'
import { t } from '../i18n'
import { pushToast, withErrorToast } from '../toast'
import { utcDate } from '../features/utcLog'

// "CHECK eQSL SENT MARKS" — the contacts holding the "QSL sent, electronic" mark that earlier
// versions copied from each eQSL card onto the contact it confirmed: eQSL's InBox carries its
// sender's own mark on every card, and the merge took it for the operator's. Nothing stored tells
// such a mark from one the operator set, so the station lists the contacts holding its shape, an
// undated electronic mark beside an eQSL card (`tempo_core::eqsl::holds_card_sent_mark`), and the
// operator decides. A mark set in Nexus is dated and never listed; one set in another program and
// brought in with the log can be, which is why every line can be unticked.
//
// Built as Check park states is (ParkStateReview), with its classes and focus rules. What differs,
// on purpose: the Logbook reads the list, since it shows the button only while one is left and
// says how many, and hands it in; every line starts ticked; and the notice after a clear carries
// Undo, which puts the mark back on exactly the contacts the clear changed, each only while it
// still holds that clear. Nothing changes until the button, nothing is uploaded, and it never
// opens or runs by itself.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Calls, bands and modes are
// data, shown as they are.

/** The notice after a clear carries Undo, so it stays long enough to use it. */
const DONE_TOAST_MS = 20_000

export function EqslSentMarkReview({
  open,
  rows,
  onClose,
  onChanged,
}: {
  open: boolean
  rows: readonly EqslSentMark[]
  onClose: () => void
  /** The log changed: a clear, or its Undo. */
  onChanged: () => void
}) {
  // The lines the operator unticked: every line the list holds starts ticked.
  const [unticked, setUnticked] = useState<ReadonlySet<string>>(new Set())
  const [busy, setBusy] = useState(false)

  // The keyboard goes into the dialog when it opens and back to where it was opened from when it
  // closes, cleared or cancelled (SpotDialog's rule).
  const boxRef = useRef<HTMLDivElement>(null)
  // The first Tab stop skips the list to its buttons: with thousands of lines, Tab otherwise
  // passes every checkbox before Cancel. It lands on Cancel, which changes nothing.
  const cancelRef = useRef<HTMLButtonElement>(null)
  const skipToButtons = () => {
    cancelRef.current?.focus({ preventScroll: true })
    cancelRef.current?.scrollIntoView?.({ block: 'nearest' })
  }
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

  useEffect(() => {
    if (open) setUnticked(new Set())
  }, [open])

  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [open, onClose])

  if (!open) return null

  const ticked = rows.filter((r) => !unticked.has(r.id))
  const toggle = (id: string) =>
    setUnticked((was) => {
      const next = new Set(was)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })

  const undo = async (made: EqslSentCleared) => {
    const n = await withErrorToast(
      () => restoreEqslSentMarks(made.ids, made.clearedUnix),
      t('logbook.eqslSentMarks.undoFailed'),
    )
    if (n == null) return
    pushToast(t('logbook.eqslSentMarks.restored', { count: n }), 'success')
    onChanged()
  }

  const clear = async () => {
    const ids = ticked.map((r) => r.id)
    if (ids.length === 0) return
    // The button disables while the clear runs; the keyboard stays in the dialog.
    boxRef.current?.focus({ preventScroll: true })
    setBusy(true)
    const made = await withErrorToast(() => clearEqslSentMarks(ids), t('logbook.eqslSentMarks.failed'))
    setBusy(false)
    if (made == null) return
    // A change to the log, so a notice: it pops up even with pop-ups off (toast.ts), with its Undo.
    pushToast(
      t('logbook.eqslSentMarks.done', { count: made.ids.length }),
      'info',
      DONE_TOAST_MS,
      made.ids.length > 0
        ? { action: () => void undo(made), actionLabel: t('logbook.eqslSentMarks.undo') }
        : {},
    )
    onChanged()
    onClose()
  }

  // Every line holds the same mark: sent, electronically, with no date.
  const mark = t('logbook.qsl.sentVia', { via: t('logbook.qsl.via.electronic') })
  return (
    <div
      className="logconfirm-backdrop"
      role="dialog"
      aria-modal="true"
      aria-label={t('logbook.eqslSentMarks.title')}
      onClick={onClose}
    >
      <div ref={boxRef} tabIndex={-1} className="logconfirm park-review" onClick={(e) => e.stopPropagation()}>
        <div className="logconfirm-head">
          <h2>{t('logbook.eqslSentMarks.title')}</h2>
        </div>
        <p className="park-review-intro">{t('logbook.eqslSentMarks.intro')}</p>
        <button type="button" className="park-review-skip" onClick={skipToButtons}>
          {t('logbook.eqslSentMarks.skip')}
        </button>
        <ul className="park-review-list">
          {rows.map((r) => {
            const on = !unticked.has(r.id)
            return (
              <li key={r.id} className={`park-review-row${on ? ' ticked' : ''}`}>
                <label>
                  <input type="checkbox" checked={on} onChange={() => toggle(r.id)} />
                  <span className="mono park-review-call">{r.call}</span>
                  <span className="mono park-review-when">{utcDate(r.whenUnix)}</span>
                  <span className="park-review-band">{`${r.band} ${r.mode}`}</span>
                  <span className="park-review-change">{mark}</span>
                </label>
              </li>
            )
          })}
        </ul>
        <div className="logconfirm-actions">
          <button ref={cancelRef} type="button" className="logconfirm-discard" onClick={onClose}>
            {t('logbook.eqslSentMarks.cancel')}
          </button>
          <button
            type="button"
            className="logconfirm-log"
            disabled={busy || ticked.length === 0}
            onClick={() => void clear()}
          >
            {t('logbook.eqslSentMarks.clear', { count: ticked.length })}
          </button>
        </div>
      </div>
    </div>
  )
}
