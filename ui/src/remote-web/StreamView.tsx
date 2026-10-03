import { useEffect, useRef, useState, useSyncExternalStore, type RefObject } from 'react'
import { t } from '../i18n'
import { useViewport } from '../useViewport'
import { initialState, startMonitor } from '../remote-monitor/session'
import { AudioListen, audioCaption, audioEnded } from './AudioListen'
import { BetaNote } from './BetaNote'
import type { HostedConnection } from './client'
import { transmitEpoch } from './operation-protocol'
import { IdReminder } from './id-reminder'
import { ClickCount, HeldInput, keyMessage, pointerMessage, textMessage, wheelMessage, type PictureBox } from './stream-capture'
import type { StopTarget, StreamLink, StreamView as LinkView } from './stream-link'
import type { AudioView } from './audio-listen'
import { shortFingerprint, type StreamKey, type StreamPointer } from './stream-protocol'
import '../remote-monitor/monitor.css'
import './remote.css'
import './stream.css'

// Invariant tokens. "Stop TX" and the TX indicator read the same on every transmit surface in
// Nexus and are English on all of them by decision (the pt-BR kit's README, "English on purpose"),
// so this page says what the picture beside it says.
const STOP_TX = 'Stop TX'
const TX = '▲ TX'
const BRAND = 'Nexus'
// How long a connection may take before the page says the station is not online. The relay refuses a
// browser whose station is not connected (`stationOffline`) before the session starts, and a browser
// cannot read why a socket was refused, so a session that has not started by now is reported as the
// station being away; the connection keeps retrying underneath and the sentence goes when it lands.
const OFFLINE_MS = 8000

/** The stream: Nexus at the shack as a picture, operated with this browser's mouse and keyboard.
 *
 *  Its authority is the chain the rest of Remote uses and nothing new: station control granted at
 *  the radio, a lease this browser acquires, and a stream offered only under that lease. Stop TX is
 *  always on screen while this view is, outside the picture, and reaches the station two ways at
 *  once - on the stream's own control channel and on the observe socket - so a stream that has
 *  frozen or died still leaves a way to unkey the rig. */
