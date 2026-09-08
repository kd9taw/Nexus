import { useEffect, useState } from 'react'
import { t } from '../i18n'
import { useViewport } from '../useViewport'
import { MonitorApp } from '../remote-monitor/MonitorApp'
import { BrowserClient, HostedConnection } from './client'
import type { AccountSession } from './client'
import '../remote-monitor/monitor.css'
import './remote.css'

const BRAND = 'Nexus Remote'
export function RemoteApp() {
  useViewport(1, true)
  const [client, setClient] = useState<BrowserClient | null>(null)
  const [ready, setReady] = useState(false)
  const [configured, setConfigured] = useState(true)
  const [session, setSession] = useState<AccountSession | null>(null)
  const [connection, setConnection] = useState<HostedConnection | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(false)
  const [code, setCode] = useState('')
  const [claimed, setClaimed] = useState(false)
  const [deviceName, setDeviceName] = useState('')
  const [loadAttempt, setLoadAttempt] = useState(0)

  useEffect(() => {
    let active = true
    setReady(false); setError(false)
    void BrowserClient.load().then(async value => {
      if (!active) return
      setClient(value); setConfigured(value !== null)
      if (value && await value.authenticated()) {
        const account = await value.post<AccountSession>('session')
        if (active) setSession(account)
      }
    }).catch(() => { if (active) setError(true) }).finally(() => { if (active) setReady(true) })
    return () => { active = false }
  }, [loadAttempt])
  useEffect(() => {
    if (!client || !session || connection) return
    let active = true, pending = false
    const interval = setInterval(() => {
      if (pending) return
      pending = true
      void client.post<AccountSession>('session').then(value => { if (active) setSession(value) })
        .catch(() => { if (active) setError(true) }).finally(() => { pending = false })
    }, 5000)
    return () => { active = false; clearInterval(interval) }
  }, [client, !!session, connection])
  useEffect(() => () => { connection?.stop() }, [connection])

  async function act(work: () => Promise<void>) {
    if (busy) return
    setBusy(true); setError(false)
    try { await work() } catch { setError(true) } finally { setBusy(false) }
  }
  async function refresh() { if (client) setSession(await client.post<AccountSession>('session')) }
  if (connection) return <MonitorApp source={connection.source} navigation={
    <button className="remote-button" onClick={() => { connection.stop(); setConnection(null) }}>{t('remote.disconnect')}</button>
  } />
  const entitled = !!session?.entitlement.enabled && session.entitlement.expiresAt > Date.now()
  return <div className="app remote-monitor-app remote-service-app">
    <header className="rm-header"><strong>{BRAND}</strong>
      {session && <button className="remote-button" disabled={busy} onClick={() => void act(async () => { await client?.signOut(); setSession(null) })}>{t('remote.signOut')}</button>}
    </header>
    <main className="rm-scroll" aria-label={t('remote.stations')}><div className="rm-content remote-account">
      <h1>{t('remote.stations')}</h1>
      <p>{t('remote.pilotIntro')}</p>
      {error && <p className="rm-warning" role="alert">{t('remote.requestFailed')}</p>}
      {!ready && <p role="status">{t('monitor.connecting')}</p>}
      {ready && !configured && <p role="status">{t('remote.notConfigured')}</p>}
      {ready && error && !client && <button className="remote-button" onClick={() => setLoadAttempt(value => value + 1)}>{t('shell.crash.retry')}</button>}
      {ready && client && !session && <button className="remote-button" disabled={busy} onClick={() => void act(async () => { await client.signIn() })}>{t('remote.signIn')}</button>}
      {session && <>
        <p>{t('remote.accountMatch')} <code>{session.accountId}</code></p>
        {!entitled && <p role="status">{t('remote.trialRequired')}</p>}
        {session.stations.map(station => <section className="rm-card remote-section" key={station.id}>
          <h2>{station.name}</h2>
          {station.device?.approved === 1 ? <div className="remote-actions">
            <button className="remote-button" disabled={busy || !entitled} onClick={() => {
              const next = new HostedConnection(client!, station.id); setConnection(next); next.start()
            }}>{t('remote.observe')}</button>
            <button className="remote-button" disabled={busy} onClick={() => void act(async () => {
              await client?.post(`stations/${station.id}/forget-device`); await refresh()
            })}>{t('remote.forgetBrowser')}</button>
          </div> : station.device ? <p role="status">{t('remote.awaitDevice')} <code>{station.device.id.slice(-6)}</code></p> : <form onSubmit={event => {
            event.preventDefault(); void act(async () => { await client?.post(`stations/${station.id}/device`, { name: deviceName }); await refresh() })
          }}>
            <label>{t('remote.browserName')}<input value={deviceName} maxLength={48} required onChange={event => setDeviceName(event.target.value)} /></label>
            <button className="remote-button" disabled={busy || !entitled || !deviceName.trim()}>{t('remote.requestApproval')}</button>
          </form>}
          <details><summary>{t('remote.stationAccess')}</summary><p>{t('remote.revokeHint')}</p>
            <button className="remote-button" disabled={busy} onClick={() => void act(async () => { await client?.post(`stations/${station.id}/revoke`); await refresh() })}>{t('remote.revokeStation')}</button>
          </details>
        </section>)}
        {entitled && session.stations.length < 2 && <section className="rm-card remote-section">
          <h2>{t('remote.pairStation')}</h2><p>{t('remote.enterCodeHint')}</p>
          <form onSubmit={event => { event.preventDefault(); void act(async () => {
            await client?.post('pair/claim', { code: code.replace(/[\s-]/g, '').toLowerCase() }); setCode(''); setClaimed(true); await refresh()
          }) }}>
            <label>{t('remote.pairingCode')}<input autoComplete="off" spellCheck={false} value={code} maxLength={24} required onChange={event => setCode(event.target.value)} /></label>
            <button className="remote-button" disabled={busy || code.replace(/[\s-]/g, '').length !== 16}>{t('remote.claimStation')}</button>
          </form>
          {claimed && <p role="status">{t('remote.approveInShack')}</p>}
        </section>}
      </>}
      <a href="/remote-licenses.txt">{t('remote.licenses')}</a>
    </div></main>
  </div>
}
