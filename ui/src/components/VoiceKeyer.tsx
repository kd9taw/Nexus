// ⚠️ THIS FILE IS ON THE **PARTIAL** LIST (i18n/hardcoded-strings.test.ts), and for one
// reason only: this pane's two ■ Stop buttons are still written in English here — ■ Stop,
// which really does end an over (stopVoice → Engine::stop_voice flushes the output ring and
// unkeys), and the ■ Stop & save beside it, which ends the RECORDER. The first is a stop
// control however few sweeps look for it (it is pane-resident, so it is deliberately on no
// stop-line list), and the second shares its vocabulary; they move together in the
// transmit-path batch so the pair cannot be reworded apart. Everything else is in the
// catalog under `phone.keyer.*` — F-key names, a slot's own label is the operator's words, and
// both stay in the code.
import { useEffect, useMemo, useRef, useState } from 'react'
import type { VoiceMessage } from '../types'
import {
  cancelVoiceRecording,
  getVoiceMessages,
  importVoiceMessage,
  playVoiceMessage,
  startVoiceRecording,
  stopVoice,
  stopVoiceRecording,
} from '../api'
import { pushToast, withErrorToast } from '../toast'
import {
  deleteVoiceSlot,
  pendingVoiceDeletes,
  subscribeVoiceDeletes,
  supersedeVoiceDelete,
} from '../features/voiceDelete'
import { dismissFlexDaxNote, flexDaxNoteDismissed } from '../features/flexDaxNote'
import { IS_MAC, FN_KEY_HINT } from '../platform'
import { t } from '../i18n'
import { T } from '../i18n/T'

/** The recorder's own plate — REC is the rig's word, the ● its state glyph. */
const REC_PLATE = '● REC'

interface Props {
  /** Whether TX is enabled (Monitor). Playback/record are no-ops when off — we surface why. */
  txEnabled: boolean
  /** Whether the operator is holding live PTT — playing a canned message then would fight
   * the live over, so we block it and say so. */
  keyed: boolean
  /** Whether a voice-keyer message is queued or on the air at the station (the snapshot's
   * `radio.voicePlaying`): its fall is the only signal that a message this pane started is over.
   * Used to know whether the unmount teardown actually ENDED an over, so it can say so, and
   * whether a message this pane sent is known to have ended (`endedOnAir`), so the unmount need
   * not stop it. Absent when false; a station that never sends it shows no rise, so nothing then
   * ever reads as ended and the unmount stops. */
  voicePlaying: boolean | undefined
  /** Field Day exchange to read ("3A WI") — when set (FD mode), hint that a slot can be
   * recorded with it for one-key sends. Empty/undefined outside FD. */
  fdExchange?: string | null
  /** ENTER SENDS MESSAGE in the Phone cockpit's contest strip: the slot the next Enter plays
   *  glows (`lit`), the keyer lends the strip its recordings and a player (`report`, null when it
   *  goes), its ■ Stop is a stop (`onStop`), and a slot played by hand is reported (`onPlay`), so
   *  the CQ slot switches ESM to Run. */
  esm?: {
    lit: readonly string[]
    report: (keyer: VoiceKeyerForEnter | null) => void
    onStop: () => void
    onPlay: (slot: number) => void
  }
  /** The active radio is a Flex on SmartSDR CAT or Hamlib (features/flexDaxNote): the first message
   *  the engine takes brings a note, once, that SSB goes out only with DAX on for voice in SmartSDR.
   *  Information only; the message is sent first. Absent on every other radio. */
  flexDaxNote?: boolean
}

/** What the keyer lends Enter Sends Message: its slots as they stand, whether it is recording,
 *  and a player that answers. */
export interface VoiceKeyerForEnter {
  messages: VoiceMessage[]
  recording: boolean
  /** Play `slot` for an Enter press: null once the engine took it, or why it did not. The same
   *  checks as a click, said to the strip instead of in a toast. */
  play: (slot: number) => Promise<string | null>
}

