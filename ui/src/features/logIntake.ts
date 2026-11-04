// The words for a contact ANOTHER PROGRAM logged into Nexus through the contact port
// (`tempo_app::log_intake`): the toast that says it landed, and the program a logged contact
// came in from, which the record carries as the app-defined ADIF field `APP_NEXUS_VIA`.
import { t } from '../i18n'

/** The ADIF field naming the program a contact came in from (`tempo_app::log_intake::VIA_FIELD`). */
export const VIA_FIELD = 'APP_NEXUS_VIA'

/** What a computer driving the station reads in place of a device's network address
 *  (`tempo_app::log_intake::FROM_A_NETWORK_DEVICE` / `FROM_THIS_COMPUTER`): the address stays at
 *  the station. */
export const FROM_A_NETWORK_DEVICE = '@network'
export const FROM_THIS_COMPUTER = '@local'

/** The program a contact came in from, as the screen names it: its own name, or, read from afar,
 *  a device on the network or this computer in the screen's own words. */
export function programLabel(program: string): string {
  if (program === FROM_A_NETWORK_DEVICE) return t('shell.intake.device.network')
  if (program === FROM_THIS_COMPUTER) return t('shell.intake.device.local')
  return program
}

/** "Logged K1ABC from WSJT-X". */
export function intakeToast(call: string, program: string): string {
  return t('shell.intake.logged', { call, program: programLabel(program) })
}

/** The program a contact came in from, or null for one logged in Nexus itself. */
export function viaOf(extra: [string, string][] | undefined): string | null {
  const v = extra?.find(([k]) => k === VIA_FIELD)?.[1]?.trim()
  return v ? v : null
}
