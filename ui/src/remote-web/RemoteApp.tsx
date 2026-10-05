import { lazy, Suspense, useEffect, useRef, useState } from 'react'
import { t } from '../i18n'
import { useViewport } from '../useViewport'
import { MonitorApp } from '../remote-monitor/MonitorApp'
import { BrowserClient, HostedConnection, RemoteError } from './client'
import { FeedWatch } from './FeedWatch'
import { StreamView } from './StreamView'
import { deviceKey } from './device-key'
import { acceptStationKey, forgetStationKey, stationKeyView, type StationKeyView } from './station-key'
import { browserLabel } from './browser-label'
import { BetaNote } from './BetaNote'
import { shortFingerprint } from './stream-protocol'
import type { AccountSession } from './client'
import '../remote-monitor/monitor.css'
import './remote.css'
import './remote-site.css'
// The Nexus mark, bundled by vite. Small enough to inline as a data: URI, which the Worker's
// img-src ('self' data: blob:) already allows; as a file it would be 'self'.
import nexusMark from './nexus-mark.svg'

// The product name is an invariant token, split only so the service half can take the accent
// colour, the way hamradiotools.io writes ham<amber>radio</amber>tools.
const BRAND = 'Nexus'
const BRAND_SERVICE = 'Remote'
// The mark is decorative: the words beside it already name the product.
const Wordmark = () => <span className="remote-site-wordmark">
  <img className="remote-site-mark" src={nexusMark} alt="" width={28} height={28} />
  <strong>{BRAND} <span className="remote-site-accent">{BRAND_SERVICE}</span></strong>
