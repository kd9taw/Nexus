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
// Transmit off: `CAT_SILENT_TX_OFF` in crates/tempo-app/src/engine.rs, the same way.

import { EN, t } from '../i18n'

/** The detail to show: those sentences translated where they appear, everything else as sent. */
export function catDetailText(detail: string): string {
  return detail
    .replace(
      EN['settings.rigControl.serialHandshake.rtsKeys'],
      () => t('settings.rigControl.serialHandshake.rtsKeys'),
    )
    .replace(EN['settings.rigControl.catSilentTxOff'], () => t('settings.rigControl.catSilentTxOff'))
}
