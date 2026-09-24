// An export the logbook database had not caught up with (SPEC-2 C15). The operator's ruling:
// write what the database holds — an export still works as a rescue when the disk is failing —
// and say how many recent changes the file lacks. The station counts them; this puts them in
// words, for the Logbook's exports and Settings' WRL export alike.
//
// Two notes, each a toast with its own count, because they are different news: changes still
// being saved will land, and exporting again later picks them up; a change the database refused
// for good will not, and the quit asks about it.

import { t } from '../i18n'
import { pushToast } from '../toast'
import type { LogExport } from '../types'

/** How long each note stays: it arrives beside the export's own toast, and both must be read. */
export const EXPORT_LACKS_TOAST_MS = 15_000

/** Say what `exported` lacked — nothing when it lacked nothing. Several files exported together
 *  (one per operator) are said once, by the most any of them lacked. */
export function sayExportLacks(exported: LogExport[]): void {
  const saving = Math.max(0, ...exported.map((e) => e.saving))
  const held = Math.max(0, ...exported.map((e) => e.held))
  if (saving > 0) pushToast(t('logbook.export.lacks.saving', { count: saving }), 'info', EXPORT_LACKS_TOAST_MS)
  if (held > 0) pushToast(t('logbook.export.lacks.held', { count: held }), 'info', EXPORT_LACKS_TOAST_MS)
}
