// Settings ▸ Station ▸ Remote access ▸ Remote over this network: this station's own listener, for
// Nexus on another computer on the same network, with no internet (src-tauri/src/remote_service/lan).
// The switch, then where it listens, the pairing window, the paired computers, and the reset.
//
// ONE PRESS (the operator's ruling of 2026-10-04): Pair a computer shows a code, and that press is
// the approval. Whoever types the code within ten minutes is paired at once, and stays paired until
// removed here (the ruling "Until revoked"), so the card says so beside the code.
//
// ONLY AT THE SHACK (the same day's ruling): turning this on or off, making a code, removing a
// computer and resetting the key are refused while the press comes through a stream
// (`isStreamInput`, the mark Lock and the Phone PTT already use). A streamed operator sees the card
// and can press nothing on it; the refusal says why.
import { useEffect, useRef, useState } from 'react'
import { getRemoteStationStatus, remoteStationAction } from '../api'
import { t } from '../i18n'
import { isStreamInput } from './stream-input'
import type { LanStatus, RemoteStationAction } from './types'
import '../remote-web/remote.css'

/** How often the card reads the station: a pairing completes, or a listener moves, at the shack. */
export const LAN_POLL_MS = 2000

/** The first `chars` hexadecimal characters, in groups of four. */
const grouped = (hex: string, chars: number) => (hex.slice(0, chars).match(/.{1,4}/g) ?? []).join(' ')
/** A key's fingerprint as both ends show it: its first 128 bits, eight groups of four. */
const fingerprint = (hex: string) => grouped(hex.toUpperCase(), 32)

function statusLine(lan: LanStatus | null): string {
  if (lan?.on && lan.listening) return t('remote.lan.listening', { address: lan.listening })
  switch (lan?.reason) {
    case 'noKey': return t('remote.lan.reason.noKey')
    case 'endedAtShack': return t('remote.lan.reason.endedAtShack')
    case 'addressGone': return t('remote.lan.reason.addressGone')
    case 'chooseAddress': return t('remote.lan.reason.chooseAddress')
    case 'portInUse': return t('remote.lan.reason.portInUse')
    case 'noNetwork': return t('remote.lan.reason.noNetwork')
    case 'unavailable': return t('remote.lan.reason.unavailable')
  }
  return lan?.on ? t('remote.lan.starting') : t('remote.lan.off')
}

function errorLine(error: string): string {
  switch (error) {
    case 'credentialStoreUnavailable': return t('remote.lan.vaultFailed')
    case 'lanFull': return t('remote.lan.full')
    case 'noKey': return t('remote.lan.reason.noKey')
  }
  return t('remote.lan.failed')
}

export function LanStation() {
  const [lan, setLan] = useState<LanStatus | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [refused, setRefused] = useState(false)
  const mounted = useRef(true)
  useEffect(() => {
    mounted.current = true
    const read = async () => {
      try { const next = await getRemoteStationStatus(); if (mounted.current) setLan(next.lan ?? null) }
      catch { /* the next read tries again */ }
    }
    void read()
    const timer = setInterval(() => void read(), LAN_POLL_MS)
    return () => { mounted.current = false; clearInterval(timer) }
  }, [])
  async function act(action: RemoteStationAction) {
    // Only at the shack: a press that comes through a stream changes nothing here. Read before
    // anything awaits, while the stream's dispatch is still the one running.
    if (isStreamInput()) { setRefused(true); return }
    setRefused(false); setBusy(true); setError(null)
    try { const next = await remoteStationAction(action); if (mounted.current) setLan(next.lan ?? null) }
    catch (failure) { if (mounted.current) setError(typeof failure === 'string' ? failure : 'unavailable') }
    finally { if (mounted.current) setBusy(false) }
  }
  const on = lan?.on === true
  const devices = lan?.devices ?? []
  return <>
    <div className="settings-field">
      <label className="settings-toggle">
        <span className="settings-label">{t('remote.lan.switch')}</span>
        <button type="button" role="switch" aria-checked={on} className={`toggle${on ? ' on' : ''}`} disabled={busy || !lan}
          onClick={() => void act({ type: on ? 'lanOff' : 'lanOn' })}>
          <span className="toggle-knob" />
        </button>
      </label>
      <span className="settings-hint">{t('remote.lan.intro')}</span>
    </div>
    <div className="remote-section remote-native">
      <p role="status">{statusLine(lan)}</p>
      {refused && <p role="alert">{t('remote.lan.atShackOnly')}</p>}
      {error && <p role="alert">{errorLine(error)}</p>}
      {on && !lan?.pairing && <div className="remote-actions">
        <button type="button" className="remote-button" disabled={busy} onClick={() => void act({ type: 'lanPair' })}>{t('remote.lan.pair')}</button>
      </div>}
      {lan?.pairing && <>
        <p>{t('remote.pairingCode')} <code>{grouped(lan.pairing.code, 16)}</code></p>
        <p>{t('remote.lan.codeHint')}</p>
        <p className="remote-warning">{t('remote.lan.codeWarning')}</p>
        {lan.listening && lan.key && <p>{t('remote.lan.thisStation', { address: lan.listening, key: fingerprint(lan.key) })}</p>}
        <div className="remote-actions">
          <button type="button" className="remote-button" disabled={busy} onClick={() => void act({ type: 'lanCancel' })}>{t('remote.cancelPairing')}</button>
        </div>
      </>}
      <h3>{t('remote.lan.computers')}</h3>
      {devices.length === 0 ? <p>{t('remote.lan.noComputers')}</p> : <ul className="remote-native-browsers">
        {devices.map(device => <li key={device.id} className="remote-native-browser">
          <span className="remote-native-browser-name">{device.name}</span>
          <span className="remote-native-browser-key">{t('remote.browserKey', { key: fingerprint(device.key) })}</span>
          <span className="remote-actions">
            <button type="button" className="remote-button" disabled={busy} onClick={() => void act({ type: 'lanRevoke', deviceId: device.id })}>{t('remote.native.remove')}</button>
          </span>
        </li>)}
      </ul>}
      <details className="remote-native-advanced"><summary>{t('remote.lan.resetTitle')}</summary>
        <p>{t('remote.lan.resetHint')}</p>
        <div className="remote-actions">
          <button type="button" className="remote-button" disabled={busy || !lan} onClick={() => void act({ type: 'lanReset' })}>{t('remote.lan.reset')}</button>
        </div>
      </details>
    </div>
  </>
}
