import { useEffect, useMemo, useRef, useState } from 'react'
import type { AppSnapshot, BandChannel, RttyState } from '../types'
import { CockpitHeader } from './CockpitHeader'
import { CockpitPaneFrame } from './panes/CockpitPaneFrame'
import { PanelsMenu } from './PanelsMenu'
import { panelHost } from '../features/panelHost'
import { RTTY_PANEL_IDS, type RttyPanelId, type PanelLayoutApi } from '../features/panelState'
import { FrequencyControl } from './FrequencyControl'
import { Waterfall } from './Waterfall'
import {
  getLicensedBandPlan,
  getRttyState,
  haltTx,
  rttyAfcReset,
  rttyArm,
  rttyAutoAbort,
  rttyAutoAnswer,
  rttyAutoCq,
  rttyClear,
  rttyNet,
  rttySend,
  rttySetAuto,
  rttySetLatched,
  rttyStop,
  rttyType,
} from '../api'
import { bandLabelForMhz } from '../band'
import { pushToast, withErrorToast } from '../toast'
import { usePinnedScroll } from '../usePinnedScroll'

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
  /** Arm/disarm TX (WSJT-X "Enable Tx") — the header pill becomes the arm control, since the
   * TopBar's Enable-Tx is hidden with the digital chrome in this view. */
  onSetTxEnabled?: (on: boolean) => void
  /** Light/dark theme — passed straight through to the waterfall colormap.
   * Optional (defaults to dark) so non-theme-aware callers/tests don't have to thread it. */
  theme?: string
  /** Wheel sensitivity (Settings) — how much scroll one tuning step costs on the readout. */
  wheelSensitivity?: number
  /** Panel visibility record — host-owned (App) so it survives remounts. Optional: without it
   *  the decode stream shows and there's no ⊞ menu. */
  panels?: PanelLayoutApi<RttyPanelId>
}

/** Display labels for the RTTY removable panels (the ⊞ Panels menu). */
const RTTY_PANEL_LABELS: Record<RttyPanelId, string> = {
  stream: 'Decoded Text',
}

/** Standard casual RTTY F-key set (599-not-5NN comes with the contest schemas).
 * Simple templates for now — {MYCALL} from the snapshot, {CALL} from the
 * their-call field; the full auto-sequencer wiring is a later wave. The engine
 * re-validates every gate (TX-enable, privileges, RTTY section) on each send. */
const MACROS: { key: string; label: string; text: string }[] = [
  { key: 'F1', label: 'CQ', text: 'CQ CQ CQ DE {MYCALL} {MYCALL} K' },
  { key: 'F2', label: 'Answer', text: '{CALL} DE {MYCALL} {MYCALL} K' },
  { key: 'F3', label: 'Exchange', text: '{CALL} DE {MYCALL} UR 599 599 K' },
  { key: 'F4', label: '73', text: '{CALL} DE {MYCALL} TU 73 SK' },
]

/** Hard ceiling on the spans the transcript renders, whatever the copy quality
 * (the decodeHistory MAX_HISTORY precedent: a live feed gets a named cap, not a
 * hope). See `confidenceRuns`. */
export const RTTY_MAX_RUNS = 200

/** Group the decoded text into runs of quantized confidence so the transcript
 * renders a handful of spans, not one per character (the ring holds up to
 * RTTY_TEXT_CAP = 4000 chars at a 500 ms poll). Low-confidence copy renders
 * FAINT — the ATC soft metric carried per character (the D3 differentiator
 * seam). Missing confidence renders solid: never hide text we decoded.
 *
 * PER CHARACTER FIRST, always: that is the honest fade and it is what every
 * transcript an operator actually reads produces. Only when the exact grouping
 * would exceed RTTY_MAX_RUNS spans does it re-group over equal BLOCKS scored by
 * their MEAN confidence. Confidence is a continuous slicer margin, so on
 * marginal copy it crosses the 75/50/25 thresholds constantly and the exact
 * grouping ran to ~1000–1900 spans (one per character in the worst case) that
 * the front-draining ring rewrote WHOLE twice a second — the field hang. Clean
 * copy always collapsed to a single span, which is why it never showed on the
 * bench.
 *
 * The fallback is LOSSY about the fade and only about the fade — every decoded
 * character still prints, in order. Averaging can hide a short bad burst inside
 * an otherwise-good block (at a full 4000-char ring the block is 20 characters,
 * so a run of up to ~10 marginal characters can average back above a
 * threshold). That is the price of the cap, and it is charged ONLY on a
 * transcript already carrying more than RTTY_MAX_RUNS quality changes — copy so
 * broken that per-character fading is noise anyway. Anything cleaner keeps
 * character-exact fidelity. */
