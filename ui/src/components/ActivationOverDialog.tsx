// ---------------------------------------------------------------------------
// ⭐ THE "ACTIVATION OVER?" CHECKLIST — what Stop on the POTA/SOTA strip OFFERS before the activation
// ends, modelled on the contest's (ContestOverDialog) and holding to its rule: every item is a tick the
// operator makes, none is ticked to begin with, "Keep activating" (and Esc) changes nothing, and
// "End activation" does what is ticked and then ends it.
//
//   · Each park's count today, in the strip's own words over the station's own count.
//   · Export: one ADIF file per park per UTC day this activation has run — the Logbook's own
//     per-activation export (features/activationExport), so the files are exactly the ones the
//     Logbook writes, under POTA's filename. Written to this computer; nothing is uploaded: POTA takes
//     a log only through its own site.
//   · Post QRT: offered only once a spot from here stands, and it goes through Spot me's own confirm.
//     Nothing reaches pota.app or the cluster without that second press.
//   · Reminders, nothing to tick: a park's day under more than one call (POTA counts each call's
//     file on its own, and the file list shows each), a K- park, a contact set carrying no grid, a state line with no
//     state picked, and that the upload is the operator's to make.
//
// ⚠️ IT OPENS BEFORE THE ACTIVATION ENDS: the QRT is refused by the station once it has, and the files
// are named from the activation's parks. THE ORDER IS: the files, then the QRT (its confirm), then
// the end. A file that fails stops the rest, says why, and leaves the activation running.
//
// Nothing here changes a contact: the files are read from the log, the end changes only what the
// next contact is stamped with.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts).
// ---------------------------------------------------------------------------
import { useEffect, useState } from 'react'
import { Dialog } from './ui/Dialog'
import { t } from '../i18n'
import type { Activation, LoggedActivation } from '../types'
import { logActivations } from '../api'
import { isStreamInput } from '../remote-native/stream-input'
import { POTA_MIN_QSOS, activationFilename, exportActivation } from '../features/activationExport'
import { activationFiles, activationParks, isLegacyUsRef, mixedCallsNote, parkProgress } from '../features/potaActivation'

type Tick = 'export' | 'qrt'