</span>
// The only route back to the operator from inside the app. An account whose trial has ended or
// been switched off could previously read an accurate sentence and then had nowhere to go, which
// reads as "this product is finished with me" rather than "ask and it can be extended". During a
// closed beta every trial is granted by hand anyway, so asking is the actual mechanism.
const BETA_CHANNEL = 'https://discord.gg/mCCBaRKj3'
// The old watch/control workspace (Open Nexus, Observe station) is hidden behind this flag while
// streaming is proven (the operator, 2026-10-02: "The old watch/control workspace is hidden, though
// its code stays so we can bring it back after streaming is proven"). Set it in the browser's console
// on this page, then reload: localStorage.setItem('nexus.remote.workspace', 'on'). The compiled sweep
// sets it to keep the workspace covered. Read on every render, so it never needs a rebuild.
const WORKSPACE_FLAG = 'nexus.remote.workspace'
const workspaceShown = () => { try { return localStorage.getItem(WORKSPACE_FLAG) === 'on' } catch { return false } }
// Browser approval lifetime: warn this long before the end that using this browser cannot move.
const APPROVAL_WARNING_MS = 7 * 86400000
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
  // The station name while the stream is the open view, or null. A view of its own: it shares the
  // connection and its lease/Stop core with the workspace and nothing else.
  const [streaming, setStreaming] = useState<string | null>(null)
  // The station name while Listen is the open view (its receive audio, no picture), or null. The
  // same connection, lease and Stop as the stream, and the audio lane the workspace's Listen used.
  const [listening, setListening] = useState<string | null>(null)
  // The station the open view is for: its key goes with the stream page's approval sentence.
  const [opened, setOpened] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [code, setCode] = useState('')
  // Keyed by station id: two cards must not share one edit box, and opening one must not put the
  // other into edit mode. null means nobody is renaming.
  const [renaming, setRenaming] = useState<{ id: string; name: string } | null>(null)
  const [loadAttempt, setLoadAttempt] = useState(0)
  // A5: this browser's device key for each station it has a device on, by station: its fingerprint,
  // shown beside the browser for the operator to compare with Nexus at the shack.
  const [keys, setKeys] = useState<Record<string, string>>({})
  // S3-L1: the station's own key, by station, for each station this browser has a device on: kept here
  // the first time the service lists one, and shown beside this browser's key to compare with Nexus at
  // the shack. While the service lists another, the card says so with both, and the stream stays off
  // until the operator accepts the new one there.
  const [stationKeys, setStationKeys] = useState<Record<string, StationKeyView>>({})
  // Counts the operator's accepts of a new station key: each one has the kept keys read again.
  const [keyAccepts, setKeyAccepts] = useState(0)
  const registered = useRef(new Set<string>())

  useEffect(() => {
    let active = true
    setReady(false); setError(null)
    void BrowserClient.load().then(async value => {
      if (!active) return
      setClient(value); setConfigured(value !== null)
      // A deny that is not the unconfirmed-email case keeps its refusal sentence; that one gets
      // its own state below instead of a banner.
      if (value?.signInRefusal === 'signInRefused') setError('signInRefused')
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
  // A5: the key is made the first time a station needs it (a browser approved before keys has none
  // yet), and the service is sent it whenever it holds another or none: the station can only pin the
  // key the service lists. Sending it leaves the approval alone; a key the station has not pinned is
  // approved again at the radio. Silent: a refusal (an ended trial) leaves the stream refused by name.
  useEffect(() => {
    if (!client || !session) return
    let active = true
    for (const station of session.stations) {
      const device = station.device
      if (!device) continue
      void deviceKey(station.id).then(async key => {
        if (!active || !key) return
        setKeys(current => current[station.id] === key.fingerprint ? current : { ...current, [station.id]: key.fingerprint })
        const attempt = `${station.id}:${key.publicKey}`
        if (device.publicKey === key.publicKey || registered.current.has(attempt)) return
        registered.current.add(attempt)
        await client.post(`stations/${station.id}/device`, { name: device.name, publicKey: key.publicKey }).catch(() => {})
      })
    }
    return () => { active = false }
  }, [client, session])
  // S3-L1: each listing of the stations: the station's key kept at first sight, against the one listed.
  // Read again after an accept, and only the latest read is shown: a poll's read begun before the
  // accept still holds the key it replaced, and would otherwise bring the warning back after it.
  useEffect(() => {
    if (!session) return
    let active = true
    void Promise.all(session.stations.map(async station =>
      [station.id, station.device ? await stationKeyView(station.id, station.stationKey) : null] as const))
      .then(views => { if (active) setStationKeys(Object.fromEntries(views.filter((view): view is readonly [string, StationKeyView] => view[1] !== null))) })
      .catch(() => {})
    return () => { active = false }
  }, [session, keyAccepts])

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
  // A5: the device is created with this browser's key, the one it will sign its streams with, under
  // the name it gives itself ("Chrome on Windows"); the shack shows both, and the key is the check.
  const ask = (stationId: string) => void act(async () => {
    const key = await deviceKey(stationId)
    await client?.post(`stations/${stationId}/device`, { name: browserLabel(), ...(key ? { publicKey: key.publicKey } : {}) }); await refresh()
  })
  function open(stationId: string, application: boolean, stream: string | null = null, listen: string | null = null) {
    // The stream rides the application socket: its signalling and the lease it is offered under
    // travel there.
    // A5: the stream's offer is signed with this browser's key for the station, and so is what the
    // older lanes carry (S1-M1). S3-M1: the station's answer must be signed with the key the service
    // lists for it.
    const station = session?.stations.find(station => station.id === stationId), device = station?.device
    const next = new HostedConnection(client!, stationId, application || stream !== null || listen !== null, undefined,
      device ? { id: device.id, key: () => deviceKey(stationId) } : undefined, station?.stationKey ?? null)
    // A refused ticket is final: the trial ended mid-session, the station or this browser was
    // revoked, or the sign-in expired. The workspace used to stay up saying "Station data
    // unavailable… check that Nexus is running", which blamed the shack. Go back to the account
    // page instead, where the trial line and the station list show what actually changed.
    next.onRefused = cause => {
      next.stop()
      setConnection(current => current === next ? null : current)
      setError(cause.code === 'signInRequired' ? 'signInRequired' : 'sessionEnded')
      void refresh().catch(() => {})
    }
    setWorkspace(application); setStreaming(stream); setListening(listen); setOpened(stationId); setConnection(next); next.start()
  }
  // S3-L1: the operator takes the key the card shows as new, having compared it with the shack's. The
  // card then shows the keys as kept, read again.
  const acceptKey = (stationId: string, view: StationKeyView) => void act(async () => {
    await acceptStationKey(stationId, view.listed)
    setKeyAccepts(value => value + 1)
  })
  const keyChanged = (stationId: string) => !!stationKeys[stationId] && stationKeys[stationId].keptPrint !== stationKeys[stationId].print
  const leave = () => { connection?.stop(); setConnection(null) }
  const signOutOfSession = () => { connection?.stop(); setConnection(null); setSession(null); void client?.signOut() }
  // Opened from the card's Stream or Listen, which IS the operator asking: the view starts once, on
  // its own, as soon as the station can be asked (the press is consumed there; a stream that ends
  // stays ended until Start is pressed again on the page).
  const browserKey = opened ? keys[opened] ?? null : null
  if (connection && streaming !== null) return <StreamView key="stream" connection={connection} station={streaming} disconnect={leave} signOut={signOutOfSession} autostart browserKey={browserKey} />
  if (connection && listening !== null) return <StreamView key="listen" connection={connection} station={listening} disconnect={leave} signOut={signOutOfSession} autostart browserKey={browserKey} mode="listen" />
  if (connection && workspace) return <Suspense fallback={<p role="status">{t('monitor.connecting')}</p>}>
    <BrowserApplication connection={connection} disconnect={leave} signOut={signOutOfSession} />
  </Suspense>
  // The observer-only browser is the one this control is most for: watching a frequency and
  // nothing else is exactly the thing a background tab would otherwise quietly stop doing.
  if (connection) return <MonitorApp source={connection.source} navigation={<>
    <FeedWatch feed={connection.feed} />
    <button className="remote-button" onClick={leave}>{t('remote.disconnect')}</button>
    <button className="remote-button" onClick={signOutOfSession}>{t('remote.signOut')}</button>
  </>} />
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
  // Why a station card cannot be used, beside the controls it disables: the trial line at the top of
  // the page says it too, but on a phone that line is off-screen by the time an operator reaches the card.
  const trialReason = trial?.state === 'ended' ? t('remote.trialEnded', { until: utcDate(trial.expiresAt) })
    : trial?.state === 'disabled' ? t('remote.trialDisabled') : t('remote.trialNotStarted')
  // The card's two ways in: Stream, the big one, and Listen, audio only. Stream only from a service
  // that carries the stream's signalling, and not while the station's key has changed (S3-L1).
  const stationActions = (stream: () => void, listen: () => void, streamOff = false) => <div className="remote-actions remote-station-actions">
    {(client?.streamVersion ?? 0) >= 1 && <button className="remote-button remote-button--primary remote-station-stream" disabled={busy || !entitled || streamOff} onClick={stream}>{t('remote.stream.open')}</button>}
    <button className="remote-button remote-station-listen" disabled={busy || !entitled} onClick={listen}>{t('remote.listen.open')}</button>
  </div>

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
    : code === 'signInRefused' ? t('remote.signInRefused')
    : code === 'stationUnavailable' ? t('remote.stationOffline')
    // One mailbox, two sign-ins (a password once, Google once): the second account is refused a
    // trial the first one is already running.
    : code === 'trialActiveElsewhere' ? t('remote.trialActiveElsewhere')
    // Rate limits on the browser's routes reset within a minute (account) or ten (claim, attach).
    : code === 'tryLater' ? t('remote.tryLater')
    // Set by the page, not the service: an open session whose next ticket was refused.
    : code === 'sessionEnded' ? t('remote.sessionEnded')
    : t('remote.requestFailed')
  // A sign-up that worked but whose address is not confirmed yet. Auth0 keeps its session through
  // the deny, so "continue" is one plain login and "use a different account" has to sign out of it.
  const confirmEmail = ready && client?.signInRefusal === 'emailUnverified' && !session
  const switchAccount = <button className="remote-button" disabled={busy} onClick={() => void act(async () => { await client?.signOut() })}>{t('remote.useDifferentAccount')}</button>
  // `remote-site` scopes the hamradiotools.io look (remote-site.css) to these account screens only.
  return <div className="app remote-monitor-app remote-service-app remote-site">
    <AccountViewport />
    <header className="rm-header"><Wordmark />
      {session && <button className="remote-button remote-button--quiet" disabled={busy} onClick={() => void act(async () => { await client?.signOut(); setSession(null) })}>{t('remote.signOut')}</button>}
    </header>
    {/* Signed out, there is no station to list: the page names the product and says in one line what
        it does. "Your stations" and the longer intro are for once there is an account. */}
    <main className="rm-scroll" aria-label={session ? t('remote.stations') : `${BRAND} ${BRAND_SERVICE}`}><div className="rm-content remote-account">
      {session ? <>
        <h1>{t('remote.stations')}</h1>
        <p className="remote-site-lead">{t('remote.pilotIntro')}</p>
      </> : <>
        <h1>{BRAND} {BRAND_SERVICE}</h1>
        <p className="remote-site-lead">{t('remote.signedOutPitch')}</p>
      </>}
      {error && <p className="rm-warning" role="alert">{refusal(error)}</p>}
      {!ready && <p role="status">{t('monitor.connecting')}</p>}
      {ready && !configured && <p role="status">{t('remote.notConfigured')}</p>}
      {ready && error !== null && !client && <button className="remote-button remote-button--primary" onClick={() => setLoadAttempt(value => value + 1)}>{t('shell.crash.retry')}</button>}
      {confirmEmail && client && <section className="rm-card remote-section remote-site-card--action">
        <h2>{t('remote.confirmEmailTitle')}</h2>
        {/* The address is not named: a denied sign-in carries no token, so the page never has it. */}
        <p>{t('remote.confirmEmailBody')}</p>
        <p>{t('remote.confirmEmailSpam')}</p>
        <div className="remote-actions">
          <button className="remote-button remote-button--primary" disabled={busy} onClick={() => void act(async () => { await client.signIn() })}>{t('remote.confirmEmailContinue')}</button>
          {switchAccount}
        </div>
      </section>}
      {ready && client && !session && !confirmEmail && <><div className="remote-actions">
        <button className="remote-button remote-button--primary" disabled={busy} onClick={() => void act(async () => { await client.signIn() })}>{t('remote.signIn')}</button>
        {/* A first-time operator should not have to find a sign-up link on somebody else's login
            form. This is the same flow, opened on the create-account screen instead. */}
        <button className="remote-button" disabled={busy} onClick={() => void act(async () => { await client.signIn(true) })}>{t('remote.createAccount')}</button>
        {/* Signing in again would only be refused again from the same Auth0 session. */}
        {client.signInRefusal === 'signInRefused' && switchAccount}
      </div>
        {/* After confirming the email, Auth0 shows its own "verified" page and does not send the
            operator back here, so the way back is said before they leave. */}
        <p>{t('remote.signUpHint')}</p></>}
      {session && <>
        {/* The account id is only an instruction while there is something to pair. Once stations
            are approved it is support detail, not a step, and a raw UUID presented as a standing
            instruction reads as something the operator still has to act on. It stays in the DOM
            either way so it can always be quoted when asking for help. */}
        {canPair && session.stations.length < 2
          ? <p>{t('remote.accountMatch')} <code>{session.accountId}</code></p>
          : <details><summary>{t('remote.supportDetails')}</summary>
              <p>{t('remote.accountMatch')} <code>{session.accountId}</code></p></details>}
        {trial?.state === 'none' && <p className="remote-site-status" role="status">{t('remote.trialNotStarted')}</p>}
        {/* The pilot branch must survive: an absent start stays absent and is NEVER computed as
            expiresAt minus fourteen days. Migration 0002 deliberately did not backfill, because an
            invented start cannot afterwards be told from a real one. */}
        {trial?.state === 'active' && <p className="remote-site-status remote-site-status--active" role="status">{trial.startedAt === null
          ? t('remote.trialUnknownStart', { until: utcDate(trial.expiresAt) })
          : t('remote.trialRunning', { days: daysLeft, from: utcDate(trial.startedAt), until: utcDate(trial.expiresAt) })}</p>}
        {trial?.state === 'ended' && <p className="remote-site-status remote-site-status--stopped" role="status">{t('remote.trialEnded', { until: utcDate(trial.expiresAt) })}</p>}
        {trial?.state === 'disabled' && <p className="remote-site-status remote-site-status--stopped" role="status">{t('remote.trialDisabled')}</p>}
        {(trial?.state === 'ended' || trial?.state === 'disabled') &&
          <p><a href={BETA_CHANNEL} target="_blank" rel="noopener noreferrer">{t('remote.askAboutAccess')}</a></p>}
        {session.stations.map(station => <section className="rm-card remote-section" key={station.id}>
          {/* A station is named at the shack while pairing, and that name used to be permanent:
              a typo meant revoking and re-pairing to fix. Renaming is housekeeping on your own
              account, so it stays available even when the trial has lapsed. */}
          {renaming?.id === station.id
            ? <form onSubmit={event => { event.preventDefault(); const name = renaming.name.trim(); if (!name) return
                void act(async () => { await client?.post(`stations/${station.id}/rename`, { name }); setRenaming(null); await refresh() }) }}>
                <label>{t('remote.stationName')}<input autoFocus value={renaming.name} maxLength={48}
                  onChange={event => setRenaming({ id: station.id, name: event.target.value })} /></label>
                <div className="remote-actions">
                  <button className="remote-button remote-button--primary" disabled={busy || !renaming.name.trim()}>{t('remote.saveName')}</button>
                  <button type="button" className="remote-button" disabled={busy} onClick={() => setRenaming(null)}>{t('remote.cancelRename')}</button>
                </div>
              </form>
            : <div className="remote-actions">
                <h2>{station.name}</h2>
                <button type="button" className="remote-button remote-button--quiet" disabled={busy}
                  onClick={() => setRenaming({ id: station.id, name: station.name })}>{t('remote.renameStation')}</button>
              </div>}
          <BetaNote />
          {/* ONE sentence for where this browser stands with this station, then the one next step
              (the operator, 2026-10-02: "Each station card has one big "Stream" button and a small
              "Listen" for audio only"). Not asked yet, Stream and Listen ask: the shack pops the
              question up with this browser's name and key. Asked, the next step is at the shack. */}
          <p className="remote-station-state" role="status">{!entitled ? trialReason
            : station.device?.approved === 1 ? t('remote.card.ready')
            : station.device ? keys[station.id]
              ? t('remote.card.waiting', { browser: station.device.name, key: shortFingerprint(keys[station.id]) })
              : t('remote.card.waitingNoKey', { browser: station.device.name })
            : t('remote.card.notApproved')}</p>
          {/* S3-L1: the service lists another key for this station than the one this browser kept. Both
              are shown, to compare with Nexus at the shack, and only Accept takes the new one. */}
          {keyChanged(station.id) && <>
            <p className="rm-warning" role="alert">{t('remote.stationKey.changed')}</p>
            <p>{t('remote.stationKey.kept', { key: shortFingerprint(stationKeys[station.id].keptPrint) })}</p>
            <p>{t('remote.stationKey.new', { key: shortFingerprint(stationKeys[station.id].print) })}</p>
            <div className="remote-actions">
              <button type="button" className="remote-button" disabled={busy} onClick={() => acceptKey(station.id, stationKeys[station.id])}>{t('remote.stationKey.accept')}</button>
            </div>
          </>}
          {station.device?.approved === 1 ? <>
            {stationActions(() => open(station.id, false, station.name), () => open(station.id, false, null, station.name), keyChanged(station.id))}
            {keys[station.id] && <p>{t('remote.thisBrowserKey', { key: shortFingerprint(keys[station.id]) })}</p>}
            {stationKeys[station.id] && !keyChanged(station.id) && <p>{t('remote.thisStationKey', { key: shortFingerprint(stationKeys[station.id].print) })}</p>}
            {/* How long this browser stays approved, off the service's clock. The warning is for the
                end that using it cannot move: before that, opening the station keeps it approved. */}
            {station.device.expires_at !== undefined && <p>{station.device.renewsUntil
              ? t('remote.thisBrowserRenewsUntil', { until: utcDate(station.device.expires_at), limit: utcDate(station.device.renewsUntil) })
              : t('remote.thisBrowserApprovedUntil', { until: utcDate(station.device.expires_at) })}</p>}
            {station.device.expires_at !== undefined && (station.device.renewsUntil ?? station.device.expires_at) - session.serverNow <= APPROVAL_WARNING_MS &&
              <p className="rm-warning">{t('remote.thisBrowserApprovalEnding', { until: utcDate(station.device.renewsUntil ?? station.device.expires_at) })}</p>}
            {/* The old watch/control workspace and the observer, hidden unless the flag is on. */}
            {workspaceShown() && <div className="remote-actions">
              <button className="remote-button" disabled={busy || !entitled} onClick={() => open(station.id, true)}>{t('remote.openNexus')}</button>
              <button className="remote-button" disabled={busy || !entitled} onClick={() => open(station.id, false)}>{t('remote.observe')}</button>
            </div>}
            <div className="remote-actions">
              <button className="remote-button remote-button--quiet" disabled={busy} onClick={() => void act(async () => {
                // S3-L1: the station's key kept here goes with this browser's approval, and is kept
                // again when this browser asks again.
                await client?.post(`stations/${station.id}/forget-device`); await forgetStationKey(station.id).catch(() => {}); await refresh()
              })}>{t('remote.forgetBrowser')}</button>
            </div>
          </> : station.device ? <p role="note">{t('remote.card.browserCode', { code: station.device.id.slice(-6) })}</p> : <>
            {stationActions(() => ask(station.id), () => ask(station.id))}
            {/* With a second station, a browser approved for the first looks like it should already work here. */}
            {session.stations.length > 1 && <p>{t('remote.browserPerStation')}</p>}
          </>}
          <details><summary>{t('remote.stationAccess')}</summary><p>{t('remote.revokeHint')}</p>
            <button className="remote-button" disabled={busy} onClick={() => void act(async () => {
              await client?.post(`stations/${station.id}/revoke`); await forgetStationKey(station.id).catch(() => {}); await refresh()
            })}>{t('remote.revokeStation')}</button>
          </details>
        </section>)}
        {/* Signing out does not remove a browser's approval (it is per station and account), so on a
            shared computer the real way out is the remove button - say so where it is. */}
        {session.stations.some(station => station.device?.approved === 1) && <p>{t('remote.sharedComputerHint')}</p>}
        {/* Waiting for the shack, as its own card rather than a line under a form that is not
            being shown. The server remembers this now, so a reload no longer drops the operator
            back to an empty pairing form - which used to look like the claim had failed, and led
            to re-typing a code that is then refused because the claim already consumed it. */}
        {session.pending && <section className="rm-card remote-section remote-site-card--action">
          <h2>{t('remote.pairStation')}</h2>
          {/* A code can be typed because the operator's own Nexus printed it, or because somebody
              sent it to them. The service cannot tell those apart, and the name on the station is
              chosen by whoever created the code - so it is no evidence either. What the operator
              CAN be asked is whether they meant to take this on at all, before anything permanent
              exists: the station, its credential, and a trial clock that never returns to 'none'. */}
          {session.pending.confirmed
            ? <p role="status">{t('remote.approveInShackNamed', {
                station: session.pending.name,
                minutes: Math.max(0, Math.ceil((session.pending.expiresAt - session.serverNow) / 60000)),
              })}</p>
            : <>
                <p role="status">{t('remote.confirmAttachPrompt', { station: session.pending.name })}</p>
                <p className="remote-warning">{t('remote.confirmAttachWarning')}</p>
                {/* Through act(), like every other service call here: outside it a refused attach
                    showed nothing at all and the rejection went unhandled. */}
                <button type="button" className="remote-button remote-button--primary" disabled={busy} onClick={() => void act(async () => {
                  // A5: the browser this confirm approves brings its key; the station is the enrollment.
                  const key = await deviceKey(session.pending!.id)
                  await client?.post('pair/confirm', { id: session.pending!.id, ...(key ? { publicKey: key.publicKey } : {}) }); await refresh()
                })}>{t('remote.confirmAttach')}</button>
              </>}
          <p>{t('remote.accountMatch')} <code>{session.accountId}</code></p>
        </section>}
        {canPair && !session.pending && session.stations.length < 2 && <section className="rm-card remote-section remote-site-card--action">
          <h2>{t('remote.pairStation')}</h2><p>{t('remote.enterCodeHint')}</p>
          {/* Guarded on the SUBMIT, not only the button: Enter in the field submits a form whose
              button is disabled, which would spend a rate-limited attempt on a typo anyway. */}
          <form onSubmit={event => { event.preventDefault(); if (!pairingCodeReady) return; void act(async () => {
            await client?.post('pair/claim', { code: pairingCode }); setCode(''); await refresh()
          }) }}>
            <label>{t('remote.pairingCode')}<input className="remote-site-code-input" autoComplete="off" spellCheck={false} value={code} maxLength={24} required onChange={event => setCode(event.target.value)} /></label>
            <button className="remote-button remote-button--primary" disabled={busy || !pairingCodeReady}>{t('remote.claimStation')}</button>
          {pairingCodeMalformed && <p role="status">{t('remote.pairingCodeMalformed')}</p>}
          </form>
        </section>}
        {/* At the limit the pairing card used to vanish with no word about why. */}
        {canPair && !session.pending && session.stations.length >= 2 && <p role="status">{t('remote.stationLimitReached')}</p>}
      </>}
    </div>
    <footer className="remote-site-footer"><Wordmark /><a href="/remote-licenses.txt">{t('remote.licenses')}</a></footer>
    </main>
  </div>
}
