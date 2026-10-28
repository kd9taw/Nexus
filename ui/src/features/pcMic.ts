// The computer microphone's over (Phone, with the radio set to take this computer's microphone),
// the UI half: an over that ended, or never keyed, for a reason the operator did not choose
// (crates/tempo-app/src/engine/pc_mic.rs). This is the operator's sentence for it; the next over
// that keys clears it.

// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts) from birth. Every key is
// written literally.

import type { PcMicCause, PcMicEnded } from '../types'
import type { StatusItem } from '../status'
import { t } from '../i18n'

function why(cause: PcMicCause): string {
  switch (cause) {
    case 'noDevice':
      return t('shell.lane.pcMic.noDevice')
    case 'sameAsReceive':
      return t('shell.lane.pcMic.sameAsReceive')
    case 'openFailed':
      return t('shell.lane.pcMic.openFailed')
    case 'deviceLost':
      return t('shell.lane.pcMic.deviceLost')
    case 'noAudio':
      return t('shell.lane.pcMic.noAudio')
    case 'ceiling':
      return t('shell.lane.pcMic.ceiling')
    case 'watchdog':
      return t('shell.lane.pcMic.watchdog')
    case 'busy':
      return t('shell.lane.pcMic.busy')
    case 'flexMic':
      return t('shell.lane.pcMic.flexMic')
    case 'radioReceive':
      return t('shell.lane.pcMic.radioReceive')
    case 'loopback':
      return t('shell.lane.pcMic.loopback')
    case 'gateDown':
      return t('shell.lane.pcMic.gateDown')
    case 'interrupted':
      return t('shell.lane.pcMic.interrupted')
    case 'keyRefused':
      return t('shell.lane.pcMic.keyRefused')
  }
}

/** The status-lane item after a computer-microphone over ended or never keyed, or null. */
export function pcMicLane(r: PcMicEnded | null | undefined): Omit<StatusItem, 'id'> | null {
  if (!r) return null
  // UTC, as every time an operator logs is.
  const time = new Date(r.at * 1000).toISOString().slice(11, 19)
  return {
    tier: 'critical',
    message: t('shell.lane.pcMic.message'),
    detail: t('shell.lane.pcMic.detail', { time, why: why(r.cause) }),
  }
}
