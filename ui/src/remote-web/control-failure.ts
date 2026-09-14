import { t } from '../i18n'
import { OperationFailure } from './operation-client'

/** The message for a failed station command. "Busy" when the station refused it as busy, before
 * changing anything; "Not sent" only when its request never left this browser, so nothing reached
 * the station; anything else stays "not confirmed". Read from the failure's data, never its text. */
export function controlFailureMessage(error: unknown): string {
  // The station refused a transmit frequency outside the operator's licence, before changing it.
  if (error instanceof Error && error.message === 'outsidePrivileges') return t('remote.b1.outsidePrivileges')
  if (!(error instanceof OperationFailure)) return t('remote.controlRequestFailed')
  return error.busy ? t('remote.controlBusy') : !error.sent ? t('remote.controlNotSent') : t('remote.controlRequestFailed')
}
