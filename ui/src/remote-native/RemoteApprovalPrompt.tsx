import { useEffect, useRef, useState } from 'react'
import { getRemoteStationStatus, remoteStationAction } from '../api'
import { Dialog } from '../components/ui/Dialog'
import { t } from '../i18n'
import { shortFingerprint } from '../remote-web/stream-protocol'
import { isNative, offersAllowTransmit, PLATFORM_NAMES, waitsForApprove, type RemoteStationStatus } from './types'
import { isStreamInput } from './stream-input'

/** How often the shack looks for a browser asking to stream, while Remote is on. Each look is the
 *  Refresh the Remote access panel makes (one request to the service); with Remote off, none is made. */
export const APPROVAL_POLL_MS = 10_000
/** Approve is not live until the question has been on screen this long, so a click or key meant for
 *  whatever was under the pointer when it appeared cannot answer it. */
export const APPROVAL_ARM_MS = 1_000

type Device = RemoteStationStatus['devices'][number]
/** A device the shack asks about, and whether the question is Allow to transmit for a held one. */
type Asked = { device: Device; held: boolean }
/** The browsers asking to stream: a request waiting here, or a browser approved before its key was
 *  pinned (it is refused a stream until approved again). Only with a key: the key is what the
 *  operator compares, and a browser with none could not stream after approval anyway. A computer
 *  running Nexus (the native client) asks the same way to connect, and its key is what the tunnel
 *  through the room is held to once it is pinned. */
function asking(status: RemoteStationStatus | null, now: number): Asked[] {
  if (!status?.stationId) return []
  const pinned = status.pinnedDevices
  const approve = status.devices.filter(device => !!device.key && device.expiresAt > now
    && (device.approved !== 1 || (!!pinned && !pinned.includes(device.id))))
  // Then a device added by its own sign-in that the station holds from transmit: exactly where the card offers
  // Allow to transmit, which is never a device asked about above.
  const held = status.devices.filter(device => !!device.key && device.expiresAt > now && offersAllowTransmit(status, device))
  return [...approve.map(device => ({ device, held: false })), ...held.map(device => ({ device, held: true }))]
}
const question = ({ device, held }: Asked) => `${held ? 'held:' : ''}${device.id}:${device.key}`

/** The question Nexus asks at the shack when a browser asks to stream (the operator, 2026-10-02: "Nexus
 *  at the shack pops up "Let Chrome on Windows stream this station? Key XXXX XXXX" with Approve / Deny
 *  ... No hunting through Settings."). The browser's name and the key it shows on the Remote page.
 *
 *  Approve is exactly Settings ▸ Remote access's approve: station controls, and the key
 *  shown here pinned (the station pins it only if the service still lists this same key). Station
 *  controls are all a stream needs, and a streaming browser is the operator at the shack, transmit
 *  included (the operator, 2026-10-03), so the question says so. It never approves by default: Deny holds the keyboard when it
 *  opens, so Enter alone denies; Escape or a click outside closes it and answers nothing; and Approve
 *  takes no click for its first second. Deny refuses a waiting request; for a browser approved before,
 *  it leaves its approval exactly as it is (it still cannot stream). Either way it is not asked again
 *  this session for the same browser and key.
 *
 *  A computer running Nexus is asked about the same way, by its name, its platform and its key in
 *  eight groups, and its Approve is the same press: its key pinned, for the tunnel through the room to
 *  check. Approving it grants nothing on the relay's lanes:
 *  what the computer may do once connected is the station's own rules for a computer driving it.
 *
 *  A device added by its own sign-in is held from transmit until the operator allows it here, and the
 *  shack asks about it the same way: "Allow DESKTOP-7Q to transmit?", with its key. Allow to transmit is
 *  the card's own button, the same shack-only action (`remote_station_action` is refused from afar), under
 *  the same name. Not now, Escape or a click outside send nothing, so the device stays held, and the
 *  card still offers Allow to transmit. The same guards hold: Not now has the keyboard, and Allow takes
 *  no click for its first second. */
