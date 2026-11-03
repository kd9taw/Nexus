// ONE RADIO, ONE WINDOW: the backend's refusal of a radio another Nexus window drives, in the
// operator's language. The backend says it in one fixed English sentence (`tempo_app::radio_claim::
// REFUSAL`, pinned to the copy below by a Rust test that reads this file); the screen never shows
// that sentence, it shows `radios.openElsewhere`. Every place that shows backend text which can
// carry it (an error toast, the radio picker, the CAT status line) passes it through here.
import { t } from './i18n'

/** The backend's sentence, exactly as `tempo_app::radio_claim::REFUSAL` has it. */
export const RADIO_OPEN_ELSEWHERE = 'That radio is open in another Nexus window — switch to that window.'

/** Is `text` the backend's one-window refusal? */
export function isRadioRefusal(text: string): boolean {
  return text.trim() === RADIO_OPEN_ELSEWHERE
}

/** `text` in the operator's language when it is the one-window refusal; any other text as it is. */
export function localizeRefusal(text: string): string {
  return isRadioRefusal(text) ? t('radios.openElsewhere') : text
}
