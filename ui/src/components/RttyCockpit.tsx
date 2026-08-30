// ⚠️ THIS FILE IS ON THE **PARTIAL** LIST (i18n/hardcoded-strings.test.ts), and for one
// reason only: four controls that STOP a transmission are still written in English here —
// the dock's Esc/Stop macro, the auto-sequencer's Abort, the `stream` pane's Auto toggle
// (its off-click aborts the QSO and unkeys) and the continuous-TX latch, plus the TX pill's
// tooltip, which is the wording that states what Stop TX does to an over in flight. Those
// move in the transmit-path batch, with the stop-line sweeps re-run. Everything else is in
// the catalog under `rtty.*`; the baud, shift, tone and AFC figures, the F-key macro TEXTS
// and the mode/direction plates are invariant tokens and stay in the code.
import { useEffect, useMemo, useRef, useState } from 'react'
import type { AppSnapshot, BandChannel, RttyState } from '../types'
import { CockpitHeader } from './CockpitHeader'
import { CockpitPaneFrame } from './panes/CockpitPaneFrame'
import { LogEntry } from './LogEntry'
import { PanelsMenu } from './PanelsMenu'
import { panelHost } from '../features/panelHost'
import { RTTY_PANEL_IDS, type RttyPanelId, type PanelLayoutApi } from '../features/panelState'
import { FrequencyControl } from './FrequencyControl'
import { Waterfall } from './Waterfall'
import {
  atuTune,
  getLicensedBandPlan,
  getRttyState,
  haltTx,
  rttyAfcReset,
  rttyArm,
  rttyAutoAbort,
  rttyAutoArm,
  rttyAutoAnswer,
  rttyAutoCq,
  rttyClear,
  rttyNet,
  rttySend,
  rttySetAuto,
  rttySetLatched,
  rttyStop,
  rttyType,
  setRfPower,
  setTune,
} from '../api'
import { bandLabelForMhz } from '../band'
import { pushToast, withErrorToast } from '../toast'
import { IS_MAC, FN_KEY_HINT } from '../platform'
import { usePinnedScroll } from '../usePinnedScroll'
import { t } from '../i18n'

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

/** This cockpit's INVARIANT vocabulary — the words that are the mode's own technical
 *  tokens rather than prose, gathered here so the i18n guard reads them as the deliberate
 *  constants they are: the mode name in the badge, the RX/TX direction plates, and the
 *  Q-code the CQ macro is named for. */
const RTTY = 'RTTY'
/** Baud · shift as the badge prints them before the decoder has answered — the HF standard
 *  pair, figures and unit both, so it is a token exactly as the live reading is. */
const DEFAULT_TONES = '45.45 · 170 Hz'
const RX_PLATE = 'RX ▼'
const TX_PLATE = 'TX ▲'
const CQ = 'CQ'
const SEVENTY_THREE = '73'

/** Display labels for the RTTY removable panels (the ⊞ Panels menu). Resolved when the
 *  menu is BUILT — a module constant would freeze the first locale loaded. */
const rttyPanelLabels = (): Record<RttyPanelId, string> => ({
  // "Waterfall", not "Scope": this strip is a band waterfall carrying the mark/space cursors,
  // and the operator's own word for it here is waterfall. The shared id is `scope` because it
  // is the same ENTRY in every cockpit; only the label is local.
  scope: t('rtty.panel.waterfall'),
  stream: t('rtty.panel.stream'),
})

/** Standard casual RTTY F-key set (599-not-5NN comes with the contest schemas).
 * Simple templates for now — {MYCALL} from the snapshot, {CALL} from the
 * their-call field; the full auto-sequencer wiring is a later wave. The engine
 * re-validates every gate (TX-enable, privileges, RTTY section) on each send.
 *
 * `text` is what goes ON THE AIR and is invariant, every character of it. The LABELS are
 * mixed: `CQ` is a Q-code and `73` a number, both invariant; the other two are words, so
 * the label is resolved when the row renders rather than at import. */
const MACROS: { key: string; label: () => string; text: string }[] = [
  { key: 'F1', label: () => CQ, text: 'CQ CQ CQ DE {MYCALL} {MYCALL} K' },
  { key: 'F2', label: () => t('rtty.macro.answer.label'), text: '{CALL} DE {MYCALL} {MYCALL} K' },
  { key: 'F3', label: () => t('rtty.macro.exchange.label'), text: '{CALL} DE {MYCALL} UR 599 599 K' },
  { key: 'F4', label: () => SEVENTY_THREE, text: '{CALL} DE {MYCALL} TU 73 SK' },
]

