import { expect, it } from 'vitest'
import { t } from '../i18n'
import { controlFailureMessage } from './control-failure'
import { OperationFailure } from './operation-client'

it('names a busy refusal, a request never sent, and a sent request with no confirmation from the failure data', () => {
  expect(controlFailureMessage(new OperationFailure('stationBusy', true, true))).toBe('The station was busy and nothing changed. Try again.')
  expect(controlFailureMessage(new OperationFailure('windowExpired', false))).toBe(t('remote.controlNotSent'))
  expect(controlFailureMessage(new OperationFailure('operationUnknown', true))).toBe(t('remote.controlRequestFailed'))
  // Control: the wording follows the busy flag, never the error text.
  expect(controlFailureMessage(new OperationFailure('stationBusy', true))).toBe(t('remote.controlRequestFailed'))
})
