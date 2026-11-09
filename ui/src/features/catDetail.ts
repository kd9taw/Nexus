// The CAT status / Test CAT detail, the UI half. The detail is the backend's own report and passes
// through verbatim, in English, with FOUR exceptions: a launch Nexus refused itself for the serial
// handshake, one it refused because this Hamlib cannot confirm the RTS/DTR holds, a NET rigctl
// radio that was given a serial port, and transmit turned off by a silent CAT link. Those sentences
// are the operator's to act on, so they are shown in their language.
//
// The refusal: Serial handshake is declared Hardware (RTS/CTS) on a port where RTS keys the
// transmitter, which would key the rig from connect (crates/tempo-audio/src/rigctld_proc.rs,
// `HARDWARE_HANDSHAKE_RTS_KEYS`). The backend sends the English catalog's sentence exactly, so the
// match is against EN itself; catDetail.test.ts holds the Rust constant to the same words.
//
// The unconfirmed holds: Hamlib gave no whole show-conf dump in time, so Nexus could not confirm
// it would hold RTS and DTR low, and did not open the port (`HAMLIB_CANNOT_CONFIRM_HOLDS` in the
// same file), matched the same way.
//
// NET rigctl on a serial port: `NET_RIGCTL_ON_A_SERIAL_PORT` in the same file, the same way, with
// the operator's own port name in it, which is carried across and never translated.
//
// Transmit off: `CAT_SILENT_TX_OFF` and `CAT_BACK_TX_OFF` in crates/tempo-app/src/engine.rs, the same way.

import { EN, t } from '../i18n'

const escaped = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')

/** The English sentence as a pattern, its first `{{port}}` captured and its second the same name. */
const NET_RIGCTL_SENTENCE = new RegExp(
  EN['settings.rigControl.netRigctl.notConnected'].split('{{port}}')
    .map(escaped)
    .reduce((so_far, part, i) => so_far + (i === 1 ? '(.+?)' : '\\1') + part),
)

/** The detail to show: those sentences translated where they appear, everything else as sent. */
export function catDetailText(detail: string): string {
  return detail
    .replace(
      EN['settings.rigControl.serialHandshake.rtsKeys'],
      () => t('settings.rigControl.serialHandshake.rtsKeys'),
    )
    .replace(
      EN['settings.rigControl.hamlibCannotConfirmHolds'],
      () => t('settings.rigControl.hamlibCannotConfirmHolds'),
    )
    .replace(EN['settings.rigControl.catSilentTxOff'], () => t('settings.rigControl.catSilentTxOff'))
    .replace(EN['settings.rigControl.catBackTxOff'], () => t('settings.rigControl.catBackTxOff'))
    .replace(NET_RIGCTL_SENTENCE, (_, port: string) =>
      t('settings.rigControl.netRigctl.notConnected', { port }),
    )
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
