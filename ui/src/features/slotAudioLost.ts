// A slot over (FT8, FT4, JS8 and the other timed-slot modes) whose audio stopped reaching the radio
// part way through, the UI half. Flex native DAX audio went off under it, or its DAX transmit route
// went, so the station ended the over there rather than leave the radio keyed and silent for the
// rest of it, halted TX and sends when (crates/tempo-app/src/engine.rs,
// `halt_tx_for_lost_slot_audio`); this is the operator's sentence for it. The status lane keeps it
// until TX is turned on again.

// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts) from birth. Every key is
// written literally.

import type { SlotAudioLost } from '../types'
import type { StatusItem } from '../status'
import { t } from '../i18n'

/** The status-lane item after a slot over that lost its audio was ended and TX halted, or null. */
export function slotAudioLostLane(
  l: SlotAudioLost | null | undefined,
): Omit<StatusItem, 'id'> | null {
  if (!l) return null
  // UTC, as every time an operator logs is.
  const time = new Date(l.at * 1000).toISOString().slice(11, 19)
  return {
    tier: 'critical',
    message: t('shell.lane.slotAudioLost.message'),
    detail: t('shell.lane.slotAudioLost.detail', { time }),
  }
}
