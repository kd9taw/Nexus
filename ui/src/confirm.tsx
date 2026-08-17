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
 * WHY THE SUITE MISSED IT. Not, as first written here, because tests mocked `window.confirm` to
 * return `true` — nothing in the suite mocks it at all. It is simpler and worse: none of the
 * converted paths had ANY coverage. A guard nothing exercises cannot fail, whatever it returns.
 * `confirm.test.tsx` pins this primitive; `SettingsPanel.removeradio.test.tsx` pins one whole
 * path end to end, which is what would actually have caught it.
 *
 * TWO `window.confirm` CALLS REMAIN, deliberately (SetupHealth's Prove TX, SstvView's ISS
 * 145.800 guard). Both are transmit-path, so converting them is a TX-behaviour change needing
 * sign-off rather than part of a UI pass. Note what they do on macOS meanwhile: both read the
 * inert `false` as "no", so each BLOCKS its action. Safe — nothing keys — but Prove TX is a dead
 * button there, and SSTV cannot be sent on 145.800 at all.
 *
 * FAIL CLOSED. If the host is not mounted, `confirmDialog` resolves FALSE — a destructive action
 * must never proceed unconfirmed. The opposite default would turn a missing dialog into a silent
 * deletion, which is the worse half of the same bug.
 */
import { useEffect, useRef, useState } from 'react'
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
  // The same request the state holds, kept where a callback can settle it without reading
  // through a render. Resolving inside a `setReq` updater would be a side effect in a function
  // React is free to call twice (StrictMode), and `close` reading `req` would answer whichever
  // request that closure captured rather than the one on screen.
  const pending = useRef<Request | null>(null)

  useEffect(() => {
    present = (next) => {
      // A second question arriving while one is open REPLACES it, so the one being replaced must
      // be answered on its way out or its `await` never settles and the caller waits for the
      // lifetime of the window. Answered NO: the operator never saw it, and an unseen
      // destructive question is a refusal. Reachable from the fire-and-forget call sites in
      // RadioProgView, which do not await one another.
      pending.current?.resolve(false)
      pending.current = next
      setReq(next)
    }
    return () => {
      present = null
    }
  }, [])

  const close = (ok: boolean) => {
    pending.current?.resolve(ok)
    pending.current = null
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
