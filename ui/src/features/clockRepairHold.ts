// A clock repair holds transmit, the UI half. From the press of Repair clock until the repair ends,
// two minutes at most, the station starts no transmission, so the clock cannot move in the middle
// of an over (crates/tempo-app/src/engine.rs, `hold_tx_for_clock_repair`). The status lane says so
// for as long as it lasts, on every screen.

// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts) from birth. Every key is
// written literally.

import type { StatusItem } from '../status'
import { t } from '../i18n'

/** The status-lane item while a clock repair holds transmit, or null. */
export function clockRepairHoldLane(held: boolean | undefined): Omit<StatusItem, 'id'> | null {
  if (!held) return null
  return {
    tier: 'warning',
    message: t('shell.lane.clockRepairHold.message'),
    detail: t('shell.lane.clockRepairHold.detail'),
  }
}
