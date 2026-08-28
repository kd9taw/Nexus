// THE TWO PILLS LEFT OF PTT — where the operator's audio actually goes and comes from.
//
// Operator design (2026-08-22). Two small controls in the PTT row, each answering one question
// permanently rather than in a tooltip:
//
//   · the MIC pill    — which microphone a transmission uses.  left = gain, right = source
//   · the OUTPUT pill — where received audio is heard.         left = volume, right = device
//
// WHY BESIDE PTT AND NOT ON IT. An earlier draft put the source icon inside the PTT button. A hit
// target inside PTT can swallow a key, and PTT is the control an operator reaches for by muscle
// memory — and in Phone it is part of the stop line. Beside it, the button keeps its whole surface
// and every handler it had.
//
// A WORD ABOUT THE WORD "MONITOR". The settings this pill drives are called `monitor_enabled` /
// `monitor_device` / `monitor_level`, and NONE of the operator-facing strings here use that word.
// In amateur practice MONITOR means listening to your own TRANSMITTED audio — it is what the MONI
// control on the rig does. What these settings actually do is play the RECEIVED audio out of a
// computer device (`monitor.rs`: "a live pass-through of the RX audio the decoder hears"). An
// experienced operator read the field name and reasonably assumed the opposite (2026-08-22), which
// is the whole argument for the UI saying "receive" wherever the code says "monitor".
//
// WHAT IS LIVE. Both pills are wired. The OUTPUT pill drives the monitor trio — "Rig" is the
// pass-through OFF (you hear the radio itself, today's default and how it ships), a computer
// device is it ON. The MIC pill's GAIN is `setMicGain`, and its SOURCE now writes the per-radio
// `liveMicDevice` through `setLiveMic`.
//
// THE SOURCE SELECTOR WAS GATED AND NO LONGER IS. It needed maintainer sign-off because streaming
// a computer mic into the transmitter is a transmit-path change (kd9taw/Nexus#149). That arrived,
// conditioned on #158 landing first — without it the monitor had no notion of keying, so a
// computer mic plus computer speakers plus a keying-blind monitor is an acoustic feedback loop.
// #158 is merged: the monitor goes quiet while the operator talks, and this is buildable rather
// than a howl. The condition attached to the sign-off is that this gets a BENCH PASS rather than
// a code review, because it is the transmit path.
//
// WHAT IS NOT OFFERED, AND IT IS A RULING NOT AN OVERSIGHT. The rig's own RX codec is NOT LISTED
// as a microphone at all (operator, 2026-08-22) — not listed-and-disabled, absent. Choosing it
// would transmit the received band back out. It is the mirror of `forbiddenOutput`, decided the
// other way: an output the monitor must not use is shown disabled with the reason, because the
// operator may be looking for it and deserves to know why not; a microphone that can only ever be
// wrong is not a choice worth rendering.
import { useEffect, useRef, useState } from 'react'
import { Mic, Volume2 } from 'lucide-react'
import type { AudioDevices, Settings } from '../types'

export type MicSource = 'rig' | { device: string }
export type OutputSink = 'rig' | { device: string }

interface Props {
  settings: Settings | null
  devices: AudioDevices | null
  /** Rig mic gain 0..1, or null when the radio does not report one. */
  micGain: number | null
  onMicGain: (gain: number) => void
  /** Persisted monitor trio — the OUTPUT pill's whole state. */
  onOutput: (enabled: boolean, device: string) => void
  /** Persist the live mic for the active radio. `''` = the rig's own mic. */
  onMicSource: (device: string) => void
  onVolume: (level: number) => void
}

/** Which output is in force, read from the settings that already model it. */
export function outputSinkOf(s: Settings | null): OutputSink {
  if (!s?.monitorEnabled) return 'rig'
  return { device: s.monitorDevice ?? '' }
}

/** Which microphone is in force, read from the ACTIVE radio's profile. `'rig'` is the default
 *  and what ships. A missing profile reads as `'rig'` rather than throwing: the roster can change
 *  under a cockpit that is mid-render, and the safe reading of "I do not know" is the rig's mic,
 *  which streams nothing from this computer. */
