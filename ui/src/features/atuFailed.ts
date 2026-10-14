// An ATU press the button accepted that came to nothing in the radio loop, the UI half. The button
// answers every refusal it can see with a toast; after it has accepted a press, the loop can still
// drop it (stale, a transmit gate closed, the connection was changing over), the radio can refuse
// the tune-up, or the tune-up can go out with no answer (crates/tempo-app/src/engine.rs,
// `note_atu_failed`). This is the operator's sentence for each, on the status lane. The next press
// the button accepts clears it.

// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts) from birth. Every key is
// written literally.

import type { AtuFailed } from '../types'
import type { StatusItem } from '../status'
import { t } from '../i18n'

/** The status-lane item after an ATU press that came to nothing, or null. */
export function atuFailedLane(f: AtuFailed | null | undefined): Omit<StatusItem, 'id'> | null {
  if (!f) return null
  // UTC, as every time an operator logs is.
  const time = new Date(f.at * 1000).toISOString().slice(11, 19)
  if (f.cause === 'refused') {
    return {
      tier: 'warning',
      message: t('shell.lane.atuFailed.refused.message'),
      detail: t('shell.lane.atuFailed.refused.detail', { time }),
    }
  }
  if (f.cause === 'noAnswer') {
    return {
      tier: 'warning',
      message: t('shell.lane.atuFailed.noAnswer.message'),
      detail: f.stoodDown
        ? t('shell.lane.atuFailed.noAnswer.stoodDown', { time })
        : t('shell.lane.atuFailed.noAnswer.detail', { time }),
    }
  }
  return {
    tier: 'warning',
    message: t('shell.lane.atuFailed.dropped.message'),
    detail: t('shell.lane.atuFailed.dropped.detail', { time, why: f.why ?? '' }),
  }
}
