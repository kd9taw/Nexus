// The words for a settings restore the station refused while the radio is keyed: the backend
// fails it with its own English sentence (`RESTORE_ON_AIR` in `crates/tempo-app/src/engine.rs`),
// and `api.ts` says it with these. Its own module so the catalog guard reads its key
// (`i18n/hardcoded-strings.test.ts`); `api.ts` itself is not a migrated file.

import { t } from '../i18n'

/** The station's refusal of a restore while the radio is keyed, word for word. */
export const RESTORE_ON_AIR = 'Finish or stop the transmission first, then restore.'

/** A restore's refusal in the operator's language when it is the keyed one; any other as it came. */
export function restoreRefusal(err: unknown): unknown {
  return err === RESTORE_ON_AIR ? t('settings.backup.restore.onAir') : err
}
