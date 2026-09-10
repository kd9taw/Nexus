import { useEffect, useRef, useState } from 'react'
import { getRemoteStationStatus, remoteStationAction } from '../api'
import { t } from '../i18n'
import type { RemoteStationAction, RemoteStationStatus } from './types'
import '../remote-web/remote.css'

export function RemoteStation() {
  const [status, setStatus] = useState<RemoteStationStatus | null>(null)
  const [name, setName] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const pending = useRef(false), mounted = useRef(true)
  useEffect(() => {
    mounted.current = true
    void (async () => { try { const next = await getRemoteStationStatus(); if (mounted.current) setStatus(next) } catch { if (mounted.current) setError('serviceUnavailable') } })()
    const timer = setInterval(() => {
      if (pending.current) return
      pending.current = true
      void (async () => {
        try { const next = await remoteStationAction({ type: 'refresh' }); if (mounted.current) setStatus(next) }
        catch (error) { if (mounted.current) setError(typeof error === 'string' ? error : 'serviceUnavailable') }
        finally { pending.current = false }
      })()
    }, 5000)
    return () => { mounted.current = false; clearInterval(timer) }
  }, [])
  async function act(action: RemoteStationAction) {
    // Disable cancels the native socket even while an HTTP request is pending.
    if (pending.current && action.type !== 'disable' && action.type !== 'takeOverLogging') return
    pending.current = true; setBusy(true); setError(null)
    try { const next = await remoteStationAction(action); if (mounted.current) setStatus(next) }
    catch (error) { if (mounted.current) setError(typeof error === 'string' ? error : 'serviceUnavailable') }
    finally { pending.current = false; if (mounted.current) setBusy(false) }
  }
  const connected = status && ['connected', 'connecting', 'reconnecting'].includes(status.phase)
  const issue = error ?? status?.error, phase = status?.phase
  return <div className="remote-section remote-native">
    <p>{t('remote.nativeIntro')}</p>
    <p role="status">{phase === 'unpaired' ? t('remote.unpaired') : phase === 'pairing' ? t('remote.pairing')
      : phase === 'approval' ? t('remote.localApproval') : phase === 'disabled' ? t('remote.disabled')
      : phase === 'connected' ? t('remote.connected') : phase === 'reconnecting' ? t('remote.reconnecting') : t('monitor.connecting')}</p>
    {issue && <p role="alert">{issue === 'credentialStoreUnavailable' ? t('remote.vaultFailed') : t('remote.requestFailed')}</p>}
    {phase === 'unpaired' && <>
      <label>{t('remote.stationName')}<input value={name} maxLength={48} onChange={event => setName(event.target.value)} /></label>
      <button type="button" className="remote-button" disabled={busy || !name.trim()} onClick={() => void act({ type: 'begin', name })}>{t('remote.pairStation')}</button>
    </>}
    {status?.pairingCode && <>
      <p>{t('remote.openService')} <a href={status.origin} target="_blank" rel="noreferrer">{status.origin}</a></p>
      <p>{t('remote.pairingCode')} <code>{status.pairingCode.match(/.{1,4}/g)?.join(' ')}</code></p>
      <p>{t('remote.codeExpires')}</p>
      {status.accountId && <p>{t('remote.accountMatch')} <code>{status.accountId}</code></p>}
      <div className="remote-actions">
        {phase === 'approval' && status.accountId && status.pairingId && <button type="button" className="remote-button" disabled={busy}
          onClick={() => void act({ type: 'approve', accountId: status.accountId!, enrollmentId: status.pairingId! })}>{t('remote.approvePairing')}</button>}
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
      {status.loggingPermissions && <><p>{t('remote.loggingLocalHint')}</p><button type="button" className="remote-button" onClick={()=>void act({type:'takeOverLogging'})}>{status.stationPermissions ? t('remote.controlTakeOver') : t('remote.loggingTakeOver')}</button></>}
      {status.stationPermissions && <p>{t('remote.controlLocalHint')}</p>}
      <h3>{t('remote.browserApprovals')}</h3><p>{t('remote.browserMatch')}</p>
      {status.devices.length === 0 && <p>{t('remote.noBrowsers')}</p>}
      {status.devices.map(device => <div key={device.id}>
        <p>{device.name} <code>{device.id.slice(-6)}</code></p>
        <button type="button" className="remote-button" disabled={busy} onClick={() => void act({ type: 'device', deviceId: device.id, approve: device.approved !== 1 })}>
          {device.approved === 1 ? t('remote.revokeBrowser') : t('remote.approveBrowser')}</button>
        {device.approved===1&&status.loggingPermissions&&<button type="button" className="remote-button" disabled={busy||!connected} onClick={()=>void act({type:'loggingPermission',deviceId:device.id,allow:!status.loggingPermissions!.includes(device.id)})}>{status.loggingPermissions.includes(device.id)?t('remote.loggingRevoke'):t('remote.loggingAllow')}</button>}
        {device.approved===1&&status.stationPermissions&&<button type="button" className="remote-button" disabled={busy||!connected} onClick={()=>void act({type:'stationPermission',deviceId:device.id,allow:!status.stationPermissions!.includes(device.id)})}>{status.stationPermissions.includes(device.id)?t('remote.controlRevoke'):t('remote.controlAllow')}</button>}
        {status.loggingController===device.id&&<p role="status">{status.stationPermissions?.includes(device.id)?t('remote.controlActive'):t('remote.loggingController')}</p>}
      </div>)}
      <details><summary>{t('remote.stationAccess')}</summary><p>{t('remote.revokeHint')}</p>
        <button type="button" className="remote-button" disabled={busy} onClick={() => void act({ type: 'forget' })}>{t('remote.revokeStation')}</button>
      </details>
    </>}
  </div>
}
