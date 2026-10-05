// A slot over (FT8, FT4, JS8 and the other timed-slot modes) whose unkey the radio did not take,
// the UI half. The station halts TX, keeps sending the unkey until the radio takes it, and sends
// when and why (crates/tempo-app/src/engine.rs, `halt_tx_for_failed_unkey`); this is the
// operator's sentence for it. WSJT-X stops on the same failure and shows its "Rig Control Error";
// here the status lane says it, and keeps saying it until TX is turned on again.

// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts) from birth. Every key is
// written literally.

import type { SlotUnkeyFailed } from '../types'
import type { StatusItem } from '../status'
import { t } from '../i18n'

/** The status-lane item after a failed slot unkey halted TX, or null. `why` is the rig link's own
 *  answer, interpolated as data and never translated. */
export function slotUnkeyFailedLane(
  f: SlotUnkeyFailed | null | undefined,
): Omit<StatusItem, 'id'> | null {
  if (!f) return null
  // UTC, as every time an operator logs is.
  const time = new Date(f.at * 1000).toISOString().slice(11, 19)
  return {
    tier: 'critical',
    message: t('shell.lane.slotUnkeyFailed.message'),
    detail: t('shell.lane.slotUnkeyFailed.detail', { time, why: f.why }),
  }
}
