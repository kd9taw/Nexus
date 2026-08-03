import { useEffect, useRef, useState } from 'react'
import type { VoiceMessage } from '../types'
import {
  cancelVoiceRecording,
  clearVoiceMessage,
  getVoiceMessages,
  importVoiceMessage,
  playVoiceMessage,
  startVoiceRecording,
  stopVoice,
  stopVoiceRecording,
} from '../api'
import { pushToast, withErrorToast } from '../toast'

interface Props {
  /** Whether TX is enabled (Monitor). Playback/record are no-ops when off — we surface why. */
  txEnabled: boolean
  /** Whether the operator is holding live PTT — playing a canned message then would fight
   * the live over, so we block it and say so. */
  keyed: boolean
  /** The rig's real keyed state (snapshot). The engine has no "voice message finished"
   * event — the radio loop simply drops PTT when the samples run out — so the rig UNKEYING
   * is the only signal that a message this pane started is over. Used for one thing: to know
   * whether the unmount teardown actually ENDED an over, so it can say so. */
  transmitting: boolean
  /** Field Day exchange to read ("3A WI") — when set (FD mode), hint that a slot can be
   * recorded with it for one-key sends. Empty/undefined outside FD. */
  fdExchange?: string | null
}

/**
 * Phone voice keyer (casual) — F1–F6 message slots, each a recorded 12 kHz mono WAV played
 * to the rig with PTT keyed for the message (the same TX path the soundcard CW keyer uses).
 * Click a slot or press its F-key to play; ● records from the input device; ⤓ imports a
 * `.wav`; ✕ clears; Esc / ■ Stop aborts.
 *
 * The ■ Stop here is a convenience, not the operator's last resort — Stop TX in the header
 * and PTT in the dock are, and neither is hideable. That is what lets this pane carry a ⊞
 * Panels entry despite transmitting: see the unmount cleanup below and THE STOP LINE in
 * features/panelState.ts.
 */
