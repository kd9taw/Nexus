// THE PER-ACTIVATION EXPORT — the file POTA asks for, one park × one UTC day × the callsign signed,
// written by the Logbook's Export activation and by the "Activation over?" checklist alike. One
// function, so the two cannot write different files: the same command, the same name, the same
// toasts.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts).
import {
  exportLogForActivation,
  noteActivationExported,
  openExternalUrl,
  revealInDownloads,
  saveTextToDownloads,
} from '../api'
import { t } from '../i18n'
import { pushToast } from '../toast'
import type { LogExport, LoggedActivation } from '../types'
import { sayExportLacks } from './exportLacks'
import { isLegacyUsRef } from './potaActivation'

/**
 * Contacts POTA requires before a park-day counts as an activation: "A successful activation
 * requires a minimum of 10 QSOs from a park in the designated list within a single UTC day (Zulu
 * day)" — docs.pota.app/docs/rules.html, read 2026-09-09. Shown, never enforced: a four-contact
 * afternoon is a real day at a park, and POTA accepts the log either way.
 */
export const POTA_MIN_QSOS = 10

/**
 * POTA's page for sending in a log, the one its documentation links as "My Log Uploads"
 * (docs.pota.app/docs/activator_reference/submitting_logs.html, read 2026-11-07). It is a page on
 * POTA's own site, so it can move; this is the one place to change when it does.
 */
export const POTA_UPLOAD_URL = 'https://pota.app/#/user/logs'

/** How long the next step stays up: long enough to read it, then find the browser and log in. */
const NEXT_STEP_TOAST_MS = 20_000

/** The three fields that identify an activation, as one `<select>` value. */
export function activationKeyOf(a: LoggedActivation): string {
  return `${a.reference}|${a.dayStartUnix}|${a.callsign ?? ''}`
}

/** Whether POTA is the program this activation goes to. A record with no program is an older POTA one. */
export function isPotaActivation(a: Pick<LoggedActivation, 'program'>): boolean {
  return (a.program ?? 'POTA').toUpperCase() === 'POTA'
}

/**
 * POTA's own submission filename: `callsign@parkNumber-activationDate.adi`
 * (docs.pota.app/docs/activator_reference/submitting_logs.html, read 2026-09-09 — their example
 * is `KA8H@US-1515-20201127.adi`). A correctly named file is half the submission, so this writes
 * the name POTA documents rather than a Nexus-shaped one.
 *
 * Two departures, both forced:
 * - A portable callsign carries a `/`, and no filesystem holds one — `KD9TAW/P` becomes
 *   `KD9TAW-P` IN THE FILENAME ONLY. The ADIF inside still carries the call exactly as logged,
 *   and that copy is the one POTA reads. POTA's documentation does not address portable suffixes
 *   anywhere (checked 2026-09-09), so nothing here is a guess at a convention that exists.
 * - A SOTA summit is not a POTA submission, so it gets a plain name instead of POTA's `@` form.
 *   Naming a summit file under POTA's convention would be a claim about a program POTA does not
 *   run.
 */
export function activationFilename(a: LoggedActivation): string {
  const safe = (v: string) => v.replace(/[^A-Za-z0-9._-]+/g, '-')
  const call = safe(a.callsign ?? '')
  const ref = safe(a.reference)
  const ymd = a.date.replace(/-/g, '')
  if (isPotaActivation(a) && call) return `${call}@${ref}-${ymd}.adi`
  // No callsign stamped on the records (a log imported from before Nexus stamped one), or a
  // program that is not POTA: name it plainly rather than half-filling POTA's convention.
  return ['nexus-log', call, ref, ymd].filter(Boolean).join('-') + '.adi'
}

/** Write one activation's file to Downloads and say so: how many contacts it holds and where it is,
 *  that a K- park exports as it was logged, and what the file lacks. Rejects with the transport's
 *  reason; the caller says it failed. */
export async function exportActivation(a: LoggedActivation): Promise<LogExport & { path: string }> {
  const exported = await exportLogForActivation(a.reference, a.dayStartUnix, a.callsign ?? null)
  const text = exported.text
  // Count what the file actually holds, as the ranged export does.
  const n = (text.match(/<eor>/gi) ?? []).length
  const path = await saveTextToDownloads(activationFilename(a), text)
  // Recorded only now the file is on disk, so a later change to these contacts can say it no longer
  // matches the log. A failure to record loses only that warning, never the file: it is not thrown.
  await noteActivationExported(a.reference, a.dayStartUnix, a.callsign ?? null).catch(() => {})
  pushToast(t('logbook.export.done', { count: n, path }), 'success')
  // A K- park exports as it was logged; whether POTA's upload still takes the pre-2024 prefix is
  // not known, so the file is written and that is said.
  if (isLegacyUsRef(a.reference)) {
    pushToast(t('ota.activation.legacyRef', { reference: a.reference }), 'info', 12_000)
  }
  sayExportLacks([exported])
  return { ...exported, path }
}

/**
 * The last step of an activation, up to where Nexus stops: open POTA's upload page in the browser
 * and show the exported file, so the operator can log in and drop it there. Nexus holds no POTA
 * login and makes no request to POTA; the browser does all of that. So nothing comes back to say
 * the log arrived, and no word here may claim it did: POTA's page is what says so.
 *
 * Each half can fail alone and neither stops the other. Never rejects: every failure is said here,
 * with what to do instead. With no internet the browser shows its own offline page, which costs
 * nothing, and the file stays in Downloads.
 */
export async function openPotaUpload(file: { filename: string; path: string }): Promise<void> {
  const { filename, path } = file
  const opened = await openExternalUrl(POTA_UPLOAD_URL).then(
    () => true,
    () => false,
  )
  if (!opened) {
    pushToast(
      t('logbook.export.potaUpload.noBrowser', { url: POTA_UPLOAD_URL, filename }),
      'error',
      NEXT_STEP_TOAST_MS,
    )
  }
  // After the browser, so the file manager is the window left on top with the file to drag.
  const shown = await revealInDownloads(filename).catch(() => null)
  if (shown === false) {
    pushToast(t('logbook.export.potaUpload.moved', { filename }), 'error', NEXT_STEP_TOAST_MS)
  } else if (shown === null) {
    pushToast(t('logbook.export.potaUpload.noFileManager', { path }), 'error', NEXT_STEP_TOAST_MS)
  } else if (opened) {
    pushToast(t('logbook.export.potaUpload.next', { filename }), 'info', NEXT_STEP_TOAST_MS)
  }
}
