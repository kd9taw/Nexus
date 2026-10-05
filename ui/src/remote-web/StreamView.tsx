import { useCallback, useEffect, useId, useRef, useState, useSyncExternalStore, type RefObject } from 'react'
import { flushSync } from 'react-dom'
import { t } from '../i18n'
import { useViewport, useViewportSize } from '../useViewport'
import { initialState, startMonitor } from '../remote-monitor/session'
import { AudioListen, audioCaption, audioEnded } from './AudioListen'
import { BetaNote } from './BetaNote'
import type { HostedConnection } from './client'

/** What the stream view uses of a connection: the hosted one, or the window's road to a station on
 *  the shack's own network (`../lan/connection.ts`). */
export type StreamConnection = Pick<HostedConnection, 'operations' | 'stream' | 'audio' | 'source'>
import { transmitEpoch } from './operation-protocol'
import { IdReminder } from './id-reminder'
import { MIC_LEVEL_DB, MIC_METER_FLOOR_DB, MIC_METER_TOP_DB, meterDb } from './mic-level'
import { ClickCount, HeldInput, PictureZoom, keyMessage, keyPress, pointerMessage, textMessage, typedChange, wheelMessage, type PictureBox, type Stage } from './stream-capture'
import { streamLayout, type StreamLayout } from './stream-layout'
import { exactSize, observeDeviceSize, type StopTarget, type StreamLink, type StreamView as LinkView } from './stream-link'
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
// How long a finger's press on the picture waits before it goes, so that a second finger landing
// meanwhile makes the touch a pinch, which is never sent.
const TOUCH_HOLD_MS = 100

/** The stream: Nexus at the shack as a picture, operated with this browser's mouse and keyboard.
 *
 *  Its authority is the chain the rest of Remote uses and nothing new: station control granted at
 *  the radio, a lease this browser acquires, and a stream offered only under that lease. Stop TX is
 *  always on screen while this view is, outside the picture, and reaches the station two ways at
 *  once - on the stream's own control channel and on the observe socket - so a stream that has
 *  frozen or died still leaves a way to unkey the rig. */
