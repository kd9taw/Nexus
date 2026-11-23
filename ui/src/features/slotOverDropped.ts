// An FT8 or FT4 over the radio keyed only after its period had ended, the UI half. A slow CAT link
// (Split operation's commands before the key, and the key itself) took the carrier into the other
// station's period, so the station unkeyed it at once, sent nothing and left TX on for the next over,
// however many in a row, as WSJT-X starts no audio once its window has closed and only its Tx
// watchdog ends the run (crates/tempo-app/src/engine.rs, `drop_over_keyed_past_its_period`). This is
// the operator's sentence for it, a lane of its own: the refused key's "TX STOPPED" would be wrong
// while TX is on.

// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts) from birth. Every key is
// written literally.

import type { SlotOverDropped } from '../types'
import type { StatusItem } from '../status'
import { t } from '../i18n'

/** The status-lane item after a dropped FT8/FT4 over, or null: a warning, TX being on. `late` is how
 *  long after the period's end the radio keyed it, interpolated as data. */
export function slotOverDroppedLane(
  d: SlotOverDropped | null | undefined,
): Omit<StatusItem, 'id'> | null {
  if (!d) return null
  // UTC, as every time an operator logs is.
  const time = new Date(d.at * 1000).toISOString().slice(11, 19)
  return {
    tier: 'warning',
    message: t('shell.lane.slotOverDropped.message'),
    detail: t('shell.lane.slotOverDropped.detail', { time, late: d.late }),
  }
}
