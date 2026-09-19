import { useRemotePresentation } from './presentation'
import { useContext, useEffect, useRef, useState, useSyncExternalStore } from 'react'
import { LogEntry } from '../components/LogEntry'
import { t } from '../i18n'
import { useStationData, RemoteOperationsContext } from '../stationAccess'
export { RemoteOperationsContext } from '../stationAccess'
import type { LoggedQso } from '../types'
import { manualRecord, type LogCapability, type LogChange, type LogChangeOutcome, type ManualRecord } from './operation-protocol'
import { pushToast } from '../toast'
import { OperationFailure, type OperationClient } from './operation-client'
import { ObserverRecallEntry, RemoteRecall, type RemoteRecallEntryProps } from './RemoteRecall'
// Radio units are invariant protocol tokens, never translated or locale-formatted.
const FREQUENCY_UNIT = 'MHz'
/** The authority line of the session banner. That banner sits above every cockpit, so an element it
 * adds or removes moves every control below it: operators saw exactly that as flicker. Its shape
 * therefore depends only on the session, never on the moment. A command outcome's re-read shows
 * the last station state with its button disabled, the gap between heartbeats keeps the label while
 * the station-reported lease runs, and the result row is reserved with its check buttons mounted.
 * What is actionable is unchanged: every button still gates on `state`, `fresh` and `busy`. */