export function StreamView({ connection, station, disconnect, signOut, autostart = false, browserKey = null, mode = 'stream' }: {
  connection: HostedConnection; station: string; disconnect: () => void; signOut: () => void
  /** Opened by the station card's Stream or Listen: that press is the operator asking, so the view
   *  starts once, the first moment Start could be pressed, and never again on its own. */
  autostart?: boolean
  /** This browser's device key for the station (its fingerprint), for the approval sentence. */
  browserKey?: string | null
  /** `listen`: the station's receive audio with no picture, under the same lease, Stop and checks. */
  mode?: 'stream' | 'listen'
}) {
  useViewport(1, true)
  const operations = connection.operations, link = connection.stream
  const ops = useSyncExternalStore(operations.subscribe, operations.getSnapshot)
  const stream = useSyncExternalStore(link.subscribe, link.getSnapshot)
  const [observation, setObservation] = useState(initialState)
  const listen = mode === 'listen'
  // Listen's audio is the relay's audio lane, the one the workspace's Listen used; the stream's own
  // audio rides its WebRTC channel and is the stream's.
  const relayAudio = connection.audio
  const heard = useSyncExternalStore(listen ? relayAudio.subscribe : idleSubscribe, listen ? relayAudio.getSnapshot : idleAudio)
  const audioOn = heard.phase === 'connecting' || heard.phase === 'live' || heard.phase === 'gap' || heard.phase === 'stalled'
  const offline = useOffline(ops.connected)
  useEffect(() => startMonitor(connection.source, setObservation), [connection.source])
  const [wanted, setWanted] = useState(false)
  // The last stream ended because nobody answered "Still there?".
  const [idled, setIdled] = useState(false)
  // Why the last Start came to nothing, until the next one: the refusal's code.
  const [refused, setRefused] = useState<string | null>(null)
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
    if (listen || !(wanted && lease && (stream.phase === 'idle' || stream.phase === 'ended'))) return
    setWanted(false); void link.start(lease)
  }, [listen, wanted, lease, stream.phase, link])
  // Listen's half: the audio starts under the held lease. A browser may refuse sound that a press did
  // not start just now (Safari keeps it for the press itself), so it starts here only while that press
  // is still the browser's current activation; otherwise the lease is held and Listen waits for a press.
  useEffect(() => {
    if (!listen || !wanted || !lease || audioOn) return
    setWanted(false)
    if (pressStillCurrent()) relayAudio.listen(lease)
  }, [listen, wanted, lease, audioOn, relayAudio])
  // Listening is given under station control, so losing control ends it at once (AudioListen's rule,
  // on the same reading of the lease): the station stops on its own re-check too.
  const audioLease = state?.phase === 'controlling' ? state.leaseId : null
  useEffect(() => { if (listen && audioOn && !audioLease) relayAudio.release('notController') }, [listen, audioOn, audioLease, relayAudio])
  // Control gone - released, taken over, or the socket lost - ends the stream here at once, rather
  // than on the station's word a moment later.
  useEffect(() => { if (running && controlling === false) link.close('notController') }, [running, controlling, link])
  // Blind means no authority, for a click as much as for PTT: a frozen picture takes no input.
  useInput(video, connection, stream.phase === 'live')

  const start = () => {
    if (listen && lease) { relayAudio.listen(lease); return }
    setWanted(true)
    setIdled(false)
    setRefused(null)
    // A refused acquire ends the request, so the button is live again, and the page says why. The
    // refusal clears the held state, so without its reason the page only went back to Ready.
    if (state?.phase === 'available') void operations.acquire().catch((error: unknown) => {
      setWanted(false)
      setRefused(error instanceof Error ? error.message : 'stationUnavailable')
    })
  }
  const end = () => { setWanted(false); link.close(); void operations.release() }
  // The card's press, carried in: the view starts the first moment Start could be pressed. A station
  // that refuses (control off for this browser, another browser in control) consumes it, and the page
  // says why with Start beside it.
  const autostarting = useRef(autostart)
  const begin = useRef(start)
  begin.current = start
  useEffect(() => {
    if (!autostarting.current || !ops.connected || !ops.fresh) return
    if (state?.phase === 'occupied' || state?.phase === 'localPermissionRequired') { autostarting.current = false; return }
    if ((state?.phase !== 'available' && state?.phase !== 'controlling') || (ops.busy && !ops.reading)) return
    autostarting.current = false
    begin.current()
  }, [ops.connected, ops.fresh, ops.busy, ops.reading, state?.phase])
  // Unanswered, "Still there?" ends the stream as End the stream does, and says why afterwards.
  const asking = useStillThere(running, link, () => { end(); setIdled(true) })
  const stopTx = () => {
    link.stopTransmit(target)
    if (ops.stopAvailable) void operations.stopTransmit().catch(() => {})
  }
  const status = statusLine(ops.connected, state?.phase ?? null, stream.phase, stream.reason, wanted, refused, offline, listen)
  // The station offers its receive audio only from a build that has it (AudioListen's own rule).
  const audioOffered = !!state?.controls?.capabilities.includes('audioListen')
  // M9: receive audio is MUTED, not ducked, while the operator's own over may be on the air. Derived
  // on every change - this page holding PTT, the station saying its over is keyed, or the observed
  // rig keyed - so it lets go on every way an over ends and can never stick.
  const onAir = stream.ptt || stream.station?.keyed === true || keyed === true
  useEffect(() => { link.audio.setMuted(onAir) }, [onAir, link])
  const identify = useIdReminder(running, stream.station?.keyed === true)

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
          data-voice={stream.station?.keyed ? 'keyed' : undefined}
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
        {/* The microphone: OFF until the operator turns it on, and only then does the browser ask.
            PTT arms an over and the voice keys it, so it works with either PTT, this page's or the
            cockpit's through the picture. */}
        {stream.control && <button type="button" className="remote-button remote-stream-mic" aria-pressed={stream.mic === 'on'}
          disabled={stream.mic === 'asking'} title={t('remote.stream.mic.title')}
          onClick={() => void link.setMic(stream.mic !== 'on')}>
          {stream.mic === 'on' ? t('remote.stream.mic.on') : t('remote.stream.mic.off')}</button>}
        {running && <button type="button" className="remote-button" onClick={end}>{t('remote.stream.end')}</button>}
        {listen && audioOn && <button type="button" className="remote-button" onClick={() => relayAudio.release()}>{t('remote.audio.stop')}</button>}
        <button type="button" className="remote-button" onClick={disconnect}>{t('remote.disconnect')}</button>
        <button type="button" className="remote-button remote-button--quiet" onClick={signOut}>{t('remote.signOut')}</button>
      </div>
      <BetaNote className="remote-stream-beta" />
    </header>
    <main className="remote-stream-stage" aria-label={t('remote.stream.stage')}>
      {!listen && <video ref={video} className="remote-stream-video" tabIndex={0} muted autoPlay playsInline aria-label={t('remote.stream.picture')} />}
      {stream.phase === 'stalled' && <p className="remote-stream-stalled" role="alert">{t('remote.stream.stalled')}</p>}
      {/* The station's own word, on its last heartbeat reply: it is not holding transmit presence for
          this browser, whatever the picture here looks like (its clock says the picture is late). */}
      {stream.phase === 'live' && stream.presence === false && <p className="remote-stream-stalled" role="alert">{t('remote.stream.noPresence')}</p>}
      {running && <MicNotes stream={stream} identify={identify} />}
      {listen && <div className="remote-stream-placeholder">
        {/* Listen: the station's receive audio and no picture. One sentence for where it stands, and
            one button: Listen, or Stop listening while it plays. */}
        <p role="status">{!heard.supported ? t('remote.audio.unsupported')
          : audioOn ? audioCaption(heard.phase)
          : (state?.phase === 'available' || state?.phase === 'controlling') && !audioOffered ? t('remote.listen.unavailable')
          : status}</p>
        {heard.phase === 'ended' && <p role="alert">{audioEnded(heard.reason)}</p>}
        {!audioOn && heard.supported && audioOffered && (state?.phase === 'available' || state?.phase === 'controlling') &&
          <button type="button" className="remote-button remote-button--primary" disabled={wanted || (ops.busy && !ops.reading)} onClick={start}>{t('remote.listen.open')}</button>}
      </div>}
      {!listen && stream.phase !== 'live' && stream.phase !== 'stalled' && <div className="remote-stream-placeholder">
        {!running && idled && <p role="note">{t('remote.stream.idle.ended')}</p>}
        <p role={running ? undefined : 'status'}>{stream.phase === 'connecting' ? t('remote.stream.waitingForPicture') : status}</p>
        {/* A5: the station asks for this browser to be approved there with its key; this is the key to compare. */}
        {!running && browserKey && (stream.reason === 'deviceNotPinned' || stream.reason === 'deviceKeyMismatch') && stream.phase === 'ended' &&
          <p>{t('remote.thisBrowserKey', { key: shortFingerprint(browserKey) })}</p>}
        {!running && identify === 'end' && <p className="remote-stream-identify" role="note">{t('remote.stream.id.end')}</p>}
        {!running && (state?.phase === 'available' || state?.phase === 'controlling') && <>
          {/* Lit through the once-a-second read, as Release is: a press then waits it out (acquire). */}
          <button type="button" className="remote-button remote-button--primary" disabled={wanted || (ops.busy && !ops.reading)} onClick={start}>{t('remote.stream.start')}</button>
          {/* What the stream needs at the shack, as far as the capture code shows it. */}
          <p className="remote-stream-entry-note" role="note">{t('remote.stream.display')}</p>
        </>}
      </div>}
      {/* "Still there?", over the picture and never over the header, so Stop TX stays where it is,
          uncovered. A press anywhere on it stays its own until it is let go (the capture), so no part
          of it lands on the picture and reaches Nexus. The button needs no handler: every click and
          key on this page is the operator's answer (useStillThere). */}
      {running && asking && <div className="remote-stream-idle"
        onPointerDown={event => { try { event.currentTarget.setPointerCapture(event.pointerId) } catch { /* it is still pressed here */ } }}>
        <p role="alert">{t('remote.stream.idle.prompt')}</p>
        <button type="button" className="remote-button remote-button--primary">{t('remote.stream.idle.keep')}</button>
      </div>}
    </main>
  </div>
}

