// RECENTLY DELETED — the contacts taken out of the log and not put back, newest first, each with
// Restore. A restore puts the contact back whole, under its own id, with every field and upload
// mark it had, so nothing goes to a service again. The list is the log's own history, so it
// survives a restart — on the logbook database. On the fallback path (the log kept in log.adi
// when its database could not be opened) the history lives in memory and starts empty each launch.
// A deleted contact's copy is kept for the days Settings names (90 by default), then removed.
// A restore is refused when the contact was logged again after it was deleted (the same station,
// band and mode within 5 minutes); the row then says so in words and offers Restore anyway, for two
// different contacts — a button of its own on a line of its own, never Restore relabelled under the
// pointer, so a double-click on Restore can never become a Restore anyway. A second click on a row
// within GUARD_MS of its answer is ignored for the same reason.
//
// Above the contacts, the purges and imports that can still be undone whole: the lasting way to undo
// one once its toast has gone. The same undo as the toast's, with its wait while transmitting. A
// whole undo is listed too, with Redo: the lasting way back from one once its own toast has gone.
// Each is asked first, in the dialog, naming how many contacts, from when, and whether they leave
// the log or come back to it: one press can take out a whole import. Only at the station, as a
// purge is: driving the station from another computer the list is read but its buttons are off,
// and a press through the Remote stream undoes nothing.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Calls, bands and modes
// are invariant tokens, shown as they are.
import { useCallback, useEffect, useRef, useState } from 'react'
import {
  recentlyDeletedQsos,
  restoreDeletedQso,
  undoableHistoryRuns,
  undoHistoryRun,
  type DeletedQso,
  type HistoryRun,
} from '../api'
import { t } from '../i18n'
import { withErrorToast } from '../toast'
import { utcDate } from '../features/utcLog'
import { drivesAnotherStation } from '../features/durableStore'
import { isStreamInput } from '../remote-native/stream-input'
import { Dialog } from './ui/Dialog'
import { sayUndone, sourceWord, utcMs } from './QsoHistory'

/** How long after a row's answer a click on that row is taken for the tail of a double-click. */
export const GUARD_MS = 1000