export function StreamView({ connection, station, disconnect, signOut, autostart = false, browserKey = null, mode = 'stream' }: {
  connection: StreamConnection; station: string; disconnect: () => void
  /** Absent where there is no account to sign out of (the road on the shack's own network). */
  signOut?: () => void
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
  // How far the last Stop got. Both paths carry it and the first acceptance is the answer; a Stop
  // that no path accepted and none is still carrying says it failed. It used to say nothing.
  const stopAccepted = ops.stopAccepted || stream.stop === 'accepted'
  const stopProgress = stopAccepted ? keyed === false ? 'stopped' : 'sent'
    : ops.stopSending || stream.stop === 'sending' ? 'sending'
    : ops.stopError || stream.stop === 'refused' ? 'failed' : 'idle'

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
  const zoom = usePictureZoom(video, link, running)
  // Blind means no authority, for a click as much as for PTT: a frozen picture takes no input.
  useInput(video, connection, stream.phase === 'live', zoom.zoom, zoom.draw)
  useExactPicture(video)

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
  useEscapeStops(canStop, stopTx)
  const fullScreen = useFullScreen(stopTx)
  const status = statusLine(ops.connected, state?.phase ?? null, stream.phase, stream.reason, wanted, refused, offline, listen)
  // The station offers its receive audio only from a build that has it (AudioListen's own rule).
  const audioOffered = !!state?.controls?.capabilities.includes('audioListen')
  // M9: receive audio is MUTED, not ducked, while the operator's own over may be on the air. Derived
  // on every change - this page holding PTT, the station saying its over is keyed, or the observed
  // rig keyed - so it lets go on every way an over ends and can never stick.
  const onAir = stream.ptt || stream.station?.keyed === true || keyed === true
  useEffect(() => { link.audio.setMuted(onAir) }, [onAir, link])
  const identify = useIdReminder(running, stream.station?.keyed === true)
  // What the PTT shows, and all that starts a hold: a live picture under a fresh lease. A press on a greyed-out PTT
  // sends nothing (the operator's pick "Refuse it on the page", 2026-10-04), and its handlers are what refuse it:
  // greyed out it is aria-disabled, never disabled, so every pointer, key and click still reaches it. A browser blurs
  // a focused button it disables (Chrome 154 does), so a disabled PTT lost the keyboard's focus at every lapse and
  // Space did nothing until the operator focused it again (the operator's pick "Stay focusable while greyed",
  // 2026-10-04). A HELD PTT stays lit whatever this says (the operator's pick "Keep held PTT enabled", 2026-10-04):
  // the state is stale for a round trip now and then. Every other way an over ends still ends it.
  const pttReady = stream.phase === 'live' && !!lease

  // THE PHONE LAYOUT (the operator's pick, 2026-10-03), from the window this page has (streamLayout): the header row,
  // a rail beside the picture on its side, bars above and below it upright. Every control keeps its place in the tree
  // in all three, so turning the phone moves controls and never remounts one: a held PTT and Listen's sound carry on.
  const [typingOpen, setTypingOpen] = useState(false)
  const [typingFocus, setTypingFocus] = useState(false)
  const layout = useLayout(typingOpen && typingFocus)
  const phone = layout !== 'header', rail = layout === 'rail', bars = layout === 'bars'
  const more = useMore(phone)
  const typingField = useRef<HTMLInputElement>(null)
  useEffect(() => { if (!bars || !running) { setTypingOpen(false); setTypingFocus(false) } }, [bars, running])
  const keyboard = () => {
    if (typingOpen) { setTypingOpen(false); setTypingFocus(false); return }
    // Opened and focused inside the press itself: a phone raises its keyboard only for a focus a press made.
    flushSync(() => setTypingOpen(true))
    typingField.current?.focus({ preventScroll: true })
  }
  const endButton = <button type="button" className="remote-button" onClick={end}>{t('remote.stream.end')}</button>
  const fullScreenButton = !listen && fullScreen.offered && <button type="button" className="remote-button" onClick={fullScreen.toggle}
    title={fullScreen.on ? undefined : t('remote.stream.fullscreen.title')}>{fullScreen.on ? t('remote.stream.fullscreen.exit') : t('remote.stream.fullscreen')}</button>
  const leave = <>
    <button type="button" className="remote-button" onClick={disconnect}>{t('remote.disconnect')}</button>
    {signOut && <button type="button" className="remote-button remote-button--quiet" onClick={signOut}>{t('remote.signOut')}</button>}
  </>

  return <div className="app remote-monitor-app remote-stream-app" data-stream-phase={stream.phase} data-layout={layout}>
    <header className="rm-header remote-stream-header">
      <span className="remote-stream-title"><strong>{BRAND}</strong> <span>{station}</span></span>
      {/* Where the stream's state is said: in the header's row, in the bar over the picture upright, and on its side
          in a chip over the picture, which carries the beta mark there because the beta line is under More. */}
      <div className="remote-stream-state">
        {rail && <span className="remote-beta-mark">{t('remote.beta.mark')}</span>}
        {/* While a stream runs this is where its state is said; otherwise the placeholder says it, once. */}
        <span className="remote-stream-status" role="status">{running ? status : null}</span>
        {bars && running && coarsePointer() && <span className="remote-stream-turn" role="note">{t('remote.stream.turn')}</span>}
      </div>
      <div className="remote-stream-controls">
        <div className="remote-stream-safety">
        {/* THE STOP LINE. First in the row, so it is on the first line however the row wraps; rendered
            whenever this view is, never behind anything, and enabled whenever either path to the
            station can carry a Stop. First in the rail and in the bar over the picture too, and the largest. */}
        <button type="button" className="remote-button remote-stream-stop" disabled={!canStop} onClick={stopTx}
          title={stopProgress === 'stopped' ? t('remote.stop.stopped.title') : stopProgress === 'sent' ? t('remote.stop.sent.title') : undefined}>{STOP_TX}</button>
        {stopProgress !== 'idle' && <span className="remote-stream-stopstate" role="status">
          {stopProgress === 'stopped' ? t('remote.stop.stopped') : stopProgress === 'sent' ? t('remote.stop.sent')
            : stopProgress === 'failed' ? t('shell.halt.failed') : t('remote.stop.sending')}</span>}
        {keyed === true && <span className="remote-stream-tx" role="status">{TX}</span>}
        </div>
        <div className="remote-stream-operate">
        {stream.control && <AudioListen audio={link.audio} client={operations} />}
        {stream.control && <button type="button" className="remote-button remote-stream-ptt" aria-pressed={stream.ptt}
          title={t('remote.stream.ptt.title')} aria-disabled={(!stream.ptt && !pttReady) || undefined} data-keyed={stream.keyed || undefined}
          data-voice={stream.station?.keyed ? 'keyed' : undefined}
          onPointerDown={event => {
            if (event.button !== 0 || !pttReady) return
            try { event.currentTarget.setPointerCapture(event.pointerId) } catch { /* the hold still starts */ }
            link.holdPtt()
          }}
          onPointerUp={() => link.releasePtt()} onPointerCancel={() => link.releasePtt()}
          onLostPointerCapture={() => link.releasePtt()} onBlur={() => link.releasePtt()}
          onKeyDown={event => { if ((event.key === ' ' || event.key === 'Enter') && !event.repeat) { event.preventDefault(); if (pttReady) link.holdPtt() } }}
          onKeyUp={event => { if (event.key === ' ' || event.key === 'Enter') { event.preventDefault(); link.releasePtt() } }}
          onContextMenu={event => event.preventDefault()}>{t('remote.stream.ptt')}</button>}
        {/* The microphone: OFF until the operator turns it on, and only then does the browser ask.
            PTT arms an over and the voice keys it, so it works with either PTT, this page's or the
            cockpit's through the picture. */}
        {stream.control && <button type="button" className="remote-button remote-stream-mic" aria-pressed={stream.mic === 'on'}
          disabled={stream.mic === 'asking'} title={t('remote.stream.mic.title')}
          onClick={() => void link.setMic(stream.mic !== 'on')}>
          {stream.mic === 'on' ? t('remote.stream.mic.on') : t('remote.stream.mic.off')}</button>}
        {stream.control && stream.mic === 'on' && <MicLevel link={link} />}
        {bars && stream.control && <button type="button" className="remote-button remote-stream-keyboard" aria-pressed={typingOpen}
          disabled={stream.phase !== 'live'} onClick={keyboard}>{t('remote.stream.keyboard')}</button>}
        {bars && typingOpen && running && <TypingBox link={link} live={stream.phase === 'live'} field={typingField} focused={setTypingFocus} />}
        </div>
        <div className="remote-stream-session">
        {!phone && running && endButton}
        {listen && audioOn && <button type="button" className="remote-button" onClick={() => relayAudio.release()}>{t('remote.audio.stop')}</button>}
        {!bars && fullScreenButton}
        {!phone && leave}
        {phone && <button type="button" ref={more.button} className="remote-button remote-stream-more" aria-expanded={more.open}
          aria-controls={more.open ? more.id : undefined} onClick={more.toggle}>{t('remote.stream.more')}</button>}
        </div>
      </div>
      {!rail && <BetaNote className="remote-stream-beta" />}
    </header>
    <main className="remote-stream-stage" aria-label={t('remote.stream.stage')}>
      {!listen && <video ref={video} className="remote-stream-video" tabIndex={0} muted autoPlay playsInline aria-label={t('remote.stream.picture')} />}
      {zoom.zoomed && <button type="button" className="remote-button remote-stream-fit" title={t('remote.stream.fit.title')} onClick={zoom.fit}>{t('remote.stream.fit')}</button>}
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
        {browserKey && (keyRefusal(refused) || (heard.phase === 'ended' && keyRefusal(heard.reason))) &&
          <p>{t('remote.thisBrowserKey', { key: shortFingerprint(browserKey) })}</p>}
        {!audioOn && heard.supported && audioOffered && (state?.phase === 'available' || state?.phase === 'controlling') &&
          <button type="button" className="remote-button remote-button--primary" disabled={wanted || (ops.busy && !ops.reading)} onClick={start}>{t('remote.listen.open')}</button>}
      </div>}
      {!listen && stream.phase !== 'live' && stream.phase !== 'stalled' && <div className="remote-stream-placeholder">
        {!running && idled && <p role="note">{t('remote.stream.idle.ended')}</p>}
        <p role={running ? undefined : 'status'}>{stream.phase === 'connecting' ? t('remote.stream.waitingForPicture') : status}</p>
        {/* A5: the station asks for this browser to be approved there with its key; this is the key to compare. */}
        {!running && browserKey && ((keyRefusal(stream.reason) && stream.phase === 'ended') || keyRefusal(refused)) &&
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
    {/* More, on a phone: what its bars have no room for, over the picture while open. A choice closes it, and so does
        a press on the picture, which the cover under it takes for itself: a press meant to close a menu is never a
        click at the shack. */}
    {more.open && <div className="remote-stream-more-cover" onPointerDown={event => {
      try { event.currentTarget.setPointerCapture(event.pointerId) } catch { /* it is still pressed here */ }
      more.close()
    }} />}
    {more.open && <div id={more.id} ref={more.panel} className="remote-stream-more-panel" role="group" aria-label={t('remote.stream.more')}
      onClick={more.close}>
      {running && endButton}
      {bars && fullScreenButton}
      {leave}
      {rail && <BetaNote className="remote-stream-beta" />}
    </div>}
  </div>
}

/** The layout for the window this page has (streamLayout), held while the operator types in the typing box: the
 *  phone's keyboard takes height and never width, and a layout that followed it could turn the page under the
 *  operator's fingers and take the field away, and the keyboard with it. Turning the phone changes the width, and
 *  the layout follows that at once. */
function useLayout(typing: boolean): StreamLayout {
  const size = useViewportSize()
  const free = size ? streamLayout(size.width, size.height) : 'header'
  const [held, setHeld] = useState<{ layout: StreamLayout; width: number } | null>(null)
  const width = size?.width ?? null
  useEffect(() => { if (!typing) setHeld(width === null ? null : { layout: free, width }) }, [typing, free, width])
  return typing && held && held.width === width ? held.layout : free
}

/** A phone's More: End, Disconnect, Sign out and what else its bars have no room for. Open, it closes on a choice
 *  in it, on a press anywhere else and on Esc, and it is never open outside the phone layouts. */
function useMore(phone: boolean) {
  const [open, setOpen] = useState(false)
  const id = useId()
  const panel = useRef<HTMLDivElement>(null), button = useRef<HTMLButtonElement>(null)
  useEffect(() => { if (!phone) setOpen(false) }, [phone])
  useEffect(() => {
    if (!open) return
    const away = (event: Event) => {
      const target = event.target instanceof Node ? event.target : null
      if (!target || !(panel.current?.contains(target) || button.current?.contains(target))) setOpen(false)
    }
    const key = (event: KeyboardEvent) => { if (event.key === 'Escape') setOpen(false) }
    document.addEventListener('pointerdown', away, true)
    document.addEventListener('keydown', key, true)
    return () => { document.removeEventListener('pointerdown', away, true); document.removeEventListener('keydown', key, true) }
  }, [open])
  return { open: phone && open, id, panel, button, toggle: () => setOpen(was => !was), close: () => setOpen(false) }
}

/** The typing box (the operator's pick, 2026-10-03). A phone has no keys to send through the picture: its keyboard
 *  composes words, which the picture's key bridge cannot carry. Here the field always reads what the field focused at
 *  the shack was given: a change goes as Backspaces and committed text (typedChange), a word still being composed goes
 *  when it is finished, Enter goes as Enter and empties the box, and Backspace with the box empty goes as Backspace.
 *  Nothing goes while the picture is not live (blind means no authority, as for a click): what was typed then is
 *  taken back out of the box. Only the contract's `text` and `key` messages, on the input path the picture uses. */
function TypingBox({ link, live, field, focused }: { link: StreamLink; live: boolean; field: RefObject<HTMLInputElement>; focused: (on: boolean) => void }) {
  const sent = useRef('')
  const sync = () => {
    const input = field.current
    if (!input) return
    if (!live) { input.value = sent.current; return }
    for (const message of typedChange(sent.current, input.value)) link.input(message)
    sent.current = input.value
  }
  return <form className="remote-stream-typing" onSubmit={event => {
    event.preventDefault()
    sync()
    if (!live || !field.current) return
    for (const message of keyPress('Enter')) link.input(message)
    field.current.value = ''
    sent.current = ''
  }}>
    <input ref={field} type="text" className="remote-stream-typing-field" aria-label={t('remote.stream.typing')} placeholder={t('remote.stream.typing')}
      maxLength={256} enterKeyHint="enter" autoComplete="off" autoCorrect="off" autoCapitalize="none" spellCheck={false}
      onInput={event => { if (!(event.nativeEvent as InputEvent).isComposing) sync() }} onCompositionEnd={sync}
      onKeyDown={event => {
        if (event.key !== 'Backspace' || event.nativeEvent.isComposing || event.currentTarget.value !== '' || !live) return
        for (const message of keyPress('Backspace')) link.input(message)
      }}
      onFocus={() => focused(true)} onBlur={() => focused(false)} />
  </form>
}

/** A phone or tablet: a coarse pointer, as useFullScreen reads it. */
const coarsePointer = () => window.matchMedia?.('(pointer: coarse)').matches ?? false

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

/** The operator's Mic level, beside Mic while it is on: how loud the voice goes to the station, kept by this
 *  browser, and a meter of what is going. Shown only where the page could build the level. Its readings are
 *  measurements, so they are not translated. */
function MicLevel({ link }: { link: StreamLink }) {
  const level = useSyncExternalStore(link.subscribe, () => link.micLevel)
  const meter = useSyncExternalStore(link.micMeter.subscribe, link.micMeter.getSnapshot)
  if (!meter.live) return null
  const db = Math.round(meterDb(meter.peak))
  return <span className="remote-stream-miclevel">
    <label title={t('remote.stream.mic.level.title')}>
      <span>{t('remote.stream.mic.level')}</span>
      <input type="range" min={MIC_LEVEL_DB.min} max={MIC_LEVEL_DB.max} step={1} value={level}
        aria-valuetext={levelReading(level)} onChange={event => link.setMicLevel(Number(event.currentTarget.value))} />
    </label>
    <span className="remote-stream-micmeter" role="meter" aria-label={t('remote.stream.mic.meter')}
      aria-valuemin={MIC_METER_FLOOR_DB} aria-valuemax={Math.round(MIC_METER_TOP_DB)} aria-valuenow={db} aria-valuetext={meterReading(db)}
      data-limited={meter.limited || undefined}>
      <span style={{ width: `${Math.min(100, (100 * (db - MIC_METER_FLOOR_DB)) / (MIC_METER_TOP_DB - MIC_METER_FLOOR_DB))}%` }} />
    </span>
  </span>
}

/** The level as gain over the microphone's own, built invariantly: dB is not translated. */
function levelReading(db: number): string {
  return db > 0 ? `+${db} dB` : `${db} dB`
}
/** The meter in dB under full scale, built invariantly for the same reason. */
function meterReading(db: number): string {
  return `${db} dBFS`
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
  if (stream.mic === 'denied') notes.push({ key: 'denied', text: deniedNote(stream.micDenied), warn: true })
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
/** Why the browser gave no microphone, said with where to fix it: this site's setting, the computer's privacy
 *  settings, the browser's question again, a microphone to connect, or the program holding it. Anything else
 *  goes with the browser's own name for the error. */
function deniedNote(denied: LinkView['micDenied']): string {
  const why = denied?.why
  return why === 'site' ? t('remote.stream.mic.denied.site')
    : why === 'system' ? t('remote.stream.mic.denied.system')
    : why === 'dismissed' ? t('remote.stream.mic.denied.dismissed')
    : why === 'noDevice' ? t('remote.stream.mic.denied.noDevice')
    : why === 'busy' ? t('remote.stream.mic.denied.busy')
    : t('remote.stream.mic.denied.other', { name: denied?.name ?? 'Error' })
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
      // S1-M1: the station holds no key for this browser, or another one: approved again there.
      : refused === 'deviceNotPinned' ? listen ? t('remote.listen.notPinned') : t('remote.stream.ended.notPinned')
      : refused === 'deviceKeyMismatch' ? listen ? t('remote.listen.keyChanged') : t('remote.stream.ended.keyChanged')
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
/** The station refused this browser's key (A5, S1-M1): its key is what the operator compares there. */
const keyRefusal = (reason: string | null | undefined) => reason === 'deviceNotPinned' || reason === 'deviceKeyMismatch'
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
    // S3-M1: the answer did not carry this station's own signature for this offer and session.
    : reason === 'stationKeyMismatch' ? t('remote.stream.ended.stationKey')
    : reason === 'stationNotSigned' ? t('remote.stream.ended.stationUnsigned')
    : reason === 'streamUnsupported' ? t('remote.stream.ended.unsupported')
    : reason === 'streamHidden' ? t('remote.stream.ended.hidden')
    : t('remote.stream.ended.failed')
}

/** The picture at its own size in device pixels when it fills the stage to within a pixel or two
 *  (`exactSize`): the station sizes it to the stage (`view`), and the browser stretching it by a hair
 *  would soften every letter. Anything else the stylesheet fits to the stage, as it always has. The
 *  input bridge reads the video's box as it is drawn, so a point maps to the frame either way. */
function useExactPicture(video: RefObject<HTMLVideoElement | null>): void {
  useEffect(() => {
    const element = video.current, stage = element?.parentElement
    if (!element || !stage) return
    let area: { width: number; height: number } | null = null
    const apply = () => {
      const size = area && exactSize({ width: element.videoWidth, height: element.videoHeight }, area, window.devicePixelRatio || 1)
      element.style.width = size ? `${size.width}px` : ''
      element.style.height = size ? `${size.height}px` : ''
      element.toggleAttribute('data-exact', size !== null)
    }
    const unobserve = observeDeviceSize(stage, (width, height) => { area = { width, height }; apply() })
    element.addEventListener('resize', apply)
    return () => { unobserve(); element.removeEventListener('resize', apply) }
  }, [video])
}

/** The operator's own zoom into the picture (PictureZoom): drawn as its transform, kept over the
 *  stage as the stage and the frame change, told to the link so the station sends the stage times the
 *  zoom (`view`, the operator's pick "Ask for more pixels", 2026-10-03), and the whole picture again
 *  when the stream ends. `zoomed` is whether Fit is offered. */
function usePictureZoom(video: RefObject<HTMLVideoElement | null>, link: StreamLink, running: boolean) {
  const [zoom] = useState(() => new PictureZoom())
  const [zoomed, setZoomed] = useState(false)
  const draw = useCallback(() => {
    const element = video.current, stage = stageOf(element)
    if (stage) zoom.keep(stage)
    if (element) element.style.transform = zoom.transform
    link.setZoom(zoom.zoom)
    setZoomed(zoom.zoom > 1)
  }, [video, link, zoom])
  const fit = useCallback(() => { zoom.fit(); draw() }, [zoom, draw])
  useEffect(() => {
    const element = video.current, stage = element?.parentElement
    if (!element || !stage) return
    const unobserve = observeDeviceSize(stage, draw)
    element.addEventListener('resize', draw)
    return () => { unobserve(); element.removeEventListener('resize', draw) }
  }, [video, draw])
  useEffect(() => { if (!running) fit() }, [running, fit])
  return { zoom, zoomed, draw, fit }
}
/** The stage the picture sits in, as laid out, and the frame's own size. */
function stageOf(element: HTMLVideoElement | null): Stage | null {
  const stage = element?.parentElement
  if (!element || !stage) return null
  const r = stage.getBoundingClientRect()
  return { left: r.left, top: r.top, width: r.width, height: r.height, videoWidth: element.videoWidth, videoHeight: element.videoHeight }
}

/** The input bridge's page half (S11, A2). Listeners go on the PICTURE and nowhere else: a pointer
 *  on the header, a key typed while any other control has focus, never reaches the station. What
 *  was pressed through the picture is released through it when it loses focus or stops being live,
 *  so nothing is left held at the shack by a key-up this page never saw.
 *
 *  A FINGER is Nexus's mouse, as it was, but its press waits TOUCH_HOLD_MS before it goes: a second
 *  finger landing meanwhile makes the touch a pinch, and NOTHING of a touch with two fingers in it is
 *  ever sent, until every finger is up. Those fingers zoom and pan the picture here (`zoom`). A pinch
 *  used to reach the shack as two presses and a drag between them, on whatever control lay under
 *  them. A tap shorter than the wait sends its press and its release together; a second finger after
 *  the press went ends it at the shack with a cancel, which clicks nothing there. A finger has no
 *  hover, so it moves nothing at the shack unless its press went. */
function useInput(video: RefObject<HTMLVideoElement | null>, connection: StreamConnection, active: boolean,
  zoom: PictureZoom, drawZoom: () => void): void {
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
    // Touch: the fingers down on the picture; a press waiting to go, with where its finger has got to
    // since; the finger whose press went, for a cancel; and whether this touch is a pinch.
    const fingers = new Map<number, { x: number; y: number }>()
    let waiting: { down: StreamPointer; last: PointerEvent | null; timer: ReturnType<typeof setTimeout> } | null = null
    let pressing: PointerEvent | null = null
    let pinching = false
    const drop = () => { if (waiting) clearTimeout(waiting.timer); waiting = null }
    const go = () => {
      const press = waiting
      drop()
      if (!press) return
      send(press.down)
      if (press.last) send(pointerMessage('move', box(), press.last, true))
    }
    const touchDown = (event: PointerEvent) => {
      fingers.set(event.pointerId, { x: event.clientX, y: event.clientY })
      event.preventDefault()
      if (pinching || fingers.size > 1) {
        if (waiting) drop()
        else if (held.dragging && pressing) send(pointerMessage('cancel', box(), pressing, true))
        pressing = null
        pinching = true
        zoom.hold([...fingers.values()])
        return
      }
      clicks = count.press(event)
      const down = pointerMessage('down', box(), event, false, clicks)
      if (!down) return
      element.focus({ preventScroll: true })
      try { element.setPointerCapture(event.pointerId) } catch { /* moves still arrive while over the picture */ }
      pressing = event
      waiting = { down, last: null, timer: setTimeout(go, TOUCH_HOLD_MS) }
    }
    const touchMove = (event: PointerEvent) => {
      if (!fingers.has(event.pointerId)) return
      fingers.set(event.pointerId, { x: event.clientX, y: event.clientY })
      if (pinching) {
        const stage = stageOf(element)
        if (stage) { zoom.move([...fingers.values()], stage); drawZoom() }
        return
      }
      pressing = event
      if (waiting) waiting.last = event
      else if (held.dragging) send(pointerMessage('move', box(), event, true))
    }
    const touchEnd = (event: PointerEvent, action: 'up' | 'cancel') => {
      if (!fingers.delete(event.pointerId)) return
      if (pinching) {
        if (fingers.size) zoom.hold([...fingers.values()])
        else pinching = false
        return
      }
      pressing = null
      // A tap: its press goes now, its release right after. A press the browser cancelled before it
      // went is nothing to end.
      if (action === 'up') go(); else drop()
      if (held.dragging) send(action === 'up' ? pointerMessage('up', box(), event, true, clicks) : pointerMessage('cancel', box(), event, true))
    }
    const down = (event: PointerEvent) => {
      if (event.pointerType === 'touch') { touchDown(event); return }
      clicks = count.press(event)
      const message = pointerMessage('down', box(), event, false, clicks)
      if (!message) return
      event.preventDefault()
      element.focus({ preventScroll: true })
      try { element.setPointerCapture(event.pointerId) } catch { /* moves still arrive while over the picture */ }
      send(message)
    }
    // A drag may run past the picture's edge, and is pinned to it; a plain hover outside is not sent.
    const move = (event: PointerEvent) => {
      if (event.pointerType === 'touch') touchMove(event)
      else send(pointerMessage('move', box(), event, held.dragging))
    }
    const up = (event: PointerEvent) => {
      if (event.pointerType === 'touch') touchEnd(event, 'up')
      else if (held.dragging) send(pointerMessage('up', box(), event, true, clicks))
    }
    const cancel = (event: PointerEvent) => {
      if (event.pointerType === 'touch') touchEnd(event, 'cancel')
      else if (held.dragging) send(pointerMessage('cancel', box(), event, true))
    }
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
    // A press still waiting never goes once the picture has lost focus or stopped being live.
    const release = () => {
      drop()
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
  }, [video, connection, active, zoom, drawZoom])
}

/** Esc is a Stop anywhere on this page, as on every screen of Nexus (the desktop's useEscStop; the
 *  operator's pick "Esc stops anywhere", 2026-10-03). It is heard in the capture phase on `window`,
 *  so no control on the page can keep it from the Stop, and never prevented, so it keeps every other
 *  meaning it has: on the picture it still goes on to Nexus at the shack, which stops there too, and
 *  a second Stop is harmless. It is Stop TX's own press, made exactly when Stop TX could be pressed;
 *  a held Esc is one press. */
function useEscapeStops(can: boolean, stop: () => void): void {
  const latest = useRef({ can, stop })
  latest.current = { can, stop }
  useEffect(() => {
    const key = (event: KeyboardEvent) => { if (event.key === 'Escape' && !event.repeat && latest.current.can) latest.current.stop() }
    window.addEventListener('keydown', key, true)
    return () => window.removeEventListener('keydown', key, true)
  }, [])
}

type KeyboardLock = { lock?: (keys: string[]) => Promise<void>; unlock?: () => void }
type OrientationLock = { lock?: (orientation: string) => Promise<void>; unlock?: () => void }
const keyboardLock = () => (navigator as Navigator & { keyboard?: KeyboardLock }).keyboard
const orientationLock = () => screen.orientation as (ScreenOrientation & OrientationLock) | undefined

/** Full screen for the whole page, Stop TX with it, never the picture alone; and Esc still a Stop
 *  wherever there is an Esc key (the operator's pick, 2026-10-03). On a desktop every exit the page
 *  did not ask for sends Stop TX, in every browser: the page cannot tell an Esc from any other way
 *  out. Chrome and Edge lock Esc to the page (Keyboard Lock), so it reaches useEscapeStops and only a
 *  two-second hold leaves, and that exit stops again, harmlessly. The lock never stands in for that
 *  Stop: it may resolve and still not hold Esc (unmeasured in WebView2), and then Esc leaves at once,
 *  unheard, as it does in a desktop browser without it (Firefox, Safari). A phone or tablet (a coarse
 *  pointer) has no Esc: it turns to landscape where it can, and its back gesture leaves with no Stop,
 *  so a swipe never ends an over. An iPhone has no full screen for a page at all, so it is not
 *  offered there. */
function useFullScreen(stop: () => void): { offered: boolean; on: boolean; toggle: () => void } {
  const [on, setOn] = useState(() => !!document.fullscreenElement)
  const latest = useRef(stop)
  latest.current = stop
  // This page's own full screen while it lasts: whether leaving it was asked for here, whether leaving
  // it any other way is a Stop, and what it locked, to let go of when it ends.
  const mine = useRef<{ asked: boolean; stops: boolean; locked: 'keyboard' | 'orientation' | null } | null>(null)
  useEffect(() => {
    const letGo = (left: { locked: 'keyboard' | 'orientation' | null }) => {
      try {
        if (left.locked === 'keyboard') keyboardLock()?.unlock?.()
        if (left.locked === 'orientation') orientationLock()?.unlock?.()
      } catch { /* nothing left to let go of */ }
    }
    const changed = () => {
      setOn(!!document.fullscreenElement)
      const left = mine.current
      if (document.fullscreenElement || !left) return
      mine.current = null
      // The Stop first, before anything else that could go wrong.
      if (left.stops && !left.asked) latest.current()
      letGo(left)
    }
    document.addEventListener('fullscreenchange', changed)
    return () => {
      document.removeEventListener('fullscreenchange', changed)
      // Leaving the stream page leaves its full screen, as asked.
      const left = mine.current
      mine.current = null
      if (left && document.fullscreenElement) { letGo(left); void document.exitFullscreen().catch(() => {}) }
    }
  }, [])
  const toggle = () => {
    if (document.fullscreenElement) {
      if (mine.current) mine.current.asked = true
      void document.exitFullscreen().catch(() => {})
      return
    }
    const touch = window.matchMedia?.('(pointer: coarse)').matches ?? false
    const entry: { asked: boolean; stops: boolean; locked: 'keyboard' | 'orientation' | null } = { asked: false, stops: !touch, locked: null }
    mine.current = entry
    document.documentElement.requestFullscreen({ navigationUI: 'hide' }).then(async () => {
      if (mine.current !== entry) return
      if (touch) {
        try { await orientationLock()?.lock?.('landscape'); entry.locked = 'orientation' } catch { /* it stays as the operator holds it */ }
        return
      }
      // Esc held for the page, so its press stops and full screen stays. Leaving still stops: a lock
      // can resolve without holding Esc, and the page cannot tell.
      const keys = keyboardLock()
      if (!keys?.lock) return
      try { await keys.lock(['Escape']); entry.locked = 'keyboard' } catch { /* not held: Esc leaves at once, unheard, and that exit stops */ }
    }, () => { if (mine.current === entry) mine.current = null })
  }
  return { offered: document.fullscreenEnabled === true, on, toggle }
}
