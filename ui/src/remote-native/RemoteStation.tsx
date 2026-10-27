import { useEffect, useRef, useState } from 'react'
import { answerRemoteAutostartOffer, getRemoteStationStatus, getSettings, remoteStationAction, setLaunchAtLogin } from '../api'
import { t } from '../i18n'
import { PLATFORM_NAMES, type AccountRole, type RemoteStationAction, type RemoteStationStatus } from './types'
import { shortFingerprint } from '../remote-web/stream-protocol'
import { DevicesList } from './DevicesList'
import { lanErrorLine } from './LanStation'
import { isStreamInput } from './stream-input'
import '../remote-web/remote.css'

/** The presses the Devices list makes: a refusal of one is said beside it. */
const DEVICES_PRESSES: ReadonlySet<RemoteStationAction['type']> = new Set(['device', 'allowTransmit', 'stationPermission', 'uninvite', 'lanRevoke'])

export function RemoteStation() {
  const [status, setStatus] = useState<RemoteStationStatus | null>(null)
  const [name, setName] = useState('')
  const [error, setError] = useState<string | null>(null)
  // A press on the Devices list refused through a stream, and a Remove of a computer known only to this
  // network refused by the network, as the network card said them.
  const [lanError, setLanError] = useState<string | null>(null)
  const [refused, setRefused] = useState(false)
  const [busy, setBusy] = useState(false)
  const [offer, setOffer] = useState(false)
  const [offerFailed, setOfferFailed] = useState(false)
  // People: the address and role of the next invitation.
  const [inviteEmail, setInviteEmail] = useState('')
  const [inviteRole, setInviteRole] = useState<AccountRole>('operate')
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
  // The sign-in / pairing buttons are dead until a station name is typed, and the field starts
  // empty — so on arrival there is nothing to do and no hint that there should be. Prefill the
  // field with the operator's callsign (the first-hour setting every station already has) so the
  // buttons are live at once; the operator can still edit it, and if there is no callsign the
  // inline hint in the unpaired block asks for a name. Done once, and only while the field is
  // still untouched, so it never overwrites an edit.
  const prefilled = useRef(false)
  useEffect(() => {
    void (async () => {
      try {
        const settings = await getSettings()
        if (!mounted.current || prefilled.current) return
        prefilled.current = true
        const call = settings.mycall?.trim()
        if (call) setName(prev => (prev === '' ? call : prev))
      } catch { /* no settings: the hint covers an empty field */ }
    })()
  }, [])
  async function act(action: RemoteStationAction) {
    // Only at the shack: nothing but a read is pressed through a stream, so a device driving this screen can never
    // approve, allow or remove itself or anyone else (the station refuses it too). Read before anything awaits,
    // while the stream's dispatch is still the one running; said beside the Devices list for its own buttons.
    if (action.type !== 'refresh' && isStreamInput()) {
      if (DEVICES_PRESSES.has(action.type)) setRefused(true)
      else setError('atShackOnly')
      return
    }
    // Disable cancels the native socket even while an HTTP request is pending; a Remove on this network's
    // list never waited for one.
    if (pending.current && action.type !== 'disable' && action.type !== 'takeOverLogging' && action.type !== 'lanRevoke') return
    const epoch = ++requestEpoch.current
    pending.current = true; setBusy(true); setError(null); setLanError(null); setRefused(false)
    try {
      const next = await remoteStationAction(action)
      if (mounted.current && epoch === requestEpoch.current) setStatus(next)
      // Approving the pairing turns Remote on too, so it is the same moment to offer.
      if (action.type === 'enable' || (action.type === 'approve' && ['connected', 'connecting', 'reconnecting'].includes(next.phase))) void offerStartAtSignIn()
    }
    catch (error) {
      if (mounted.current && epoch === requestEpoch.current) {
        if (action.type === 'lanRevoke') setLanError(typeof error === 'string' ? error : 'unavailable')
        else setError(typeof error === 'string' ? error : 'serviceUnavailable')
      }
    }
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
  const account = status?.account
  const roleName = (role: AccountRole) => role === 'listen' ? t('remote.account.role.listen') : t('remote.account.role.operate')
  // One card (the operator, 2026-10-02: "Settings ▸ Remote access becomes one card"): where this
  // station stands with the Remote service and the one thing to do next, then its devices by person
  // (`DevicesList`). Everything else the panel had folds under Advanced, unchanged: the same state, the
  // same actions, the same defaults. Nothing here grants anything the old panel did not.
  return <div className="remote-section remote-native">
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
      // One trial per callsign and per shack PC: approval refused for the callsign this Nexus sent, or
      // for this PC, each with what the operator can do about it here. And a service that needs a newer
      // Nexus to approve at all.
      : issue === 'callsignInvalid' ? t('remote.nativeCallsignInvalid')
      : issue === 'callsignNotLicensed' ? t('remote.nativeCallsignNotLicensed')
      : issue === 'trialUsedByCallsign' ? t('remote.nativeTrialUsedByCallsign')
      : issue === 'trialUsedOnThisPc' ? t('remote.nativeTrialUsedOnThisPc')
      : issue === 'updateRequired' ? t('remote.nativeUpdateRequired')
      // Approval refused because this mailbox already has a trial under another sign-in. It used to
      // read as access denied, which does not say that the sign-in is what needs changing.
      : issue === 'trialActiveElsewhere' ? t('remote.nativeTrialActiveElsewhere')
      // Approving at the shack before the browser agreed to attach the station. Falling through to
      // requestFailed would point an operator standing at the radio at their network.
      : issue === 'awaitingConfirmation' ? t('remote.nativeAwaitingConfirmation')
      // Signing in at the shack, and inviting people.
      : issue === 'accountNotReady' ? t('remote.account.notReady')
      : issue === 'signInPortBusy' ? t('remote.account.signInPortBusy')
      : issue === 'signInFailed' || issue === 'proofInvalid' || issue.startsWith('token') ? t('remote.account.signInFailed')
      : issue === 'proofNotYours' ? t('remote.account.notOwner')
      : issue === 'stationLimit' ? t('remote.account.stationLimit')
      : issue === 'inviteLimit' ? t('remote.account.inviteLimit')
      : issue === 'noKey' ? t('remote.vaultFailed')
      : issue === 'notSignInDevice' ? t('remote.account.allowTransmitGone')
      // Removed on every road the station could link it to; the other list may still hold it.
      : issue === 'removeElsewhere' ? t('remote.removeElsewhere')
      : issue === 'atShackOnly' ? t('remote.lan.atShackOnly')
      : t('remote.requestFailed')}</p>}
    {/* S3-M1: browsers check this station's stream against the key the service recorded for it at
        pairing. Another one there is not fixed by waiting, so it gets its own line until it is. */}
    {status?.keyRefused && <p role="alert">{t('remote.nativeStationKeyPinned')}</p>}
    {/* The notice: a device the station granted on its own proof, until the operator has seen it. Inline,
        never a modal: nothing here may stand between the operator and Stop TX. */}
    {account?.added?.map(added => { const what = added.kind === 'browser' ? t('remote.account.aBrowser')
      : t('remote.native.computerPlatform', { platform: added.platform ? PLATFORM_NAMES[added.platform] : '—' })
    const key = added.key ? shortFingerprint(added.key) : '—'
    return <div key={added.deviceId} role="status" className="remote-native-browser">
      <span>{added.guest ? t('remote.account.addedGuest', { name: added.name, what, key, role: roleName(added.role) })
        : t('remote.account.added', { name: added.name, what, key })}</span>
      <span className="remote-actions">
        <button type="button" className="remote-button" disabled={busy}
          onClick={() => void act({ type: 'device', deviceId: added.deviceId, approve: false })}>{t('remote.native.remove')}</button>
        <button type="button" className="remote-button" disabled={busy}
          onClick={() => void act({ type: 'dismissAdded', deviceId: added.deviceId })}>{t('remote.account.gotIt')}</button>
      </span>
    </div> })}
    {account?.reproof && <p role="alert">{t('remote.account.reproof')}</p>}
    {account?.removalsUnreadable && <p role="alert">{t('remote.account.removalsUnreadable')}</p>}
    {/* Signing in again, for a station bound already: the same sign-in, the station otherwise as it was. */}
    {phase !== 'unpaired' && (account?.signingIn ? <>
      <p role="status">{t('remote.account.signingIn')}</p>
      <button type="button" className="remote-button" onClick={() => void act({ type: 'cancelSignInAgain' })}>{t('remote.account.cancelSignIn')}</button>
    </> : account?.reproof && <div className="remote-actions">
      <button type="button" className="remote-button" disabled={busy} onClick={() => void act({ type: 'signInAgain' })}>{t('remote.account.signInAgain')}</button>
    </div>)}
    {/* Linking the station to the account IS the sign-in, so its steps stay in the card while it is
        not linked. */}
    {phase === 'unpaired' && <>
      <label>{t('remote.stationName')}<input value={name} maxLength={48} onChange={event => setName(event.target.value)} /></label>
      {/* The buttons below need a name; prefill usually supplies one, but when the station has no
          callsign yet the field is empty, so say what to do rather than leave a dead button. */}
      {!name.trim() && <p className="settings-hint">{t('remote.stationNameHint')}</p>}
      {/* Signing in binds the station with no code; pairing with a code stays beside it. */}
      {account?.signingIn ? <>
        <p role="status">{t('remote.account.signingIn')}</p>
        <button type="button" className="remote-button" onClick={() => void act({ type: 'cancel' })}>{t('remote.account.cancelSignIn')}</button>
      </> : <>
        <p>{t('remote.account.signInHint')}</p>
        <div className="remote-actions">
          <button type="button" className="remote-button" disabled={busy || !name.trim()} onClick={() => void act({ type: 'signIn', name })}>{t('remote.account.signIn')}</button>
          <button type="button" className="remote-button" disabled={busy || !name.trim()} onClick={() => void act({ type: 'begin', name })}>{t('remote.pairStation')}</button>
        </div>
      </>}
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
      <div className="remote-actions">
        <button type="button" className="remote-button" disabled={!connected && busy} onClick={() => void act({ type: connected ? 'disable' : 'enable' })}>
          {connected ? t('remote.disable') : t('remote.enable')}</button>
      </div>
      {offer && <div role="group" aria-label={t('settings.launchAtLogin.legend')}>
        <p>{t('remote.autostartOffer')}</p>
        <div className="remote-actions">
          <button type="button" className="remote-button" onClick={() => void answerOffer(true)}>{t('remote.autostartOfferAccept')}</button>
          <button type="button" className="remote-button" onClick={() => void answerOffer(false)}>{t('remote.autostartOfferDismiss')}</button>
        </div>
      </div>}
      {offerFailed && <p role="alert">{t('remote.autostartOfferFailed')}</p>}
      {/* S3-L1: this station's own key, as the Remote page shows the key it kept for the station: the
          operator compares the two, group by group. */}
      {status.stationKey && <p>{t('remote.thisStationKey', { key: shortFingerprint(status.stationKey) })}</p>}
    </>}
    {/* Devices (the operator's ruling of 2026-10-25): one list, by person, of every computer, phone and browser
        that reaches this station, on any road; this network's paired computers with them, so it is shown for a
        station on this network alone too. */}
    {(status?.stationId || (status?.lan?.devices.length ?? 0) > 0) && <>
      {refused && <p role="alert">{t('remote.lan.atShackOnly')}</p>}
      {lanError && <p role="alert">{lanErrorLine(lanError)}</p>}
      <DevicesList status={status} busy={busy} act={action => void act(action)} roleName={roleName}
        removeLan={deviceId => void act({ type: 'lanRevoke', deviceId })} />
    </>}
    {status?.stationId && <>
      {/* People: invited by the address they sign in with, to operate or only to listen. Their devices are
          added when they sign in, and listed under them above; the station checks each one's proof against
          this list itself. */}
      <h3>{t('remote.account.people')}</h3>
      <p>{t('remote.account.peopleHint')}</p>
      {/* An invitation taken back takes off the LAN every guest's computer the station cannot tell whose it is:
          said by name, so their owners know to sign in on them again. */}
      {(account?.alsoUninvited?.length ?? 0) > 0
        && <p role="status">{t('remote.account.alsoUninvited', { names: account!.alsoUninvited!.join(', ') })}</p>}
      <label>{t('remote.account.email')}<input type="email" value={inviteEmail} maxLength={254} onChange={event => setInviteEmail(event.target.value)} /></label>
      <label>{t('remote.account.role')}<select value={inviteRole} onChange={event => setInviteRole(event.target.value === 'listen' ? 'listen' : 'operate')}>
        <option value="operate">{t('remote.account.role.operate')}</option>
        <option value="listen">{t('remote.account.role.listen')}</option>
      </select></label>
      {/* Someone invited to operate holds what the owner's own other computer holds: said where Operate is offered,
          so the owner invites to operate only someone they would hand the shack to. */}
      {inviteRole === 'operate' && <p className="remote-warning">{t('remote.account.operateIsYours')}</p>}
      <div className="remote-actions">
        <button type="button" className="remote-button" disabled={busy || !inviteEmail.trim()}
          onClick={() => void act({ type: 'invite', email: inviteEmail.trim(), role: inviteRole })}>{t('remote.account.invite')}</button>
      </div>
    </>}
    {/* Advanced: the old panel's options, kept as they were (the operator, 2026-10-02: "The old
        pairing and permission options fold under "Advanced" (kept, not deleted)"). */}
    <details className="remote-native-advanced"><summary>{t('remote.native.advanced')}</summary>
      <p>{t('remote.nativeIntro')}</p>
      {status?.stationId && <>
        <p>{t('remote.restartHint')}</p>
        <div className="remote-actions">
          <button type="button" className="remote-button" disabled={busy} onClick={() => void act({ type: 'refresh' })}>{t('remote.refreshDevices')}</button>
        </div>
        {status.stationPermissions && <><p>{t('remote.controlLocalHint')}</p><button type="button" className="remote-button" onClick={()=>void act({type:'takeOverLogging'})}>{t('remote.controlTakeOver')}</button></>}
        <details><summary>{t('remote.stationAccess')}</summary><p>{t('remote.revokeHint')}</p>
          <button type="button" className="remote-button" disabled={busy} onClick={() => void act({ type: 'forget' })}>{t('remote.revokeStation')}</button>
        </details>
      </>}
    </details>
  </div>
}