export function VoiceKeyer({ txEnabled, keyed, transmitting, fdExchange }: Props) {
  const [msgs, setMsgs] = useState<VoiceMessage[]>([])
  const [recording, setRecording] = useState<number | null>(null)
  const fileRefs = useRef<Record<number, HTMLInputElement | null>>({})
  // Mirror `recording` into a ref so the unmount cleanup (which captures [] deps) can see
  // whether a recording is still in flight and cancel it.
  const recordingRef = useRef<number | null>(null)
  useEffect(() => {
    recordingRef.current = recording
  }, [recording])

  // The slot whose message this pane put on the air, or null. Set when the operator sends
  // one, cleared by ■ Stop / Esc and by the rig UNKEYING — the falling edge of `transmitting`
  // is how a message ending on its own is observed, there being no completion event. A bare
  // `if (!transmitting) clear` would not do: the snapshot lags the send by a poll, so it
  // would wipe the slot before the rig ever keyed.
  const playingRef = useRef<number | null>(null)
  // The same flag the cleanup can read — it captures [] deps and cannot see the prop.
  const onAirRef = useRef(false)
  useEffect(() => {
    if (onAirRef.current && !transmitting) playingRef.current = null
    onAirRef.current = transmitting
  }, [transmitting])

  useEffect(() => {
    void getVoiceMessages()
      .then(setMsgs)
      .catch(() => {})
  }, [])

  // Unmount tears down BOTH transmit paths. This runs when the operator leaves the Phone
  // section and, since 2026-08-03, when he unticks Voice Keyer in ⊞ Panels — which is the
  // whole reason the pane is allowed a menu entry at all (features/panelState.ts, clause
  // (b) of THE STOP LINE: a hideable pane's hide path must itself be the stop).
  //
  // The two teardowns are not the same act, and the second one is DESTRUCTIVE:
  //   · stopVoice ends a message that is playing. That is a stop, and stopping is the point.
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
      // AND the rig is keyed right now. Either alone would lie — a message the engine refused
      // (no privileges) leaves the first set with nothing ever on the air, and live mic PTT
      // satisfies the second with no keyer message involved. The cost is the ~1 poll between
      // pressing an F-key and the snapshot showing TX: a hide inside that window says nothing.
      // A missed notice beats a false one.
      const over = onAirRef.current ? playingRef.current : null
      void stopVoice().catch(() => {})
      if (over !== null) {
        pushToast(`F${over} was on the air — the voice keyer closed and stopped it`, 'info', 6000)
      }
      const slot = recordingRef.current
      if (slot !== null) {
        void cancelVoiceRecording()
          .then(() =>
            pushToast(`Recording for F${slot} discarded — the voice keyer closed`, 'info', 6000),
          )
          .catch(() =>
            pushToast(
              `Could not stop the recorder for F${slot} — it may still be running. Reopen the voice keyer.`,
              'error',
              8000,
            ),
          )
      }
    }
  }, [])

  const play = (slot: number) => {
    const m = msgs.find((x) => x.slot === slot)
    if (!m || !m.file) {
      pushToast(`F${slot} has no recording yet — record or import one`, 'info', 3000)
      return
    }
    if (recording !== null) {
      pushToast('Finish the recording first', 'info', 2500)
      return
    }
    if (keyed) {
      pushToast('Release PTT before sending a voice message', 'info', 3000)
      return
    }
    if (!txEnabled) {
      pushToast('TX is off (Monitor) — enable transmit to play a message', 'info', 3000)
      return
    }
    playingRef.current = slot
    void playVoiceMessage(slot).catch(() => {
      playingRef.current = null
      pushToast(`Could not play F${slot}`, 'error')
    })
  }

  const stop = () => {
    playingRef.current = null
    void stopVoice().catch(() => {})
  }

  const startRec = (slot: number) => {
    setRecording(slot)
    void startVoiceRecording().catch(() => {
      setRecording(null)
      pushToast('Could not start recording', 'error')
    })
  }

  const stopRec = async (slot: number) => {
    const label = msgs.find((m) => m.slot === slot)?.label ?? ''
    setRecording(null)
    const list = await withErrorToast(() => stopVoiceRecording(slot, label), 'Could not save recording')
    if (list) {
      setMsgs(list)
      pushToast(`Saved F${slot} (${label})`, 'success')
    }
  }

  const onImport = async (slot: number, file: File) => {
    const label = msgs.find((m) => m.slot === slot)?.label ?? ''
    const bytes = Array.from(new Uint8Array(await file.arrayBuffer()))
    const list = await withErrorToast(
      () => importVoiceMessage(slot, label, bytes),
      'Could not import the WAV',
    )
    if (list) {
      setMsgs(list)
      pushToast(`Imported F${slot} (${label})`, 'success')
    }
  }

  const clear = async (slot: number) => {
    const list = await withErrorToast(() => clearVoiceMessage(slot), 'Could not clear the slot')
    if (list) setMsgs(list)
  }

  // F1–F6 play their slot; Esc stops. Ignored while typing in a field.
  useEffect(() => {
    const isField = (t: EventTarget | null) =>
      t instanceof HTMLElement && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA')
    const onKey = (e: KeyboardEvent) => {
      if (isField(e.target)) return
      // Esc always stops playback (and is the only key honored mid-record).
      if (e.key === 'Escape') {
        e.preventDefault()
        stop()
        return
      }
      // Don't let an F-key key the rig while a recording is in progress.
      if (recording !== null) return
      const m = /^F([1-6])$/.exec(e.key)
      if (m) {
        e.preventDefault()
        play(Number(m[1]))
      }
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
        <span className="vk-hint">click or press F1–F6 to send · Esc stops</span>
        <span className="vk-spacer" />
        <button type="button" className="vk-stop" onClick={stop} title="Abort playback (Esc)">
          ■ Stop
        </button>
      </div>
      <p className="vk-note">
        ● records from your <strong>input device</strong> — often the rig's RX audio, not a
        mic. If so, record your message elsewhere and use Import (⤓).
      </p>
      {fdExchange && (
        <p className="vk-fd-hint">
          Field Day: record a slot with your exchange <strong>“{fdExchange}”</strong> for
          one-key sends.
        </p>
      )}
      <div className="vk-grid">
        {msgs.map((m) => {
          const isRec = recording === m.slot
          const hasFile = !!m.file
          return (
            <div key={m.slot} className={`vk-slot${hasFile ? ' has' : ''}${isRec ? ' rec' : ''}`}>
              <button
                type="button"
                className="vk-play"
                onClick={() => (hasFile ? play(m.slot) : startRec(m.slot))}
                disabled={recording !== null && !isRec}
                title={hasFile ? `Play F${m.slot} (${m.label})` : `Record F${m.slot}`}
              >
                <span className="vk-fkey">F{m.slot}</span>
                <span className="vk-label">{m.label || `Slot ${m.slot}`}</span>
                <span className="vk-state">{isRec ? '● REC' : hasFile ? '▶' : 'record'}</span>
              </button>
              <div className="vk-tools">
                {isRec ? (
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
                      title="Record from your input device"
                    >
                      ●
                    </button>
                    <button
                      type="button"
                      className="vk-tool"
                      onClick={() => fileRefs.current[m.slot]?.click()}
                      disabled={recording !== null}
                      title="Import a .wav file"
                    >
                      ⤓
                    </button>
                    <button
                      type="button"
                      className="vk-tool"
                      onClick={() => void clear(m.slot)}
                      disabled={recording !== null || !hasFile}
                      title="Clear this recording"
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
                  accept=".wav,audio/wav,audio/x-wav"
                  hidden
                  onChange={(e) => {
                    const f = e.target.files?.[0]
                    if (f) void onImport(m.slot, f)
                    e.target.value = '' // allow re-importing the same file
                  }}
                />
              </div>
            </div>
          )
        })}
      </div>
    </div>
  )
}
