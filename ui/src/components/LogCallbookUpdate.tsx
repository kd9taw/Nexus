// UPDATE FROM CALLBOOK — fill the EMPTY fields of many contacts from the callbook: the ones ticked in
// the list, or every one the list's search and filters show. Each distinct call is looked up once,
// through the same lookup (and its cache) the callsign card uses, at the station, slowly: one
// network lookup every few seconds (Settings ▸ Callbook lookups), never more in a day than the cap
// set there. A call the cache already answers costs nothing. Each call asks the services set up in
// order — QRZ, then HamQTH, then callook.info for a US call — the next only when the one before has
// no record, all in the call's one paced turn. The fill is the log's own callbook rule: a field the
// contact holds is never replaced, a contest exchange's fields are never touched, the call,
// country, upload marks and confirmations stay, and nothing is uploaded. Where the station is
// (grid, QTH, state, county, zones, position, UTC offset) goes only on a contact from the
// last 12 months: a callbook says where a station is now.
//
// Preview first: the contacts, the calls, the services asked, how many are already looked up, the
// time the rest take, the day's lookups against the cap, per field how many contacts it could fill,
// and how many older contacts keep their location blank. The button
// then asks once more before it starts. Progress shows while it runs, with Cancel; whatever stops
// it (done, cancelled, the day's cap, QRZ refusing, a failed lookup), what it found is ONE change
// the toast's Undo takes back, and a later run picks up the rest from the cache at no cost.
//
// The end is the Logbook's to report (useCallbookUpdateEnd), not this pane's: the operator may
// close the dialog and keep operating for the minutes a run takes, and the toast, its Undo and the
// list's reload must come all the same. The pane shows the progress, and the end when it is open.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Field names and calls are
// invariant tokens, shown as they are.
import { useCallback, useEffect, useRef, useState } from 'react'
import {
  callbookUpdateCancel,
  callbookUpdatePreview,
  callbookUpdateStart,
  callbookUpdateStatus,
  callbookUpdateTakeEnd,
  type BulkTarget,
  type CallbookUpdateEnd,
  type CallbookUpdatePreview,
  type CallbookUpdateStatus,
} from '../api'
import { t } from '../i18n'
import { pushToast, withErrorToast } from '../toast'
import type { LogQuery } from '../features/logQuery'
import { changesRefusedHere, sayRun } from '../features/logCleanup'

/** Each service's name, as the preview lists the ones each call is asked of. */
function serviceName(s: CallbookUpdatePreview['services'][number]): string {
  return s === 'qrz' ? t('logbook.tools.callbook.service.qrz')
    : s === 'hamqth' ? t('logbook.tools.callbook.service.hamqth')
    : t('logbook.tools.callbook.service.callook')
}

/** How often the progress is asked while an update runs. */
export const POLL_MS = 1000

/** What QRZ said, in its own words (its spec asks that its Message and Error reach the operator),
 *  and whether it was left out for want of the XML subscription. */
function QrzNotes({ skipped, said }: { skipped: boolean, said: string | null }) {
  return (
    <>
      {skipped && <p className="qso-detail-bare">{t('logbook.tools.callbook.qrzSkipped')}</p>}
      {said && (
        <p className="qso-detail-bare">
          {t('logbook.tools.callbook.qrzSaid')} <span className="mono">{said}</span>
        </p>
      )}
    </>
  )
}

/** Tell the operator how an update ended: the toast whose Undo takes its fill back (a fill stopped
 *  part-way too), why it stopped when it did not finish, and the list reloaded. */
function reportEnd(end: CallbookUpdateEnd, onChanged: () => void): void {
  const why = end.error ?? (end.stop === 'refused' ? t('logbook.tools.callbook.end.refused') : null)
  if (why) pushToast(why, 'error', 12_000)
  if (end.made) sayRun(end.made, t('logbook.tools.callbook.done', { count: end.made.changed }), onChanged)
  else if (!why) pushToast(t('logbook.tools.nothing'), 'info', 8000)
  if (end.made && end.made.changed > 0) onChanged()
}

/** The Logbook's half: reports an update's end once, with the dialog open or not. Answers the
 *  function to call when an update starts; one already running when the Logbook mounts is followed,
 *  and one that ended unreported is reported. */