export function confidenceRuns(
  text: string,
  conf: number[],
): { text: string; opacity: number }[] {
  const level = (c: number) => {
    if (c >= 75) return 1
    if (c >= 50) return 0.75
    if (c >= 25) return 0.5
    return 0.3
  }
  const n = text.length
  const group = (block: number) => {
    const runs: { text: string; opacity: number }[] = []
    for (let i = 0; i < n; i += block) {
      const end = Math.min(i + block, n)
      let sum = 0
      // A missing entry is full confidence — the decoder reported no doubt.
      for (let k = i; k < end; k++) sum += conf[k] ?? 100
      const op = level(sum / (end - i))
      const last = runs[runs.length - 1]
      if (last && last.opacity === op) last.text += text.slice(i, end)
      else runs.push({ text: text.slice(i, end), opacity: op })
    }
    return runs
  }
  // block = 1 is the exact per-character grouping (a one-character mean is the
  // character). Both passes are O(n) over a 4000-char ring and the caller
  // memoizes, so the second pass only runs on copy that already blew the cap.
  const exact = group(1)
  if (exact.length <= RTTY_MAX_RUNS) return exact
  // ceil: the block count can never exceed RTTY_MAX_RUNS.
  return group(Math.max(1, Math.ceil(n / RTTY_MAX_RUNS)))
}

/** "+12 Hz" (signed) AFC readout. */
function fmtAfc(hz: number): string {
  const r = Math.round(hz)
  return `${r >= 0 ? '+' : ''}${r} Hz`
}

/** Human label for the auto-sequencer's state string. */
function seqLabel(s: string): string {
  switch (s) {
    case 'calling_cq':
      return 'Calling CQ'
    case 'answering':
      return 'Answering'
    case 'exchange_sent':
      return 'Exchange sent'
    case 'confirmed':
      return 'Confirmed'
    case 'done':
      return 'Done'
    default:
      return 'Idle'
  }
}

/**
 * RTTY operating cockpit (Digital rail: FT · Tempo · RTTY · SSTV) — live RX
 * (arm the decoder; the tempo_core::rtty demod prints with per-character
 * confidence fading + the acquire-then-freeze AFC readout) and operator-keyed
 * TX (macro row + compose through the AFSK/FSK backend the Settings pick; every
 * send is engine-gated on TX-enable, license privileges and the RTTY section
 * owning the rig — nothing here ever keys on its own). Mounted in a keep-alive
 * host (like Operate) so the decoded stream keeps accumulating while the
 * operator is on another section.
 */