export function RemoteApprovalPrompt() {
  const [status, setStatus] = useState<RemoteStationStatus | null>(null)
  const [answered, setAnswered] = useState<ReadonlySet<string>>(() => new Set())
  const [busy, setBusy] = useState(false)
  const [armed, setArmed] = useState(false)
  const [failed, setFailed] = useState(false)
  const [refused, setRefused] = useState(false)
  const mounted = useRef(true)
  useEffect(() => {
    mounted.current = true
    let pending = false
    const look = async () => {
      if (pending) return
      pending = true
      try {
        // A local read first: only a station with Remote on asks the service anything.
        const local = await getRemoteStationStatus()
        const on = ['connected', 'connecting', 'reconnecting'].includes(local.phase)
        const next = on ? await remoteStationAction({ type: 'refresh' }) : local
        if (mounted.current) setStatus(next)
      } catch { /* Settings says why; this only asks when it can see a request */ }
      finally { pending = false }
    }
    void look()
    const timer = setInterval(() => void look(), APPROVAL_POLL_MS)
    return () => { mounted.current = false; clearInterval(timer) }
  }, [])
  const asked = asking(status, Date.now()).find(candidate => !answered.has(question(candidate))) ?? null
  const device = asked?.device ?? null, held = !!asked?.held
  const current = asked ? question(asked) : null
  useEffect(() => {
    setArmed(false); setFailed(false); setRefused(false)
    if (!current) return
    const timer = setTimeout(() => setArmed(true), APPROVAL_ARM_MS)
    return () => clearTimeout(timer)
  }, [current])

  const settle = (key: string) => setAnswered(previous => new Set(previous).add(key))
  async function answer(approve: boolean) {
    if (!asked || !device || busy) return
    const key = question(asked)
    // Not now leaves a held device held: nothing is sent.
    if (!approve && held) { settle(key); return }
    // A browser approved before keeps its approval on a Deny; only a waiting request is refused.
    if (!approve && device.approved === 1) { settle(key); return }
    // Only at the shack: an answer that sends something is refused through a stream, so a device driving this
    // screen never approves or allows itself (the station refuses it too). The question stays.
    if (isStreamInput()) { setRefused(true); return }
    setBusy(true); setFailed(false); setRefused(false)
    try {
      const next = await remoteStationAction(held ? { type: 'allowTransmit', deviceId: device.id } : approve
        ? { type: 'device', deviceId: device.id, approve: true, key: device.key ?? undefined, kind: device.kind ?? 'browser' }
        : { type: 'device', deviceId: device.id, approve: false })
      if (!mounted.current) return
      setStatus(next); settle(key)
    } catch { if (mounted.current) setFailed(true) }
    finally { if (mounted.current) setBusy(false) }
  }

  const fingerprint = device?.key ? shortFingerprint(device.key) : ''
  const platform = device?.platform ? PLATFORM_NAMES[device.platform] : ''
  // Approved before, here: never a device added by its sign-in that waits for its first Approve here.
  const again = !!device && device.approved === 1 && !waitsForApprove(status, device.id)
  return <Dialog
    open={!!device}
    // Escape and a click outside: closed, and nothing answered. The request stays in Settings.
    onOpenChange={open => { if (!open && current) settle(current) }}
    title={!device ? '' : held ? t('remote.approval.heldTitle', { device: device.name }) : isNative(device) ? t('remote.approval.computerTitle', { computer: device.name })
      : t('remote.approval.title', { browser: device.name })}
    description={!device ? undefined : held ? isNative(device)
      ? t('remote.approval.heldComputerBody', { platform, key: fingerprint })
      : t('remote.approval.heldBody', { key: fingerprint })
      : isNative(device) ? again
      ? t('remote.approval.computerBodyAgain', { platform, key: fingerprint })
      : t('remote.approval.computerBody', { platform, key: fingerprint })
      : again
        ? t('remote.approval.bodyAgain', { key: fingerprint })
        : t('remote.approval.body', { key: fingerprint })}
    className="remote-approval"
  >
    {/* The key again, large, for comparing group by group with the browser's screen. */}
    <p className="remote-approval-key"><code>{fingerprint}</code></p>
    {/* A computer compares this station's key too, in the same call: its window shows the key it was
        given for this station, and holds the internet road until the operator says they match. */}
    {device && !held && isNative(device) && status?.stationKey
      && <p>{t('remote.thisStationKey', { key: shortFingerprint(status.stationKey) })}</p>}
    {failed && <p role="alert">{t('remote.requestFailed')}</p>}
    {refused && <p role="alert">{t('remote.lan.atShackOnly')}</p>}
    <div className="confirm-actions">
      <button type="button" className="settings-refresh" autoFocus disabled={busy} onClick={() => void answer(false)}>{held ? t('remote.approval.notNow') : t('remote.approval.deny')}</button>
      <button type="button" className="settings-save" disabled={busy || !armed} onClick={() => void answer(true)}>{held ? t('remote.account.allowTransmit') : t('remote.approve')}</button>
    </div>
  </Dialog>
}
