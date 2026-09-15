import { useEffect, useRef, useState } from 'react'
import { answerRemoteAutostartOffer, getRemoteStationStatus, getSettings, remoteStationAction, setLaunchAtLogin } from '../api'
import { t } from '../i18n'
import type { RemoteStationAction, RemoteStationStatus } from './types'
import '../remote-web/remote.css'

// Browser approval lifetime: the shack warns this long before the end that use cannot move.
const APPROVAL_WARNING_MS = 7 * 86400000
// A UTC calendar date, never locale formatting: a date an operator plans around must not read
// differently from one country to the next (the browser page does the same).
const utcDate = (ms: number) => new Date(ms).toISOString().slice(0, 10)

export function RemoteStation() {
  const [status, setStatus] = useState<RemoteStationStatus | null>(null)
  const [name, setName] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [offer, setOffer] = useState(false)
  const [offerFailed, setOfferFailed] = useState(false)
  // "Also allow FT8/FT4 transmit" on the pairing approval and on each browser's approval. Off unless ticked.
  const [pairingTransmit, setPairingTransmit] = useState(false)
  const [browserTransmit, setBrowserTransmit] = useState<Record<string, boolean>>({})
  const pending = useRef(false), mounted = useRef(true), requestEpoch = useRef(0)
  useEffect(() => {
    mounted.current = true
    const initial = ++requestEpoch.current
    void (async () => { try { const next = await getRemoteStationStatus(); if (mounted.current && initial === requestEpoch.current) setStatus(next) } catch { if (mounted.current && initial === requestEpoch.current) setError('serviceUnavailable') } })()
    const timer = setInterval(() => {
      if (pending.current) return
      pending.current = true
      const epoch = ++requestEpoch.current
      void (async () => {
        try { const next = await remoteStationAction({ type: 'refresh' }); if (mounted.current && epoch === requestEpoch.current) setStatus(next) }
        catch (error) { if (mounted.current && epoch === requestEpoch.current) setError(typeof error === 'string' ? error : 'serviceUnavailable') }
        finally { if (epoch === requestEpoch.current) pending.current = false }
      })()
    }, 5000)
    return () => { mounted.current = false; requestEpoch.current++; pending.current = false; clearInterval(timer) }
  }, [])
  async function act(action: RemoteStationAction) {
    // Disable cancels the native socket even while an HTTP request is pending.
    if (pending.current && action.type !== 'disable' && action.type !== 'takeOverLogging' && !(action.type === 'transmitPermission' && !action.allow)) return
    const epoch = ++requestEpoch.current
    pending.current = true; setBusy(true); setError(null)
    try {
      const next = await remoteStationAction(action)
      if (mounted.current && epoch === requestEpoch.current) setStatus(next)
      // Approving the pairing turns Remote on too, so it is the same moment to offer.
      if (action.type === 'enable' || (action.type === 'approve' && ['connected', 'connecting', 'reconnecting'].includes(next.phase))) void offerStartAtSignIn()
    }
    catch (error) { if (mounted.current && epoch === requestEpoch.current) setError(typeof error === 'string' ? error : 'serviceUnavailable') }
    finally { if (epoch === requestEpoch.current) { pending.current = false; if (mounted.current) setBusy(false) } }
  }
  // Remote comes back after a restart only if Nexus itself starts. So when the operator turns Remote
  // on (or approves the pairing, which turns it on), and Nexus is not set to start at sign-in, offer
  // it: once, only on that click (never on a
  // launch that turned Remote back on by itself), never switched on without a click, and never
  // asked again once answered either way.
  async function offerStartAtSignIn() {
    try {
      const settings = await getSettings()
      if (mounted.current && !settings.launchAtLogin && !settings.remoteAutostartOfferAnswered) setOffer(true)
    } catch { /* no settings, no offer: the switch in Settings is always there */ }
  }
  async function answerOffer(accept: boolean) {
    setOffer(false); setOfferFailed(false)
    try { await answerRemoteAutostartOffer() } catch { /* closed for this session either way */ }
    if (!accept) return
    try { await setLaunchAtLogin(true) } catch { if (mounted.current) setOfferFailed(true) }
  }
  const connected = status && ['connected', 'connecting', 'reconnecting'].includes(status.phase)
  const issue = error ?? status?.error, phase = status?.phase
  return <div className="remote-section remote-native">
    <p>{t('remote.nativeIntro')}</p>
    <p role="status">{phase === 'unpaired' ? t('remote.unpaired') : phase === 'pairing' ? t('remote.pairing')
      : phase === 'approval' ? t('remote.localApproval') : phase === 'disabled' ? t('remote.disabled')
      : phase === 'connected' ? t('remote.connected') : phase === 'reconnecting' ? t('remote.reconnecting') : t('monitor.connecting')}</p>
    {/* Every refusal except the vault one used to render "the request could not be completed",
        which points at the network. A station whose access was revoked from a browser, or whose
        service access has run out, has not got a network problem and cannot fix one. */}
    {issue && <p role="alert">{
      issue === 'credentialStoreUnavailable' ? t('remote.vaultFailed')
      : issue === 'stationRevoked' ? t('remote.nativeRevoked')
      // The service returns these two only from approval, so the station was never attached. They
      // need different next steps: an ended trial is not an account that was switched off.
      : issue === 'trialEnded' ? t('remote.nativeTrialEnded')
      : issue === 'trialDisabled' ? t('remote.nativeTrialDisabled')
      // Approving at the shack before the browser agreed to attach the station. Falling through to
      // requestFailed would point an operator standing at the radio at their network.
      : issue === 'awaitingConfirmation' ? t('remote.nativeAwaitingConfirmation')
      : t('remote.requestFailed')}</p>}
    {phase === 'unpaired' && <>
      <label>{t('remote.stationName')}<input value={name} maxLength={48} onChange={event => setName(event.target.value)} /></label>
      <button type="button" className="remote-button" disabled={busy || !name.trim()} onClick={() => void act({ type: 'begin', name })}>{t('remote.pairStation')}</button>
    </>}
    {status?.pairingCode && <>
      <p>{t('remote.openService')} <a href={status.origin} target="_blank" rel="noreferrer">{status.origin}</a></p>
      <p>{t('remote.pairingCode')} <code>{status.pairingCode.match(/.{1,4}/g)?.join(' ')}</code></p>
      <p>{t('remote.codeExpires')}</p>
      {status.accountId && <p>{t('remote.accountMatch')} <code>{status.accountId}</code></p>}
      {phase === 'approval' && status.accountId && status.pairingId && <label>
        <input type="checkbox" checked={pairingTransmit} onChange={event => setPairingTransmit(event.target.checked)} />{t('remote.approveTransmit')}</label>}
      <div className="remote-actions">
        {phase === 'approval' && status.accountId && status.pairingId && <button type="button" className="remote-button" disabled={busy}
          onClick={() => void act({ type: 'approve', accountId: status.accountId!, enrollmentId: status.pairingId!, transmit: pairingTransmit })}>{t('remote.approvePairing')}</button>}
        <button type="button" className="remote-button" disabled={busy} onClick={() => void act({ type: 'cancel' })}>{t('remote.cancelPairing')}</button>
      </div>
    </>}
    {status?.stationId && <>
      <p>{t('remote.restartHint')}</p>
      <div className="remote-actions">
        <button type="button" className="remote-button" disabled={!connected && busy} onClick={() => void act({ type: connected ? 'disable' : 'enable' })}>
          {connected ? t('remote.disable') : t('remote.enable')}</button>
        <button type="button" className="remote-button" disabled={busy} onClick={() => void act({ type: 'refresh' })}>{t('remote.refreshDevices')}</button>
      </div>
      {offer && <div role="group" aria-label={t('settings.launchAtLogin.legend')}>
        <p>{t('remote.autostartOffer')}</p>
        <div className="remote-actions">
          <button type="button" className="remote-button" onClick={() => void answerOffer(true)}>{t('remote.autostartOfferAccept')}</button>
          <button type="button" className="remote-button" onClick={() => void answerOffer(false)}>{t('remote.autostartOfferDismiss')}</button>
        </div>
      </div>}
      {offerFailed && <p role="alert">{t('remote.autostartOfferFailed')}</p>}
      {status.loggingPermissions && <><p>{t('remote.loggingLocalHint')}</p><button type="button" className="remote-button" onClick={()=>void act({type:'takeOverLogging'})}>{status.stationPermissions ? t('remote.controlTakeOver') : t('remote.loggingTakeOver')}</button></>}
      {status.stationPermissions && <p>{t('remote.controlLocalHint')}</p>}
      {status.transmitPermissions && <p>{t('remote.transmitLocalHint')}</p>}
      <h3>{t('remote.browserApprovals')}</h3><p>{t('remote.browserMatch')}</p>
      {status.devices.length === 0 && <p>{t('remote.noBrowsers')}</p>}
      {status.devices.map(device => <div key={device.id}>
        <p>{device.name} <code>{device.id.slice(-6)}</code></p>
        {/* One approval: approving grants station controls and logging, plus transmit if ticked.
            The switches below then only restrict (or give back) what an approved browser holds. */}
        {device.approved === 1
          ? <button type="button" className="remote-button" disabled={busy} onClick={() => void act({ type: 'device', deviceId: device.id, approve: false })}>{t('remote.revokeBrowser')}</button>
          : <>
              <label><input type="checkbox" checked={browserTransmit[device.id] ?? false}
                onChange={event => { const checked = event.target.checked; setBrowserTransmit(current => ({ ...current, [device.id]: checked })) }} />{t('remote.approveTransmit')}</label>
              <button type="button" className="remote-button" disabled={busy}
                onClick={() => void act({ type: 'device', deviceId: device.id, approve: true, transmit: browserTransmit[device.id] ?? false })}>{t('remote.approveBrowser')}</button>
            </>}
        {device.approved === 1 && <p>{device.renewsUntil
          ? t('remote.browserRenewsUntil', { until: utcDate(device.expiresAt), limit: utcDate(device.renewsUntil) })
          : t('remote.browserApprovedUntil', { until: utcDate(device.expiresAt) })}</p>}
        {/* The last week before the end that use cannot move. Approving again is a NEW approval: it
            grants station controls and logging again, and transmit only if ticked here. */}
        {device.approved === 1 && (device.renewsUntil ?? device.expiresAt) - Date.now() <= APPROVAL_WARNING_MS && <>
          <p className="remote-warning">{t('remote.browserApprovalEnding', { until: utcDate(device.renewsUntil ?? device.expiresAt) })}</p>
          <label><input type="checkbox" checked={browserTransmit[device.id] ?? false}
            onChange={event => { const checked = event.target.checked; setBrowserTransmit(current => ({ ...current, [device.id]: checked })) }} />{t('remote.approveTransmit')}</label>
          <button type="button" className="remote-button" disabled={busy}
            onClick={() => void act({ type: 'device', deviceId: device.id, approve: true, transmit: browserTransmit[device.id] ?? false })}>{t('remote.approveAgain')}</button>
        </>}
        {device.approved===1&&status.loggingPermissions&&<button type="button" className="remote-button" disabled={busy||!connected} onClick={()=>void act({type:'loggingPermission',deviceId:device.id,allow:!status.loggingPermissions!.includes(device.id)})}>{status.loggingPermissions.includes(device.id)?t('remote.loggingRevoke'):t('remote.loggingAllow')}</button>}
        {device.approved===1&&status.stationPermissions&&<button type="button" className="remote-button" disabled={busy||!connected} onClick={()=>void act({type:'stationPermission',deviceId:device.id,allow:!status.stationPermissions!.includes(device.id)})}>{status.stationPermissions.includes(device.id)?t('remote.controlRevoke'):t('remote.controlAllow')}</button>}
        {device.approved===1&&status.transmitPermissions&&<button type="button" className="remote-button"
          disabled={!status.transmitPermissions.includes(device.id)&&(busy||!connected||!status.stationPermissions?.includes(device.id))}
          onClick={()=>void act({type:'transmitPermission',deviceId:device.id,allow:!status.transmitPermissions!.includes(device.id)})}>
          {status.transmitPermissions.includes(device.id)?t('remote.transmitRevoke'):t('remote.transmitAllow')}</button>}
        {status.loggingController===device.id&&<p role="status">{status.stationPermissions?.includes(device.id)?t('remote.controlActive'):t('remote.loggingController')}</p>}
      </div>)}
      <details><summary>{t('remote.stationAccess')}</summary><p>{t('remote.revokeHint')}</p>
        <button type="button" className="remote-button" disabled={busy} onClick={() => void act({ type: 'forget' })}>{t('remote.revokeStation')}</button>
      </details>
    </>}
  </div>
}
