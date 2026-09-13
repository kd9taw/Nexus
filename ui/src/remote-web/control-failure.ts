import { t } from '../i18n'
import { OperationFailure } from './operation-client'

/** The message for a failed station command. "Not sent" only when its request never left this
 * browser, so nothing reached the station; anything else stays "not confirmed". */
export function controlFailureMessage(error: unknown): string {
  return error instanceof OperationFailure && !error.sent ? t('remote.controlNotSent') : t('remote.controlRequestFailed')
}