export function RttyCockpit({ snap, onSnap, active = true, onSetFrequency, onSetTxEnabled, theme = 'dark', wheelSensitivity, panels }: Props) {
  // Panels (Phase 3): the waterfall, the header, the auto-seq strip, the macros and the compose
  // bar are pinned; only the decoded-text stream is removable, filling the space between them.
  // NOT "all TX chrome is pinned" — the `stream` pane hosts the Auto toggle, whose off-click is
  // a real stop (see the pane comment below). What is pinned is this cockpit's stop-line census.
  const host = panels
    ? panelHost(panels, { menu: RTTY_PANEL_IDS, side: [], main: 'stream', labels: RTTY_PANEL_LABELS })
    : null
  const shown = (id: RttyPanelId) => (host ? host.shown(id) : true)
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

  // Auto-sequencer: the pure RTTY QSO state machine wired to TX + the logbook.
  // It NEVER keys on its own — toggling Auto only builds the machine; a QSO starts
  // only when the operator clicks CQ (run) or Answer (a surfaced CQ).
  const auto = rtty?.auto === true
  const seqState = rtty?.seqState ?? 'idle'
  const peer = rtty?.peer ?? null
  const peerExchange = rtty?.peerExchange ?? []
  const heardCq = rtty?.heardCq ?? null
  const toggleAuto = () => {
    void rttySetAuto(!auto)
      .then(setRtty)
      .catch(() => pushToast('Could not switch the RTTY auto-sequencer', 'error'))
  }
  const autoCq = () => {
    void withErrorToast(() => rttyAutoCq(), 'Auto CQ refused').then((s) => {
      if (s) setRtty(s)
    })
  }
  const autoAnswer = () => {
    if (!heardCq) return
    void withErrorToast(() => rttyAutoAnswer(heardCq), 'Auto answer refused').then((s) => {
      if (s) setRtty(s)
    })
  }
  const autoAbort = () => {
    void rttyAutoAbort()
      .then(setRtty)
      .catch(() => {})
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

  // --- TX: compose + macros. Simple {MYCALL}/{CALL} substitution for now (the
  // auto-sequencer wave brings the full template layer). The ENGINE is the
  // authority on every send — it re-checks TX-enable / privileges / section
  // ownership and returns why a send was refused (surfaced as a toast).
  const [text, setText] = useState('')
  const [hisCall, setHisCall] = useState('')
  // Live snapshot ref so send() reads the CURRENT privilege state (same pattern
  // as the CW cockpit's keyboard handler).
  const snapRef = useRef(snap)
  snapRef.current = snap
  const send = (t: string) => {
    if (!t.trim()) return
    const mycall = snapRef.current?.mycall?.trim() ?? ''
    if (t.includes('{MYCALL}') && !mycall) {
      pushToast('Set your callsign in Settings before transmitting', 'info', 3500)
      return
    }
    if (t.includes('{CALL}') && !hisCall.trim()) {
      pushToast('Enter their call first (the {CALL} field)', 'info', 3000)
      return
    }
    // The engine blocks keying outside privileges anyway; surface why up front.
    if (snapRef.current && !snapRef.current.radio.txAllowed) {
      pushToast('TX locked — this frequency is outside your license privileges', 'info', 3500)
      return
    }
    const expanded = t
      .replace(/\{MYCALL\}/g, mycall)
      .replace(/\{CALL\}/g, hisCall.trim().toUpperCase())
    void withErrorToast(() => rttySend(expanded), 'RTTY send failed').then((s) => {
      if (s) setRtty(s)
    })
  }
  const sendTyped = () => {
    send(text)
    setText('')
  }
  const stop = () => {
    // Stop RTTY (abort the over + drop the queue + unkey) AND drop any tune
    // carrier / stray PTT — a true stop-everything, like the CW cockpit's Esc.
    void rttyStop()
      .then(setRtty)
      .catch(() => {})
    void haltTx()
  }

  const sending = rtty?.sending === true
  const backend = (rtty?.backend ?? 'afsk').toUpperCase()

  // --- CONTINUOUS TX (the MMTTY "TX" latch) -----------------------------------
  // Stay keyed and type into a live transmission (the air carries LTRS diddle
  // between keystrokes) instead of one keyed over per Enter. The engine owns all
  // of the safety: it runs the same gate a send runs before the latch comes up,
  // re-checks every TX gate on every radio-loop tick while it is up, and drops
  // the latch the moment one goes down. This component only asks.
  const latched = rtty?.latched === true
  const latchedRef = useRef(latched)
  latchedRef.current = latched
  const toggleLatch = () => {
    void withErrorToast(() => rttySetLatched(!latched), 'Continuous TX refused').then((s) => {
      if (s) setRtty(s)
    })
    // Latching off leaves the compose field holding text that has already been
    // transmitted; start the next over clean.
    if (latched) setText('')
  }
  // Stream ONE INSERTION AT A TIME, and never a diff of the field's value. A diff
  // cannot tell a paste from a caret move from a backspace, and RTTY HAS NO
  // UN-SEND: a wrong diff is a wrong transmission. So while latched the field is
  // append-only — the insertion is cancelled, appended by us, and handed to the
  // engine, which keeps the field and what went on the air identical by
  // construction rather than by agreement.
  const composeRef = useRef<HTMLInputElement | null>(null)
  useEffect(() => {
    const el = composeRef.current
    if (!el || !latched) return
    const onBeforeInput = (e: Event) => {
      const ev = e as InputEvent
      if (!latchedRef.current) return
      if (ev.inputType === 'insertText' && ev.data) {
        ev.preventDefault()
        const t = ev.data
        setText((prev) => prev + t)
        void withErrorToast(() => rttyType(t), 'RTTY typing refused').then((s) => {
          if (s) setRtty(s)
        })
      } else {
        // Backspace, paste, a drop, an IME commit — none of them can un-send
        // what is already on the air, so none of them may touch the field.
        ev.preventDefault()
      }
    }
    // A native listener, not React's onBeforeInput: the synthetic event does not
    // carry a reliable `inputType`, and `inputType` is the whole discrimination.
    el.addEventListener('beforeinput', onBeforeInput)
    return () => el.removeEventListener('beforeinput', onBeforeInput)
  }, [latched])
  // Esc stops RTTY from anywhere in the cockpit. RTTY had no keyboard binding at
  // all (the Stop macro's "Esc" glyph was decoration); a latched transmitter is
  // what makes that gap matter. Bound only while this is the VISIBLE view — the
  // cockpit stays mounted in the keep-alive host, so an unconditional listener
  // would fire Stop TX from inside another section.
  useEffect(() => {
    if (!active) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        stop()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active])

  const text_rx = rtty?.text ?? ''
  // Only re-walk the ring when the transcript itself changed. App re-renders
  // unconditionally every 400 ms and this cockpit stays MOUNTED in the
  // keep-alive host, so without the memo the whole 4000-char ring was regrouped
  // (and every span reconciled) on renders that carried no new copy at all.
  const runs = useMemo(() => confidenceRuns(text_rx, rtty?.charConf ?? []), [text_rx, rtty?.charConf])
  // Stream transcript: bottom-pinned via the shared discipline. The old
  // unconditional snap on every poll that changed the text made mid-QSO
  // scroll-back impossible — the pane reset to the bottom before the operator
  // could re-read a callsign. Pinned follows new copy; scrolled-up reads.
  const streamPin = usePinnedScroll<HTMLDivElement>()

  return (
    <main className="layout single rtty-cockpit">
      {snap && (
        <CockpitHeader
          snap={snap}
          onSnap={onSnap}
          txActiveLabel="▲ RTTY"
          onStopTx={stop}
          onSetTxEnabled={onSetTxEnabled}
          modeIndicator={
            <>
              <span
                className="cw-mode-badge"
                title="RTTY — Baudot/ITA2 at the configured baud + shift (45.45 / 170 Hz is the HF standard; change it in Settings → RTTY)"
              >
                RTTY {rtty ? `${rtty.baud} · ${rtty.shiftHz} Hz` : '45.45 · 170 Hz'}
              </span>
              <span
                className="rtty-backend-pill"
                title={
                  backend === 'FSK'
                    ? 'True FSK — data bits on the serial keyline, rig in RTTY mode (its narrow RTTY filters work). Change the backend in Settings → RTTY.'
                    : 'AFSK — soundcard tones through the rig in LSB (soundcard-clocked, the robust default). Change the backend in Settings → RTTY.'
                }
              >
                {backend}
              </span>
              {sending && (
                <span className="rtty-tx-pill" title="RTTY transmission on the air (Stop TX aborts)">
                  TX ▲
                </span>
              )}
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
          digitTune={onSetFrequency != null}
          wheelSensitivity={wheelSensitivity}
          actions={
            host && panels ? (
              <PanelsMenu
                items={host.menuItems}
                onToggle={(id, show) => panels.setPanelState(id as RttyPanelId, show ? 'docked' : 'removed')}
                onUndo={panels.undo}
                canUndo={panels.canUndo}
                onReset={panels.reset}
              />
            ) : undefined
          }
        />
      )}

      {rtty && (
        <Waterfall
          theme={theme}
          active={active}
          rowMs={50} // live band instrument — rig-scope cadence, not the FT slot default
          transmitting={snap?.radio.transmitting ?? false}
          rxOffsetHz={(rtty.markHz + rtty.spaceHz) / 2}
          txOffsetHz={0}
          cursors={[
            { hz: rtty.markHz, color: '#3ddc8c', label: 'M' },
            { hz: rtty.spaceHz, color: '#ffb347', label: 'S' },
          ]}
          hint="click nets the decoder"
          onTune={(hz) => void rttyNet(hz).then(setRtty).catch(() => {})}
        />
      )}

      {rtty?.keyerError && (
        <div className="cw-keyer-warn" role="alert">
          ⚠ {rtty.keyerError}
        </div>
      )}

      {/* THE ONE CONTENT PANE. RTTY adopts CockpitPaneFrame (per-pane scroll, the shipped
          thin scrollbar) but deliberately NO .cockpit-panes region: with a single content
          block every column template leaves a track empty, which is the dead space this
          rebuild deletes rather than relocates. The frame is the shell's grower
          (`.rtty-cockpit > .pane-frame`, styles.css) and the transcript scrolls inside it.

          THIS PANE HOSTS A STOP CONTROL, and that is allowed. The "Auto on" toggle below,
          clicked off, is rttySetAuto(false) → seq.abort() + Engine::rtty_stop(): the queue is
          cleared and rtty_abort + slot_tx_abort unkey the rig. Hiding `stream` takes it away —
          fine under THE STOP LINE (features/panelState.ts), because Stop TX and the TX-enable
          latch are in the header and the Esc/Stop macro and the sequencer's Abort are in the
          dock, none of them with a ⊞ id. A pane's own stop is a convenience; those four are
          what hold the guarantee up — and while an over is actually keying outside an auto
          sequence, THREE of the four are live: Stop TX (never disabled), the Esc/Stop macro
          (disabled={!(sending || latched)}, so enabled exactly then — and, since continuous TX
          landed, from the instant the TX latch goes up rather than from the first keyed chunk)
          and the latch (a button, because
          radio.transmitting is the slot-TX indicator and is false here). The sequencer's Abort
          is not rendered then. This pane is the second of exactly two like it in the app
          (Phone's voice keyer is the other) and the reason the FOURTH wording of the rule was
          falsified. Its hide ENDS nothing — unmounting calls no wire — so it correctly carries
          no ⊞ note, and it is not a sender: Arm RX is RX-only and Auto ON never keys by
          itself. Do NOT add the Auto toggle to stop-line.test.tsx's RTTY stopControls; that
          would demand this cockpit's only ⊞ entry be unhideable. */}
      {shown('stream') && (
      <CockpitPaneFrame title="Decoded text" paneId="stream">
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
          <button
            type="button"
            className={`rtty-arm${auto ? ' on' : ''}`}
            aria-pressed={auto}
            onClick={toggleAuto}
            title={
              auto
                ? 'Auto-sequencer ON — the RTTY QSO machine runs the exchange for you. It NEVER keys on its own; you start each QSO with CQ or Answer below. Click to turn off.'
                : 'Auto — arm the RTTY auto-sequencer. It runs the QSO after you click CQ (run) or answer a heard CQ (search & pounce); it never transmits on its own.'
            }
          >
            {auto ? 'Auto on' : 'Auto'}
          </button>
          {armed && rtty && (
            <span
              className={`rtty-afc-pill${rtty.afcLocked ? ' locked' : ''}`}
              title={
                rtty.afcLocked
                  ? 'AFC locked — acquired the mark/space pair and frozen on it (offset from the nominal tones)'
                  : 'AFC offset from the nominal mark/space tone pair — locks once a signal is acquired'
              }
            >
              {fmtAfc(rtty.afcHz)}
              {rtty.afcLocked ? ' 🔒' : ''}
            </span>
          )}
          {armed && (
            <button
              type="button"
              className="rtty-arm"
              onClick={() => {
                void rttyAfcReset()
                  .then(setRtty)
                  .catch(() => {})
              }}
              title="Re-acquire AFC — drop and rebuild the demodulator (use when it froze on the wrong signal)"
            >
              Re-tune
            </button>
          )}
          <button
            className="cw-decode-clear"
            onClick={() => {
              void rttyClear()
                .then(setRtty)
                .catch(() => {})
              // A wipe re-pins (same as Operate's Erase): the emptied pane must
              // follow the next copy even if the operator had scrolled up.
              streamPin.repin()
            }}
            title="Clear the decoded transcript"
          >
            Clear
          </button>
        </div>
        <div className="cw-decode-text" ref={streamPin.ref} onScroll={streamPin.onScroll}>
          {text_rx ? (
            runs.map((run, i) => (
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
      </CockpitPaneFrame>
      )}

      {/* TX DOCK — the auto-sequencer row, the macros (each one a one-click transmit), the
          continuous-TX latch, Stop and the compose bar, pinned OUTSIDE any pane so nothing
          can scroll them out of reach. None of these has an id in the RTTY panel vocabulary
          ('stream' is the only entry). For the sequencer's Abort, the Esc/Stop macro and the
          header's Stop TX / TX-arm that is THE STOP LINE — those four render outside every
          ⊞-removable pane, so unticking 'stream' (which takes its own Auto-toggle stop with
          it) cannot take any of them away. FOUR IS THE LIST, NOT THE LIVE COUNT: mid-over
          outside an auto sequence three are operable (Stop TX, the Esc/Stop macro, the latch)
          and the Abort below is not on screen at all; inside an auto sequence all four are.
          For the macros and the compose bar it is this cockpit's own choice: the rule is
          indifferent to senders.

          THE CONTINUOUS-TX (TX) BUTTON IS A SENDER, NOT A STOP, and must not be added to
          stop-line.test.tsx's RTTY stopControls. Clicking it off stops ACCEPTING characters
          and lets what was already typed finish keying — a mode toggle, deliberately not an
          immediate cut. The immediate cuts are unchanged and each of them also drops the
          latch: Stop TX, the Esc/Stop macro (now `disabled={!(sending || latched)}` so it is
          live from the instant the latch goes up, not from the first keyed chunk), the
          TX-enable latch, and Esc — which this cockpit now actually binds (it had no keyboard
          handler at all; the "Esc" glyph on the Stop macro was decoration). Esc is
          keyboard-only, so like Phone's Space and CW's Esc it is census-only and outside both
          sweeps by construction. A FIFTH stop reaches a latched over that reaches no other
          over: the engine's per-tick gate re-check, which unkeys within one tick on a section
          change, a QSY out of privileges, a tune, or a radio handoff — none of which is a
          control the operator pressed. */}
      <div className="cockpit-txdock">
      {auto && (
        <div className="cw-macros rtty-auto-row" role="group" aria-label="RTTY auto-sequencer">
          {seqState === 'idle' ? (
            <>
              <button
                type="button"
                className="cw-macro rtty-auto-cq"
                onClick={autoCq}
                title="Call CQ and auto-run the QSO — the engine keys only after you click, never on its own"
              >
                <span className="cw-macro-key">CQ</span>
                <span className="cw-macro-label">Auto call</span>
              </button>
              <button
                type="button"
                className="cw-macro rtty-auto-answer"
                onClick={autoAnswer}
                disabled={!heardCq}
                title={
                  heardCq
                    ? `Answer ${heardCq} and auto-run the exchange (search & pounce)`
                    : 'No CQ heard yet — Answer lights up when the decoder surfaces one'
                }
              >
                <span className="cw-macro-key">Answer</span>
                <span className="cw-macro-label">{heardCq ?? '—'}</span>
              </button>
            </>
          ) : (
            <>
              <span className="rtty-auto-status" aria-live="polite">
                <span className="rtty-auto-state">{seqLabel(seqState)}</span>
                {peer && <span className="rtty-auto-peer">{peer}</span>}
                {peerExchange.length > 0 && (
                  <span className="rtty-auto-exch">
                    {peerExchange.map(([k, v]) => `${k} ${v}`).join('  ')}
                  </span>
                )}
              </span>
              <button
                type="button"
                className="cw-macro rtty-auto-stop"
                onClick={autoAbort}
                title="Abort the auto QSO — stop the sequencer, drop the queue, unkey"
              >
                <span className="cw-macro-key">Esc</span>
                <span className="cw-macro-label">Abort</span>
              </button>
            </>
          )}
        </div>
      )}

      <div className="cw-macros rtty-macros" role="group" aria-label="RTTY macros">
        <input
          className="settings-input rtty-hiscall"
          value={hisCall}
          onChange={(e) => setHisCall(e.target.value.toUpperCase())}
          placeholder="Their call…"
          aria-label="Worked station callsign (the {CALL} macro token)"
          autoComplete="off"
          spellCheck={false}
        />
        {MACROS.map((m) => (
          <button
            key={m.key}
            type="button"
            className="cw-macro"
            onClick={() => send(m.text)}
            title={m.text
              .replace(/\{MYCALL\}/g, snap?.mycall ?? '{MYCALL}')
              .replace(/\{CALL\}/g, hisCall.trim().toUpperCase() || '{CALL}')}
          >
            <span className="cw-macro-key">{m.key}</span>
            <span className="cw-macro-label">{m.label}</span>
          </button>
        ))}
        <button
          type="button"
          className={`cw-macro rtty-tx-latch${latched ? ' on' : ''}`}
          aria-pressed={latched}
          onClick={toggleLatch}
          title={
            latched
              ? 'Continuous TX ON — the transmitter stays keyed and idles on diddle; type and it goes out as you type. Click to stop transmitting once what you have typed has gone out. (Stop TX or Esc cuts immediately.)'
              : 'Continuous TX — key up and stay keyed, then type into the live transmission (MMTTY-style), instead of pressing Enter for every line'
          }
        >
          <span className="cw-macro-key">TX</span>
          <span className="cw-macro-label">{latched ? 'On air' : 'Continuous'}</span>
        </button>
        <button
          type="button"
          className="cw-macro rtty-stop"
          onClick={stop}
          // `sending || latched`, NOT `sending` alone. `sending` is stamped by the
          // radio loop from the audio actually in flight, so it is false for the
          // tick between the latch going up and the first chunk being keyed — and
          // false for good if the FSK keyline never opens. This control is on
          // RTTY's stop-line census; a census stop control that is mounted and
          // disabled is the same loss as one that is gone.
          disabled={!(sending || latched)}
          title="Stop RTTY — abort the transmission in progress, drop anything queued, unkey"
        >
          <span className="cw-macro-key">Esc</span>
          <span className="cw-macro-label">Stop</span>
        </button>
      </div>

      <div className="cw-send">
        <input
          ref={composeRef}
          className="settings-input cw-type"
          value={text}
          // Unlatched this is an ordinary field: type a line, press Enter, one
          // keyed over. Latched, every insertion is intercepted in `beforeinput`
          // above and this never fires — but it stays wired so nothing depends on
          // the interception to keep the field consistent.
          onChange={(e) => {
            if (!latched) setText(e.target.value)
          }}
          onKeyDown={(e) => {
            if (e.key !== 'Enter') return
            e.preventDefault()
            if (latched) {
              // Latched, Enter is a NEW LINE on the air (CR LF — both live in
              // both ITA2 planes), not a send: the transmitter is already up.
              setText('')
              void withErrorToast(() => rttyType('\r\n'), 'RTTY typing refused').then((s) => {
                if (s) setRtty(s)
              })
            } else {
              sendTyped()
            }
          }}
          placeholder={latched ? 'Typing on the air…' : 'Type RTTY to send… (Enter)'}
          autoComplete="off"
          spellCheck={false}
          aria-label="RTTY compose"
        />
        <button
          type="button"
          className="cw-send-btn"
          onClick={sendTyped}
          // Latched, there is nothing to "send": characters go out as they are
          // typed and the macros type into the live transmission too.
          disabled={latched || !text.trim()}
        >
          Send
        </button>
      </div>
      </div>
    </main>
  )
}
