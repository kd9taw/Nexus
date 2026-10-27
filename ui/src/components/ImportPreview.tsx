// ---------------------------------------------------------------------------
// THE IMPORT PREVIEW — what a log file holds and what its import would do, shown BEFORE anything is
// written, then the no-loss report of what the import did.
//
// The engine reads the file in its own format (ADIF, ADX or Cabrillo) and counts by the log's own
// duplicate rule from the plan the import itself would make, let go (`preview_import`), so the
// counts here are the import's. The operator chooses Import, Import new only (every contact the log
// already holds left exactly as it is) or Cancel; nothing is written before that choice.
//
// The import is one run in the log's history, as it always was: the host's toast carries its Undo,
// and Recently deleted can undo it later. After it, this dialog shows what the report found and
// saves the report as a text file through the app's own save path.
// ---------------------------------------------------------------------------
import { useEffect, useState } from 'react'
import { Dialog } from './ui/Dialog'
import { t } from '../i18n'
import { importLog, previewImport, saveTextToDownloads } from '../api'
import { pushToast, failureText } from '../toast'
import {
  droppedCount,
  formatName,
  importFailure,
  noteAt,
  noteLabel,
  reportText,
  unmappedLabel,
  unmappedName,
  utcDay,
  type ImportDone,
  type ImportPreview as Preview,
} from '../features/logImport'
import type { ImportStats } from '../types'
import './ImportPreview.css'

/** How many of the file's fields the coverage list shows before "show all". */
const COVERAGE_SHOWN = 12
/** How many of what is not stored the list shows; the rest are counted. */
const UNMAPPED_SHOWN = 200

