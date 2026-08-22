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
// WHAT IS LIVE AND WHAT IS NOT. The OUTPUT pill is complete: it drives that trio — "Rig" is the
// pass-through OFF (you hear the radio itself, today's default and how it ships), a computer
// device is it ON. The MIC
// pill's GAIN is live too (`setMicGain`). Its SOURCE selector is deliberately not wired to a live
// microphone yet: streaming a computer mic into the transmitter is a transmit-path change and
// needs the maintainer's sign-off (kd9taw/Nexus#149). Until then the computer entries are shown
// and disabled with the reason, rather than hidden — an operator who wants this should be able to
// see that it is coming and why it is not here.
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
  onVolume: (level: number) => void
}

/** Which output is in force, read from the settings that already model it. */
export function outputSinkOf(s: Settings | null): OutputSink {
  if (!s?.monitorEnabled) return 'rig'
  return { device: s.monitorDevice ?? '' }
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

  return (
    <div className="ph-audio-pills" ref={wrapRef}>
      {/* ---- MIC ---------------------------------------------------------------------- */}
      <div className="ph-pill-wrap">
        <button
          type="button"
          // `rig` today, always: the computer-mic path is not wired (see the header).
          className="ph-audio-pill mic on-rig"
          aria-label="Microphone — click for gain, right-click to choose the source"
          title={
            micGain == null
              ? "Talking on the RIG's own mic. Right-click to see the source options."
              : "Talking on the RIG's own mic. Click for mic gain, right-click for the source."
          }
          onClick={() => setOpen(open === 'micGain' ? null : 'micGain')}
          onContextMenu={(e) => {
            e.preventDefault()
            setOpen(open === 'micSource' ? null : 'micSource')
          }}
        >
          <Mic size={13} aria-hidden="true" />
          <span className="ph-audio-pill-tag">RIG</span>
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
            <button type="button" role="menuitemradio" aria-checked className="ph-audio-item sel">
              Rig&apos;s own mic
            </button>
            {(devices?.input ?? []).map((d) => (
              <button
                key={d.name}
                type="button"
                role="menuitemradio"
                aria-checked={false}
                className="ph-audio-item"
                disabled
                title="Not yet available — streaming a computer mic into the transmitter is a transmit-path change awaiting maintainer sign-off (#149)."
              >
                {d.name}
              </button>
            ))}
            <span className="ph-audio-note">
              Computer microphones are listed but not yet selectable — see #149.
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