export function useCallbookUpdateEnd(onChanged: () => void): () => void {
  const [following, setFollowing] = useState(false)
  // Reported once, even when two takes overlap.
  const live = useRef(false)
  const changed = useRef(onChanged)
  useEffect(() => { changed.current = onChanged })

  const take = useCallback(async () => {
    const end = await callbookUpdateTakeEnd()
    if (!end || !live.current) return
    live.current = false
    setFollowing(false)
    reportEnd(end, () => changed.current())
  }, [])

  useEffect(() => {
    const check = async () => {
      const s = await callbookUpdateStatus()
      live.current = true
      if (s.running) setFollowing(true)
      else await take()
    }
    check().catch(() => {})
  }, [take])

  useEffect(() => {
    if (!following) return
    const timer = setInterval(() => { take().catch(() => {}) }, POLL_MS)
    return () => clearInterval(timer)
  }, [following, take])

  return useCallback(() => {
    live.current = true
    setFollowing(true)
  }, [])
}

export function LogCallbookUpdate({
  onStarted,
  afar,
  picked,
  query,
  shown,
}: {
  /** An update started: the Logbook follows it to its end (useCallbookUpdateEnd). */
  onStarted: () => void
  afar: boolean
  picked: ReadonlySet<string>
  query: LogQuery
  shown: number
}) {
  const [which, setWhich] = useState<'picked' | 'shown'>(picked.size > 0 ? 'picked' : 'shown')
  const [preview, setPreview] = useState<CallbookUpdatePreview | null>(null)
  const [asking, setAsking] = useState(false)
  const [busy, setBusy] = useState(false)
  const [status, setStatus] = useState<CallbookUpdateStatus | null>(null)
  const inFlight = useRef(false)

  const target = (): BulkTarget => (which === 'picked' ? { ids: [...picked] } : { query })
  const queryKey = JSON.stringify(query)
  const pickedKey = [...picked].join(' ')
  useEffect(() => { setPreview(null); setAsking(false) }, [queryKey, pickedKey])

  // One running, or the last one's end: its progress, or how it ended.
  useEffect(() => {
    let live = true
    callbookUpdateStatus().then((s) => {
      if (live) setStatus(s)
    }).catch(() => {})
    return () => { live = false }
  }, [])

  const running = status?.running ?? false
  useEffect(() => {
    if (!running) return
    const timer = setInterval(() => {
      callbookUpdateStatus().then(setStatus).catch(() => {})
    }, POLL_MS)
    return () => clearInterval(timer)
  }, [running])

  const onPreview = async () => {
    setBusy(true)
    const p = await withErrorToast(() => callbookUpdatePreview(target()), t('logbook.tools.bulk.previewFailed'))
    setBusy(false)
    setPreview(p)
    setAsking(false)
  }

  const onStart = async () => {
    if (changesRefusedHere() || inFlight.current || !preview) return
    inFlight.current = true
    setBusy(true)
    setAsking(false)
    const started = await withErrorToast(
      () => callbookUpdateStart(preview.ids, preview.selected).then(() => true),
      t('logbook.tools.callbook.failed'),
    )
    inFlight.current = false
    setBusy(false)
    if (!started) return
    onStarted()
    setPreview(null)
    setStatus({
      running: true, done: 0, total: preview.calls, fromCache: 0, lookedUp: 0,
      spent: preview.spent, cap: preview.cap, waitingForTx: false, qrzSaid: null, qrzSkipped: false,
      ended: null,
    })
  }

  const left = preview ? Math.max(0, preview.cap - preview.spent) : 0
  // At most: a call still to look up is counted as if it filled every field it could.
  const upTo = preview ? preview.toLookUp > 0 : false
  const minutes = preview ? Math.max(1, Math.ceil(preview.estimateSecs / 60)) : 0
  const ended = status && !status.running ? status.ended : null

  return (
    <div className="log-tools-pane">
      <p className="qso-detail-bare">{t('logbook.tools.callbook.intro')}</p>
      <fieldset className="log-tools-row" disabled={running}>
        <legend>{t('logbook.tools.bulk.which')}</legend>
        <label>
          <input
            type="radio"
            name="callbook-which"
            checked={which === 'picked'}
            disabled={picked.size === 0}
            onChange={() => { setWhich('picked'); setPreview(null) }}
          />{' '}
          {t('logbook.tools.bulk.picked', { count: picked.size })}
        </label>
        <label>
          <input
            type="radio"
            name="callbook-which"
            checked={which === 'shown'}
            onChange={() => { setWhich('shown'); setPreview(null) }}
          />{' '}
          {t('logbook.tools.bulk.shown', { count: shown })}
        </label>
        <button type="button" className="export-btn" disabled={busy || running} onClick={() => void onPreview()}>
          {t('logbook.tools.bulk.preview')}
        </button>
      </fieldset>
      {preview && !running && (
        <>
          <p className="qso-detail-bare">
            {t('logbook.tools.callbook.previewed', {
              selected: preview.selected,
              calls: preview.calls,
              kept: preview.kept,
              toLookUp: preview.toLookUp,
            })}
          </p>
          {preview.services.length > 0 && (
            <p className="qso-detail-bare">
              {t('logbook.tools.callbook.services', {
                services: preview.services
                  .map(serviceName)
                  .join(t('logbook.tools.callbook.then')),
              })}
              {preview.services.includes('hamqth') && <> {t('logbook.tools.callbook.hamqthCredit')}</>}
            </p>
          )}
          {preview.toLookUp > 0 && (
            <p className="qso-detail-bare">
              {t('logbook.tools.callbook.time', { minutes, pace: preview.paceSecs })}
            </p>
          )}
          <p className="qso-detail-bare">
            {t('logbook.tools.callbook.today', { spent: preview.spent, cap: preview.cap })}
          </p>
          {preview.lookupsNeeded > left && (
            <p className="qso-detail-bare">
              <strong>{t('logbook.tools.callbook.overCap', { count: left })}</strong>
              {preview.lookupsNeeded > preview.toLookUp && (
                <> {t('logbook.tools.callbook.stroked', { count: preview.lookupsNeeded })}</>
              )}
              {preview.qrzCount !== null && (
                <> {t('logbook.tools.callbook.qrzCount', { count: preview.qrzCount })}</>
              )}
            </p>
          )}
          {!preview.configured && (
            <p className="qso-detail-bare">
              <strong>{t('logbook.tools.callbook.notSetUp')}</strong>
            </p>
          )}
          {preview.fields.length > 0 && (
            <p className="qso-detail-bare">
              {t('logbook.tools.callbook.fields')}{' '}
              <span className="mono">{preview.fields.map(([tag, n]) => `${tag} ${n}`).join(' · ')}</span>
            </p>
          )}
          <p className="qso-detail-bare">{t('logbook.tools.callbook.location')}</p>
          {preview.locationTooOld > 0 && (
            <p className="qso-detail-bare">
              {t('logbook.tools.callbook.locationTooOld', { count: preview.locationTooOld })}
            </p>
          )}
          <div className="log-tools-row">
            <button
              type="button"
              className="export-btn"
              disabled={busy || afar || preview.couldChange === 0 || !preview.configured}
              onClick={() => setAsking(true)}
            >
              {upTo
                ? t('logbook.tools.callbook.applyUpTo', { count: preview.couldChange })
                : t('logbook.tools.callbook.apply', { count: preview.couldChange })}
            </button>
            {asking && (
              <span className="log-tools-confirm">
                <span>{t('logbook.tools.callbook.ask', { count: preview.toLookUp })}</span>
                <button type="button" className="export-btn danger" disabled={busy} onClick={() => void onStart()}>
                  {t('logbook.tools.callbook.confirm')}
                </button>
                <button type="button" className="export-btn" onClick={() => setAsking(false)}>
                  {t('logbook.tools.cancel')}
                </button>
              </span>
            )}
          </div>
        </>
      )}
      {status && running && (
        <div className="log-tools-row" role="status">
          <span>
            {status.waitingForTx ? t('logbook.tools.callbook.waitingForTx')
              : t('logbook.tools.callbook.progress', {
                done: status.done,
                total: status.total,
                fromCache: status.fromCache,
                spent: status.spent,
                cap: status.cap,
              })}
          </span>
          <button type="button" className="export-btn" onClick={() => void callbookUpdateCancel().catch(() => {})}>
            {t('logbook.tools.callbook.stop')}
          </button>
        </div>
      )}
      {status && running && <QrzNotes skipped={status.qrzSkipped} said={status.qrzSaid} />}
      {ended && (
        <div className="qso-detail-bare" role="status">
          <p>
            {ended.stop === 'done' ? t('logbook.tools.callbook.end.done')
              : ended.stop === 'cancelled' ? t('logbook.tools.callbook.end.cancelled')
              : ended.stop === 'limit' ? t('logbook.tools.callbook.end.limit', { cap: status?.cap ?? 0 })
              : ended.stop === 'refused' ? t('logbook.tools.callbook.end.refused')
              : t('logbook.tools.callbook.end.failed')}
          </p>
          <QrzNotes skipped={ended.qrzSkipped} said={ended.qrzSaid} />
          {ended.notLookedUp > 0 && (
            <p>{t('logbook.tools.callbook.resume', { count: ended.notLookedUp })}</p>
          )}
          {ended.unknown.length > 0 && (
            <p>
              {t('logbook.tools.callbook.unknown', { count: ended.unknown.length })}{' '}
              <span className="mono">{ended.unknown.join(' ')}</span>
            </p>
          )}
        </div>
      )}
      <p className="qso-detail-bare">{t('logbook.tools.callbook.rules')}</p>
    </div>
  )
}
