// The one-time UI-scale prompt on a fractionally scaled display ("Nexus fits this screen better
// at 80%. Keep it?").
//
// `useScale` decides the size; this only asks about it. It raises the notice when `fitOffer`
// first appears — the main window, never asked, sitting at a smaller auto-fit than the classic
// rule would give it (useScale.ts `scaledFitScale`) — and RECORDS THE QUESTION AS ASKED the
// moment it does, with the default answer ('scaled'). So closing the notice keeps the new size
// and it never comes back, on this launch or any later one: the same once-per-install gate the
// DPI seed uses (`capNeverChosen`), where the absence of the key is the only "not yet".
//
// Keep confirms; the second button records 'classic', and auto-fit goes back to the size it
// always gave this screen. Either answer is changeable afterwards in Settings ▸ Appearance by
// pinning a scale, which the scaled rule never overrides.
import { useEffect, useRef } from 'react'
import { t } from './i18n'
import { pushToast } from './toast'
import type { FitOffer, FitRule } from './useScale'

export function useScaleFitNotice(offer: FitOffer | null, setFitRule: (r: FitRule) => void): void {
  const shown = useRef(false)
  useEffect(() => {
    if (!offer || shown.current) return
    shown.current = true
    setFitRule('scaled')
    pushToast(t('settings.workspace.scale.fit.notice', { scaled: offer.scaled }), 'info', 0, {
      action: () => setFitRule('scaled'),
      actionLabel: t('settings.workspace.scale.fit.keep', { scaled: offer.scaled }),
      secondaryAction: () => setFitRule('classic'),
      secondaryLabel: t('settings.workspace.scale.fit.classic', { classic: offer.classic }),
    })
  }, [offer, setFitRule])
}