// THE INPUT-DEVICE WARNING, and it is not chrome. It had a `.vk-note` paragraph of its own —
// two permanent lines of the pane at every window, in a leading column that already overflows
// its track (density pass, 2026-08-04). The ROW went; the SENTENCE did not, because recording
// the rig's RX audio into a slot puts the WRONG AUDIO on the air the moment the slot is
// played, which is an on-air failure, not 40px of ornament. It rides BOTH controls that start
// a recording — the ● tool, and the slot button itself while the slot is empty — since either
// one is where the operator is standing when it matters. Pinned in
// PhoneCockpit.density.test.tsx, which asserts the sentence rather than the deletion.
//
// It is no longer a `RECORD_HINT` constant spliced onto "Record F3. " but a clause of ONE
// catalog sentence, `phone.keyer.slot.record.title`: a warning glued to a stem cannot be
// translated into a language that orders the two the other way round. Both controls name the
// whole sentence and pass their slot.

/** How long the rig must have read unkeyed, after keying for a message, before that message
 *  counts as over: one unkeyed snapshot mid-message is not an end. */
const VOICE_END_HOLD_MS = 2000

/** Whether a message this pane sent is KNOWN to have ended: the rig keyed after it was sent,
 *  has unkeyed since, and has stayed unkeyed for `VOICE_END_HOLD_MS` (`fellAt` is set only by a
 *  fall after that rise, and cleared by the next rise). Anything less is doubt, and the unmount
 *  stops it. */
function endedOnAir(p: { fellAt: number | null }): boolean {
  return p.fellAt !== null && Date.now() - p.fellAt >= VOICE_END_HOLD_MS
}

/**
 * Phone voice keyer (casual) — message slots on F1–F6 and F8 (AGN, the slot Enter Sends Message
 * plays for AGN in a contest), each a recorded 12 kHz mono WAV played to the rig with PTT keyed
 * for the message (the same TX path the soundcard CW keyer uses).
 * Click a slot or press its F-key to play; ● records from the input device; ⤓ imports a
 * `.wav`; ✕ clears, with an Undo window (features/voiceDelete.ts); Esc / ■ Stop aborts. The three
 * tools sit on the slot's LEFT, away from its ▶: under it, ✕ was hit reaching for Play.
 *
 * The ■ Stop here IS a control that stops a transmission (stopVoice → Engine::stop_voice: the
 * radio loop flushes the output ring and unkeys) and it GOES AWAY WITH THIS PANE. That is
 * allowed and is not a loophole: it is a convenience, not the operator's last resort. Stop TX
 * and Tune in the header and PTT in the dock are the last resort, they render outside every
 * ⊞-removable pane, and none of them is hideable — which is THE STOP LINE
 * (features/panelState.ts) and the whole of why this pane carries a ⊞ Panels entry. Nothing
 * else about it bears on that: not that it transmits (six panes do), not that it hosts this
 * button (two do), not that its unmount cleanup is itself a stop. The cleanup below is not
 * what buys the entry — it is what the entry's note has to WARN ABOUT.
 */
