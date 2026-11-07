// The CAT status / Test CAT detail, the UI half. The detail is the backend's own report and passes
// through verbatim, in English, with TWO exceptions: a launch Nexus refused itself, and a NET rigctl
// radio that was given a serial port. Those sentences are the operator's to act on, so they are
// shown in their language.
//
// The refusal: Serial handshake is declared Hardware (RTS/CTS) on a port where RTS keys the
// transmitter, which would key the rig from connect (crates/tempo-audio/src/rigctld_proc.rs,
// `HARDWARE_HANDSHAKE_RTS_KEYS`). The backend sends the English catalog's sentence exactly, so the
// match is against EN itself; catDetail.test.ts holds the Rust constant to the same words.
//
// NET rigctl on a serial port: `NET_RIGCTL_ON_A_SERIAL_PORT` in the same file, the same way, with
// the operator's own port name in it, which is carried across and never translated.

import { EN, t } from '../i18n'

const escaped = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')

/** The English sentence as a pattern, its first `{{port}}` captured and its second the same name. */
const NET_RIGCTL_SENTENCE = new RegExp(
  EN['settings.rigControl.netRigctl.notConnected'].split('{{port}}')
    .map(escaped)
    .reduce((so_far, part, i) => so_far + (i === 1 ? '(.+?)' : '\\1') + part),
)

/** The detail to show: the two sentences translated where they appear, everything else as sent. */
export function catDetailText(detail: string): string {
  const sentence = EN['settings.rigControl.serialHandshake.rtsKeys']
  return (
    detail.includes(sentence)
      ? detail.replace(sentence, t('settings.rigControl.serialHandshake.rtsKeys'))
      : detail
  ).replace(NET_RIGCTL_SENTENCE, (_, port: string) =>
    t('settings.rigControl.netRigctl.notConnected', { port }),
  )
}