export function LoggingAuthority({ client, unavailable }: { client: OperationClient; unavailable?: boolean }) {
  const view = useSyncExternalStore(client.subscribe, client.getSnapshot)
  const sawControls = useRef(false)
  if (!client.enabled) return null
  const shown = view.state ?? view.retainedState ?? null
  const phase = shown?.phase
  if (shown?.controls || view.controlPending || view.controlResult || view.controlError) sawControls.current = true
  const station = sawControls.current
  // A command outcome's re-read keeps its own wording ("Updating…", then unavailable if it lapses).
  // The state is held through that re-read (its window spent, the controls still lit), so the
  // wording follows `controlRefreshing`, not the absence of a state.
  // The steady label applies to the ordinary gap between heartbeats, where a state is held.
  const label = !view.connected
    ? station ? t('remote.controlOffline') : t('remote.loggingOffline')
    : view.supported && view.controlRefreshing && !view.error && !view.controlError
      ? t('remote.controlRefreshing')
      : !view.state && view.supported
        ? t('remote.loggingStatusUnavailable')
      : view.error === 'stationUnsupported'
        ? t('remote.loggingUnsupported')
        : phase === 'controlling'
          ? view.fresh || (view.leaseHeld && !view.error)
            ? station ? t('remote.controlActive') : t('remote.loggingActive')
            : t('remote.loggingStatusUnavailable')
          : phase === 'occupied'
            ? station ? t('remote.controlOccupied') : t('remote.loggingOccupied')
            : phase === 'available'
              ? station ? t('remote.controlAvailable') : t('remote.loggingAvailable')
              : station ? t('remote.controlRequired') : t('remote.loggingPermissionRequired')
  const failure = view.controlError && !view.controlPending ? view.controlError : null
  const outcome = failure ? (failure.busy ? t('remote.controlBusy') : failure.sent ? t('remote.controlRequestFailed') : t('remote.controlNotSent'))
    : !(view.controlPending || view.controlResult || view.controlError) ? ''
    : view.controlResult?.outcome === 'applied' ? view.controlResult.evidence === 'settingsSaved' ? t('remote.controlSettingsSaved') : t('remote.controlApplied')
    : view.controlResult?.outcome === 'rejected' ? view.controlResult.reason === 'stationBusy' ? t('remote.controlBusy') : t('remote.controlRefused')
    : (view.controlSending || view.controlResult?.outcome === 'pending') && view.connected ? t('remote.controlPending') : t('remote.controlUnknown')
  // The check buttons are for a command that needs checking: one still pending with no request in
  // flight for it. While its own request is in flight both were disabled anyway (busy), and showing
  // them then made every ordinary command pop two buttons in and out.
  // Reserved, not removed: a hidden check button keeps its box, so the row is the same size whether
  // or not a command awaits checking. Hidden also means disabled, out of the tab order and unnamed.
  // On a phone that box is too dear (the banner is capped at 40% of the height and a reserved pair
  // pushed the Quick contact form under its nav), so xs/sm drop the reservation in CSS.
  const checking = !!view.controlPending && !view.controlSending
  const reserved = checking ? {} : { 'data-reserved': true, 'aria-hidden': true, tabIndex: -1 }
  return (
    <div className="remote-logging-authority" data-station-state={view.state ? 'current' : shown ? 'retained' : undefined}>
      <span className="remote-session-label">
        {unavailable !== undefined && <span role="alert" className="remote-session-unavailable">{unavailable ? t('remote.applicationUnavailable') + ' ' : null}</span>}
        <span role="status">{label}</span>
      </span>
      {phase === 'available' && (
        <button
          type="button"
          className="remote-button"
          disabled={!view.fresh || view.busy || view.requestReady === false}
          onClick={() => void client.acquire().catch(() => {})}
        >
          {station ? t('remote.controlAcquire') : t('remote.loggingAcquire')}
        </button>
      )}
      {phase === 'controlling' && (
        <button
          type="button"
          className="remote-button"
          disabled={(view.busy && !view.reading) || !view.state}
          onClick={() => void client.release()}
        >
          {station ? t('remote.controlRelease') : t('remote.loggingRelease')}
        </button>
      )}
      {station && <div className="remote-control-result" data-idle={outcome ? undefined : true}>
        <span role={failure ? 'alert' : outcome ? 'status' : undefined} data-control-failure={failure ? failure.busy ? 'busy' : failure.sent ? 'unconfirmed' : 'notSent' : undefined}>{outcome}</span>
        <button type="button" className="remote-button" {...reserved} disabled={!checking || view.busy || !view.connected || view.requestReady === false} onClick={() => void client.refreshControl().catch(() => {})}>{t('remote.controlCheckResult')}</button>
        <button type="button" className="remote-button" {...reserved} disabled={!checking || view.busy || view.controlResult?.outcome === 'pending'} onClick={() => void client.acknowledgeControl().catch(() => {})}>{t('remote.controlCheckedStation')}</button>
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
  selectedCall,
  pendingWork,
  onConsumeWork,
}: RemoteRecallEntryProps & { client: OperationClient }) {
  const display = useRemotePresentation()
  const view = useSyncExternalStore(client.subscribe, client.getSnapshot),
    available = useStationData()
  const [error, setError] = useState<string | null>(null),
    [resetKey, setResetKey] = useState(0),
    [logged, setLogged] = useState(false),
    // Whether the failed entry's request left this browser; only an unsent one says "not sent".
    [errorSent, setErrorSent] = useState(true),
    [errorBusy, setErrorBusy] = useState(false)
  const submitted = useRef<string | null>(null)
  useEffect(() => {
    const result = view.resolved,
      own = submitted.current
    if (!own) return
    if (result?.operationId === own && result.outcome !== 'unknown') {
      submitted.current = null
      setError(result.outcome === 'rejected' ? 'rejected' : null)
      setErrorSent(true)
      setErrorBusy(false)
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
  // Not `view.fresh`: a brief control lapse keeps Log usable, and OperationClient.log waits for
  // current control before sending (or refuses as not sent).
  const canSubmit =
    available && view.requestReady !== false && view.state?.phase === 'controlling' && view.state.actions.includes('log.manual') && !view.unresolved && !view.controlPending
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
      setErrorSent(!(e instanceof OperationFailure) || e.sent)
      setErrorBusy(e instanceof OperationFailure && e.busy)
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
      setErrorSent(true)
      setErrorBusy(false)
    } catch {
      setError('unconfirmed')
      setErrorSent(true)
      setErrorBusy(false)
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
      <p className="dim" hidden={display?.presentation === 'quick' && !snap.fieldDay && ['CW', 'SSB', 'FM'].includes(mode)}>{t('remote.loggingHint')}</p>
      {logged && <p role="status">{t('remote.loggingSaved')}</p>}
      {view.submitting && <p role="status">{t('remote.loggingSaving')}</p>}
      {!view.submitting && (error || view.unresolved) && (
        <p role="alert">
          {view.unresolved ? t('remote.loggingUnknown') : errorBusy ? t('remote.loggingBusy') : errorSent ? t('remote.loggingRefused') : t('remote.loggingNotSent')}
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
            disabled={view.busy || !view.connected || view.requestReady === false}
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
                .catch(() => {
                  setError('unconfirmed')
                  setErrorSent(true)
                  setErrorBusy(false)
                })
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
          pendingWork={pendingWork ?? (selectedCall ? { call: selectedCall, ts: 0 } : null)}
          onConsumeWork={onConsumeWork}
          remote={{
            submit,
            canSubmit,
            busy: view.busy,
            pending: !!view.unresolved || view.submitting,
            resetKey,
            recall: (call, context) => (
              <RemoteRecall snap={snap} call={call} mode={mode} context={context} onOpenLog={onOpenLog} />
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
const idleSubscribe = () => () => {}
const idleView = () => null
/** Whether a log write may be sent now: logging control is current and the station offers it (a
 * manual entry as an action, a change as a v4 capability). A write still waiting for its result
 * blocks every other one (see RemoteLogCheck). */
export function useLogChange(capability: LogCapability | 'log.manual'): boolean {
  const client = useContext(RemoteOperationsContext), available = useStationData()
  const view = useSyncExternalStore(client?.subscribe ?? idleSubscribe, client?.getSnapshot ?? idleView)
  const manual = capability === 'log.manual'
  return !!(client && (manual || client.operationVersion >= 4) && available && view?.connected && view.fresh &&
    view.requestReady !== false && !view.unresolved && !view.controlPending && view.state?.phase === 'controlling' &&
    (manual ? view.state.actions.includes('log.manual') : view.state.controls?.capabilities.includes(capability)))
}
/** Send one log change and say what happened: a stale row, any other refusal, a busy station and a
 * request that never left are told apart. An unknown outcome is left to RemoteLogCheck. */
export async function sendLogChange(client: OperationClient, change: LogChange): Promise<LogChangeOutcome | null> {
  try {
    const outcome = await client.change(change)
    // A self-spot neither target took names both reasons from its `spot`, told by the caller.
    if (outcome.outcome === 'rejected' && outcome.reason !== 'spotNotPosted')
      pushToast(outcome.reason === 'clusterUnavailable' ? t('remote.spotNoCluster')
        : outcome.reason !== 'contextChanged' ? t('remote.logChangeFailed')
        : change.kind === 'selfSpot' ? t('ota.selfSpot.moved') : t('remote.logChangeStale'), 'error', 6000)
    return outcome
  } catch (e) {
    const failure = e instanceof OperationFailure ? e : null
    pushToast(failure?.busy ? t('remote.controlBusy') : failure && !failure.sent ? t('remote.controlNotSent') : t('remote.logChangeFailed'), 'error', 6000)
    return null
  }
}
/** A log write whose outcome never arrived. Nothing else may change the log until the operator
 * checks the station's receipt, or says they checked the log at the station. */
export function RemoteLogCheck({ client }: { client: OperationClient }) {
  const view = useSyncExternalStore(client.subscribe, client.getSnapshot)
  if (!client.enabled || !view.unresolved || view.submitting) return null
  return (
    <div className="remote-log-entry remote-log-check">
      <p role="alert">{view.pendingDraft ? t('remote.loggingUnknown') : t('remote.logChangeUnknown')}</p>
      <div className="remote-actions">
        <button type="button" className="remote-button" disabled={view.busy || !view.connected || view.requestReady === false}
          onClick={() => void client.resolve().catch(() => {})}>{t('remote.logChangeCheck')}</button>
        <button type="button" className="remote-button" disabled={view.busy}
          onClick={() => void client.acknowledgeAfterCheckingLog().catch(() => {})}>{t('remote.loggingCheckedLog')}</button>
      </div>
    </div>
  )
}
