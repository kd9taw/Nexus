import { useEffect, useRef, useState } from 'react'
import { getRemoteStationStatus, remoteStationAction } from '../api'
import { Dialog } from '../components/ui/Dialog'
import { t } from '../i18n'
import { shortFingerprint } from '../remote-web/stream-protocol'
import type { RemoteStationStatus } from './types'

/** How often the shack looks for a browser asking to stream, while Remote is on. Each look is the
 *  Refresh the Remote access panel makes (one request to the service); with Remote off, none is made. */
export const APPROVAL_POLL_MS = 10_000
/** Approve is not live until the question has been on screen this long, so a click or key meant for
 *  whatever was under the pointer when it appeared cannot answer it. */
export const APPROVAL_ARM_MS = 1_000

type Device = RemoteStationStatus['devices'][number]
/** The browsers asking to stream: a request waiting here, or a browser approved before its key was
 *  pinned (it is refused a stream until approved again). Only with a key: the key is what the
 *  operator compares, and a browser with none could not stream after approval anyway. */
function asking(status: RemoteStationStatus | null, now: number): Device[] {
  if (!status?.stationId) return []
  const pinned = status.pinnedDevices
  return status.devices.filter(device => !!device.key && device.expiresAt > now
    && (device.approved !== 1 || (!!pinned && !pinned.includes(device.id))))
}
const question = (device: Device) => `${device.id}:${device.key}`

/** The question Nexus asks at the shack when a browser asks to stream (the operator, 2026-10-02: "Nexus
 *  at the shack pops up "Let Chrome on Windows stream this station? Key XXXX XXXX" with Approve / Deny
 *  ... No hunting through Settings."). The browser's name and the key it shows on the Remote page.
 *
 *  Approve is exactly Settings ▸ Remote access's approve with its transmit tick off: station controls
 *  and logging, no FT8/FT4 transmit, and the key shown here pinned (the station pins it only if the
 *  service still lists this same key). It never approves by default: Deny holds the keyboard when it
 *  opens, so Enter alone denies; Escape or a click outside closes it and answers nothing; and Approve
 *  takes no click for its first second. Deny refuses a waiting request; for a browser approved before,
 *  it leaves its approval exactly as it is (it still cannot stream). Either way it is not asked again
 *  this session for the same browser and key. */
export function RemoteApprovalPrompt() {
  const [status, setStatus] = useState<RemoteStationStatus | null>(null)
  const [answered, setAnswered] = useState<ReadonlySet<string>>(() => new Set())
  const [busy, setBusy] = useState(false)
  const [armed, setArmed] = useState(false)
  const [failed, setFailed] = useState(false)
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
  const device = asking(status, Date.now()).find(candidate => !answered.has(question(candidate))) ?? null
  const current = device ? question(device) : null
  useEffect(() => {
    setArmed(false); setFailed(false)
    if (!current) return
    const timer = setTimeout(() => setArmed(true), APPROVAL_ARM_MS)
    return () => clearTimeout(timer)
  }, [current])

  const settle = (key: string) => setAnswered(previous => new Set(previous).add(key))
  async function answer(approve: boolean) {
    if (!device || busy) return
    const key = question(device)
    // A browser approved before keeps its approval on a Deny; only a waiting request is refused.
    if (!approve && device.approved === 1) { settle(key); return }
    setBusy(true); setFailed(false)
    try {
      const next = await remoteStationAction(approve
        ? { type: 'device', deviceId: device.id, approve: true, transmit: false, key: device.key ?? undefined }
        : { type: 'device', deviceId: device.id, approve: false })
      if (!mounted.current) return
      setStatus(next); settle(key)
    } catch { if (mounted.current) setFailed(true) }
    finally { if (mounted.current) setBusy(false) }
  }

  const fingerprint = device?.key ? shortFingerprint(device.key) : ''
  return <Dialog
    open={!!device}
    // Escape and a click outside: closed, and nothing answered. The request stays in Settings.
    onOpenChange={open => { if (!open && current) settle(current) }}
    title={device ? t('remote.approval.title', { browser: device.name }) : ''}
    description={device ? device.approved === 1
      ? t('remote.approval.bodyAgain', { key: fingerprint })
      : t('remote.approval.body', { key: fingerprint }) : undefined}
    className="remote-approval"
  >
    {/* The key again, large, for comparing group by group with the browser's screen. */}
    <p className="remote-approval-key"><code>{fingerprint}</code></p>
    {failed && <p role="alert">{t('remote.requestFailed')}</p>}
    <div className="confirm-actions">
      <button type="button" className="settings-refresh" autoFocus disabled={busy} onClick={() => void answer(false)}>{t('remote.approval.deny')}</button>
      <button type="button" className="settings-save" disabled={busy || !armed} onClick={() => void answer(true)}>{t('remote.approve')}</button>
    </div>
  </Dialog>
}