export function micSourceOf(s: Settings | null): MicSource {
  const id = s?.activeRadio
  const prof = s?.radios?.find((r) => r.id === id)
  const dev = (prof?.liveMicDevice ?? '').trim()
  return dev === '' ? 'rig' : { device: dev }
}

/** The rig's own RX codec, which must never be offered as a microphone: choosing it would put the
 *  received band back on the air. Operator ruling (2026-08-22) — NOT LISTED, not listed-and-
 *  disabled. Compare `forbiddenOutput`, which shows its rejection because the operator may be
 *  hunting for that device; here the entry could only ever be wrong, so it is not a choice. */
export function forbiddenMic(s: Settings | null, name: string): boolean {
  const rx = (s?.audioIn ?? '').trim()
  return rx !== '' && name.trim() === rx
}

/** A device the monitor must never be pointed at: the rig's own TX device would put the received
 *  band back on the air. The same rule the mic picker follows, from the other direction. */
export function forbiddenOutput(s: Settings | null, name: string): boolean {
  const tx = (s?.audioOut ?? '').trim()
  return tx !== '' && name.trim() === tx
}

export function PttAudioPills({
  settings,
  devices,
  micGain,
  onMicGain,
  onOutput,
  onMicSource,
  onVolume,
}: Props) {
  const [open, setOpen] = useState<null | 'micGain' | 'micSource' | 'vol' | 'out'>(null)
  const wrapRef = useRef<HTMLDivElement | null>(null)

  // One popover at a time, and a click anywhere else closes it. Escape too: this sits next to the
  // key, and a panel the operator cannot dismiss is a panel over the PTT button.
  useEffect(() => {
    if (!open) return
    const onDown = (e: MouseEvent) => {
      if (!wrapRef.current?.contains(e.target as Node)) setOpen(null)
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(null)
    }
    window.addEventListener('mousedown', onDown)
    window.addEventListener('keydown', onKey)
    return () => {
      window.removeEventListener('mousedown', onDown)
      window.removeEventListener('keydown', onKey)
    }
  }, [open])

  const sink = outputSinkOf(settings)
  const onComputer = sink !== 'rig'
  const vol = settings?.monitorLevel ?? 0.5
  const micSrc = micSourceOf(settings)
  const onRigMic = micSrc === 'rig'
  const micDev = onRigMic ? '' : (micSrc as { device: string }).device
  // The rig's own RX codec is filtered OUT, not shown disabled — see the header ruling.
  const micChoices = (devices?.input ?? []).filter((d) => !forbiddenMic(settings, d.name))

  return (
    <div className="ph-audio-pills" ref={wrapRef}>
      {/* ---- MIC ---------------------------------------------------------------------- */}
      <div className="ph-pill-wrap">
        <button
          type="button"
          className={`ph-audio-pill mic ${onRigMic ? 'on-rig' : 'on-computer'}`}
          aria-label="Microphone — click for gain, right-click to choose the source"
          title={
            !onRigMic
              ? `Talking on ${micDev} — this computer's microphone, streamed to the radio while`
                + ' you hold PTT. Right-click to change it.'
              : micGain == null
                ? "Talking on the RIG's own mic. Right-click to choose a different source."
                : "Talking on the RIG's own mic. Click for mic gain, right-click for the source."
          }
          onClick={() => setOpen(open === 'micGain' ? null : 'micGain')}
          onContextMenu={(e) => {
            e.preventDefault()
            setOpen(open === 'micSource' ? null : 'micSource')
          }}
        >
          <Mic size={13} aria-hidden="true" />
          <span className="ph-audio-pill-tag">{onRigMic ? 'RIG' : micDev}</span>
        </button>

        {open === 'micGain' && (
          <div className="ph-audio-pop" role="group" aria-label="Microphone gain">
            {micGain == null ? (
              <span className="ph-audio-note">This radio does not report a mic gain.</span>
            ) : (
              <>
                <label className="ph-audio-row">
                  <span>Mic gain</span>
                  <input
                    type="range"
                    min="0"
                    max="1"
                    step="0.01"
                    value={micGain}
                    aria-label="Mic gain"
                    onChange={(e) => onMicGain(Number(e.target.value))}
                  />
                  <span className="mono">{Math.round(micGain * 100)}%</span>
                </label>
                <span className="ph-audio-note">The gain on the radio, not on this computer.</span>
              </>
            )}
          </div>
        )}

        {open === 'micSource' && (
            <div className="ph-audio-pop" role="menu" aria-label="Microphone source">
              <button
                type="button"
                role="menuitemradio"
                aria-checked={onRigMic}
                className={`ph-audio-item${onRigMic ? ' sel' : ''}`}
                onClick={() => {
                  onMicSource('')
                  setOpen(null)
                }}
              >
                Rig&apos;s own mic
              </button>
              {micChoices.map((d) => (
                <button
                  key={d.name}
                  type="button"
                  role="menuitemradio"
                  aria-checked={!onRigMic && d.name === micDev}
                  className={`ph-audio-item${!onRigMic && d.name === micDev ? ' sel' : ''}`}
                  onClick={() => {
                    onMicSource(d.name)
                    setOpen(null)
                  }}
                >
                  {d.name}
                </button>
              ))}
              <span className="ph-audio-note">
                A computer mic is streamed to the radio only while you hold PTT.
              </span>
            </div>
          )}
        </div>

        {/* ---- OUTPUT ------------------------------------------------------------------- */}
      <div className="ph-pill-wrap">
        <button
          type="button"
          className={`ph-audio-pill out${onComputer ? ' on-computer' : ' on-rig'}`}
          aria-label="Audio output — click for volume, right-click to choose the device"
          title={
            onComputer
              ? 'Receive audio is going to this computer. Click for volume, right-click for the device.'
              : 'Listening on the RIG. Click for volume, right-click to send audio to this computer.'
          }
          onClick={() => setOpen(open === 'vol' ? null : 'vol')}
          onContextMenu={(e) => {
            e.preventDefault()
            setOpen(open === 'out' ? null : 'out')
          }}
        >
          <Volume2 size={13} aria-hidden="true" />
          <span className="ph-audio-pill-tag">{onComputer ? 'PC' : 'RIG'}</span>
        </button>

        {open === 'vol' && (
          <div className="ph-audio-pop" role="group" aria-label="Receive volume">
            <label className="ph-audio-row">
              <span>Volume</span>
              <input
                type="range"
                min="0"
                max="1"
                step="0.01"
                value={vol}
                aria-label="Receive volume"
                disabled={!onComputer}
                onChange={(e) => onVolume(Number(e.target.value))}
              />
              <span className="mono">{Math.round(vol * 100)}%</span>
            </label>
            <span className="ph-audio-note">
              {onComputer
                ? 'The level of the audio this computer is playing.'
                : 'Listening on the rig — its own AF gain sets the level. Right-click to send audio here instead.'}
            </span>
          </div>
        )}

        {open === 'out' && (
          <div className="ph-audio-pop" role="menu" aria-label="Audio output">
            <button
              type="button"
              role="menuitemradio"
              aria-checked={!onComputer}
              className={`ph-audio-item${onComputer ? '' : ' sel'}`}
              onClick={() => {
                onOutput(false, settings?.monitorDevice ?? '')
                setOpen(null)
              }}
            >
              Rig
            </button>
            {(devices?.output ?? []).map((d) => {
              const forbidden = forbiddenOutput(settings, d.name)
              const chosen = onComputer && (settings?.monitorDevice ?? '') === d.name
              return (
                <button
                  key={d.name}
                  type="button"
                  role="menuitemradio"
                  aria-checked={chosen}
                  className={`ph-audio-item${chosen ? ' sel' : ''}`}
                  disabled={forbidden}
                  title={
                    forbidden
                      ? "This is the rig's own transmit device — playing the received band into it would put it back on the air."
                      : undefined
                  }
                  onClick={() => {
                    onOutput(true, d.name)
                    setOpen(null)
                  }}
                >
                  {d.name}
                </button>
              )
            })}
          </div>
        )}
      </div>
    </div>
  )
}
