// The operator's PTT press that Nexus's own Flex client kept off the air, the UI half. Right after
// the transmit slice left a digital mode for Phone, or native audio went off, the radio still took
// its transmit audio from the DAX Nexus had set, not its mic, so the voice would not have gone out;
// the station kept the key off the air and sends when and in what mode
// (crates/tempo-app/src/engine.rs, `set_ptt_refused`). This is the operator's sentence for it, in
// place of the PTT and CAT advice a refused key otherwise gets. TX stays on; the next press answers
// anew.

// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts) from birth. Every key is
// written literally.

import type { PttRefused } from '../types'
import type { StatusItem } from '../status'
import { t } from '../i18n'

/** The status-lane item after a PTT press Nexus kept off the air for the radio's DAX, or null.
 *  `mode` is the radio's own mode word, interpolated as data and never translated. */
export function pttRefusedLane(r: PttRefused | null | undefined): Omit<StatusItem, 'id'> | null {
  if (!r) return null
  // UTC, as every time an operator logs is.
  const time = new Date(r.at * 1000).toISOString().slice(11, 19)
  return {
    tier: 'critical',
    message: t('shell.lane.pttRefused.message'),
    detail: t('shell.lane.pttRefused.detail', { time, mode: r.mode }),
  }
}
