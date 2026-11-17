// The words for a call that client mode refused, or could not make: the backend fails it with
// `nexusClient:<code>` (`src-tauri/src/client_mode.rs`), and `api.ts` says it with these, at the
// one place every call passes. Its own module so the catalog guard reads its keys
// (`i18n/hardcoded-strings.test.ts`); `api.ts` itself is not a migrated file.

import { t } from '../i18n'

/** A call that client mode refused, or could not make, fails with `nexusClient:<code>`
 *  (`src-tauri/src/client_mode.rs`): said here in the operator's language. Literal calls, so the
 *  catalog guard can see each key. */
export function clientWords(code: string): string {
  switch (code) {
    case 'unclassed': return t('client.refused.unclassed')
    case 'runsHere': return t('client.refused.runsHere')
    case 'shackOnly': return t('client.refused.shackOnly')
    case 'atShackOnly': return t('remote.lan.keysLockedWhileStreamed')
    case 'transmitNotYet': return t('client.refused.transmitNotYet')
    case 'heldPress': return t('client.refused.heldPress')
    case 'noStopRoad': return t('client.refused.noStopRoad')
    case 'noPresence': return t('client.refused.noPresence')
    case 'staleState': return t('client.refused.staleState')
    case 'pressEnded': return t('client.refused.pressEnded')
    case 'lockCapped': return t('client.refused.lockCapped')
    case 'recording': return t('client.refused.recording')
    case 'pressHeld': return t('client.refused.pressHeld')
    case 'typedPassword': return t('client.refused.typedPassword')
    case 'notController': return t('client.refused.notController')
    case 'busy': return t('client.refused.busy')
    case 'rateLimited': return t('client.refused.rateLimited')
    case 'replayed': return t('client.refused.replayed')
    case 'changedAtShack': return t('client.refused.changedAtShack')
    case 'badArguments': return t('client.refused.badArguments')
    case 'stopped': return t('client.refused.stopped')
    case 'tooLarge': return t('client.refused.tooLarge')
    case 'gone': return t('client.refused.gone')
    case 'notLive': return t('client.refused.notLive')
    case 'udpBlocked': return t('client.refused.udpBlocked')
    default: return t('client.refused.unknown')
  }
}