export function ImportPreview({
  text,
  fileName,
  onClose,
  onImported,
}: {
  /** The file's text, read in the webview. */
  text: string
  fileName: string
  onClose: () => void
  /** The import's counts and run, for the host's toast and its Undo. */
  onImported: (stats: ImportStats) => void
}) {
  const [preview, setPreview] = useState<Preview | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [done, setDone] = useState<ImportDone | null>(null)
  const [allFields, setAllFields] = useState(false)

  useEffect(() => {
    let live = true
    previewImport(text).then(
      (p) => live && setPreview(p),
      (err: unknown) => live && setError(importFailure(t('logbook.import.preview.failed'), err)),
    )
    return () => {
      live = false
    }
  }, [text])

  const run = async (newOnly: boolean) => {
    setBusy(true)
    try {
      const d = await importLog(text, newOnly)
      setDone(d)
      onImported(d.stats)
    } catch (err) {
      pushToast(importFailure(t('logbook.import.failed'), err), 'error')
    } finally {
      setBusy(false)
    }
  }

  const saveReport = async () => {
    if (!done) return
    const stamp = new Date().toISOString().slice(0, 10)
    const stem = fileName.replace(/\.[^.]*$/, '') || 'log'
    try {
      const path = await saveTextToDownloads(`nexus-import-report-${stem}-${stamp}.txt`, reportText(done.report, fileName, new Date()))
      pushToast(t('logbook.import.done.saved', { path }), 'success')
    } catch (err) {
      pushToast(failureText(t('logbook.import.done.saveFailed'), err), 'error')
    }
  }

  const close = () => {
    if (!busy) onClose()
  }

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) close()
      }}
      title={t('logbook.import.preview.title')}
      description={fileName}
      className="import-preview-dialog"
    >
      <div className="import-preview">
        {error && <p className="import-preview-error">{error}</p>}
        {!error && !preview && <p>{t('logbook.import.preview.reading')}</p>}
        {preview && !done && (
          <>
            <p className="import-preview-format">
              {formatName(preview.format)}
              {preview.template && ` · ${t('logbook.import.preview.template', { contest: preview.template })}`}
            </p>
            <ul className="import-preview-counts">
              <li>{t('logbook.import.preview.records', { count: preview.records })}</li>
              {preview.firstUnix !== null && preview.lastUnix !== null && (
                <li>
                  {t('logbook.import.preview.range', { from: utcDay(preview.firstUnix), to: utcDay(preview.lastUnix) })}
                </li>
              )}
              <li className="import-preview-new">{t('logbook.import.preview.new', { count: preview.new })}</li>
              <li>{t('logbook.import.preview.dupes', { count: preview.duplicates })}</li>
              {preview.updates > 0 && <li>{t('logbook.import.preview.updates', { count: preview.updates })}</li>}
            </ul>

            <h3>{t('logbook.import.preview.problems')}</h3>
            {preview.notes.length === 0 ? (
              <p className="import-preview-ok">{t('logbook.import.preview.noProblems')}</p>
            ) : (
              <ul className="import-preview-notes">
                {preview.notes.map((g) => (
                  <li key={g.kind}>
                    <span className="import-preview-note">
                      {noteLabel(g.kind)}: <strong>{g.count}</strong>
                    </span>
                    <ul>
                      {g.examples.map((e, i) => (
                        <li key={i} className="mono">
                          {noteAt(preview.format, e.at)}: {e.text}
                        </li>
                      ))}
                    </ul>
                  </li>
                ))}
              </ul>
            )}

            {preview.unmapped.length > 0 && (
              <>
                <h3>{t('logbook.import.preview.unmapped')}</h3>
                <ul className="import-preview-unmapped">
                  {preview.unmapped.slice(0, UNMAPPED_SHOWN).map((u, i) => (
                    <li key={i}>
                      <span className="mono">{unmappedName(u)}</span> ×{u.count} · {unmappedLabel(u.reason)}
                    </li>
                  ))}
                  {preview.unmapped.length > UNMAPPED_SHOWN && (
                    <li>{t('logbook.import.preview.moreUnmapped', { count: preview.unmapped.length - UNMAPPED_SHOWN })}</li>
                  )}
                </ul>
              </>
            )}

            <h3>{t('logbook.import.preview.coverage')}</h3>
            <ul className="import-preview-coverage mono">
              {(allFields ? preview.coverage : preview.coverage.slice(0, COVERAGE_SHOWN)).map((c) => (
                <li key={c.field}>
                  {c.field} <span className="import-preview-count">{c.count}</span>
                </li>
              ))}
            </ul>
            {!allFields && preview.coverage.length > COVERAGE_SHOWN && (
              <button type="button" className="import-preview-more" onClick={() => setAllFields(true)}>
                {t('logbook.import.preview.allFields', { count: preview.coverage.length })}
              </button>
            )}

            <div className="logconfirm-actions">
              <button type="button" className="logconfirm-discard" onClick={close} disabled={busy}>
                {t('logbook.import.preview.cancel')}
              </button>
              {preview.duplicates > 0 && (
                <button
                  type="button"
                  className="logconfirm-discard"
                  onClick={() => void run(true)}
                  disabled={busy || preview.new === 0}
                  title={t('logbook.import.preview.importNewHint')}
                >
                  {t('logbook.import.preview.importNew')}
                </button>
              )}
              <button
                type="button"
                className="logconfirm-log"
                onClick={() => void run(false)}
                disabled={busy || preview.contacts === 0}
              >
                {busy ? t('logbook.import.preview.busy') : t('logbook.import.preview.import')}
              </button>
            </div>
          </>
        )}
        {done && (
          <>
            <p>
              {t('logbook.import.imported', { count: done.stats.added })}
              {done.stats.skipped ? t('logbook.import.dupes', { count: done.stats.skipped }) : ''}
              {done.stats.updated ? t('logbook.import.updated', { count: done.stats.updated }) : ''}
            </p>
            <p className={done.report.accounted ? 'import-preview-ok' : 'import-preview-error'}>
              {done.report.accounted ? t('logbook.import.done.accounted') : t('logbook.import.done.notAccounted')}
            </p>
            {droppedCount(done.report) > 0 && (
              <p>{t('logbook.import.done.dropped', { count: droppedCount(done.report) })}</p>
            )}
            <div className="logconfirm-actions">
              <button type="button" className="logconfirm-discard" onClick={() => void saveReport()}>
                {t('logbook.import.done.save')}
              </button>
              <button type="button" className="logconfirm-log" onClick={onClose}>
                {t('logbook.import.done.close')}
              </button>
            </div>
          </>
        )}
      </div>
    </Dialog>
  )
}