export function ActivationOverDialog({
  act,
  canQrt,
  onKeep,
  onEnd,
}: {
  /** The activation Stop was pressed on; `null` is closed. */
  act: Activation | null
  /** A spot from here stands, so a QRT can be offered. */
  canQrt: boolean
  /** Nothing is done: the dialog closes and the activation keeps running. */
  onKeep: () => void
  /** End the activation, after the QRT's own confirm when `qrt`. The host closes the dialog first. */
  onEnd: (opts: { qrt: boolean }) => Promise<void>
}) {
  const [ticked, setTicked] = useState<Set<Tick>>(new Set())
  // `'unreadable'`: the log's activations could not be read — said so, never shown as "none".
  const [files, setFiles] = useState<LoggedActivation[] | 'unreadable' | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const open = act != null
  // Every opening starts with nothing ticked.
  useEffect(() => {
    setTicked(new Set())
    setError(null)
  }, [open])
  // And reads the files the log holds now: once per opening, and again when the UTC day turns
  // while it is open, so the new day's file is listed (each file is read whole at export anyway).
  const utcDay = act?.utcDay
  useEffect(() => {
    setFiles(null)
    if (!open) return
    let live = true
    logActivations()
      .then((list) => live && setFiles(activationFiles(list, act)))
      .catch(() => live && setFiles('unreadable'))
    return () => {
      live = false
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, utcDay])
  if (!act) return null

  const tick = (k: Tick) =>
    setTicked((prev) => {
      const next = new Set(prev)
      if (next.has(k)) next.delete(k)
      else next.add(k)
      return next
    })

  const parks = activationParks(act)
  const legacy = parks.filter(isLegacyUsRef)
  const stateUnpicked = !act.myState && (act.states?.length ?? 0) > 1

  const end = async () => {
    // Only at the station: a press through the Remote stream writes no file, as the Logbook's
    // export is the station's. Read before anything awaits, inside the dispatch.
    if (isStreamInput() && ticked.has('export')) {
      setError(t('activationOver.export.remote'))
      return
    }
    setBusy(true)
    setError(null)
    try {
      if (ticked.has('export')) {
        for (const f of Array.isArray(files) ? files : []) await exportActivation(f)
      }
      await onEnd({ qrt: ticked.has('qrt') && canQrt })
    } catch (e) {
      setError(typeof e === 'string' ? e : e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog
      open
      // Esc and a click outside are "Keep activating" — but never while a step is running.
      onOpenChange={(o) => {
        if (!o && !busy) onKeep()
      }}
      title={t('activationOver.title')}
      description={t('activationOver.desc', { reference: act.reference ?? '' })}
    >
      <div className="activation-over">
        <ul className="activation-over-parks" aria-label={t('activation.pane.parks')}>
          {(act.parks ?? []).map((p) => {
            const g = parkProgress(p.count)
            return (
              <li key={p.reference} className={`pota-act-progress${g.valid ? ' valid' : ''}`}>
                {t('ota.activation.progress', { reference: p.reference, count: p.count })}
                {' · '}
                {g.valid ? t('ota.activation.valid') : t('ota.activation.toGo', { count: g.toGo })}
              </li>
            )
          })}
        </ul>
        <label className="contest-over-item">
          <input
            type="checkbox"
            checked={ticked.has('export')}
            onChange={() => tick('export')}
            disabled={busy || !Array.isArray(files) || files.length === 0}
          />
          <span>
            {files == null
              ? t('activationOver.export.reading')
              : files === 'unreadable'
                ? t('activationOver.export.unreadable')
                : files.length === 0
                  ? t('activationOver.export.none')
                  : t('activationOver.export.label', { count: files.length })}
          </span>
        </label>
        {Array.isArray(files) && files.length > 0 && (
          <ul className="activation-over-files">
            {files.map((f) => (
              <li key={activationFilename(f)} className="mono">
                {activationFilename(f)}
                {' — '}
                {t('activationOver.export.file', { count: f.qsos })}
                {f.qsos < POTA_MIN_QSOS ? t('logbook.export.activation.short', { min: POTA_MIN_QSOS }) : ''}
              </li>
            ))}
          </ul>
        )}
        {canQrt && (
          <label className="contest-over-item" title={t('ota.selfSpot.qrt.title')}>
            <input type="checkbox" checked={ticked.has('qrt')} onChange={() => tick('qrt')} disabled={busy} />
            <span>{t('ota.selfSpot.qrt.label')}</span>
          </label>
        )}
        <ul className="activation-over-notes">
          {(act.parks ?? []).map((p) => {
            const note = mixedCallsNote(p)
            return note ? <li key={`calls-${p.reference}`}>{note}</li> : null
          })}
          {legacy.map((ref) => (
            <li key={ref}>{t('ota.activation.legacyRef', { reference: ref })}</li>
          ))}
          {!act.grid && <li>{t('activationOver.note.noGrid')}</li>}
          {stateUnpicked && <li>{t('activationOver.note.noState')}</li>}
          <li>{t('activationOver.note.upload')}</li>
        </ul>
        {error && (
          <p className="contest-over-error" role="alert">
            {error}
          </p>
        )}
        {/* "Keep activating" first, so it takes the focus: the answer that changes nothing is the default. */}
        <div className="confirm-actions">
          <button type="button" className="settings-refresh" onClick={onKeep} disabled={busy} autoFocus>
            {t('activationOver.keep')}
          </button>
          <button type="button" className="settings-save" onClick={() => void end()} disabled={busy}>
            {busy ? t('activationOver.busy') : t('activationOver.end')}
          </button>
        </div>
      </div>
    </Dialog>
  )
}