/** "Still there?" (the operator's pick, "15 min + prompt"): true while it is asked. Any click, key or
 *  turn of the wheel on this page is the operator's, wherever it lands, and the link counts a held PTT; watching,
 *  listening and the station transmitting are not. A press keeps the prompt up until it is let go.
 *  Unanswered for a minute, `idle` runs. Each look reads the link's clock, every second and whenever
 *  the tab is shown or hidden, so a tab whose timers are throttled still ends on its first look past
 *  the minute, however few looks it had. */
function useStillThere(running: boolean, link: StreamLink, idle: () => void): boolean {
  const [asking, setAsking] = useState(false)
  const ending = useRef(idle)
  ending.current = idle
  useEffect(() => {
    if (!running) return
    let done = false
    const look = () => {
      if (done) return
      const state = link.idle.state()
      if (state === 'end') { done = true; ending.current(); return }
      setAsking(state === 'prompt')
    }
    const pressed = () => link.idle.active()
    const input = () => { link.idle.active(); look() }
    const timer = setInterval(look, 1000)
    document.addEventListener('pointerdown', pressed, true)
    document.addEventListener('pointerup', look, true)
    document.addEventListener('pointercancel', look, true)
    document.addEventListener('keydown', input, true)
    // Assistive technology activates a control with a click alone, no press before it.
    document.addEventListener('click', input, true)
    // Tuning with the wheel is the operator at work (ruling B3).
    document.addEventListener('wheel', input, { capture: true, passive: true })
    document.addEventListener('visibilitychange', look)
    return () => {
      clearInterval(timer)
      document.removeEventListener('pointerdown', pressed, true)
      document.removeEventListener('pointerup', look, true)
      document.removeEventListener('pointercancel', look, true)
      document.removeEventListener('keydown', input, true)
      document.removeEventListener('click', input, true)
      document.removeEventListener('wheel', input, true)
      document.removeEventListener('visibilitychange', look)
      setAsking(false)
    }
  }, [running, link])
  return asking
}

