import { useEffect, useRef, useState, useSyncExternalStore, type RefObject } from 'react'
import { t } from '../i18n'
import { useViewport } from '../useViewport'
import { initialState, startMonitor } from '../remote-monitor/session'
import { AudioListen } from './AudioListen'
import type { HostedConnection } from './client'
import { transmitEpoch } from './operation-protocol'
import { ClickCount, HeldInput, keyMessage, pointerMessage, textMessage, wheelMessage, type PictureBox } from './stream-capture'
import type { StopTarget } from './stream-link'
import type { StreamKey, StreamPointer } from './stream-protocol'
import '../remote-monitor/monitor.css'
import './remote.css'
import './stream.css'

// Invariant tokens. "Stop TX" and the TX indicator read the same on every transmit surface in
// Nexus and are English on all of them by decision (the pt-BR kit's README, "English on purpose"),
// so this page says what the picture beside it says.
const STOP_TX = 'Stop TX'
const TX = '▲ TX'
const BRAND = 'Nexus'

/** The stream: Nexus at the shack as a picture, operated with this browser's mouse and keyboard.
 *
 *  Its authority is the chain the rest of Remote uses and nothing new: station control granted at
 *  the radio, a lease this browser acquires, and a stream offered only under that lease. Stop TX is
 *  always on screen while this view is, outside the picture, and reaches the station two ways at
 *  once - on the stream's own control channel and on the observe socket - so a stream that has
 *  frozen or died still leaves a way to unkey the rig. */
export function StreamView({ connection, station, disconnect, signOut }: {
  connection: HostedConnection; station: string; disconnect: () => void; signOut: () => void
}) {
  useViewport(1, true)
  const operations = connection.operations, link = connection.stream
  const ops = useSyncExternalStore(operations.subscribe, operations.getSnapshot)
  const stream = useSyncExternalStore(link.subscribe, link.getSnapshot)
  const [observation, setObservation] = useState(initialState)
  useEffect(() => startMonitor(connection.source, setObservation), [connection.source])
  const [wanted, setWanted] = useState(false)
  const video = useRef<HTMLVideoElement>(null)

  const state = ops.state
  // Only a FRESH controlling state starts anything: the lease it names is the one this session holds now.
  const lease = ops.connected && ops.fresh && state?.phase === 'controlling' ? state.leaseId : null
  const running = stream.phase === 'connecting' || stream.phase === 'live' || stream.phase === 'stalled'
  // Known to be controlling, known not to be, or unknown (a re-read in flight clears the state).
  const controlling = !ops.connected ? false : state ? state.phase === 'controlling' : null
  const keyed = observation.frame?.station.radio.rigKeyed ?? null
  const target: StopTarget | null = state?.phase === 'controlling' && state.leaseId && transmitEpoch(state.transmitEpoch)
    ? { stationBootId: state.stationBootId, leaseId: state.leaseId, transmitEpoch: state.transmitEpoch } : null
  const canStop = !!ops.stopAvailable || (stream.control && !!target)
  const stopProgress = ops.stopSending ? 'sending' : ops.stopAccepted ? keyed === false ? 'stopped' : 'sent' : 'idle'

  useEffect(() => { link.attachVideo(video.current); return () => link.attachVideo(null) }, [link])
  // A3, the page's half: the offer goes only under the lease this session holds right now. The
  // operator's request is CONSUMED by the start, so a stream that ends - the station said so, control
  // went, the link failed - stays ended until they ask again; it is never re-offered on its own.
  useEffect(() => {
    if (wanted && lease && (stream.phase === 'idle' || stream.phase === 'ended')) { setWanted(false); void link.start(lease) }
  }, [wanted, lease, stream.phase, link])
  // Control gone - released, taken over, or the socket lost - ends the stream here at once, rather
  // than on the station's word a moment later.
  useEffect(() => { if (running && controlling === false) link.close('notController') }, [running, controlling, link])
  // Blind means no authority, for a click as much as for PTT: a frozen picture takes no input.
  useInput(video, connection, stream.phase === 'live')

  const start = () => {
    setWanted(true)
    // A refused acquire (someone else took control first) ends the request, so the button is live again.
    if (state?.phase === 'available') void operations.acquire().catch(() => setWanted(false))
  }
  const end = () => { setWanted(false); link.close(); void operations.release() }
  const stopTx = () => {
    link.stopTransmit(target)
    if (ops.stopAvailable) void operations.stopTransmit().catch(() => {})
  }
  const status = statusLine(ops.connected, state?.phase ?? null, stream.phase, stream.reason, wanted)

  return <div className="app remote-monitor-app remote-stream-app" data-stream-phase={stream.phase}>
    <header className="rm-header remote-stream-header">
      <span className="remote-stream-title"><strong>{BRAND}</strong> <span>{station}</span></span>
      {/* While a stream runs this is where its state is said; otherwise the placeholder says it, once. */}
      <span className="remote-stream-status" role="status">{running ? status : null}</span>
      <div className="remote-stream-controls">
        {/* THE STOP LINE. First in the row, so it is on the first line however the row wraps; rendered
            whenever this view is, never behind anything, and enabled whenever either path to the
            station can carry a Stop. */}
        <button type="button" className="remote-button remote-stream-stop" disabled={!canStop} onClick={stopTx}
          title={stopProgress === 'stopped' ? t('remote.stop.stopped.title') : stopProgress === 'sent' ? t('remote.stop.sent.title') : undefined}>{STOP_TX}</button>
        {stopProgress !== 'idle' && <span className="remote-stream-stopstate" role="status">
          {stopProgress === 'stopped' ? t('remote.stop.stopped') : stopProgress === 'sent' ? t('remote.stop.sent') : t('remote.stop.sending')}</span>}
        {keyed === true && <span className="remote-stream-tx" role="status">{TX}</span>}
        {stream.control && <AudioListen audio={link.audio} client={operations} />}
        {stream.control && <button type="button" className="remote-button remote-stream-ptt" aria-pressed={stream.ptt}
          title={t('remote.stream.ptt.title')} disabled={stream.phase !== 'live' || !lease} data-keyed={stream.keyed || undefined}
          onPointerDown={event => {
            if (event.button !== 0) return
            try { event.currentTarget.setPointerCapture(event.pointerId) } catch { /* the hold still starts */ }
            link.holdPtt()
          }}
          onPointerUp={() => link.releasePtt()} onPointerCancel={() => link.releasePtt()}
          onLostPointerCapture={() => link.releasePtt()} onBlur={() => link.releasePtt()}
          onKeyDown={event => { if ((event.key === ' ' || event.key === 'Enter') && !event.repeat) { event.preventDefault(); link.holdPtt() } }}
          onKeyUp={event => { if (event.key === ' ' || event.key === 'Enter') { event.preventDefault(); link.releasePtt() } }}
          onContextMenu={event => event.preventDefault()}>{t('remote.stream.ptt')}</button>}
        {running && <button type="button" className="remote-button" onClick={end}>{t('remote.stream.end')}</button>}
        <button type="button" className="remote-button" onClick={disconnect}>{t('remote.disconnect')}</button>
        <button type="button" className="remote-button remote-button--quiet" onClick={signOut}>{t('remote.signOut')}</button>
      </div>
    </header>
    <main className="remote-stream-stage" aria-label={t('remote.stream.stage')}>
      <video ref={video} className="remote-stream-video" tabIndex={0} muted autoPlay playsInline aria-label={t('remote.stream.picture')} />
      {stream.phase === 'stalled' && <p className="remote-stream-stalled" role="alert">{t('remote.stream.stalled')}</p>}
      {/* The station's own word, on its last heartbeat reply: it is not holding transmit presence for
          this browser, whatever the picture here looks like (its clock says the picture is late). */}
      {stream.phase === 'live' && stream.presence === false && <p className="remote-stream-stalled" role="alert">{t('remote.stream.noPresence')}</p>}
      {stream.phase !== 'live' && stream.phase !== 'stalled' && <div className="remote-stream-placeholder">
        <p role={running ? undefined : 'status'}>{stream.phase === 'connecting' ? t('remote.stream.waitingForPicture') : status}</p>
        {!running && (state?.phase === 'available' || state?.phase === 'controlling') &&
          <button type="button" className="remote-button remote-button--primary" disabled={wanted || ops.busy} onClick={start}>{t('remote.stream.start')}</button>}
      </div>}
    </main>
  </div>
}

