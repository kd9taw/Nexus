// The station's audio-error line in the status lane, the UI half. The station writes one line for
// several different problems (a PTT key the rig did not accept, a sound card that failed, a
// headphone monitor held off, a voice mic that fell back …) and names the kind of each beside its
// sentence (crates/tempo-app/src/dto.rs, `AudioErrorKind`). This heads the sentence with words for
// that kind, and tiers it by what it costs the operator: critical when receiving or transmitting
// has stopped, a warning for a notice the radio works through. The sentence itself is the
// tooltip, whole, as the station wrote it. One headline for all of them ("RADIO STOPPED") told an
// operator whose rig refused PTT that the radio had stopped while the dial and CAT both worked.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts) from birth. Every key is
// written literally.

import type { AudioErrorKind } from '../types'
import type { StatusItem, StatusTier } from '../status'
import { t } from '../i18n'

/** Each kind's headline and tier. A `Record`, so a kind added to the type without words here does
 *  not compile. */
const KINDS: Record<AudioErrorKind, { tier: StatusTier; headline: () => string }> = {
  // The radio loop ended: nothing is sent or received until Nexus restarts.
  engineStopped: { tier: 'critical', headline: () => t('shell.lane.audio.engineStopped') },
  soundCard: { tier: 'critical', headline: () => t('shell.lane.audio.soundCard') },
  noReceiveAudio: { tier: 'critical', headline: () => t('shell.lane.audio.noReceiveAudio') },
  // The over is not sent on a key the rig refused.
  ptt: { tier: 'critical', headline: () => t('shell.lane.audio.ptt') },
  // Already back on the sound card, by design.
  flexAudio: { tier: 'warning', headline: () => t('shell.lane.audio.flexAudio') },
  // Only the native panadapter waits for the address.
  flexAddress: { tier: 'warning', headline: () => t('shell.lane.audio.flexAddress') },
  monitor: { tier: 'warning', headline: () => t('shell.lane.audio.monitor') },
  // A recording falls back to the shared input.
  voiceMic: { tier: 'warning', headline: () => t('shell.lane.audio.voiceMic') },
  recording: { tier: 'warning', headline: () => t('shell.lane.audio.recording') },
  // One period lost; receive continues.
  decodeCrash: { tier: 'warning', headline: () => t('shell.lane.audio.decodeCrash') },
}

/** The status-lane item for the station's audio-error line, or null while there is none. A
 *  station older than the kinds names none, and a newer one may name a kind this page does not
 *  know: either gets the plain headline, critical, because nothing says the radio works through
 *  it. */
export function audioErrorLane(
  message: string | null | undefined,
  kind: AudioErrorKind | null | undefined,
): Omit<StatusItem, 'id'> | null {
  if (!message) return null
  const known = kind != null && Object.prototype.hasOwnProperty.call(KINDS, kind) ? KINDS[kind] : null
  if (!known) return { tier: 'critical', message: t('shell.lane.audio.other'), detail: message }
  return { tier: known.tier, message: known.headline(), detail: message }
}
