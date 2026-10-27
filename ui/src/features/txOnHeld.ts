// A TX On the sign-in hold refused, the UI half. While a newly signed-in device that is not yet
// approved controls the station, the station starts no transmission and refuses TX On too
// (crates/tempo-app/src/engine.rs, `set_tx_enabled`). The latch just stays dark, so the status lane
// says why, for as long as the hold stands: at the shack, where to approve the device; on a
// computer driving the station, that this device must be approved there. Display only.

// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts) from birth. Every key is
// written literally.

import type { StatusItem } from '../status'
import type { TxHold } from '../types'
import { t } from '../i18n'

/** What a computer driving the station is told of its hold: Allow to transmit at the shack, and
 *  "or about N min" only when the station sent the time left (`leftSecs`, which it sends only
 *  when it would lift the hold by itself). The minutes are rounded up, never below 1. */
export function heldLine(hold: TxHold | null | undefined): string {
  const left = hold?.leftSecs
  if (left == null) return t('client.ptt.refusal.onHold')
  return t('client.ptt.refusal.onHoldLeft', { minutes: String(Math.max(1, Math.ceil(left / 60))) })
}

/** The status-lane item after a TX On the sign-in hold refused, or null. `client` is whether this
 *  computer drives another station, and `hold` the station's `radio.txHold`. */
export function txOnHeldLane(
  held: boolean | undefined,
  client: boolean,
  hold?: TxHold | null,
): Omit<StatusItem, 'id'> | null {
  if (!held) return null
  return {
    tier: 'warning',
    message: t('shell.lane.txOnHeld.message'),
    detail: client ? heldLine(hold) : t('shell.lane.txOnHeld.detail'),
  }
}