/** The transcript renderer is SHARED with the PSK cockpit (and every keyboard
 * mode after it) — lifted verbatim to `../transcript` in Keyboard Modes
 * Phase 1. Re-exported here under RTTY's original names so this cockpit's
 * tests and call sites read exactly as they did when it was the only keyboard
 * mode. The full design story (per-character-first, the RTTY_MAX_RUNS cap, the
 * field hang it fixed) lives with the function. */
export { confidenceRuns, TRANSCRIPT_MAX_RUNS as RTTY_MAX_RUNS } from '../transcript'
import { confidenceRuns } from '../transcript'

/** "+12 Hz" (signed) AFC readout. */
function fmtAfc(hz: number): string {
  const r = Math.round(hz)
  return `${r >= 0 ? '+' : ''}${r} Hz`
}

/** Human label for the auto-sequencer's state string (the wire value is the switch's
 *  subject and never moves; only the word the operator reads does). */
function seqLabel(s: string): string {
  switch (s) {
    case 'calling_cq':
      return t('rtty.seq.callingCq')
    case 'answering':
      return t('rtty.seq.answering')
    case 'exchange_sent':
      return t('rtty.seq.exchangeSent')
    case 'confirmed':
      return t('rtty.seq.confirmed')
    case 'done':
      return t('rtty.seq.done')
    default:
      return t('rtty.seq.idle')
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
    ? panelHost(panels, { menu: RTTY_PANEL_IDS, side: [], main: 'stream', labels: rttyPanelLabels() })
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

  // Arm the decoder on ENTERING the view. RTTY was the ONLY decode mode without this — PSK,
  // APRS and SSTV all auto-arm — and its decoder could be started in exactly one place, the
  // Arm RX button inside the `stream` pane, which is also the one pane an operator can hide.
  // That is very likely what the "RTTY is not decoding" reports were. Rising edge of `active`,
  // not mount: this cockpit is kept alive across navigation, so mount fires once a session.
  //
  // ⚠️ RX-ONLY, and the ENGINE is what guarantees it: `rtty_auto_arm` only upgrades from off,
  // refuses once the operator has explicitly stopped the decoder this session (the decline
  // memory), and honours the Settings opt-out (`rttyRxAutoArm`, default on). None of that
  // policy is duplicated here — PSK, APRS and SSTV answer the same question in the engine, and
  // a second client-side answer is a second thing to keep true. Arming a decoder cannot key:
  // TX starts only from an explicit send behind the engine's gate.
  //
  // ⚠️ THE BACKEND VERB IS WAVE 2. Until `rtty_auto_arm` lands in engine.rs/lib.rs this call
  // rejects and the `.catch` below swallows it, leaving RTTY armed by hand exactly as today.
  const autoArmed = useRef(false)
  useEffect(() => {
    if (!active) {
      autoArmed.current = false
      return
    }
    if (autoArmed.current) return
    autoArmed.current = true
    void rttyAutoArm()
      .then((s) => setRtty(s))
      .catch(() => {})
  }, [active])

  const armed = rtty?.armed === true
  const toggleArm = () => {
    void rttyArm(!armed)
      .then(setRtty)
      .catch(() => pushToast(t('rtty.arm.failed'), 'error'))
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
      .catch(() => pushToast(t('rtty.auto.failed'), 'error'))
  }
  const autoCq = () => {
    void withErrorToast(() => rttyAutoCq(), t('rtty.autoCq.failed')).then((s) => {
      if (s) setRtty(s)
    })
  }
  const autoAnswer = () => {
    if (!heardCq) return
    void withErrorToast(() => rttyAutoAnswer(heardCq), t('rtty.autoAnswer.failed')).then((s) => {
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

  // RF POWER. RTTY is a 100%-duty-cycle mode — the carrier is on for the whole over
  // with no let-up — so most rigs want it run well below their SSB rating or the finals
  // and the ALC both suffer. That is a decision the operator makes per over, at the
  // radio, which is why the control belongs in the header beside Tune rather than in a
  // settings page. Mirrors the rig's read-back and never fights an in-flight drag.
  const [power, setPower] = useState(100)
  const powerDrag = useRef(false)
  useEffect(() => {
    const rb = snap?.radio.rfPower
    if (rb != null && !powerDrag.current) {
      const pct = Math.round(rb * 100)
      setPower((p) => (Math.abs(p - pct) >= 2 ? pct : p))
    }
  }, [snap?.radio.rfPower])

  // Commit a typed dial from the shared header readout (same path as the
  // band-plan QSY); rejects out-of-plan frequencies with a toast.
  const commitDial = (mhz: number) => {
    // An EMPTY band label is not a refusal: listening off the ham bands is first-class (operator,
    // 2026-08-13), so a typed WWV/shortwave/inter-band frequency tunes there. This used to toast
    // "outside the band plan" and discard the entry.
    onSetFrequency?.(mhz, bandLabelForMhz(mhz), snap?.radio.sideband || 'USB')
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
  // `line`, not `t` — the catalog lookup is `t()` in every migrated file, so a parameter
  // by that name would shadow it here and nowhere else.
  const send = (line: string) => {
    if (!line.trim()) return
    const mycall = snapRef.current?.mycall?.trim() ?? ''
    if (line.includes('{MYCALL}') && !mycall) {
      pushToast(t('rtty.send.noCallsign'), 'info', 3500)
      return
    }
    if (line.includes('{CALL}') && !hisCall.trim()) {
      pushToast(t('rtty.send.noTheirCall'), 'info', 3000)
      return
    }
    // The engine blocks keying outside privileges anyway; surface why up front.
    if (snapRef.current && !snapRef.current.radio.txAllowed) {
      pushToast(t('rtty.send.txLocked'), 'info', 3500)
      return
    }
    const expanded = line
      .replace(/\{MYCALL\}/g, mycall)
      .replace(/\{CALL\}/g, hisCall.trim().toUpperCase())
    void withErrorToast(() => rttySend(expanded), t('rtty.send.failed')).then((s) => {
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
  // The badge's figures — invariant tokens, assembled here rather than in the JSX so the
  // i18n guard reads them as the measurements they are.
  const tones = rtty ? `${rtty.baud} · ${rtty.shiftHz} Hz` : DEFAULT_TONES

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
    void withErrorToast(() => rttySetLatched(!latched), t('rtty.latch.failed')).then((s) => {
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
        const ch = ev.data
        setText((prev) => prev + ch)
        void withErrorToast(() => rttyType(ch), t('rtty.type.failed')).then((s) => {
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
          power={{
            value: power,
            unit: '%',
            onChange: (pct: number) => {
              setPower(pct)
              void setRfPower(pct / 100)
            },
            label: t('rtty.header.power.label'),
            title: t('rtty.header.power.title'),
            onPointerDown: () => {
              powerDrag.current = true
            },
            onPointerUp: () => {
              powerDrag.current = false
            },
          }}
          // TUNE — the steady carrier you set power and load the antenna against. It is a
          // stop control (it stops the carrier it started) and is on this cockpit's
          // stop-line census and its sweep; MAX_TUNE_MS bounds it regardless.
          onTune={(on) => void setTune(on).then((s) => onSnap?.(s))}
          // The RIG's own ATU, rendered by the header only when the rig reports a tuner.
          onAtuTune={() =>
            void atuTune()
              .then((s) => onSnap?.(s))
              .catch((e) => pushToast(String(e), 'error'))
          }
          modeIndicator={
            <>
              <span className="cw-mode-badge" title={t('rtty.header.mode.title')}>
                {RTTY} {tones}
              </span>
              <span
                className="rtty-backend-pill"
                title={
                  backend === 'FSK'
                    ? t('rtty.header.backend.fsk.title')
                    : t('rtty.header.backend.afsk.title')
                }
              >
                {backend}
              </span>
              {sending && (
                // ⚠️ NOT MIGRATED, and it is the transmit-path deferral, not an oversight:
                // this tooltip states what Stop TX does to an over in flight. It moves with
                // the stop controls, in the transmit-path batch, with the sweeps re-run.
                <span className="rtty-tx-pill" title="RTTY transmission on the air (Stop TX aborts)">
                  {TX_PLATE}
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
              <span className="cockpit-ph-pill" title={t('rtty.header.band.title')}>
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

      {/* THE BAND WATERFALL, ⊞-hideable since 2026-08-16. A shell child whose render is gated,
          which leaves RTTY's census (header, waterfall, keyer-error banner, ONE pane frame, TX
          dock) intact — one kind is conditional, as `stream` already was. `.rtty-cockpit
          .waterfall-wrap` is `flex: 0 0 auto` with a 22%-of-viewport height, so hiding it hands
          that height straight to `.rtty-cockpit > .pane-frame`, the shell's only grower: the
          transcript gets taller, nothing is stranded, and there is no seam to clean up (this is
          the one scope in the tree with no Splitter).

          It hosts no stop control — the cursors and click-to-net are the decoder's tuning aid,
          and net() moves the DECODER, not the rig's key. Stop TX and the TX-enable latch stay in
          the header, the Esc/Stop macro and the sequencer's Abort in the dock (THE STOP LINE).
          With this and `stream` both unticked the cockpit still holds all four. */}
      {rtty && shown('scope') && (
        <Waterfall
          theme={theme}
          active={active}
          rowMs={50} // live band instrument — rig-scope cadence, not the FT slot default
          // NO `txBlanks` — deliberately, do not add it. An RTTY over has no fixed length and
          // a latched one runs to the 10-minute ceiling; the FT surfaces' dark band would
          // scroll solid black for all of it, which field reports (2026-08-17) read as a dead
          // waterfall. See the prop's doc in Waterfall.tsx.
          transmitting={snap?.radio.transmitting ?? false}
          rxOffsetHz={(rtty.markHz + rtty.spaceHz) / 2}
          txOffsetHz={0}
          cursors={[
            { hz: rtty.markHz, color: '#3ddc8c', label: 'M' },
            { hz: rtty.spaceHz, color: '#ffb347', label: 'S' },
          ]}
          hint={t('rtty.waterfall.hint')}
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
      <CockpitPaneFrame title={t('rtty.pane.stream.title')} paneId="stream">
      <div className="cw-decode rtty-stream" title={t('rtty.stream.title')}>
        <div className="cw-decode-head">
          <span className="cw-decode-label">{RX_PLATE}</span>
          <button
            type="button"
            className={`rtty-arm${armed ? ' on' : ''}`}
            aria-pressed={armed}
            onClick={toggleArm}
            title={armed ? t('rtty.arm.on.title') : t('rtty.arm.off.title')}
          >
            {armed ? t('rtty.arm.on.label') : t('rtty.arm.off.label')}
          </button>
          {/* ⚠️ NOT MIGRATED, and deliberately: clicking this toggle OFF is
              rttySetAuto(false) → seq.abort() + Engine::rtty_stop() — the queue is cleared
              and the rig unkeyed. It is a real stop control (a pane-resident one, which is
              why no sweep lists it), and this batch moves no control that stops a
              transmission. Its label and both tooltips move with the rest of them. */}
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
              title={rtty.afcLocked ? t('rtty.afc.locked.title') : t('rtty.afc.title')}
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
              title={t('rtty.afcReset.title')}
            >
              {t('rtty.afcReset.label')}
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
            title={t('rtty.clear.title')}
          >
            {t('rtty.clear.label')}
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
              {armed ? t('rtty.stream.listening') : t('rtty.stream.idle')}
            </span>
          )}
        </div>
      </div>
      </CockpitPaneFrame>
      )}

      {/* THE LOG STRIP — PSK's pane, verbatim, and for the same reason (#159 there): this
          cockpit rendered no LogEntry at all, so a station worked by hand with the F-key
          macros — which is how a great deal of RTTY is actually operated — had nowhere to be
          written down. The auto-sequencer logs its own completed QSOs and always did; nothing
          about that path changes here.

          ⚠️ AND IT IS THE FIELD DAY ENTRY. Field Day is all-mode, and WFD's own rules data
          keeps RTTY legal as Digital. `snap.fieldDay` (non-null exactly while the FD master
          switch is on) flips the strip to the class/section exchange and routes the contact to
          the CONTEST log — the only log that scores it, claims its section and writes its
          Cabrillo line. The SCORING CLASS is DIG (2 points); `fdSubmode` names what was
          actually on the air, so the export emits RTTY/RY rather than the FT tier the engine
          would otherwise fill in.

          NOT ⊞-REMOVABLE, and it renders as the shell's second bare pane frame. `paneId` is a
          test/pop-out handle, not a vocabulary id: RTTY's vocabulary is {stream}, so this pane
          has no menu entry and no ✕. It hosts NO stop control — the four that hold the stop
          line up (Stop TX, the dock's Esc/Stop macro, the TX-enable latch, the sequencer's
          Abort) are all in the header or the dock below, exactly where the sweep looks.

          `titled={false}` and `weight={1.5}`: PSK's ruling and PSK's MEASUREMENT, which apply
          here unchanged because the two shells share one declaration in styles.css
          (`.layout.single.rtty-cockpit, .layout.single.psk-cockpit`), floor knob included.
          The strip needs 296 px of pane height to put its Log button on screen; an even 1:1
          split gives 292 px at 1920×1080 and misses it by four pixels, and `fit="content"`
          (488 px with a recall card open) drives the SHELL onto its deficit valve in the
          ordinary case. See PskCockpit.tsx for the full table of sizes. */}
      {snap && (
        <CockpitPaneFrame title={t('rtty.pane.log.title')} paneId="log" weight={1.5}>
          <LogEntry
            snap={snap}
            mode={RTTY}
            defaultRst="599"
            exchange="terrestrial"
            titled={false}
            fieldDay={snap.fieldDay ?? null}
            fdMode="DIG"
            fdSubmode={RTTY}
          />
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
        <div className="cw-macros rtty-auto-row" role="group" aria-label={t('rtty.seq.aria')}>
          {seqState === 'idle' ? (
            <>
              <button
                type="button"
                className="cw-macro rtty-auto-cq"
                onClick={autoCq}
                title={t('rtty.autoCq.title')}
              >
                <span className="cw-macro-key">{CQ}</span>
                <span className="cw-macro-label">{t('rtty.autoCq.label')}</span>
              </button>
              <button
                type="button"
                className="cw-macro rtty-auto-answer"
                onClick={autoAnswer}
                disabled={!heardCq}
                title={
                  heardCq
                    ? t('rtty.autoAnswer.title', { call: heardCq })
                    : t('rtty.autoAnswer.none.title')
                }
              >
                <span className="cw-macro-key">{t('rtty.autoAnswer.label')}</span>
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
              {/* ⚠️ NOT MIGRATED — the sequencer's Abort is on RTTY's stop-line census
                  (features/panelState.ts). It moves in the transmit-path batch. */}
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

      <div className="cw-macros rtty-macros" role="group" aria-label={t('rtty.macros.aria')}>
        <input
          className="settings-input rtty-hiscall"
          value={hisCall}
          onChange={(e) => setHisCall(e.target.value.toUpperCase())}
          placeholder={t('rtty.hisCall.placeholder')}
          aria-label={t('rtty.hisCall.aria')}
          autoComplete="off"
          spellCheck={false}
        />
        {/* The buttons ADVERTISE their F-keys; default Mac keyboards eat bare F-keys as
            media keys, so the tooltip carries the cure there (mac QA audit). */}
        {MACROS.map((m) => (
          <button
            key={m.key}
            type="button"
            className="cw-macro"
            onClick={() => send(m.text)}
            title={`${m.text
              .replace(/\{MYCALL\}/g, snap?.mycall ?? '{MYCALL}')
              .replace(/\{CALL\}/g, hisCall.trim().toUpperCase() || '{CALL}')}${
              IS_MAC ? `\n${FN_KEY_HINT}` : ''
            }`}
          >
            <span className="cw-macro-key">{m.key}</span>
            <span className="cw-macro-label">{m.label()}</span>
          </button>
        ))}
        {/* ⚠️ NOT MIGRATED — the continuous-TX latch is a transmit-path control, and its
            tooltip is the wording that states what clicking it off does NOT do (it lets
            what was typed finish keying). Label and tooltips move with the stop line. */}
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
        {/* ⚠️ NOT MIGRATED — the Esc/Stop macro is on RTTY's stop-line census and is found
            by ACCESSIBLE NAME by components/stop-line.test.tsx (/^esc\s*stop$/i). Both spans
            and the tooltip move in the transmit-path batch, with that sweep re-run. */}
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
              void withErrorToast(() => rttyType('\r\n'), t('rtty.type.failed')).then((s) => {
                if (s) setRtty(s)
              })
            } else {
              sendTyped()
            }
          }}
          placeholder={
            latched ? t('rtty.compose.placeholder.latched') : t('rtty.compose.placeholder')
          }
          autoComplete="off"
          spellCheck={false}
          aria-label={t('rtty.compose.aria')}
        />
        <button
          type="button"
          className="cw-send-btn"
          onClick={sendTyped}
          // Latched, there is nothing to "send": characters go out as they are
          // typed and the macros type into the live transmission too.
          disabled={latched || !text.trim()}
        >
          {t('rtty.compose.send.label')}
        </button>
      </div>
      </div>
    </main>
  )
}
