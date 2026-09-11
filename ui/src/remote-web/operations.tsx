import { useContext, useEffect, useRef, useState, useSyncExternalStore } from 'react'
import { LogEntry } from '../components/LogEntry'
import { t } from '../i18n'
import { useStationData, RemoteOperationsContext } from '../stationAccess'
export { RemoteOperationsContext } from '../stationAccess'
import type { LoggedQso } from '../types'
import { manualRecord, type ManualRecord } from './operation-protocol'
import type { OperationClient } from './operation-client'
import { ObserverRecallEntry, RemoteRecall, type RecallProps } from './RemoteRecall'
// Radio units are invariant protocol tokens, never translated or locale-formatted.
const FREQUENCY_UNIT = 'MHz'
export function LoggingAuthority({ client }: { client: OperationClient }) {
  const view = useSyncExternalStore(client.subscribe, client.getSnapshot)
  if (!client.enabled) return null
  const phase = view.state?.phase
  const station = !!view.state?.controls || !!view.controlPending || !!view.controlResult || !!view.controlError
  const label = !view.connected
    ? station ? t('remote.controlOffline') : t('remote.loggingOffline')
    : !view.state && view.supported
      ? t('remote.loggingStatusUnavailable')
      : view.error === 'stationUnsupported'
        ? t('remote.loggingUnsupported')
        : phase === 'controlling'
          ? view.fresh
            ? station ? t('remote.controlActive') : t('remote.loggingActive')
            : t('remote.loggingStatusUnavailable')
          : phase === 'occupied'
            ? station ? t('remote.controlOccupied') : t('remote.loggingOccupied')
            : phase === 'available'
              ? station ? t('remote.controlAvailable') : t('remote.loggingAvailable')
              : station ? t('remote.controlRequired') : t('remote.loggingPermissionRequired')
  return (
    <div className="remote-logging-authority">
      <span role="status">{label}</span>
      {phase === 'available' && (
        <button
          type="button"
          className="remote-button"
          disabled={!view.fresh || view.busy}
          onClick={() => void client.acquire().catch(() => {})}
        >
          {station ? t('remote.controlAcquire') : t('remote.loggingAcquire')}
        </button>
      )}
      {phase === 'controlling' && (
        <button
          type="button"
          className="remote-button"
          disabled={view.busy}
          onClick={() => void client.release()}
        >
          {station ? t('remote.controlRelease') : t('remote.loggingRelease')}
        </button>
      )}
      {(view.controlPending || view.controlResult || view.controlError) && <div className="remote-control-result">
        {view.controlError && !view.controlPending ? <span role="alert">{t('remote.controlRequestFailed')}</span> :
        <span role="status">{view.controlResult?.outcome === 'applied' ? view.controlResult.evidence === 'settingsSaved' ? t('remote.controlSettingsSaved') : t('remote.controlApplied')
          : view.controlResult?.outcome === 'rejected' ? t('remote.controlRefused')
          : view.controlResult?.outcome === 'pending' && view.connected ? t('remote.controlPending') : t('remote.controlUnknown')}</span>}
        {view.controlPending && <>
          <button type="button" className="remote-button" disabled={view.busy || !view.connected} onClick={() => void client.refreshControl().catch(() => {})}>{t('remote.controlCheckResult')}</button>
          <button type="button" className="remote-button" disabled={view.busy || view.controlResult?.outcome === 'pending'} onClick={() => void client.acknowledgeControl().catch(() => {})}>{t('remote.controlCheckedStation')}</button>
        </>}
      </div>}
    </div>
  )
}
function SubmittedLog({ record }: { record: ManualRecord }) {
  const fields = [
    [t('logEntry.rstSent.label'), record.rstSent],
    [t('logEntry.rstRcvd.label'), record.rstRcvd],
    [t('logEntry.grid.placeholder'), record.grid],
    [t('logEntry.name.placeholder'), record.name],
    [t('logEntry.qth.placeholder'), record.qth],
    [t('logEntry.state.placeholder'), record.state],
    [t('logEntry.country.placeholder'), record.country],
    [t('logEntry.comment.placeholder'), record.comment],
    [t('logEntry.notes.placeholder'), record.notes],
    [
      t('logEntry.override.time.label'),
      record.whenUnix === null
        ? t('remote.loggingStationTime')
        : new Date(record.whenUnix * 1000).toISOString()
    ],
    ...(record.ota ? [[record.ota.theirProgram, record.ota.theirRef]] : [])
  ].filter(([, value]) => value)
  return (
    <details className="remote-log-submitted" open>
      <summary>{t('remote.loggingSubmitted')}</summary>
      <p>
        <strong>{record.call}</strong> · {record.mode} · {record.band} · {record.freqMhz}{' '}
        {FREQUENCY_UNIT}
      </p>
      <dl>
        {fields.map(([label, value]) => (
          <div key={label}>
            <dt>{label}</dt>
            <dd>{value}</dd>
          </div>
        ))}
      </dl>
    </details>
  )
}
export function RemoteLogEntry({
  client,
  snap,
  mode,
  onOpenLog,
  selectedCall
}: Omit<RecallProps, 'call' | 'bounded'> & { client: OperationClient; selectedCall?: string }) {
  const view = useSyncExternalStore(client.subscribe, client.getSnapshot),
    available = useStationData()
  const [error, setError] = useState<string | null>(null),
    [resetKey, setResetKey] = useState(0),
    [logged, setLogged] = useState(false)
  const submitted = useRef<string | null>(null)
  useEffect(() => {
    const result = view.resolved,
      own = submitted.current
    if (!own) return
    if (result?.operationId === own && result.outcome !== 'unknown') {
      submitted.current = null
      setError(result.outcome === 'rejected' ? 'rejected' : null)
      if (result.outcome === 'applied') {
        setResetKey((k) => k + 1)
        setLogged(true)
      }
    }
    if (view.dismissed === own) {
      submitted.current = null
      setResetKey((k) => k + 1)
      setError(null)
    }
  }, [view.resolved, view.dismissed])
  const canSubmit =
    available && view.fresh && view.state?.phase === 'controlling' && view.state.actions.includes('log.manual') && !view.unresolved && !view.controlPending
  async function submit(record: LoggedQso, time: 'station' | 'explicit') {
    setError(null)
    setLogged(false)
    try {
      const outcome = await client.log(
        manualRecord(
          JSON.parse(
            JSON.stringify({ ...record, whenUnix: time === 'station' ? null : record.whenUnix })
          )
        ),
        (id) => {
          submitted.current = id
        }
      )
      if (outcome.outcome !== 'applied') throw Error(outcome.outcome)
      setLogged(true)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'unconfirmed')
      throw e
    }
  }
  async function resolve() {
    setError(null)
    setLogged(false)
    try {
      const result = await client.resolve()
      if (result.outcome === 'applied') setLogged(true)
      else setError(result.outcome === 'rejected' ? 'rejected' : 'unconfirmed')
    } catch {
      setError('unconfirmed')
    }
  }
  if (!view.supported && !view.unresolved)
    return (
      <ObserverRecallEntry
        snap={snap}
        mode={mode}
        onOpenLog={onOpenLog}
        selectedCall={selectedCall}
      />
    )
  return (
    <div className="remote-log-entry" data-operation-error={error ?? undefined}>
      <p className="dim">{t('remote.loggingHint')}</p>
      {logged && <p role="status">{t('remote.loggingSaved')}</p>}
      {view.submitting && <p role="status">{t('remote.loggingSaving')}</p>}
      {!view.submitting && (error || view.unresolved) && (
        <p role="alert">
          {view.unresolved ? t('remote.loggingUnknown') : t('remote.loggingRefused')}
        </p>
      )}
      {view.unresolved && !view.submitting && view.pendingDraft && (
        <SubmittedLog record={view.pendingDraft} />
      )}
      {view.unresolved && !view.submitting && (
        <div className="remote-actions">
          <button
            type="button"
            className="remote-button"
            disabled={view.busy || !view.connected}
            onClick={() => void resolve()}
          >
            {t('remote.loggingCheckResult')}
          </button>
          <button
            type="button"
            className="remote-button"
            disabled={view.busy}
            onClick={() => {
              void client
                .acknowledgeAfterCheckingLog()
                .then(() => setError(null))
                .catch(() => setError('unconfirmed'))
            }}
          >
            {t('remote.loggingCheckedLog')}
          </button>
        </div>
      )}
      <fieldset disabled={!!view.unresolved} className="remote-log-fields">
        <LogEntry
          titled={false}
          snap={snap}
          mode={mode}
          defaultRst={mode === 'SSB' || mode === 'FM' ? '59' : '599'}
          exchange={mode === 'SAT' ? 'satellite' : 'terrestrial'}
          onOpenLogbook={onOpenLog}
          pendingWork={selectedCall ? { call: selectedCall, ts: 0 } : null}
          remote={{
            submit,
            canSubmit,
            busy: view.busy,
            resetKey,
            recall: (call) => (
              <RemoteRecall snap={snap} call={call} mode={mode} onOpenLog={onOpenLog} />
            )
          }}
        />
      </fieldset>
    </div>
  )
}
export function useRemoteOperations() {
  return useContext(RemoteOperationsContext)
}
