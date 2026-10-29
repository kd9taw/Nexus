// A LoTW upload whose contacts changed while TQSL was signing them. TQSL runs for tens of
// seconds with the log unlocked, and a contact edited or deleted in that window is not marked
// with the upload's result: LoTW holds the version that was signed, so an edited contact is
// offered again with the next upload. The station's report counts them (the connection log
// carries the same counts); this puts them in words, for the Logbook's upload and the Awards
// upload buttons alike.
//
// …and the contacts the upload HELD BACK because TQSL does not know their mode yet (a JTTY
// contact, written `MODE=MFSK SUBMODE=JTTY`): sent, TQSL would drop it and the whole batch would
// read as rejected, so the station keeps it unsent and says so (`heldMode`, the connection log's
// sentence). Without this the screen showed an upload that left a contact behind for no reason.
//
// …and the contacts signed with the Station Location in Settings because no station profile told
// which location signs them (`locationTie`, `locationUnmatched`): a contact signed with a place it
// was not made at is one LoTW never matches, so the operator is told how many, and why. Signing
// from the ADIF (`locationFromAdif`), no Station Location signed them: the words say so.
//
// …and the contacts a later TQSL run never sent (`unsentAfterError`): the batch is signed in one
// run per Station Location, and when one cannot run at all the earlier runs' contacts have gone,
// so the toast says both. A run TQSL signed whose mark could not be shown on disk
// (`unconfirmedAfterError`) went to LoTW: the toast says that, not "not sent".

import { t } from '../i18n'
import type { UploadReport } from '../types'

/** The toast for a LoTW upload that skipped or held back contacts, or null when it recorded every
 *  one — including a report from a station older than the counts, which carries none. */
export function lotwSkipNote(r: UploadReport): string | null {
  const edited = r.skippedEdited ?? 0
  const deleted = r.skippedDeleted ?? 0
  const held = r.heldMode ?? 0
  const skipped =
    edited + deleted === 0
      ? null
      : t('logbook.lotw.skipped.changed', { count: edited + deleted }) +
        (edited > 0 ? t('logbook.lotw.skipped.edited', { count: edited }) : '') +
        (deleted > 0 ? t('logbook.lotw.skipped.deleted', { count: deleted }) : '')
  const heldBack = held > 0 ? t('logbook.lotw.held', { count: held }) : null
  const tie = r.locationTie ?? 0
  const unmatched = r.locationUnmatched ?? 0
  const adif = r.locationFromAdif === true
  const unsent = r.unsentAfterError ?? 0
  const unconfirmed = r.unconfirmedAfterError ?? 0
  const notes = [
    skipped,
    heldBack,
    tie > 0
      ? adif
        ? t('logbook.lotw.location.tieAdif', { count: tie })
        : t('logbook.lotw.location.tie', { count: tie })
      : null,
    unmatched > 0
      ? adif
        ? t('logbook.lotw.location.unmatchedAdif', { count: unmatched })
        : t('logbook.lotw.location.unmatched', { count: unmatched })
      : null,
    unconfirmed > 0
      ? t('logbook.lotw.unconfirmedAfterError', { count: unconfirmed, detail: r.unsentError ?? '' })
      : null,
    unsent > 0
      ? t('logbook.lotw.unsentAfterError', { count: unsent, detail: r.unsentError ?? '' })
      : null,
  ].filter((n): n is string => n != null)
  return notes.length === 0 ? null : notes.join(' ')
}

/** How long the toast stays: three short sentences are more than the default four seconds. */
export const LOTW_SKIP_TOAST_MS = 15_000