export function RecentlyDeleted({
  open,
  onClose,
  onRestored,
}: {
  open: boolean
  onClose: () => void
  onRestored: () => void
}) {
  const [rows, setRows] = useState<DeletedQso[] | null>(null)
  const [runs, setRuns] = useState<HistoryRun[]>([])
  const [failed, setFailed] = useState(false)
  const [busy, setBusy] = useState<string | null>(null)
  // The rows whose restore was refused because the contact was logged again since.
  const [blocked, setBlocked] = useState<ReadonlySet<string>>(new Set())
  // When each row's last answer came: a click on it sooner than GUARD_MS after is ignored.
  const answeredAt = useRef(new Map<string, number>())
  // One restore or undo at a time, held before React re-renders the buttons disabled.
  const inFlight = useRef(false)
  // The whole run asked about, waiting for the operator's answer.
  const [asking, setAsking] = useState<HistoryRun | null>(null)
  // Why a press did nothing (one through the Remote stream).
  const [note, setNote] = useState<string | null>(null)
  // Driving the station from another computer: the whole runs are the station's own to undo.
  const afar = drivesAnotherStation()

  const load = useCallback(async () => {
    try {
      setRows(await recentlyDeletedQsos())
      setFailed(false)
    } catch {
      setFailed(true)
    }
    // The whole runs are a convenience above the list: one that cannot be read is left out.
    setRuns(await undoableHistoryRuns().catch(() => []))
  }, [])

  useEffect(() => {
    if (open) void load()
    else {
      setRows(null); setRuns([]); setBlocked(new Set()); setAsking(null); setNote(null)
      answeredAt.current.clear()
    }
  }, [open, load])

  const settling = (key: string) => Date.now() - (answeredAt.current.get(key) ?? -Infinity) < GUARD_MS

  const changed = () => {
    void load()
    onRestored()
  }

  const restore = async (r: DeletedQso, anyway: boolean) => {
    if (inFlight.current || settling(r.id)) return
    inFlight.current = true
    setBusy(r.id)
    const answer = await withErrorToast(() => restoreDeletedQso(r.id, anyway), t('logbook.deleted.failed'))
    answeredAt.current.set(r.id, Date.now())
    inFlight.current = false
    setBusy(null)
    if (!answer) return
    if (answer.changed === 0 && answer.duplicate > 0) setBlocked((b) => new Set(b).add(r.id))
    sayUndone(answer, t('logbook.deleted.restored', { call: r.call }), changed)
    changed()
  }

  // Only at the station: a press through the Remote stream undoes no run. Read before anything
  // awaits, inside the stream's dispatch.
  const fromAfar = () => {
    if (!isStreamInput()) return false
    setAsking(null)
    setNote(t('logbook.deleted.run.remote'))
    return true
  }

  const ask = (run: HistoryRun) => {
    if (fromAfar()) return
    setNote(null)
    setAsking(run)
  }

  // A whole purge, import or undo, through the toast's own undo, once the operator said yes.
  // Refused while transmitting, it says why and stays listed, to be pressed again.
  const undoRun = async (run: HistoryRun) => {
    if (fromAfar() || inFlight.current || settling(run.run)) return
    inFlight.current = true
    setAsking(null)
    setBusy(run.run)
    const answer = await withErrorToast(() => undoHistoryRun(run.run), t('qso.history.undo.failed'))
    answeredAt.current.set(run.run, Date.now())
    inFlight.current = false
    setBusy(null)
    if (!answer) return
    sayUndone(
      answer,
      run.kind === 'purge' ? t('logbook.purge.undone')
        : run.kind === 'import' ? t('logbook.import.undone')
        : run.kind === 'contestColumns' ? t('logbook.contestColumns.undone')
        : t('qso.history.redone'),
      changed,
      run.kind === 'undo',
    )
    changed()
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => { if (!o) onClose() }}
      title={t('logbook.deleted.title')}
      description={t('logbook.deleted.intro')}
      className="recently-deleted-dialog"
    >
      {failed && <p className="qso-detail-bare">{t('logbook.deleted.readFailed')}</p>}
      {!failed && rows === null && <p className="qso-detail-bare">{t('logbook.deleted.reading')}</p>}
      {!failed && rows !== null && rows.length === 0 && (
        <p className="qso-detail-bare">{t('logbook.deleted.empty')}</p>
      )}
      {runs.length > 0 && (
        <>
          <p className="qso-detail-bare">{t('logbook.deleted.runsIntro')}</p>
          {note && <p role="alert" className="qso-detail-bare">{note}</p>}
          <ul className="recently-deleted-list">
            {runs.map((run) => (
              <li key={run.run} className="recently-deleted-row">
                <span className="recently-deleted-when">{runLabel(run)}</span>
                <button
                  type="button"
                  className="export-btn"
                  disabled={busy !== null || afar}
                  onClick={() => ask(run)}
                >
                  {run.kind === 'purge' ? t('logbook.deleted.undoPurge')
                    : run.kind === 'import' ? t('logbook.deleted.undoImport')
                    : run.kind === 'contestColumns' ? t('logbook.deleted.undoContestColumns')
                    : t('logbook.deleted.redo')}
                </button>
                {afar && <span className="recently-deleted-when">{t('logbook.deleted.atStation')}</span>}
                {asking?.run === run.run && (
                  // On a line of its own under the row, never where the pointer already is: a
                  // double-click on the row's button cannot answer the question.
                  <span className="recently-deleted-anyway">
                    <span>{runQuestion(run)}</span>
                    <button
                      type="button"
                      className="export-btn danger"
                      disabled={busy !== null}
                      onClick={() => void undoRun(run)}
                    >
                      {run.kind === 'undo' ? t('logbook.deleted.confirm.redo') : t('logbook.deleted.confirm.undo')}
                    </button>
                    <button type="button" className="export-btn" onClick={() => setAsking(null)}>
                      {t('logbook.deleted.confirm.cancel')}
                    </button>
                  </span>
                )}
              </li>
            ))}
          </ul>
        </>
      )}
      {!failed && rows !== null && rows.length > 0 && (
        <ul className="recently-deleted-list">
          {rows.map((r) => (
            <li key={r.id} className="recently-deleted-row">
              <span className="mono recently-deleted-call">{r.call}</span>
              <span className="mono">{utcDate(r.whenUnix)}</span>
              <span className="mono">{`${r.band} ${r.mode}`}</span>
              <span className="recently-deleted-when">
                {t('logbook.deleted.when', { when: utcMs(r.deletedAtMs), source: sourceWord(r.source) })}
              </span>
              <button
                type="button"
                className="export-btn"
                disabled={busy !== null}
                onClick={() => void restore(r, false)}
              >
                {t('logbook.deleted.restore')}
              </button>
              {blocked.has(r.id) && (
                <span className="recently-deleted-anyway">
                  <span>{t('logbook.deleted.restoreAnywayHint')}</span>
                  <button
                    type="button"
                    className="export-btn"
                    disabled={busy !== null}
                    onClick={() => void restore(r, true)}
                  >
                    {t('logbook.deleted.restoreAnyway')}
                  </button>
                </span>
              )}
            </li>
          ))}
        </ul>
      )}
    </Dialog>
  )
}

/** A whole run as the list names it. */
function runLabel(run: HistoryRun): string {
  const count = run.contacts
  const when = utcMs(run.atMs)
  switch (run.kind) {
    case 'purge': return t('logbook.deleted.run.purge', { count, when })
    case 'import': return t('logbook.deleted.run.import', { count, when })
    case 'contestColumns': return t('logbook.deleted.run.contestColumns', { count, when })
    default: return t('logbook.deleted.run.undo', { count, when })
  }
}

/** What the operator is asked before a whole run is undone: how many, from when, and where they go. */
function runQuestion(run: HistoryRun): string {
  const count = run.contacts
  const when = utcDate(Math.floor(run.atMs / 1000))
  // The copy's undo takes nothing out of the log and puts nothing back: its question says what it does.
  if (run.kind === 'contestColumns') return t('logbook.deleted.confirm.contestColumns', { count, when })
  const asked = run.kind === 'purge' ? t('logbook.deleted.confirm.purge', { count, when })
    : run.kind === 'import' ? t('logbook.deleted.confirm.import', { count, when })
    : t('logbook.deleted.confirm.undoRun', { count, when })
  const where = run.removes
    ? t('logbook.deleted.confirm.removes', { count })
    : t('logbook.deleted.confirm.putsBack', { count })
  return `${asked} ${where}`
}
