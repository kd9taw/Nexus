import { useEffect, useRef, useState } from 'react'
import type { AppSnapshot, BandChannel, RttyState } from '../types'
import { CockpitHeader } from './CockpitHeader'
import { FrequencyControl } from './FrequencyControl'
import { getLicensedBandPlan, getRttyState, rttyArm } from '../api'
import { bandLabelForMhz } from '../band'
import { pushToast } from '../toast'

interface Props {
  /** Live snapshot — may be absent while the app is still connecting; the shell
   * (stream / macros / compose) renders without it, only the header needs it. */
  snap?: AppSnapshot | null
  /** Apply a snapshot returned by a command without waiting for the poll. */
  onSnap?: (snap: AppSnapshot) => void
  /** True when RTTY is the visible view. The cockpit stays MOUNTED in its
   * keep-alive host across navigation (the backend decode ring keeps
   * accumulating either way); this flag pauses the display poll while hidden —
   * the same gate the FT8 cockpit uses for its render loop. */
  active?: boolean
  /** QSY to a band-plan channel (the shared App setFrequency path). */
  onSetFrequency?: (dialMhz: number, band: string, mode: string) => void
}

/** Standard casual RTTY F-key set (599-not-5NN comes with the contest schemas).
 * Disabled placeholders — TX is a later, safety-reviewed wave (AFSK/FSK keying
 * + the auto-sequencer); the tooltips show the template each key will send. */
const MACROS: { key: string; label: string; text: string }[] = [
  { key: 'F1', label: 'CQ', text: 'CQ CQ CQ DE {MYCALL} {MYCALL} K' },
  { key: 'F2', label: 'Answer', text: '{CALL} DE {MYCALL} {MYCALL} K' },
  { key: 'F3', label: 'Exchange', text: '{CALL} DE {MYCALL} UR 599 599 {NAME} {NAME} K' },
  { key: 'F4', label: '73', text: '{CALL} DE {MYCALL} TU 73 SK' },
]

/** Group the decoded text into runs of equal quantized confidence so the
 * transcript renders a handful of spans, not one per character (the ring holds
 * up to ~4000 chars at a 500 ms poll). Low-confidence copy renders FAINT — the
 * ATC soft metric carried per character (the D3 differentiator seam). Missing
 * confidence renders solid: never hide text we decoded. */
export function confidenceRuns(
  text: string,
  conf: number[],
): { text: string; opacity: number }[] {
  const level = (i: number) => {
    const c = conf[i]
    if (c == null || c >= 75) return 1
    if (c >= 50) return 0.75
    if (c >= 25) return 0.5
    return 0.3
  }
  const runs: { text: string; opacity: number }[] = []
  for (let i = 0; i < text.length; i++) {
    const op = level(i)
    const last = runs[runs.length - 1]
    if (last && last.opacity === op) last.text += text[i]
    else runs.push({ text: text[i], opacity: op })
  }
  return runs
}

/** "+12 Hz" (signed) AFC readout. */
function fmtAfc(hz: number): string {
  const r = Math.round(hz)
  return `${r >= 0 ? '+' : ''}${r} Hz`
}

/**
 * RTTY operating cockpit (Digital rail: FT · Tempo · RTTY · SSTV) — LIVE RX.
 * Arm the decoder and the tempo_core::rtty demod prints here with per-character
 * confidence fading + the acquire-then-freeze AFC readout. TX (macros/compose,
 * the AFSK/FSK keying paths and the FSK-vs-AFSK rig-mode policy) is a later,
 * safety-reviewed wave — those controls stay disabled. Mounted in a keep-alive
 * host (like Operate) so the decoded stream keeps accumulating while the
 * operator is on another section.
 */
