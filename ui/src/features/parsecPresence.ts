// Parsec presence mode, the UI half: the words for what the station reports. The station sends
// tokens only; it decides what to stop and stops it (crates/tempo-app/src/engine/parsec_presence.rs).
// Pure, so the placement rule, the readout and the lane item are testable without a panel.

// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts) from birth. Every key is
// written LITERALLY — a `t(variable)` would be skipped by the placeholder guard, whose skip count
// is capped — so the token tables are switches, not lookups.

import type { ParsecPresence } from '../types'
import type { StatusItem } from '../status'
import { t } from '../i18n'

/**
 * Where the switch appears. The Parsec HOST is Windows, so it is offered there and explained as
 * unavailable elsewhere; the Remote page never shows it — the key is not in Remote's settings
 * projection, and the page's command surface is frozen.
 */
export function parsecSwitchPlacement(
  isWindows: boolean,
  remote: boolean,
): 'offered' | 'unavailable' | 'hidden' {
  if (remote) return 'hidden'
  return isWindows ? 'offered' : 'unavailable'
}

/** The Settings readout under the switch: what the watcher last found in Parsec's log. A status
 *  this build does not know reads as "checking" — never as a claim either way. */
export function parsecStatusText(status: string): string {
  switch (status) {
    case 'connected':
      return t('settings.transmit.parsecStop.status.connected')
    case 'notConnected':
      return t('settings.transmit.parsecStop.status.notConnected')
    case 'unreadable':
      return t('settings.transmit.parsecStop.status.unreadable')
    default:
      return t('settings.transmit.parsecStop.status.starting')
  }
}

/** One stopped transmission, by the token the station sends. */
function stoppedName(token: string): string {
  switch (token) {
    case 'ptt':
      return t('shell.lane.parsecStop.what.ptt')
    case 'rtty':
      return t('shell.lane.parsecStop.what.rtty')
    case 'psk':
      return t('shell.lane.parsecStop.what.psk')
    case 'tune':
      return t('shell.lane.parsecStop.what.tune')
    default:
      return token
  }
}

/**
 * The status-lane item after presence mode stopped a transmission, or null. Said in the lane
 * rather than a toast: the operator was not there when it happened, so the reason has to still
 * be on screen when they reconnect. The station keeps it until they transmit again.
 */
export function parsecStopLane(p: ParsecPresence | null | undefined): Omit<StatusItem, 'id'> | null {
  if (!p || p.stoppedAt == null || p.stopped.length === 0) return null
  // UTC, as every time an operator logs is.
  const time = new Date(p.stoppedAt * 1000).toISOString().slice(11, 19)
  const what = p.stopped.map(stoppedName).join(', ')
  return {
    tier: 'warning',
    message: t('shell.lane.parsecStop.message'),
    detail: t('shell.lane.parsecStop.detail', { time, what }),
  }
}
