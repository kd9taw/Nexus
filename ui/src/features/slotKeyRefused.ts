// A slot over (FT8, FT4, JS8 and the other timed-slot modes) the radio would not key, the UI half.
// The station drops the over, halts TX and sends when and why (crates/tempo-app/src/engine.rs,
// `halt_tx_for_refused_key`); this is the operator's sentence for it. WSJT-X stops on the same
// failure and shows its "Rig Control Error"; here the status lane says it, and keeps saying it
// until TX is turned on again.

// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts) from birth. Every key is
// written literally.

import type { SlotKeyRefused } from '../types'
import type { StatusItem } from '../status'
import { t } from '../i18n'

/** The status-lane item after a refused slot key halted TX, or null. `why` is the rig link's own
 *  answer, interpolated as data and never translated. A key Nexus's own Flex client kept off the
 *  air for the audio route has a sentence of its own, by cause: the radio refused nothing, and the
 *  PTT and CAT advice would be wrong. */
export function slotKeyRefusedLane(
  r: SlotKeyRefused | null | undefined,
): Omit<StatusItem, 'id'> | null {
  if (!r) return null
  // UTC, as every time an operator logs is.
  const time = new Date(r.at * 1000).toISOString().slice(11, 19)
  return {
    tier: 'critical',
    message: t('shell.lane.slotKeyRefused.message'),
    detail: !r.flexAudio
      ? t('shell.lane.slotKeyRefused.detail', { time, why: r.why })
      : r.flexAudio.cause === 'daxUnfed'
        ? t('shell.lane.slotKeyRefused.flex.daxUnfed', { time, mode: r.flexAudio.mode })
        : t('shell.lane.slotKeyRefused.flex.notYetDax', { time, mode: r.flexAudio.mode }),
  }
}