export function RttyCockpit({ snap, onSnap, active = true, onSetFrequency }: Props) {
  // Live decoder state — polled at 2 Hz while this is the visible view. The
  // backend ring keeps decoding while we're hidden; the first tick on
  // re-activation catches the display up.
  const [rtty, setRtty] = useState<RttyState | null>(null)
  useEffect(() => {
    if (!active) return
    let alive = true
    const tick = () => {
      getRttyState()
        .then((s) => {
          if (alive) setRtty(s)
        })
        .catch(() => {})
    }
    tick()
    const id = window.setInterval(tick, 500)
    return () => {
      alive = false
      window.clearInterval(id)
    }
  }, [active])

  const armed = rtty?.armed === true
  const toggleArm = () => {
    void rttyArm(!armed)
      .then(setRtty)
      .catch(() => pushToast('Could not switch the RTTY decoder', 'error'))
  }

  // Licensed RTTY watering holes (built-in band plan, WSJT-X-style) — same
  // source the CW/Phone BandPicker uses, filtered to digital privileges.
  const [plan, setPlan] = useState<BandChannel[]>([])
  useEffect(() => {
    void getLicensedBandPlan('rtty').then(setPlan).catch(() => {})
  }, [])

  // Commit a typed dial from the shared header readout (same path as the
  // band-plan QSY); rejects out-of-plan frequencies with a toast.
  const commitDial = (mhz: number) => {
    const band = bandLabelForMhz(mhz)
    if (!band) {
      pushToast(`${mhz.toFixed(4)} MHz is outside the band plan`, 'error', 3000)
      return
    }
    onSetFrequency?.(mhz, band, snap?.radio.sideband || 'USB')
  }

  const text = rtty?.text ?? ''
  const streamRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    // Autoscroll: newest text stays in view (same behavior as the CW transcript).
    const el = streamRef.current
    if (el) el.scrollTop = el.scrollHeight
  }, [text])

  return (
    <main className="layout single rtty-cockpit">
      {snap && (
        <CockpitHeader
          snap={snap}
          onSnap={onSnap}
          modeIndicator={
            <>
              <span
                className="cw-mode-badge"
                title="RTTY RX — 45.45 baud Baudot, 170 Hz shift (the HF standard). 75 baud + wide shifts come in a later build."
              >
                RTTY 45.45 · 170 Hz
              </span>
              <span
                className="rtty-backend-pill"
                title="Keying backend — AFSK (soundcard tones through the rig, the robust default) vs true FSK (serial keyline). The picker lands with the TX wiring."
              >
                AFSK
              </span>
            </>
          }
          bandControl={
            onSetFrequency ? (
              <FrequencyControl
                channels={plan}
                dialMhz={snap.radio.dialMhz}
                band={snap.radio.band}
                mode={snap.radio.sideband}
                variant="compact"
                showReadout={false}
                showModeToggle={false}
                onSet={onSetFrequency}
              />
            ) : (
              <span className="cockpit-ph-pill" title="Showing the rig's current band">
                {bandLabelForMhz(snap.radio.dialMhz) || '— band —'}
              </span>
            )
          }
          onCommitDial={onSetFrequency ? commitDial : undefined}
        />
      )}

      <div
        className="cw-decode rtty-stream"
        title="Decoded RTTY text — faint characters are low-confidence copy (the demodulator's soft metric)"
      >
        <div className="cw-decode-head">
          <span className="cw-decode-label">RX ▼</span>
          <button
            type="button"
            className={`rtty-arm${armed ? ' on' : ''}`}
            aria-pressed={armed}
            onClick={toggleArm}
            title={
              armed
                ? 'RX armed — decoding the receive audio (RX only, never keys the rig). Click to disarm.'
                : 'Arm RX — start decoding RTTY from the receive audio (RX only, never keys the rig)'
            }
          >
            {armed ? 'RX armed' : 'Arm RX'}
          </button>
          {armed && rtty && (
            <span
              className={`rtty-afc-pill${rtty.afcLocked ? ' locked' : ''}`}
              title={
                rtty.afcLocked
                  ? 'AFC locked — acquired the mark/space pair and frozen on it (offset from the nominal 2125/2295 Hz tones)'
                  : 'AFC offset from the nominal 2125/2295 Hz tone pair — locks once a signal is acquired'
              }
            >
              {fmtAfc(rtty.afcHz)}
              {rtty.afcLocked ? ' 🔒' : ''}
            </span>
          )}
        </div>
        <div className="cw-decode-text" ref={streamRef}>
          {text ? (
            confidenceRuns(text, rtty?.charConf ?? []).map((run, i) => (
              <span key={i} style={run.opacity < 1 ? { opacity: run.opacity } : undefined}>
                {run.text}
              </span>
            ))
          ) : (
            <span className="cw-decode-idle">
              {armed ? 'listening…' : 'Arm RX to decode RTTY from the receive audio'}
            </span>
          )}
        </div>
      </div>

      <div className="cw-macros rtty-macros" role="group" aria-label="RTTY macros">
        {MACROS.map((m) => (
          <button
            key={m.key}
            type="button"
            className="cw-macro"
            disabled
            title={`${m.text} — TX comes in a later, safety-reviewed build`}
          >
            <span className="cw-macro-key">{m.key}</span>
            <span className="cw-macro-label">{m.label}</span>
          </button>
        ))}
      </div>

      <div className="cw-send">
        <input
          className="settings-input cw-type"
          disabled
          placeholder="Type RTTY to send… (TX comes in a later, safety-reviewed build)"
          autoComplete="off"
          spellCheck={false}
          aria-label="RTTY compose (disabled — TX not wired yet)"
        />
        <button
          type="button"
          className="cw-send-btn"
          disabled
          title="TX comes in a later, safety-reviewed build"
        >
          Send
        </button>
      </div>
    </main>
  )
}
