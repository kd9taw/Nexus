/** In-app confirmation for destructive actions.
 *
 * WHY THIS EXISTS. `window.confirm` is INERT in this webview. A JS confirm only appears if the
 * host implements WKWebView's `runJavaScriptConfirmPanel`; wry does not, so `confirm()` returns
 * `false` immediately and shows nothing. Every guard written as
 *
 *     if (!window.confirm('…')) return
 *
 * therefore returns instantly, and the action silently does nothing at all. Reported by the
 * operator on 2026-08-14 for Remove radio, then confirmed on Reset: no dialog, no effect, no
 * error. Fifteen destructive actions were built on it.
 *
 * The unit tests could never catch it: they mock `window.confirm` to return `true`, so the suite
 * exercises a dialog that does not exist in the real app.
 *
 * FAIL CLOSED. If the host is not mounted, `confirmDialog` resolves FALSE — a destructive action
 * must never proceed unconfirmed. The opposite default would turn a missing dialog into a silent
 * deletion, which is the worse half of the same bug.
 */
import { useEffect, useState } from 'react'
import { Dialog } from './components/ui/Dialog'

export interface ConfirmOptions {
  /** The question, as a title. Say what will happen, not "Are you sure?". */
  title: string
  /** What the operator needs to know before answering — scope, and what is NOT affected. */
  body?: string
  /** Label for the affirmative button. Name the ACT ("Remove radio"), never "OK". */
  confirmLabel?: string
  cancelLabel?: string
  /** Style the affirmative button as destructive. */
  danger?: boolean
}

interface Request extends ConfirmOptions {
  resolve: (ok: boolean) => void
}

let present: ((r: Request) => void) | null = null

/** Ask the operator to confirm. Resolves true only on an explicit yes. */
export function confirmDialog(opts: ConfirmOptions): Promise<boolean> {
  if (!present) {
    // No host mounted. Refuse rather than proceed: see FAIL CLOSED above.
    console.error('confirmDialog: no <ConfirmHost/> mounted — refusing the action')
    return Promise.resolve(false)
  }
  return new Promise<boolean>((resolve) => present?.({ ...opts, resolve }))
}

/** Mount ONCE, near the app root. */
export function ConfirmHost() {
  const [req, setReq] = useState<Request | null>(null)

  useEffect(() => {
    present = setReq
    return () => {
      present = null
    }
  }, [])

  const close = (ok: boolean) => {
    req?.resolve(ok)
    setReq(null)
  }

  return (
    <Dialog
      open={req !== null}
      // ESC and overlay-click land here: anything that is not an explicit yes is a no.
      onOpenChange={(open) => {
        if (!open) close(false)
      }}
      title={req?.title ?? ''}
      description={req?.body}
    >
      <div className="confirm-actions">
        <button type="button" className="settings-refresh" onClick={() => close(false)} autoFocus>
          {req?.cancelLabel ?? 'Cancel'}
        </button>
        <button
          type="button"
          className={req?.danger ? 'settings-save danger' : 'settings-save'}
          onClick={() => close(true)}
        >
          {req?.confirmLabel ?? 'Confirm'}
        </button>
      </div>
    </Dialog>
  )
}
