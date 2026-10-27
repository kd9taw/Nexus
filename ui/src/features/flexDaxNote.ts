// THE FLEX DAX NOTE: shown once, the first time a voice-keyer message plays on a FlexRadio that Nexus
// drives through SmartSDR CAT or Hamlib, until the operator dismisses it.
//
// Nexus keys such a radio for a recorded message exactly as it does for FT8, and sends the message to
// the DAX transmit device. FT8 runs in DIGU, which takes its transmit audio from DAX. A voice message
// runs in USB or LSB, where the Flex takes its transmit audio from its own mic unless DAX is switched on
// for voice in SmartSDR's P/CW panel, so the radio keys and sends no RF. Nothing on the CAT path can
// switch that for the operator, so the note says where it is. It is information only: it never blocks
// or delays a message.
//
// Not shown when Nexus's own Flex client serves the radio (it switches the radio to DAX for each
// message itself), nor on TCI, which stores a model of its own.
//
// The dismissal is SHARED (storage-scope.test.ts): once dismissed, it is dismissed for this computer,
// whichever window showed it, like the contest screen's work-in hint.
import type { Settings } from '../types'

export const FLEX_DAX_NOTE_KEY = 'nexus.phone.flexDaxNote'

/** The FlexRadio profiles: SmartSDR CAT (2036) and Hamlib's SmartSDR backend (23005). The same pair
 *  Rust's `FLEX_CLIENT_RIGS` names. */
const FLEX_MODELS = [2036, 23005]

/** Whether the active radio is a Flex driven over SmartSDR CAT or Hamlib, where a voice message goes
 *  out only with DAX switched on for voice. Mirrors Rust's `flex_client_ip` for the native client. */
export function flexVoiceNeedsDax(
  s: Pick<Settings, 'rigModel' | 'rigConn' | 'flexNativeCat' | 'flexRadioIp'>,
): boolean {
  if (!FLEX_MODELS.includes(s.rigModel) || s.rigConn === 'tci') return false
  const nativeClient = s.flexNativeCat === true && (s.flexRadioIp ?? '').trim() !== ''
  return !nativeClient
}

/** Whether the note has been dismissed on this computer. */
export function flexDaxNoteDismissed(): boolean {
  try {
    return window.localStorage.getItem(FLEX_DAX_NOTE_KEY) === '1'
  } catch {
    return false
  }
}

export function dismissFlexDaxNote(): void {
  try {
    window.localStorage.setItem(FLEX_DAX_NOTE_KEY, '1')
  } catch {
    // A blocked store: dismissed for this window until it closes.
  }
}
