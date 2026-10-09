// ENTER SENDS MESSAGE — its words. The steps' names, and one sentence for each reason the step
// table gives (`esm.ts`): why Enter sent nothing and logged nothing (a refusal), or why ESM
// stepped aside and Enter logs as it does with ESM off. One reason, one sentence, so a reason
// reads the same in the strip, on the ESM plate and in Settings' role picker.
//
// CQ, TU and AGN are on-air words and invariant, held as constants the way the macro captions
// are; the other steps are words, named in the catalog. A key name (F1–F8) is invariant too.
import { t } from '../i18n'
import { slotCaption } from './contestSlots'
import type { EsmInert, EsmRefusal, EsmRole } from './esm'

const CQ = 'CQ'
const TU = 'TU'
const AGN = 'AGN'

/** A step's name, as the strip and the role picker say it. */
export function esmStepName(role: EsmRole): string {
  switch (role) {
    case 'cq':
      return CQ
    case 'callExch':
      return t('contest.esm.step.callExch')
    case 'tu':
      return TU
    case 'myCall':
      return t('contest.esm.step.myCall')
    case 'exch':
      return t('contest.esm.step.exch')
    case 'again':
      return AGN
  }
}

/** Why Enter sent nothing and logged nothing. A step's refusal names the step, and the key. */
export function esmRefusalText(refusal: EsmRefusal): string {
  switch (refusal.why) {
    case 'dupe':
      return t('contest.esm.refused.dupe')
    case 'txOff':
      return t('contest.esm.refused.txOff')
    case 'txLocked':
      return t('contest.esm.refused.txLocked')
    case 'clockRepair':
      return t('contest.esm.refused.clockRepair')
    case 'recording':
      return t('contest.esm.refused.recording')
    case 'pttHeld':
      return t('contest.esm.refused.pttHeld')
    case 'radioHasMic':
      return t('contest.esm.refused.radioHasMic')
    case 'unmapped':
      return t('contest.esm.refused.unmapped', { step: esmStepName(refusal.role) })
    case 'empty':
      return t('contest.esm.refused.empty', { step: esmStepName(refusal.role), key: refusal.key })
    case 'oneSlot':
      return t('contest.esm.refused.oneSlot', { step: esmStepName(refusal.role) })
    case 'history':
      return t('contest.esm.refused.history', { field: slotCaption(refusal.slot) })
  }
}

/** Why ESM stepped aside: Enter logs as it does with ESM off. */
export function esmInertText(why: EsmInert): string {
  switch (why) {
    case 'noKeyer':
      return t('contest.esm.inert.noKeyer')
    case 'auto':
      return t('contest.esm.inert.auto')
    case 'continuousTx':
      return t('contest.esm.inert.continuousTx')
    case 'noRoles':
      return t('contest.esm.inert.noRoles')
  }
}