export function VoiceKeyer({ txEnabled, keyed, voicePlaying, fdExchange, esm, flexDaxNote }: Props) {
  const [stored, setMsgs] = useState<VoiceMessage[]>([])
  // A deleted slot inside its Undo window still has its recording in the backend; everything
  // here (the slot, its F-key, Enter Sends Message) treats it as empty until Undo or the clear.
  const [deleting, setDeleting] = useState(pendingVoiceDeletes)
  useEffect(
    () =>
      subscribeVoiceDeletes((list) => {
        if (list) setMsgs(list)
        setDeleting(pendingVoiceDeletes())
      }),
    [],
  )
  const msgs = useMemo(
    () => stored.map((m) => (deleting.has(m.slot) ? { ...m, file: '' } : m)),
    [stored, deleting],
  )
  const [recording, setRecording] = useState<number | null>(null)
  const fileRefs = useRef<Record<number, HTMLInputElement | null>>({})
  // Mirror `recording` into a ref so the unmount cleanup (which captures [] deps) can see
  // whether a recording is still in flight and cancel it.
  const recordingRef = useRef<number | null>(null)
  useEffect(() => {
    recordingRef.current = recording
  }, [recording])

  // The message this pane put on the air and has not seen end, or null: its slot, a number of
  // its own (`gen`), and what the rig has done since it was sent. Set when the operator sends
  // one; cleared only by ■ Stop / Esc and by the engine refusing THAT send. It is never cleared
  // by `voicePlaying` falling, because that edge need not be this message's: an F-key pressed
  // while the last message is still on the air sees the last one end before its own keys, and a
  // stale snapshot can read it ended mid-message. Instead the unmount asks whether this
  // message is known to have ended (`endedOnAir`), and anything short of that stops it.
  const playingRef = useRef<{ slot: number; gen: number; rose: boolean; fellAt: number | null } | null>(
    null,
  )
  const genRef = useRef(0)
  const send = (slot: number) => {
    genRef.current += 1
    playingRef.current = { slot, gen: genRef.current, rose: false, fellAt: null }
    return genRef.current
  }
  const refused = (gen: number) => {
    if (playingRef.current?.gen === gen) playingRef.current = null
  }
  // THE FLEX DAX NOTE: raised by a message the engine TOOK (a refused one played nothing), never
  // before the send, and shown until dismissed. The prop is read through a ref: Enter Sends
  // Message's player resolves after a render that may have changed it.
  const [daxNote, setDaxNote] = useState(false)
  const flexDaxNoteRef = useRef(flexDaxNote)
  flexDaxNoteRef.current = flexDaxNote
  const tookMessage = () => {
    if (flexDaxNoteRef.current && !flexDaxNoteDismissed()) setDaxNote(true)
  }
  // The same flag the cleanup can read — it captures [] deps and cannot see the prop.
  const onAirRef = useRef(false)
  useEffect(() => {
    const p = playingRef.current
    if (p && !onAirRef.current && voicePlaying) {
      // A rise AFTER the send: the station had a message once this one was sent. A rise is the
      // only edge that can start one, so a message pressed while another is on the air waits for one.
      p.rose = true
      p.fellAt = null
    } else if (p?.rose && onAirRef.current && !voicePlaying) {
      p.fellAt = Date.now()
    }
    onAirRef.current = voicePlaying === true
  }, [voicePlaying])

  useEffect(() => {
    void getVoiceMessages()
      .then(setMsgs)
      .catch(() => {})
  }, [])

  // Unmount tears down BOTH transmit paths. This runs when the operator leaves the Phone
  // section and, since 2026-08-03, when he unticks Voice Keyer in ⊞ Panels. It is not what
  // permits the menu entry — nothing had to permit it (features/panelState.ts, THE STOP
  // LINE) — it is why the entry carries a warning: a stop the operator did not ask for is
  // indistinguishable from a dropout.
  //
  // The two teardowns are not the same act, and the second one is DESTRUCTIVE:
  //   · stopVoice ends a message that is playing. Cleanup, not safety — the operator could
  //     have stopped it from the dock either way — but it is an over cut short, so it speaks.
  //     ⛔ Sent unless this pane sent nothing or its last message is known to have ended
  //     (playingRef, endedOnAir): in doubt, it stops. On a computer driving a station,
  //     stop_voice is a Stop of EVERYTHING there (both roads), so an unconditional one on every
  //     unmount turned TX off at the station each time the operator left Phone. The ■ Stop
  //     button and Esc still always send it.
  //   · cancelVoiceRecording DISCARDS an in-progress capture (lib.rs: take + drop the
  //     buffer). Nothing about hiding a pane implies "throw away the recording I am making",
  //     so it may not happen silently. It still has to happen — leaving the recorder running
  //     puts its only Stop & save button off-screen, and saving instead would overwrite the
  //     slot's existing message with a truncated one, destroying something else.
  // The ⊞ entry names both before the tick (VOICE_KEYER_STOPS_ON_HIDE, and
  // VOICE_KEYER_UNDO_ENDS on the menu's other hide path), and this names them after, for the
  // operator who walked off the Phone screen and never opened a menu at all.
  //
  // BOTH announcements are CONDITIONAL ON WHAT ACTUALLY HAPPENED, in opposite directions
  // from where they started. The over used to end in silence — the same argument that says a
  // binned recording must be announced says an aborted transmission must be, and it is the
  // louder of the two. And the discard used to be announced unconditionally while
  // cancelVoiceRecording's rejection was swallowed, which told the operator his take was
  // gone while the backend recorder ran on; a failure now says so instead.
  useEffect(() => {
    return () => {
      // BOTH conditions, and only these two: this pane started a message it has not seen end,
      // AND the station says a voice message is playing right now. Either alone would lie — a
      // message the engine dropped before it played (TX went off first) leaves the first set with
      // nothing ever on the air, and another pane's message satisfies the second with none of
      // this one's. The cost is the ~1 poll between pressing an F-key and the snapshot showing
      // it: a hide inside that window says nothing. A missed notice beats a false one.
      const p = playingRef.current
      const playing = p !== null && !endedOnAir(p) ? p.slot : null
      const over = onAirRef.current ? playing : null
      if (playing !== null) void stopVoice().catch(() => {})
      if (over !== null) {
        pushToast(t('phone.keyer.hide.stoppedOver', { slot: over }), 'info', 6000)
      }
      const slot = recordingRef.current
      if (slot !== null) {
        void cancelVoiceRecording()
          .then(() => pushToast(t('phone.keyer.hide.discarded', { slot }), 'info', 6000))
          .catch(() =>
            pushToast(t('phone.keyer.hide.recorderStuck', { slot }), 'error', 8000),
          )
      }
    }
  }, [])

  /** Why `slot` cannot play now, or null: what a click is told in a toast, and an Enter press
   *  in the contest strip instead. */
  const refusal = (slot: number): { text: string; ms: number } | null => {
    const m = msgs.find((x) => x.slot === slot)
    if (!m || !m.file) return { text: t('phone.keyer.empty', { slot }), ms: 3000 }
    if (recording !== null) return { text: t('phone.keyer.busyRecording'), ms: 2500 }
    if (keyed) return { text: t('phone.keyer.releasePtt'), ms: 3000 }
    // The radio having the mic (Nexus's own Flex client, native audio on, Phone at the shack) is
    // not a refusal since 2026-10-12 ("Change it now"): the station switches the radio to DAX for
    // the message and back. A message it then drops says why on the status lane.
    if (!txEnabled) {
      // NAME THE CONTROL THAT IS ON THIS SCREEN (#81). "enable transmit" was honest and
      // useless: the Phone cockpit shows no Enable-Tx button — App hides the TopBar's TX
      // cluster in this view and the header's TX pill is display-only — so the operator was
      // told to flip a switch he could not find. PTT is that switch when TX is off (it reads
      // "■ TX OFF — CLICK TO ENABLE" and arms transmit on the press).
      return { text: t('phone.keyer.txOff'), ms: 3500 }
    }
    return null
  }

  const play = (slot: number) => {
    // The slot the operator pressed (the CQ slot switches Enter Sends Message to Run).
    esmRef.current?.onPlay(slot)
    const why = refusal(slot)
    if (why) {
      pushToast(why.text, 'info', why.ms)
      return
    }
    const gen = send(slot)
    // The toast carries the engine's reason: a message refused outside the privileges (or with
    // TX off after all) used to be ignored without a word, and now says why.
    void withErrorToast(() => playVoiceMessage(slot), t('phone.keyer.playFailed', { slot })).then(
      (snap) => {
        if (snap === null) refused(gen)
        else tookMessage()
      },
    )
  }

  const stop = () => {
    playingRef.current = null
    void stopVoice().catch(() => {})
    esmRef.current?.onStop()
  }

  // ENTER SENDS MESSAGE's player: `play`'s checks and send, answered rather than toasted. The
  // strip holds the press until this resolves, and logs a contact only once it resolved null.
  const playForEnter = async (slot: number): Promise<string | null> => {
    const why = refusal(slot)
    if (why) return why.text
    const gen = send(slot)
    try {
      await playVoiceMessage(slot)
      tookMessage()
      return null
    } catch (e) {
      refused(gen)
      return e instanceof Error ? e.message : String(e)
    }
  }
  const esmRef = useRef(esm)
  esmRef.current = esm
  const playForEnterRef = useRef(playForEnter)
  playForEnterRef.current = playForEnter
  // The strip is lent the slots as they stand and a player that is always this render's.
  useEffect(() => {
    esmRef.current?.report({
      messages: msgs,
      recording: recording !== null,
      play: (slot) => playForEnterRef.current(slot),
    })
  }, [msgs, recording])
  useEffect(() => () => esmRef.current?.report(null), [])

  const startRec = (slot: number) => {
    setRecording(slot)
    // The toast carries the reason, as a play's does: a computer driving a station is refused
    // Record while its PTT is held, and says so.
    void withErrorToast(() => startVoiceRecording(), t('phone.keyer.recordFailed')).then((snap) => {
      if (snap === null) setRecording(null)
    })
  }

  const stopRec = async (slot: number) => {
    const label = msgs.find((m) => m.slot === slot)?.label ?? ''
    setRecording(null)
    // Clicking a deleted slot records over it; the Undo stands until the take is saved.
    await supersedeVoiceDelete(slot)
    const list = await withErrorToast(
      () => stopVoiceRecording(slot, label),
      t('phone.keyer.saveFailed'),
    )
    if (list) {
      setMsgs(list)
      pushToast(t('phone.keyer.saved', { slot, label }), 'success')
    }
  }

  const onImport = async (slot: number, file: File) => {
    const label = msgs.find((m) => m.slot === slot)?.label ?? ''
    const bytes = Array.from(new Uint8Array(await file.arrayBuffer()))
    await supersedeVoiceDelete(slot)
    const list = await withErrorToast(
      () => importVoiceMessage(slot, label, bytes),
      t('phone.keyer.importFailed'),
    )
    if (list) {
      setMsgs(list)
      pushToast(t('phone.keyer.imported', { slot, label }), 'success')
    }
  }

  const clear = (m: VoiceMessage) =>
    deleteVoiceSlot(m.slot, m.label || t('phone.keyer.slot.unnamed', { slot: m.slot }))

  // A slot's F-key plays it from anywhere on the Phone screen, the log line's boxes included, as
  // CW's macros send from anywhere in the CW cockpit (operator, 2026-10-11). F1–F12 are all held,
  // with a slot or without, before the slot list is there, when it could not be read and while
  // recording, so none reaches the webview, where F5 reloads the app on Windows. With Alt, Ctrl or
  // Cmd held a key is the system's (Alt+F4 closes the window), as in CW. Esc stops; in a text box
  // it is left to the shared Esc stop (useEscStop: App's halt), as it always was.
  useEffect(() => {
    const isField = (t: EventTarget | null) =>
      t instanceof HTMLElement && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA')
    const onKey = (e: KeyboardEvent) => {
      // Esc always stops playback (and is the only key honored mid-record).
      if (e.key === 'Escape') {
        if (isField(e.target)) return
        e.preventDefault()
        stop()
        return
      }
      if (!/^F([1-9]|1[0-2])$/.test(e.key) || e.altKey || e.ctrlKey || e.metaKey) return
      e.preventDefault()
      // Don't let an F-key key the rig while a recording is in progress.
      if (recording !== null) return
      const slot = Number(/^F(\d)$/.exec(e.key)?.[1])
      if (msgs.some((x) => x.slot === slot)) play(slot)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [msgs, txEnabled, keyed, recording])

  return (
    <div className="vk">
      {/* No title of its own: the keyer renders inside a CockpitPaneFrame whose head
          already says "Voice keyer" — the old <h2> here would double it. */}
      <div className="vk-head">
        {/* Default Mac keyboards eat bare F-keys as media keys — say so where the binding
            is advertised, or the feature silently reads as broken there (mac QA audit). */}
        <span className="vk-hint" title={IS_MAC ? FN_KEY_HINT : undefined}>
          {IS_MAC ? t('phone.keyer.hint.mac') : t('phone.keyer.hint')}
        </span>
        <span className="vk-spacer" />
        {/* ⚠️ NOT MIGRATED — this button STOPS A TRANSMISSION (stopVoice → Engine::stop_voice:
            the radio loop flushes the output ring and unkeys). It is pane-resident, so it is
            deliberately on no sweep's stopControls list — but the transmit-path batch moves
            every control that stops an over, whether or not a sweep looks for it, and its label
            and tooltip go with it. */}
        <button type="button" className="vk-stop" onClick={stop} title="Abort playback (Esc)">
          ■ Stop
        </button>
      </div>
      {/* The `.vk-note` paragraph stood here until 2026-08-04 — see the input-device note
          above, which is where its sentence went. */}
      {fdExchange && (
        <p className="vk-fd-hint">
          <T k="phone.keyer.fd.hint" tags={{ b: <strong /> }} vals={{ exchange: fdExchange }} />
        </p>
      )}
      {flexDaxNote && daxNote && (
        <p className="vk-flex-dax-note" role="note">
          {t('phone.keyer.flexDax.note')}{' '}
          <button
            type="button"
            className="settings-linkbtn"
            onClick={() => {
              dismissFlexDaxNote()
              setDaxNote(false)
            }}
          >
            {t('phone.keyer.flexDax.dismiss')}
          </button>
        </p>
      )}
      <div className="vk-grid">
        {msgs.map((m) => {
          const isRec = recording === m.slot
          const hasFile = !!m.file
          return (
            <div key={m.slot} className={`vk-slot${hasFile ? ' has' : ''}${isRec ? ' rec' : ''}`}>
              <div className="vk-tools">
                {isRec ? (
                  // ⚠️ NOT MIGRATED — this one ends the RECORDER, not an over, but it is the
                  // other half of this pane's ■ Stop vocabulary and moves with it, so the pair
                  // cannot be reworded apart.
                  <button type="button" className="vk-tool stop" onClick={() => void stopRec(m.slot)}>
                    ■ Stop &amp; save
                  </button>
                ) : (
                  <>
                    <button
                      type="button"
                      className="vk-tool"
                      onClick={() => startRec(m.slot)}
                      disabled={recording !== null}
                      title={t('phone.keyer.slot.record.title', { slot: m.slot })}
                    >
                      ●
                    </button>
                    <button
                      type="button"
                      className="vk-tool"
                      onClick={() => fileRefs.current[m.slot]?.click()}
                      disabled={recording !== null}
                      title={t('phone.keyer.import.title')}
                    >
                      ⤓
                    </button>
                    <button
                      type="button"
                      className="vk-tool"
                      onClick={() => clear(m)}
                      disabled={recording !== null || !hasFile}
                      title={t('phone.keyer.clear.title')}
                    >
                      ✕
                    </button>
                  </>
                )}
                <input
                  ref={(el) => {
                    fileRefs.current[m.slot] = el
                  }}
                  type="file"
                  accept=".wav"
                  hidden
                  onChange={(e) => {
                    const f = e.target.files?.[0]
                    if (f) void onImport(m.slot, f)
                    e.target.value = '' // allow re-importing the same file
                  }}
                />
              </div>
              <button
                type="button"
                className={`vk-play${esm?.lit.includes(`F${m.slot}`) ? ' esm-lit' : ''}`}
                onClick={() => (hasFile ? play(m.slot) : startRec(m.slot))}
                disabled={recording !== null && !isRec}
                title={
                  hasFile
                    ? t('phone.keyer.slot.play.title', { slot: m.slot, label: m.label })
                    : t('phone.keyer.slot.record.title', { slot: m.slot })
                }
              >
                <span className="vk-fkey">F{m.slot}</span>
                <span className="vk-label">
                  {m.label || t('phone.keyer.slot.unnamed', { slot: m.slot })}
                </span>
                <span className="vk-state">
                  {isRec ? REC_PLATE : hasFile ? '▶' : t('phone.keyer.slot.state.record')}
                </span>
              </button>
            </div>
          )
        })}
      </div>
    </div>
  )
}
