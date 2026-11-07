// The CAT status / Test CAT detail, the UI half. The detail is the backend's own report and passes
// through verbatim, in English, with TWO exceptions: a launch Nexus refused itself, and transmit
// turned off by a silent CAT link. Those sentences are the operator's to act on, so they are shown
// in their language.
//
// The refusal: Serial handshake is declared Hardware (RTS/CTS) on a port where RTS keys the
// transmitter, which would key the rig from connect (crates/tempo-audio/src/rigctld_proc.rs,
// `HARDWARE_HANDSHAKE_RTS_KEYS`). The backend sends the English catalog's sentence exactly, so the
// match is against EN itself; catDetail.test.ts holds the Rust constant to the same words.
//
// Transmit off: `CAT_SILENT_TX_OFF` and `CAT_BACK_TX_OFF` in crates/tempo-app/src/engine.rs, the same way.

import { EN, t } from '../i18n'

/** The detail to show: those sentences translated where they appear, everything else as sent. */
export function catDetailText(detail: string): string {
  return detail
    .replace(
      EN['settings.rigControl.serialHandshake.rtsKeys'],
      () => t('settings.rigControl.serialHandshake.rtsKeys'),
    )
    .replace(EN['settings.rigControl.catSilentTxOff'], () => t('settings.rigControl.catSilentTxOff'))
    .replace(EN['settings.rigControl.catBackTxOff'], () => t('settings.rigControl.catBackTxOff'))
}

/**
 * Does this CAT status say a lost link has turned transmit off, and is the link still lost or back?
 * The backend ends the status with one of the two sentences for exactly as long as that holds, so
 * the TX strip's state follows it with no field of its own: the status is a tooltip on the cockpits,
 * and the strip can be read without hovering.
 */
export function catSaysTxOff(detail: string | undefined): 'lost' | 'back' | null {
  if (detail?.includes(EN['settings.rigControl.catSilentTxOff'])) return 'lost'
  return detail?.includes(EN['settings.rigControl.catBackTxOff']) ? 'back' : null
}
