// A LoTW upload whose contacts changed while TQSL was signing them. TQSL runs for tens of
// seconds with the log unlocked, and a contact edited or deleted in that window is not marked
// with the upload's result: LoTW holds the version that was signed, so an edited contact is
// offered again with the next upload. The station's report counts them (the connection log
// carries the same counts); this puts them in words, for the Logbook's upload and the Awards
// upload buttons alike.

import { t } from '../i18n'
import type { UploadReport } from '../types'

/** The toast for a LoTW upload that skipped contacts, or null when it recorded every one —
 *  including a report from a station older than the counts, which carries none. */
export function lotwSkipNote(r: UploadReport): string | null {
  const edited = r.skippedEdited ?? 0
  const deleted = r.skippedDeleted ?? 0
  if (edited + deleted === 0) return null
  return (
    t('logbook.lotw.skipped.changed', { count: edited + deleted }) +
    (edited > 0 ? t('logbook.lotw.skipped.edited', { count: edited }) : '') +
    (deleted > 0 ? t('logbook.lotw.skipped.deleted', { count: deleted }) : '')
  )
}

/** How long the toast stays: three short sentences are more than the default four seconds. */
export const LOTW_SKIP_TOAST_MS = 15_000
