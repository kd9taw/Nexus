import { useState } from 'react'
import { t } from '../i18n'
import { LOCALE_BETA_NOTE, useLocale } from '../i18n/useLocale'

/** Where a dismissal is kept, per language: dismissing the Italian note says nothing about a later one. */
export const localeBetaKey = (locale: string) => `nexus.localeBeta.dismissed.${locale}`

const dismissedFor = (locale: string) => {
  try {
    return localStorage.getItem(localeBetaKey(locale)) === '1'
  } catch {
    return false
  }
}

/**
 * The beta-translation note: one dismissible line in the operator's language, while the language on
 * screen is a beta one (`LOCALE_BETA_NOTE`) and the note has not been dismissed for it. The same
 * banner as the first-run `OnboardingBanner` (its classes, its place in the shell); the text opens
 * Settings, where the language is switched.
 */
export function LocaleBetaBanner({ onOpenSettings }: { onOpenSettings: () => void }) {
  const locale = useLocale()
  const [dismissed, setDismissed] = useState<Record<string, boolean>>({})
  const note = LOCALE_BETA_NOTE[locale]
  if (!note || dismissed[locale] || dismissedFor(locale)) return null
  const dismiss = () => {
    try {
      localStorage.setItem(localeBetaKey(locale), '1')
    } catch {
      // Storage refused: the note goes for this session only.
    }
    setDismissed((d) => ({ ...d, [locale]: true }))
  }
  return (
    <div className="onboarding-banner" role="note">
      <span className="onboarding-icon" aria-hidden>ℹ️</span>
      <button type="button" className="onboarding-text" onClick={onOpenSettings}>
        {note}
      </button>
      <button type="button" className="onboarding-dismiss" aria-label={t('common.dismiss')} onClick={dismiss}>
        ×
      </button>
    </div>
  )
}
