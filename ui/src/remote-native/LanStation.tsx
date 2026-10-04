// Settings ▸ Station ▸ Remote access ▸ Remote over this network: this station's own listener, for
// Nexus on another computer on the same network, with no internet (src-tauri/src/remote_service/lan).
// The switch, then where it listens, the pairing window, the paired computers, and the reset.
//
// ONE PRESS (the operator's ruling of 2026-10-04): Pair a computer shows a code, and that press is
// the approval. Whoever types the code within ten minutes is paired at once, and stays paired until
// removed here (the ruling "Until revoked"), so the card says so beside the code.
//
// ONLY AT THE SHACK (the same day's ruling): turning this on or off, picking the network, making a
// code, removing a computer and resetting the key are refused while the press comes through a
// stream (`isStreamInput`, the mark Lock and the Phone PTT already use). A streamed operator sees
// the card and can press nothing on it; the refusal says why.
//
// THE NETWORK: with more than one left (virtual adapters and VPNs are not offered), the operator
// picks. WINDOWS' OWN PROMPT (the same day's ruling): Nexus adds no firewall rule. The card says
// beforehand to allow Private networks only, says what in the firewall stands in the way once it
// listens, and says plainly what it cannot see from here: a network that keeps its devices apart.
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
    case 'addressGone': return t('remote.lan.reason.addressGone', { address: lan.picked ?? '' })
    case 'chooseAddress': return t('remote.lan.reason.chooseAddress')
    case 'portInUse': return t('remote.lan.reason.portInUse')
    case 'noNetwork': return t('remote.lan.reason.noNetwork')
    case 'unavailable': return t('remote.lan.reason.unavailable')
  }
  return lan?.on ? t('remote.lan.starting') : t('remote.lan.off')
}

/** What stands in the way in Windows' firewall, on the network it listens on. */
function firewallLine(firewall: LanStatus['firewall']): string | null {
  switch (firewall) {
    case 'blocksAll': return t('remote.lan.firewall.blocksAll')
    case 'blocked': return t('remote.lan.firewall.blocked')
    case 'managed': return t('remote.lan.firewall.managed')
    case 'public': return t('remote.lan.firewall.public')
    case 'silent': return t('remote.lan.firewall.silent')
    case 'ask': return t('remote.lan.firewall.ask')
  }
  return null
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
  const networks = lan?.networks ?? []
  // A pick this computer does not have right now stays in the list, so it is seen and can be undone.
  const pickedGone = lan?.picked && !networks.some(network => network.address === lan.picked) ? lan.picked : null
  const firewall = lan?.listening ? firewallLine(lan.firewall) : null
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
      {!on && <span className="settings-hint">{t('remote.lan.firewallFirst')}</span>}
    </div>
    <div className="remote-section remote-native">
      <p role="status">{statusLine(lan)}</p>
      {refused && <p role="alert">{t('remote.lan.atShackOnly')}</p>}
      {error && <p role="alert">{errorLine(error)}</p>}
      {on && (networks.length > 1 || pickedGone) && <label className="settings-field">
        <span className="settings-label">{t('remote.lan.network')}</span>
        <select className="settings-input" value={lan?.picked ?? ''} disabled={busy}
          onChange={event => void act(event.target.value ? { type: 'lanAddress', address: event.target.value } : { type: 'lanAddress' })}>
          <option value="">{t('remote.lan.networkAuto')}</option>
          {networks.map(network => <option key={network.address} value={network.address}>
            {t('remote.lan.networkChoice', { name: network.name, address: network.address })}</option>)}
          {pickedGone && <option value={pickedGone}>{t('remote.lan.networkGone', { address: pickedGone })}</option>}
        </select>
        <span className="settings-hint">{t('remote.lan.networkHint')}</span>
      </label>}
      {firewall && <p className="remote-warning">{firewall}</p>}
      {lan?.listening && lan.named === false && <p>{t('remote.lan.notNamed')}</p>}
      {lan?.listening && <p>{t('remote.lan.guest')}</p>}
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
