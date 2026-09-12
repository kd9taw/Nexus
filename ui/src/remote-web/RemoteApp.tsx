import { lazy, Suspense, useEffect, useState } from 'react'
import { t } from '../i18n'
import { useViewport } from '../useViewport'
import { MonitorApp } from '../remote-monitor/MonitorApp'
import { BrowserClient, HostedConnection, RemoteError } from './client'
import type { AccountSession } from './client'
import '../remote-monitor/monitor.css'
import './remote.css'

const BRAND = 'Nexus Remote'
// The service names what it refused. Anything that is not a refusal - a dropped connection, a
// parse failure - has no name worth showing, so it falls back to the generic message.
const reason = (cause: unknown) => cause instanceof RemoteError ? cause.code : 'remoteUnavailable'
function AccountViewport() { useViewport(1, true); return null }
const BrowserApplication = lazy(() => import('./BrowserApplication').then(module => ({ default: module.BrowserApplication })))
export function RemoteApp() {
  const [client, setClient] = useState<BrowserClient | null>(null)
  const [ready, setReady] = useState(false)
  const [configured, setConfigured] = useState(true)
  const [session, setSession] = useState<AccountSession | null>(null)
  const [connection, setConnection] = useState<HostedConnection | null>(null)
  const [workspace, setWorkspace] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [code, setCode] = useState('')
  const [claimed, setClaimed] = useState(false)
  const [deviceName, setDeviceName] = useState('')
  const [loadAttempt, setLoadAttempt] = useState(0)

  useEffect(() => {
    let active = true
    setReady(false); setError(null)
    void BrowserClient.load().then(async value => {
      if (!active) return
      setClient(value); setConfigured(value !== null)
      if (value && await value.authenticated()) {
        const account = await value.post<AccountSession>('session')
        if (active) setSession(account)
      }
    }).catch((cause: unknown) => { if (active) setError(reason(cause)) }).finally(() => { if (active) setReady(true) })
    return () => { active = false }
  }, [loadAttempt])
  useEffect(() => {
    if (!client || !session || connection) return
    let active = true, pending = false
    const interval = setInterval(() => {
      if (pending) return
      pending = true
      void client.post<AccountSession>('session').then(value => { if (active) setSession(value) })
        .catch((cause: unknown) => { if (active) setError(reason(cause)) }).finally(() => { pending = false })
    }, 5000)
    return () => { active = false; clearInterval(interval) }
  }, [client, !!session, connection])
  useEffect(() => () => { connection?.stop() }, [connection])

  async function act(work: () => Promise<void>) {
    if (busy) return
    setBusy(true); setError(null)
    // Keep the service's own word for the refusal. Collapsing every failure to one boolean is
    // why an expired trial, a full station list and a stale pairing code all used to produce
    // the same sentence, none of which told the operator what to actually do next.
    try { await work() }
    catch (cause) { setError(cause instanceof RemoteError ? cause.code : 'remoteUnavailable') }
    finally { setBusy(false) }
  }
  async function refresh() { if (client) setSession(await client.post<AccountSession>('session')) }
  if (connection && workspace) return <Suspense fallback={<p role="status">{t('monitor.connecting')}</p>}>
    <BrowserApplication connection={connection} disconnect={() => { connection.stop(); setConnection(null) }} />
  </Suspense>
  if (connection) return <MonitorApp source={connection.source} navigation={
    <button className="remote-button" onClick={() => { connection.stop(); setConnection(null) }}>{t('remote.disconnect')}</button>
  } />
  // The service decides this and says so; the browser must not re-derive it from enabled plus
  // expiresAt against its own clock, because a laptop with the wrong time would then disagree
  // with the service about whether the trial is live.
  const trial = session?.entitlement
  const entitled = trial?.state === 'active'
  // An account that has never held a trial may pair - that is the self-serve path, and gating
  // the pairing form on `entitled` made it unreachable for exactly the people who need it.
  const canPair = trial?.state === 'active' || trial?.state === 'none'
  const daysLeft = trial && session
    ? Math.max(0, Math.ceil((trial.expiresAt - session.serverNow) / 86400000))
    : 0
  // Formatted invariantly as a UTC calendar date, not through Intl.DateTimeFormat: this project
  // forbids locale date formatting on this path, and for a date an operator may be planning
  // around, 2026-09-26 cannot be misread the way 09/12 can between one country and the next.
  const utcDate = (ms: number) => new Date(ms).toISOString().slice(0, 10)
  // Written out rather than looked up in a map: the catalog's orphan check scans for literal
  // t() calls, so a dynamic lookup reads as "nobody uses these keys" and the compiler stops
  // checking them too. Anything the service refuses without a name falls through to the last arm.
  // A pairing code is sixteen hex characters. Checking only the LENGTH let an O-for-0 typo reach
  // the service, and `pair/claim` is rate limited to five attempts per ten minutes - the same ten
  // minutes the code itself lives. Three typos off the shack screen could cost the whole code.
  const pairingCode = code.replace(/[\s-]/g, '').toLowerCase()
  const pairingCodeReady = /^[0-9a-f]{16}$/.test(pairingCode)
  const pairingCodeMalformed = pairingCode.length === 16 && !pairingCodeReady

  // `trialRequired` is deliberately absent: the trial-state line above already tells the operator
  // which of the four states the account is in, with dates, and repeating it in the refusal banner
  // says the same thing twice in different words.
  // Every code below is one the BROWSER can actually receive. `stationLimit` and `pairingExpired`
  // are thrown only inside enroll/check and enroll/approve, which refuse any request carrying an
  // Origin header - the shack's native client only - so naming them here was answering questions
  // nobody in a browser can ask, while the refusals operators really hit fell through to the
  // generic sentence about checking the connection.
  const refusal = (code: string) =>
    // Same sentence the state line uses, so the two never disagree about the date.
    code === 'trialEnded' && trial ? t('remote.trialEnded', { until: utcDate(trial.expiresAt) })
    : code === 'trialDisabled' ? t('remote.trialDisabled')
    : code === 'invalidPairingCode' ? t('remote.pairingExpired')
    : code === 'stationLimit' ? t('remote.stationLimitReached')
    : code === 'deviceLimit' ? t('remote.deviceLimitReached')
    : code === 'signInRequired' ? t('remote.signInAgain')
    : code === 'stationUnavailable' ? t('remote.stationOffline')
    : t('remote.requestFailed')
  return <div className="app remote-monitor-app remote-service-app">
    <AccountViewport />
    <header className="rm-header"><strong>{BRAND}</strong>
      {session && <button className="remote-button" disabled={busy} onClick={() => void act(async () => { await client?.signOut(); setSession(null) })}>{t('remote.signOut')}</button>}
    </header>
    <main className="rm-scroll" aria-label={t('remote.stations')}><div className="rm-content remote-account">
      <h1>{t('remote.stations')}</h1>
      <p>{t('remote.pilotIntro')}</p>
      {error && <p className="rm-warning" role="alert">{refusal(error)}</p>}
      {!ready && <p role="status">{t('monitor.connecting')}</p>}
      {ready && !configured && <p role="status">{t('remote.notConfigured')}</p>}
      {ready && error !== null && !client && <button className="remote-button" onClick={() => setLoadAttempt(value => value + 1)}>{t('shell.crash.retry')}</button>}
      {ready && client && !session && <div className="remote-actions">
        <button className="remote-button" disabled={busy} onClick={() => void act(async () => { await client.signIn() })}>{t('remote.signIn')}</button>
        {/* A first-time operator should not have to find a sign-up link on somebody else's login
            form. This is the same flow, opened on the create-account screen instead. */}
        <button className="remote-button" disabled={busy} onClick={() => void act(async () => { await client.signIn(true) })}>{t('remote.createAccount')}</button>
      </div>}
      {session && <>
        {/* The account id is only an instruction while there is something to pair. Once stations
            are approved it is support detail, not a step, and a raw UUID presented as a standing
            instruction reads as something the operator still has to act on. It stays in the DOM
            either way so it can always be quoted when asking for help. */}
        {canPair && session.stations.length < 2
          ? <p>{t('remote.accountMatch')} <code>{session.accountId}</code></p>
          : <details><summary>{t('remote.supportDetails')}</summary>
              <p>{t('remote.accountMatch')} <code>{session.accountId}</code></p></details>}
        {trial?.state === 'none' && <p role="status">{t('remote.trialNotStarted')}</p>}
        {/* The pilot branch must survive: an absent start stays absent and is NEVER computed as
            expiresAt minus fourteen days. Migration 0002 deliberately did not backfill, because an
            invented start cannot afterwards be told from a real one. */}
        {trial?.state === 'active' && <p role="status">{trial.startedAt === null
          ? t('remote.trialUnknownStart', { until: utcDate(trial.expiresAt) })
          : t('remote.trialRunning', { days: daysLeft, from: utcDate(trial.startedAt), until: utcDate(trial.expiresAt) })}</p>}
        {trial?.state === 'ended' && <p role="status">{t('remote.trialEnded', { until: utcDate(trial.expiresAt) })}</p>}
        {trial?.state === 'disabled' && <p role="status">{t('remote.trialDisabled')}</p>}
        {session.stations.map(station => <section className="rm-card remote-section" key={station.id}>
          <h2>{station.name}</h2>
          {station.device?.approved === 1 ? <div className="remote-actions">
            <button className="remote-button" disabled={busy || !entitled} onClick={() => {
              const next = new HostedConnection(client!, station.id, true); setWorkspace(true); setConnection(next); next.start()
            }}>{t('remote.openNexus')}</button>
            <button className="remote-button" disabled={busy || !entitled} onClick={() => {
              const next = new HostedConnection(client!, station.id); setWorkspace(false); setConnection(next); next.start()
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
        {canPair && session.stations.length < 2 && <section className="rm-card remote-section">
          <h2>{t('remote.pairStation')}</h2><p>{t('remote.enterCodeHint')}</p>
          {/* Guarded on the SUBMIT, not only the button: Enter in the field submits a form whose
              button is disabled, which would spend a rate-limited attempt on a typo anyway. */}
          <form onSubmit={event => { event.preventDefault(); if (!pairingCodeReady) return; void act(async () => {
            await client?.post('pair/claim', { code: pairingCode }); setCode(''); setClaimed(true); await refresh()
          }) }}>
            <label>{t('remote.pairingCode')}<input autoComplete="off" spellCheck={false} value={code} maxLength={24} required onChange={event => setCode(event.target.value)} /></label>
            <button className="remote-button" disabled={busy || !pairingCodeReady}>{t('remote.claimStation')}</button>
          {pairingCodeMalformed && <p role="status">{t('remote.pairingCodeMalformed')}</p>}
          </form>
          {claimed && <p role="status">{t('remote.approveInShack')}</p>}
        </section>}
      </>}
      <a href="/remote-licenses.txt">{t('remote.licenses')}</a>
    </div></main>
  </div>
}