/** One sentence for where things stand. Written out rather than looked up in a map: the catalog's
 *  orphan check reads literal t() calls, so a lookup table reads as keys nobody uses. */
function statusLine(connected: boolean, phase: string | null, stream: string, reason: string | null, wanted: boolean): string {
  if (stream === 'live') return t('remote.stream.live')
  if (stream === 'stalled') return t('remote.stream.stalled')
  if (stream === 'connecting') return t('remote.stream.starting')
  if (stream === 'ended') return ended(reason)
  if (!connected) return t('remote.stream.connecting')
  if (phase === null) return t('remote.stream.waiting')
  if (phase === 'localPermissionRequired') return t('remote.stream.permission')
  if (phase === 'occupied') return t('remote.stream.occupied')
  return wanted ? t('remote.stream.starting') : t('remote.stream.ready')
}
function ended(reason: string | null): string {
  return reason === 'notController' ? t('remote.stream.ended.notController')
    : reason === 'streamUnavailable' ? t('remote.stream.ended.unavailable')
    : reason === 'serviceAccessExpired' ? t('remote.stream.ended.accessExpired')
    : reason === 'tryLater' ? t('remote.stream.ended.tryLater')
    : reason === 'streamClosed' ? t('remote.stream.ended.station')
    : reason === 'streamDisabled' ? t('remote.stream.ended.disabled')
    : reason === 'streamInUse' ? t('remote.stream.ended.inUse')
    : reason === 'invalidOffer' ? t('remote.stream.ended.invalidOffer')
    : reason === 'insecureAnswer' ? t('remote.stream.ended.insecure')
    : reason === 'streamUnsupported' ? t('remote.stream.ended.unsupported')
    : reason === 'streamHidden' ? t('remote.stream.ended.hidden')
    : t('remote.stream.ended.failed')
}

