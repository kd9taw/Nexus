import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { applyParkStates, parkStateReview, type ParkStateRow } from '../api'
import { t } from '../i18n'
import { pushToast, withErrorToast } from '../toast'
import { utcDate } from '../features/utcLog'

// "CHECK PARK STATES" — the contacts already in the log whose park or summit is in one state and
// which hold another state, or none: until the log form placed a hunted contact by its park, it
// took the activator's home state (their licence's address), so an Ohio ham in a North Dakota
// park was logged OH. Listed old → new for the operator to tick.
//
// Nothing changes until Apply, and then only what is ticked, each only while it still holds the
// state shown here. A confirmed contact starts unticked: its confirmation came with the state the
// other station signed, and that is the operator's call. Nothing is uploaded again: the station
// re-uploads an edit only for a corrected call. It never opens or runs by itself.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Calls, park references,
// bands, modes and state codes are invariant tokens, shown as they are.
export function ParkStateReview({
  open,
  onClose,
  onApplied,
}: {
  open: boolean
  onClose: () => void
  onApplied: () => void
}) {
  const [rows, setRows] = useState<ParkStateRow[] | null>(null)
  const [ticked, setTicked] = useState<ReadonlySet<string>>(new Set())
  const [busy, setBusy] = useState(false)

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

  useEffect(() => {
    if (!open) return
    let live = true
    setRows(null)
    void withErrorToast(() => parkStateReview(), t('logbook.parkStates.failed')).then((found) => {
      if (!live) return
      if (!found) {
        onClose()
        return
      }
      setRows(found)
      setTicked(new Set(found.filter((r) => !r.confirmed).map((r) => r.id)))
    })
    return () => {
      live = false
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
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

  const toggle = (id: string) =>
    setTicked((was) => {
      const next = new Set(was)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })

  const apply = async () => {
    if (!rows) return
    const changes = rows
      .filter((r) => ticked.has(r.id))
      .map((r) => ({ id: r.id, state: r.state, parkState: r.parkState }))
    if (changes.length === 0) return
    setBusy(true)
    const made = await withErrorToast(() => applyParkStates(changes), t('logbook.parkStates.failed'))
    setBusy(false)
    if (made == null) return
    pushToast(t('logbook.parkStates.done', { count: made }), 'success')
    onApplied()
    onClose()
  }

  return (
    <div
      className="logconfirm-backdrop"
      role="dialog"
      aria-modal="true"
      aria-label={t('logbook.parkStates.title')}
      onClick={onClose}
    >
      <div ref={boxRef} tabIndex={-1} className="logconfirm park-review" onClick={(e) => e.stopPropagation()}>
        <div className="logconfirm-head">
          <h2>{t('logbook.parkStates.title')}</h2>
        </div>
        <p className="park-review-intro">{t('logbook.parkStates.intro')}</p>
        {rows == null ? (
          <p className="park-review-note">{t('logbook.parkStates.reading')}</p>
        ) : rows.length === 0 ? (
          <p className="park-review-note">{t('logbook.parkStates.none')}</p>
        ) : (
          <ul className="park-review-list">
            {rows.map((r) => (
              <li key={r.id} className={`park-review-row${ticked.has(r.id) ? ' ticked' : ''}`}>
                <label>
                  <input type="checkbox" checked={ticked.has(r.id)} onChange={() => toggle(r.id)} />
                  <span className="mono park-review-call">{r.call}</span>
                  <span className="mono park-review-when">{utcDate(r.whenUnix)}</span>
                  <span className="park-review-band">{`${r.band} ${r.mode}`}</span>
                  <span className="mono park-review-ref">{r.reference}</span>
                  <span className="mono park-review-change">
                    {t('logbook.parkStates.change', {
                      from: r.state ?? t('logbook.parkStates.noState'),
                      to: r.parkState,
                    })}
                  </span>
                  {r.confirmed && (
                    <span className="park-review-confirmed" title={t('logbook.parkStates.confirmedTitle')}>
                      {t('logbook.parkStates.confirmed')}
                    </span>
                  )}
                </label>
              </li>
            ))}
          </ul>
        )}
        <div className="logconfirm-actions">
          <button type="button" className="logconfirm-discard" onClick={onClose}>
            {t('logbook.parkStates.cancel')}
          </button>
          <button
            type="button"
            className="logconfirm-log"
            disabled={busy || rows == null || ticked.size === 0}
            onClick={() => void apply()}
          >
            {t('logbook.parkStates.apply', { count: ticked.size })}
          </button>
        </div>
      </div>
    </div>
  )
}