/** What the microphone is doing, in a stack of notes at the foot of the picture that never takes a
 *  click from it. The station's word on its over comes first: why an over ended (the audio design's
 *  §7), the rig showing no power while the voice arrives (display only, the operator's ruling "State
 *  it + warn"), a press with the microphone off (it keys nothing: the ruling "Arms your mic"). */
function MicNotes({ stream, identify }: { stream: LinkView; identify: 'due' | 'end' | null }) {
  const notes: { key: string; text: string; warn?: boolean }[] = []
  const station = stream.station
  if (stream.uplinkStalled) notes.push({ key: 'uplink', text: t('remote.stream.mic.uplink'), warn: true })
  if (station?.noPowerOut) notes.push({ key: 'noPower', text: t('remote.stream.mic.noPower'), warn: true })
  if (station?.armed && stream.mic !== 'on') notes.push({ key: 'needed', text: t('remote.stream.mic.needed'), warn: true })
  if (!station?.armed && station?.ended) {
    const why = endedNote(station.ended)
    if (why) notes.push({ key: `ended-${station.ended}`, text: why, warn: true })
  }
  if (stream.mic === 'denied') notes.push({ key: 'denied', text: t('remote.stream.mic.denied'), warn: true })
  if (stream.mic === 'unavailable') notes.push({ key: 'unavailable', text: t('remote.stream.mic.unavailable'), warn: true })
  if (stream.mic === 'on' && stream.micProcessing) notes.push({ key: 'processing', text: t('remote.stream.mic.processing') })
  // The operator's ruling "State it + warn": said where the microphone is turned on.
  if (stream.mic === 'on') notes.push({ key: 'usb', text: t('remote.stream.mic.usb') })
  if (identify === 'due') notes.push({ key: 'identify', text: t('remote.stream.id.due') })
  if (!notes.length) return null
  return <ul className="remote-stream-notes" aria-label={t('remote.stream.mic.notes')}>
    {notes.map(note => <li key={note.key} className="remote-stream-note" data-tone={note.warn ? 'warn' : undefined}
      role={note.warn ? 'alert' : 'status'}>{note.text}</li>)}
  </ul>
}
function endedNote(ended: string): string | null {
  return ended === 'audioGap' ? t('remote.stream.mic.ended.audioGap')
    : ended === 'presence' ? t('remote.stream.mic.ended.presence')
    : ended === 'ceiling' ? t('remote.stream.mic.ended.ceiling')
    : ended === 'watchdog' ? t('remote.stream.mic.ended.watchdog')
    : ended === 'routeChanged' ? t('remote.stream.mic.ended.routeChanged')
    // Let go, or stopped at the station: the operator knows, or the station's own reading says so.
    : null
}

/** M11: the ID reminder, display only. `due` while a prompt stands (thirty seconds), `end` once the
 *  stream has ended with a run of overs in it, `null` otherwise. */
function useIdReminder(running: boolean, keyed: boolean): 'due' | 'end' | null {
  const reminder = useRef<IdReminder | null>(null)
  reminder.current ??= new IdReminder()
  const onAir = useRef(keyed)
  onAir.current = keyed
  const [due, setDue] = useState(false)
  const [end, setEnd] = useState(false)
  useEffect(() => {
    if (!running) return
    setEnd(false)
    const look = () => { if (reminder.current!.observe(Date.now(), onAir.current)) setDue(true) }
    look()
    const timer = setInterval(look, 1000)
    return () => { clearInterval(timer); if (reminder.current!.end()) setEnd(true); setDue(false) }
  }, [running])
  useEffect(() => {
    if (!due) return
    const hide = setTimeout(() => setDue(false), 30_000)
    return () => clearTimeout(hide)
  }, [due])
  return due ? 'due' : end ? 'end' : null
}

/** One sentence for where things stand. Written out rather than looked up in a map: the catalog's
 *  orphan check reads literal t() calls, so a lookup table reads as keys nobody uses. */