/** The input bridge's page half (S11, A2). Listeners go on the PICTURE and nowhere else: a pointer
 *  on the header, a key typed while any other control has focus, never reaches the station. What
 *  was pressed through the picture is released through it when it loses focus or stops being live,
 *  so nothing is left held at the shack by a key-up this page never saw. */
function useInput(video: RefObject<HTMLVideoElement | null>, connection: HostedConnection, active: boolean): void {
  // Did the Space key now down go down as PTT? Then its key-up is never sent as a key either, even
  // one that arrives after the picture lost focus and got it back.
  const spacePtt = useRef(false)
  useEffect(() => {
    const element = video.current
    if (!element || !active) return
    const link = connection.stream, held = new HeldInput()
    const box = (): PictureBox => {
      const r = element.getBoundingClientRect()
      return { left: r.left, top: r.top, width: r.width, height: r.height, videoWidth: element.videoWidth, videoHeight: element.videoHeight }
    }
    const send = (message: StreamPointer | StreamKey | null) => { if (message) link.input(held.note(message)) }
    // The press's click count, which its release carries too: a click at the shack is a click.
    const count = new ClickCount()
    let clicks = 0
    const down = (event: PointerEvent) => {
      clicks = count.press(event)
      const message = pointerMessage('down', box(), event, false, clicks)
      if (!message) return
      event.preventDefault()
      element.focus({ preventScroll: true })
      try { element.setPointerCapture(event.pointerId) } catch { /* moves still arrive while over the picture */ }
      send(message)
    }
    // A drag may run past the picture's edge, and is pinned to it; a plain hover outside is not sent.
    const move = (event: PointerEvent) => send(pointerMessage('move', box(), event, held.dragging))
    const up = (event: PointerEvent) => { if (held.dragging) send(pointerMessage('up', box(), event, true, clicks)) }
    const cancel = (event: PointerEvent) => { if (held.dragging) send(pointerMessage('cancel', box(), event, true)) }
    // A paste on the picture is committed text for the field focused at the shack.
    const paste = (event: ClipboardEvent) => {
      const message = textMessage(event.clipboardData?.getData('text/plain') ?? '')
      if (!message) return
      event.preventDefault()
      link.input(message)
    }
    const wheel = (event: WheelEvent) => {
      const message = wheelMessage(box(), event)
      if (!message) return
      event.preventDefault()
      link.input(message)
    }
    const key = (action: 'down' | 'up') => (event: KeyboardEvent) => {
      const message = keyMessage(action, event)
      if (!message) return
      event.preventDefault()
      send(message)
    }
    const keyDown = key('down'), keyUp = key('up')
    // ⛔ THE PTT KEY NEVER TRAVELS AS A KEY. Where the station says Space is its push-to-talk key, a
    // Space press on the picture is a held PTT, re-asserted on the stream's own channel until the
    // key comes up, and nothing at all on `control`. Anywhere else Space is a key like any other (a
    // space typed into a field at the shack), and if the station's word was out of date Nexus's
    // window drops it rather than key the rig. A press keeps the kind it started as.
    const keydown = (event: KeyboardEvent) => {
      if (event.code === 'Space' && !event.repeat) spacePtt.current = link.getSnapshot().pttKey
      if (event.code === 'Space' && spacePtt.current) {
        event.preventDefault()
        if (!event.repeat) link.holdPtt()
        return
      }
      keyDown(event)
    }
    const keyup = (event: KeyboardEvent) => {
      if (event.code === 'Space' && spacePtt.current) { event.preventDefault(); spacePtt.current = false; link.releasePtt(); return }
      keyUp(event)
    }
    const release = () => {
      if (spacePtt.current) link.releasePtt()
      for (const message of held.releaseAll()) link.input(message)
    }
    const menu = (event: Event) => event.preventDefault()
    element.addEventListener('pointerdown', down)
    element.addEventListener('pointermove', move)
    element.addEventListener('pointerup', up)
    element.addEventListener('pointercancel', cancel)
    element.addEventListener('wheel', wheel, { passive: false })
    element.addEventListener('keydown', keydown)
    element.addEventListener('keyup', keyup)
    element.addEventListener('blur', release)
    element.addEventListener('contextmenu', menu)
    element.addEventListener('paste', paste)
    return () => {
      release()
      element.removeEventListener('pointerdown', down)
      element.removeEventListener('pointermove', move)
      element.removeEventListener('pointerup', up)
      element.removeEventListener('pointercancel', cancel)
      element.removeEventListener('wheel', wheel)
      element.removeEventListener('keydown', keydown)
      element.removeEventListener('keyup', keyup)
      element.removeEventListener('blur', release)
      element.removeEventListener('contextmenu', menu)
      element.removeEventListener('paste', paste)
    }
  }, [video, connection, active])
}
