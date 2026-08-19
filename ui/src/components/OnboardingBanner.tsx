import { t } from '../i18n'

interface Props {
  /** Navigate to the Settings view. */
  onOpenSettings: () => void
  /** Persist that the user dismissed the nudge. */
  onDismiss: () => void
}

/**
 * First-run nudge prompting the operator to set their callsign and station.
 * Rendered only when the callsign is still empty / the placeholder and the
 * nudge has not been dismissed (visibility decided by the parent).
 */
export function OnboardingBanner({ onOpenSettings, onDismiss }: Props) {
  return (
    <div className="onboarding-banner" role="note">
      <span className="onboarding-icon" aria-hidden>👋</span>
      <button type="button" className="onboarding-text" onClick={onOpenSettings}>
        {t('onboarding.setStation')}
      </button>
      <button
        type="button"
        className="onboarding-dismiss"
        aria-label={t('common.dismiss')}
        onClick={onDismiss}
      >
        ×
      </button>
    </div>
  )
}
