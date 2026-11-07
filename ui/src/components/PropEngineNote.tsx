import { t } from '../i18n'

const DISMISSED_KEY = 'nexus.propEngineNoteDismissed'

/** Whether this browser profile has already dismissed the note. The settings file is the record;
 *  this holds when a refused save could not write it, so the note does not come back each launch. */
export function propEngineNoteDismissed(): boolean {
  try {
    return localStorage.getItem(DISMISSED_KEY) === '1'
  } catch {
    return false
  }
}

export function rememberPropEngineNoteDismissed(): void {
  try {
    localStorage.setItem(DISMISSED_KEY, '1')
  } catch {
    // The settings file still records it when it can.
  }
}

interface Props {
  /** Navigate to the Settings view, where the engine can be switched back. */
  onOpenSettings: () => void
  onDismiss: () => void
}

/**
 * The one-time note after the move to P.533. A line in the page's flow above the shell, the same
 * as the first-run nudge: it covers nothing, least of all a transmit control, and waits for the
 * operator without taking the keyboard or asking for an answer.
 */
export function PropEngineNote({ onOpenSettings, onDismiss }: Props) {
  return (
    <div className="onboarding-banner" role="note">
      <button type="button" className="onboarding-text" onClick={onOpenSettings}>
        {t('prop.engine.note')}
      </button>
      <button type="button" className="onboarding-dismiss" aria-label={t('common.dismiss')} onClick={onDismiss}>
        ×
      </button>
    </div>
  )
}