function statusLine(connected: boolean, phase: string | null, stream: string, reason: string | null, wanted: boolean, refused: string | null,
  offline: boolean, listen: boolean): string {
  if (stream === 'live') return t('remote.stream.live')
  if (stream === 'stalled') return t('remote.stream.stalled')
  if (stream === 'connecting') return t('remote.stream.starting')
  // A refused start is what the operator just did, so it goes ahead of how the last stream ended.
  // The station's standing word (control off, another browser in control) still says it once it lands.
  if (refused && connected && phase !== 'localPermissionRequired' && phase !== 'occupied') {
    return refused === 'localPermissionRequired' ? t('remote.stream.permission')
      : refused === 'controllerBusy' ? t('remote.stream.occupied')
      : listen ? t('remote.listen.refused') : t('remote.stream.refused')
  }
  if (stream === 'ended') return ended(reason)
  // The station away (Nexus closed, Remote off, the PC asleep) and a slow link read the same from
  // here; past OFFLINE_MS the first is the likely one, and it is the one with a next step.
  if (!connected) return offline ? t('remote.stream.offline') : t('remote.stream.connecting')
  if (phase === null) return t('remote.stream.waiting')
  if (phase === 'localPermissionRequired') return t('remote.stream.permission')
  if (phase === 'occupied') return t('remote.stream.occupied')
  if (listen) return wanted ? t('remote.audio.connecting') : t('remote.listen.ready')
  return wanted ? t('remote.stream.starting') : t('remote.stream.ready')
}
// The stream page never reads the relay's audio lane: Listen is its own view.
const IDLE_AUDIO: AudioView = { phase: 'off', reason: null, supported: true }
const idleSubscribe = () => () => {}
const idleAudio = () => IDLE_AUDIO
/** True once the page has gone OFFLINE_MS without a session; false again the moment it has one. */
function useOffline(connected: boolean): boolean {
  const [offline, setOffline] = useState(false)
  useEffect(() => {
    setOffline(false)
    if (connected) return
    const timer = setTimeout(() => setOffline(true), OFFLINE_MS)
    return () => clearTimeout(timer)
  }, [connected])
  return offline
}
/** Whether the press that asked for sound is still the browser's current user activation. Where the
 *  browser cannot say, the press is taken as current (the page starts sound as it always has). */
function pressStillCurrent(): boolean {
  const activation = (globalThis.navigator as (Navigator & { userActivation?: { isActive: boolean } }) | undefined)?.userActivation
  return activation ? activation.isActive : true
}
function ended(reason: string | null): string {
  return reason === 'notController' ? t('remote.stream.ended.notController')
    : reason === 'streamUnavailable' ? t('remote.stream.ended.unavailable')
    : reason === 'serviceAccessExpired' ? t('remote.stream.ended.accessExpired')
    : reason === 'remoteOff' ? t('remote.stream.ended.remoteOff')
    : reason === 'tryLater' ? t('remote.stream.ended.tryLater')
    : reason === 'streamClosed' ? t('remote.stream.ended.station')
    : reason === 'streamDisabled' ? t('remote.stream.ended.disabled')
    : reason === 'streamInUse' ? t('remote.stream.ended.inUse')
    : reason === 'invalidOffer' ? t('remote.stream.ended.invalidOffer')
    // A5: the station holds the offer to this browser's key, as pinned when it was approved there.
    : reason === 'deviceNotPinned' ? t('remote.stream.ended.notPinned')
    : reason === 'deviceKeyMismatch' ? t('remote.stream.ended.keyChanged')
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
  useEffect(() => {
    const element = video.current
    if (!element || !active) return
    const link = connection.stream, held = new HeldInput()
    const box = (): PictureBox => {
      const r = element.getBoundingClientRect()
      return { left: r.left, top: r.top, width: r.width, height: r.height, videoWidth: element.videoWidth, videoHeight: element.videoHeight }
    }
    // Every press and release goes as itself, and what is left held is re-asserted (the dead-man).
    const send = (message: StreamPointer | StreamKey | null) => {
      if (!message) return
      link.input(held.note(message))
      const now = held.held()
      link.holdInput(now.keys, now.buttons)
    }
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
    const keydown = key('down'), keyup = key('up')
    const release = () => {
      for (const message of held.releaseAll()) link.input(message)
      link.holdInput([], 0)
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
    // The browser window losing focus sends the key-ups of what is held here to another program.
    window.addEventListener('blur', release)
    element.addEventListener('contextmenu', menu)
    element.addEventListener('paste', paste)
    return () => {
      release()
      window.removeEventListener('blur', release)
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
