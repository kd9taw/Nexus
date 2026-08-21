// Toast rendering via Radix Toast (accessible live-region + dismissal), wired to
// the existing dependency-free toast.ts bus and the --alert-* tokens. The bus
// owns TTL/auto-dismiss; Radix duration is Infinity so it doesn't double-expire.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). A toast's MESSAGE
// and its action label arrive already translated from whoever raised it; what lives here is
// the chrome — the default action word and the close button's name.
import { useEffect, useState } from 'react'
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
            if (!o) dismissToast(toast.id)
          }}
        >
          <RToast.Description className="ui-toast-msg">{toast.message}</RToast.Description>
          {toast.action && (
            <RToast.Action asChild altText={toast.actionLabel ?? t('toast.action.default')}>
              <button
                type="button"
                className="ui-toast-action"
                onClick={() => {
                  toast.action?.()
                  dismissToast(toast.id)
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
