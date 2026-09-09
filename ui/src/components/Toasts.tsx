// Toast rendering via Radix Toast (accessible live-region + dismissal), wired to
// the existing dependency-free toast.ts bus and the --alert-* tokens. The bus
// owns TTL/auto-dismiss; Radix duration is Infinity so it doesn't double-expire.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). A toast's MESSAGE
// and its action label arrive already translated from whoever raised it; what lives here is
// the chrome — the default action word and the close button's name.
import { useEffect, useRef, useState } from 'react'
import * as RToast from '@radix-ui/react-toast'
import { t } from '../i18n'
import { dismissToast, subscribeToasts, type Toast, type ToastKind } from '../toast'

const KIND_CLASS: Record<ToastKind, string> = {
  error: 'kind-error',
  info: 'kind-info',
  success: 'kind-success',
}

export function Toasts() {
  const [toasts, setToasts] = useState<Toast[]>([])
  useEffect(() => subscribeToasts(setToasts), [])
  // ⚠️ IDS WHOSE ACTION IS IN FLIGHT OR HAS FAILED (R3). Radix closes a Root when its Action is
  // pressed — right for a one-shot action, wrong for one that can FAIL, and it is the second
  // half of the bug: even once the click handler stopped dismissing unconditionally, Radix's
  // own close still retired the toast through `onOpenChange`. An id parked here opts out of
  // that one path (never out of ✕ or swipe) until its action either succeeds — it is dismissed
  // explicitly then — or fails, when the toast must stay so the operator can act again.
  const held = useRef(new Set<number>())

  return (
    <RToast.Provider swipeDirection="right" duration={Infinity}>
      {/* The bound toast is `toast`, not `t` — `t` is the translator here. */}
      {toasts.map((toast) => (
        <RToast.Root
          key={toast.id}
          // Screen-reader severity: errors + prominent alerts ("W1AW is calling
          // you") interrupt (foreground = assertive); routine notes (QSY, saved-
          // file confirmations) queue politely (background) so they never talk
          // over the operator mid-exchange.
          type={toast.kind === 'error' || toast.prominent ? 'foreground' : 'background'}
          className={`ui-toast ${KIND_CLASS[toast.kind]}${toast.prominent ? ' prominent' : ''}`}
          open
          onOpenChange={(o) => {
            if (!o && !held.current.has(toast.id)) dismissToast(toast.id)
          }}
        >
          <RToast.Description className="ui-toast-msg">{toast.message}</RToast.Description>
          {toast.action && (
            <RToast.Action asChild altText={toast.actionLabel ?? t('toast.action.default')}>
              <button
                type="button"
                className="ui-toast-action"
                // ⚠️ AN ACTION THAT FAILS MUST NOT RETIRE ITS TOAST (R3). This used to run the
                // action and dismiss unconditionally and synchronously, so the update toast's
                // Download button closed the notice whether or not a browser ever opened — and
                // the updater, seeing the same false success, recorded the version as dismissed
                // for good. An action that returns a promise now decides: resolve = done, close
                // it; reject = it did not happen, leave the toast so the operator can act again.
                // Sync actions (every other toast in the app — Work, Answer, QSY) are untouched.
                onClick={() => {
                  const result: unknown = toast.action?.()
                  if (typeof (result as PromiseLike<unknown> | undefined)?.then !== 'function') {
                    dismissToast(toast.id)
                    return
                  }
                  // Runs BEFORE Radix's own close handler (Slot composes the child's onClick
                  // first), so the id is parked by the time `onOpenChange` asks.
                  held.current.add(toast.id)
                  void Promise.resolve(result).then(
                    () => {
                      held.current.delete(toast.id)
                      dismissToast(toast.id)
                    },
                    () => {
                      // The action reported its own failure. Release the hold so ✕ works again,
                      // and leave the toast up.
                      held.current.delete(toast.id)
                    },
                  )
                }}
              >
                {toast.actionLabel ?? t('toast.action.default')} →
              </button>
            </RToast.Action>
          )}
          <RToast.Close className="ui-toast-close" aria-label={t('toast.dismiss')}>
            ×
          </RToast.Close>
        </RToast.Root>
      ))}
      <RToast.Viewport className="ui-toast-viewport" />
    </RToast.Provider>
  )
}
