// The words for a contact ANOTHER PROGRAM logged into Nexus through the contact port
// (`tempo_app::log_intake`): the toast that says it landed, and the program a logged contact
// came in from, which the record carries as the app-defined ADIF field `APP_NEXUS_VIA`.
import { t } from '../i18n'

/** The ADIF field naming the program a contact came in from (`tempo_app::log_intake::VIA_FIELD`). */
export const VIA_FIELD = 'APP_NEXUS_VIA'

/** "Logged K1ABC from WSJT-X". */
export function intakeToast(call: string, program: string): string {
  return t('shell.intake.logged', { call, program })
}

/** The program a contact came in from, or null for one logged in Nexus itself. */
export function viaOf(extra: [string, string][] | undefined): string | null {
  const v = extra?.find(([k]) => k === VIA_FIELD)?.[1]?.trim()
  return v ? v : null
}
