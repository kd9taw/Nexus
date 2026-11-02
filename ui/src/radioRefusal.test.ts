// The one-window refusal reaches the operator in their language, never as the backend's English.
import { describe, expect, it } from 'vitest'
import { RADIO_OPEN_ELSEWHERE, localizeRefusal } from './radioRefusal'
import { failureText } from './toast'
import { t } from './i18n'

describe('the one-window refusal', () => {
  it('is said with the catalog entry, not the backend sentence', () => {
    expect(localizeRefusal(RADIO_OPEN_ELSEWHERE)).toBe(t('radios.openElsewhere'))
    expect(localizeRefusal(`  ${RADIO_OPEN_ELSEWHERE}\n`)).toBe(t('radios.openElsewhere'))
  })
  it('an error toast says it whole, without the generic "failed" prefix', () => {
    expect(failureText('Could not switch radio', RADIO_OPEN_ELSEWHERE)).toBe(t('radios.openElsewhere'))
    expect(failureText('Could not switch radio', new Error(RADIO_OPEN_ELSEWHERE))).toBe(t('radios.openElsewhere'))
  })
  it('CONTROL: any other backend text passes through as it always did', () => {
    expect(localizeRefusal('CAT confirmed')).toBe('CAT confirmed')
    expect(failureText('Could not save', 'disk full')).toBe('Could not save: disk full')
  })
})
