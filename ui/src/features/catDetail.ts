// The CAT status / Test CAT detail, the UI half. The detail is the backend's own report and passes
// through verbatim, in English, with ONE exception: a launch Nexus refused itself. That sentence is
// the operator's to act on, so it is shown in their language.
//
// The refusal: Serial handshake is declared Hardware (RTS/CTS) on a port where RTS keys the
// transmitter, which would key the rig from connect (crates/tempo-audio/src/rigctld_proc.rs,
// `HARDWARE_HANDSHAKE_RTS_KEYS`). The backend sends the English catalog's sentence exactly, so the
// match is against EN itself; catDetail.test.ts holds the Rust constant to the same words.

import { EN, t } from '../i18n'

/** The detail to show: the refusal sentence translated where it appears, everything else as sent. */
export function catDetailText(detail: string): string {
  const sentence = EN['settings.rigControl.serialHandshake.rtsKeys']
  return detail.includes(sentence)
    ? detail.replace(sentence, t('settings.rigControl.serialHandshake.rtsKeys'))
    : detail
}
