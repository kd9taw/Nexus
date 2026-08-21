// ⚠️ THIS FILE IS ON THE **PARTIAL** LIST (i18n/hardcoded-strings.test.ts), and for one
// reason only: the dock's Esc/Stop macro (PSK's stop-line census) and the continuous-TX
// latch beside it are still written in English here, as is the TX pill's tooltip, which is
// the wording that states what Stop TX does to an over in flight. Those move in the
// transmit-path batch, with the stop-line sweeps re-run. Everything else is in the catalog
// under `psk.*`; the sub-mode names and their hints live in `pskModes.ts` and move with that
// module, and the baud, AFC and cursor figures are invariant tokens that stay in the code.
import { useEffect, useMemo, useRef, useState } from 'react'
import type { AppSnapshot, BandChannel, PskState } from '../types'
import { CockpitHeader } from './CockpitHeader'
import { CockpitPaneFrame } from './panes/CockpitPaneFrame'
import { PanelsMenu } from './PanelsMenu'
import { panelHost } from '../features/panelHost'
import { PSK_PANEL_IDS, type PskPanelId, type PanelLayoutApi } from '../features/panelState'
import { FrequencyControl } from './FrequencyControl'
import { Waterfall } from './Waterfall'
import {
  atuTune,
  getLicensedBandPlan,
  getPskState,
  haltTx,
  pskAfcReset,
  pskArm,
  pskAutoArm,
  pskClear,
  pskNet,
  pskSend,
  pskSetLatched,
  pskSetMode,
  pskStop,
  pskType,
  setRfPower,
  setTune,
} from '../api'
import { bandLabelForMhz } from '../band'
import { pushToast, withErrorToast } from '../toast'
import { IS_MAC, FN_KEY_HINT } from '../platform'
import { usePinnedScroll } from '../usePinnedScroll'
import { confidenceRuns } from '../transcript'
import { PSK_MODES, PSK_MODE_BY_SLUG } from '../pskModes'
import { t } from '../i18n'

interface Props {
  /** Live snapshot — may be absent while the app is still connecting; the stream
   * pane renders without it, only the header needs it. */
  snap?: AppSnapshot | null
  /** Apply a snapshot returned by a command without waiting for the poll. */
  onSnap?: (snap: AppSnapshot) => void
  /** True when PSK is the visible view. The cockpit stays MOUNTED in its
   * keep-alive host across navigation (the backend decode ring keeps
   * accumulating either way); this flag pauses the display poll while hidden
   * and drives the view-entry auto-arm (rising edge). */
  active?: boolean
  /** QSY to a band-plan channel / a typed dial (the shared App setFrequency
   * path) — a QSY, never TX. Omit ⇒ the readout is display-only. */
  onSetFrequency?: (dialMhz: number, band: string, mode: string) => void
  /** Arm/disarm TX (WSJT-X "Enable Tx") — the header pill becomes the arm control, since the
   * TopBar's Enable-Tx is hidden with the digital chrome in this view. */
  onSetTxEnabled?: (on: boolean) => void
  /** Light/dark theme — passed straight through to the waterfall colormap. */
  theme?: string
  /** Wheel sensitivity (Settings) — how much scroll one tuning step costs on the readout. */
  wheelSensitivity?: number
  /** Panel visibility record — host-owned (App) so it survives remounts. Optional: without it
   *  the decode stream shows and there's no ⊞ menu. */
  panels?: PanelLayoutApi<PskPanelId>
}

/** This cockpit's INVARIANT vocabulary — the mode's own technical tokens, gathered as
 *  constants so the i18n guard reads them as the deliberate tokens they are: the baud
 *  symbol beside the sub-mode name, the RX/TX direction plates, the polarity control's own
 *  name, and the Q-code the CQ macro is named for. */
const BAUD_SYMBOL = 'Bd'
const RX_PLATE = 'RX ▼'
const TX_PLATE = 'TX ▲'
const CQ = 'CQ'
const SEVENTY_THREE = '73'

/** Display labels for the PSK removable panels (the ⊞ Panels menu). Resolved when the menu
 *  is BUILT — a module constant would freeze the first locale loaded. */
const pskPanelLabels = (): Record<PskPanelId, string> => ({
  // Same id, cockpit-local label — the RTTY/SSTV convention (see SCOPE_PANEL_ID).
  scope: t('psk.panel.waterfall'),
  stream: t('psk.panel.stream'),
})

/** Standard casual PSK31 F-key set — mixed case on purpose (full-ASCII
 * varicode is the point over Baudot; lowercase-heavy text also runs SHORTER
 * on the wire, the frequency-ordered code table). The engine re-validates
 * every gate (TX-enable, privileges, the Keyboard section) on each send.
 *
 * `text` is what goes ON THE AIR and is invariant. The LABELS are mixed, exactly as RTTY's
 * are: `CQ` is a Q-code and `73` a number, both invariant; the other two are words. */
const MACROS: { key: string; label: () => string; text: string }[] = [
  { key: 'F1', label: () => CQ, text: 'CQ CQ CQ de {MYCALL} {MYCALL} pse k' },
  { key: 'F2', label: () => t('psk.macro.answer.label'), text: '{CALL} de {MYCALL} {MYCALL} k' },
  {
    key: 'F3',
    label: () => t('psk.macro.exchange.label'),
    text: '{CALL} de {MYCALL} ur 599 599 btu k',
  },
  { key: 'F4', label: () => SEVENTY_THREE, text: '{CALL} de {MYCALL} tnx qso 73 sk' },
]

/** "+12 Hz" (signed) AFC readout. */
function fmtAfc(hz: number): string {
  const r = Math.round(hz)
  return `${r >= 0 ? '+' : ''}${r} Hz`
}

/**
 * PSK operating cockpit (Digital rail: FT · Tempo · RTTY · PSK · SSTV · APRS) —
 * Keyboard Modes Phase 2: live RX (auto-armed decoder, click a trace to net it,
 * per-character confidence fading + the slew-limited AFC readout) and
 * operator-keyed TX following RTTY's surface exactly — macro row + compose
 * through the soundcard BPSK modulator, plus the continuous-TX latch (stay
 * keyed, idle on reversals, type into the live transmission). Every send is
 * engine-gated on TX-enable, license privileges and the Keyboard section
 * owning the rig — nothing here ever keys on its own.
 *
 * THE STOP LINE census here (all outside every ⊞-removable pane, mirrored in
 * stop-line.test.tsx's PSK case): Stop TX (header, never disabled), the dock's
 * Esc/Stop macro (`disabled={!(sending || latched)}` — live from the instant
 * the latch goes up), the TX-enable latch (header arm; `set_tx_enabled(false)`
 * arms `psk_abort` in the engine, so it is a real stop here exactly as in
 * RTTY/SSTV), and Esc (keyboard-only, census-only — bound while this is the
 * visible view). The continuous-TX ("TX") button is a SENDER, not a stop, and
 * must never be added to the sweep's stopControls — RTTY's ruling, verbatim.
 *
 * Mounted in a keep-alive host (like RTTY/SSTV) so the decoded stream keeps
 * accumulating while the operator is on another section.
 */
export function PskCockpit({ snap, onSnap, active = true, onSetFrequency, onSetTxEnabled, theme = 'dark', wheelSensitivity, panels }: Props) {
  const host = panels
    ? panelHost(panels, { menu: PSK_PANEL_IDS, side: [], main: 'stream', labels: pskPanelLabels() })
    : null
  const shown = (id: PskPanelId) => (host ? host.shown(id) : true)

  // Live decoder state — polled at 2 Hz while this is the visible view. The
  // backend ring keeps decoding while we're hidden; the first tick on
  // re-activation catches the display up.
  const [psk, setPsk] = useState<PskState | null>(null)
  useEffect(() => {
    if (!active) return
    let alive = true
    const tick = () => {
      getPskState()
        .then((s) => {
          if (alive) setPsk(s)
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

  // Arm the decoder on ENTERING the view (operator ruling 2026-08-17), so PSK
  // does not open on a dead screen the operator has to notice and fix. Rising
  // edge of `active`, not mount — the cockpit is kept alive across navigation.
  //
  // ⚠️ RX-ONLY, and the ENGINE is what guarantees that: `psk_auto_arm` only
  // upgrades from off, refuses once the operator has explicitly stopped the
  // decoder this session (the decline memory), and honours the Settings
  // opt-out. Arming the decoder cannot key — TX starts only from an explicit
  // send behind the engine's gate.
  const autoArmed = useRef(false)
  useEffect(() => {
    if (!active) {
      autoArmed.current = false
      return
    }
    if (autoArmed.current) return
    autoArmed.current = true
    void pskAutoArm()
      .then((s) => setPsk(s))
      .catch(() => {})
  }, [active])

  const armed = psk?.armed === true
  const toggleArm = () => {
    void pskArm(!armed)
      .then(setPsk)
      .catch(() => pushToast(t('psk.arm.failed'), 'error'))
  }

  // Sub-mode selector — COCKPIT STATE held by the ENGINE (the sstvModes
  // pattern for the rows; engine-owned since Phase 3 because the RX thread
  // and both TX paths must run the SAME mode). The engine refuses a switch
  // mid-transmission — surfaced as a toast, the selector snaps back on the
  // next poll. `reverse` is QPSK31's sideband polarity (LSB), same ownership.
  const modeSlug = psk?.mode ?? 'psk31'
  const reverse = psk?.reverse === true
  const mode = PSK_MODE_BY_SLUG[modeSlug] ?? PSK_MODES[0]
  const setMode = (slug: string, rev: boolean) => {
    void withErrorToast(() => pskSetMode(slug, rev), t('psk.mode.failed')).then((s) => {
      if (s) setPsk(s)
    })
  }

  // Licensed PSK watering holes (built-in band plan, WSJT-X-style) — same
  // source the RTTY cockpit uses, filtered to data privileges.
  const [plan, setPlan] = useState<BandChannel[]>([])
  useEffect(() => {
    void getLicensedBandPlan('psk').then(setPlan).catch(() => {})
  }, [])

  // RF POWER, and in PSK31 it is not a convenience control — it is the mode's one
  // operating hazard. A BPSK31 signal is a constant-envelope carrier only while it is
  // idling on reversals; through real text the envelope swings, so an overdriven rig
  // clips it into IMD splatter that reads clean on your own waterfall and dirty on
  // everyone else's. The cure is drive low enough that ALC never moves, and that means
  // the drive control has to be ON THIS SCREEN with the meters, not four clicks away in
  // the rig menu. Mirrors the rig's own read-back (Phone's pattern, same 2% deadband)
  // and never fights an in-flight drag.
  const [power, setPower] = useState(100)
  const powerDrag = useRef(false)
  useEffect(() => {
    const rb = snap?.radio.rfPower
    if (rb != null && !powerDrag.current) {
      const pct = Math.round(rb * 100)
      setPower((p) => (Math.abs(p - pct) >= 2 ? pct : p))
    }
  }, [snap?.radio.rfPower])

  // Commit a typed dial from the shared header readout. An EMPTY band label is
  // not a refusal: listening off the ham bands is first-class (the RTTY rule).
  const commitDial = (mhz: number) => {
    onSetFrequency?.(mhz, bandLabelForMhz(mhz), snap?.radio.sideband || 'USB')
  }

  // --- TX: compose + macros. Simple {MYCALL}/{CALL} substitution (the RTTY
  // shape). The ENGINE is the authority on every send — it re-checks
  // TX-enable / privileges / section ownership and returns why a send was
  // refused (surfaced as a toast).
  const [text, setText] = useState('')
  const [hisCall, setHisCall] = useState('')
  const snapRef = useRef(snap)
  snapRef.current = snap
  // `line`, not `t` — the catalog lookup is `t()` in every migrated file, so a parameter by
  // that name would shadow it here and nowhere else (the RTTY cockpit reads the same).
  const send = (line: string) => {
    if (!line.trim()) return
    const mycall = snapRef.current?.mycall?.trim() ?? ''
    if (line.includes('{MYCALL}') && !mycall) {
      pushToast(t('psk.send.noCallsign'), 'info', 3500)
      return
    }
    if (line.includes('{CALL}') && !hisCall.trim()) {
      pushToast(t('psk.send.noTheirCall'), 'info', 3000)
      return
    }
    if (snapRef.current && !snapRef.current.radio.txAllowed) {
      pushToast(t('psk.send.txLocked'), 'info', 3500)
      return
    }
    const expanded = line
      .replace(/\{MYCALL\}/g, mycall)
      .replace(/\{CALL\}/g, hisCall.trim().toUpperCase())
    void withErrorToast(() => pskSend(expanded), t('psk.send.failed')).then((s) => {
      if (s) setPsk(s)
    })
  }
  const sendTyped = () => {
    send(text)
    setText('')
  }
  const stop = () => {
    // Stop PSK (abort the over + drop the queue + unkey) AND drop any tune
    // carrier / stray PTT — a true stop-everything, like RTTY's.
    void pskStop()
      .then(setPsk)
      .catch(() => {})
    void haltTx()
  }

  const sending = psk?.sending === true

  // --- CONTINUOUS TX (the MMTTY-style latch, RTTY's semantics verbatim) -----
  // Stay keyed and type into a live transmission (the air carries idle
  // reversals between keystrokes). The engine owns all of the safety: the
  // same gate a send runs before the latch comes up, every TX gate re-checked
  // on every radio-loop tick while it is up. This component only asks.
  const latched = psk?.latched === true
  const latchedRef = useRef(latched)
  latchedRef.current = latched
  const toggleLatch = () => {
    void withErrorToast(() => pskSetLatched(!latched), t('psk.latch.failed')).then((s) => {
      if (s) setPsk(s)
    })
    // Latching off leaves the compose field holding text that has already
    // been transmitted; start the next over clean.
    if (latched) setText('')
  }
  // Stream ONE INSERTION AT A TIME, never a diff of the field's value — PSK
  // has no un-send, so the field is append-only while latched (the RTTY
  // interception, verbatim; see that cockpit for the full rationale).
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
        void withErrorToast(() => pskType(ch), t('psk.type.failed')).then((s) => {
          if (s) setPsk(s)
        })
      } else {
        // Backspace, paste, a drop, an IME commit — none can un-send what is
        // already on the air, so none may touch the field.
        ev.preventDefault()
      }
    }
    el.addEventListener('beforeinput', onBeforeInput)
    return () => el.removeEventListener('beforeinput', onBeforeInput)
  }, [latched])
  // Esc stops PSK from anywhere in the cockpit — bound only while this is the
  // VISIBLE view (the cockpit stays mounted in the keep-alive host, so an
  // unconditional listener would fire Stop TX from inside another section).
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

  const centerHz = psk?.centerHz ?? 1000
  const text_rx = psk?.text ?? ''
  // Only re-walk the ring when the transcript itself changed (the RTTY memo —
  // App re-renders unconditionally and this cockpit stays mounted).
  const runs = useMemo(() => confidenceRuns(text_rx, psk?.charConf ?? []), [text_rx, psk?.charConf])
  const streamPin = usePinnedScroll<HTMLDivElement>()

  return (
    <main className="layout single psk-cockpit">
      {snap && (
        <CockpitHeader
          snap={snap}
          onSnap={onSnap}
          txActiveLabel="▲ PSK"
          onStopTx={stop}
          onSetTxEnabled={onSetTxEnabled}
          power={{
            value: power,
            unit: '%',
            onChange: (pct: number) => {
              setPower(pct)
              void setRfPower(pct / 100)
            },
            label: t('psk.header.power.label'),
            title: t('psk.header.power.title'),
            onPointerDown: () => {
              powerDrag.current = true
            },
            onPointerUp: () => {
              powerDrag.current = false
            },
          }}
          // TUNE — a steady carrier, which in PSK is how you set the drive above: key it,
          // wind the power up until ALC just starts to move, back off. It is also a stop
          // control (it stops the carrier it started), so it is on this cockpit's stop-line
          // census and its sweep. The engine's MAX_TUNE_MS ceiling bounds it either way.
          onTune={(on) => void setTune(on).then((s) => onSnap?.(s))}
          // The RIG's own ATU. Beside Tune because it keys the transmitter too — and the
          // header renders it only when the rig actually reports a tuner. A refusal (TX off,
          // outside privileges, no tuner) comes back as the backend's reason, not silence.
          onAtuTune={() =>
            void atuTune()
              .then((s) => onSnap?.(s))
              .catch((e) => pushToast(String(e), 'error'))
          }
          modeIndicator={
            <>
              {/* The sub-mode NAME and its one-line hint come from `pskModes.ts` and move
                  with that module; here they are values. */}
              <span
                className="cw-mode-badge"
                title={`${mode.name} — ${mode.hint}`}
              >
                {mode.name} · {mode.baud} {BAUD_SYMBOL}
              </span>
              {PSK_MODES.length > 1 ? (
                <select
                  className="settings-input psk-mode-select"
                  value={modeSlug}
                  onChange={(e) => setMode(e.target.value, reverse)}
                  aria-label={t('psk.header.mode.aria')}
                >
                  {PSK_MODES.map((m) => (
                    <option key={m.slug} value={m.slug} title={m.hint}>
                      {m.name}
                    </option>
                  ))}
                </select>
              ) : null}
              {/* QPSK31 is sideband-SENSITIVE (the ±90° rotations mirror on
                  LSB, where BPSK's 0/180 don't care) — the selector-adjacent
                  Rev toggle flips the demod's AND the modulator's rotation
                  sense. Normal = USB, the Keyboard section's convention. */}
              {modeSlug === 'qpsk31' && (
                <button
                  type="button"
                  className={`rtty-arm psk-rev${reverse ? ' on' : ''}`}
                  aria-pressed={reverse}
                  onClick={() => setMode(modeSlug, !reverse)}
                  title={reverse ? t('psk.rev.on.title') : t('psk.rev.off.title')}
                >
                  {reverse ? t('psk.rev.on.label') : t('psk.rev.off.label')}
                </button>
              )}
              {sending && (
                // ⚠️ NOT MIGRATED, and it is the transmit-path deferral, not an oversight:
                // this tooltip states what Stop TX does to an over in flight. It moves with
                // the stop controls, in the transmit-path batch, with the sweeps re-run.
                <span className="rtty-tx-pill" title="PSK transmission on the air (Stop TX aborts)">
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
              <span className="cockpit-ph-pill" title={t('psk.header.band.title')}>
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
                onToggle={(id, show) => panels.setPanelState(id as PskPanelId, show ? 'docked' : 'removed')}
                onUndo={panels.undo}
                canUndo={panels.canUndo}
                onReset={panels.reset}
              />
            ) : undefined
          }
        />
      )}

      {/* THE BAND WATERFALL — ⊞-hideable, the RTTY shape (`.psk-cockpit .waterfall-wrap`
          shares its strip CSS). It hosts no stop control and no sender: the single cursor
          marks where the decoder listens AND where TX transmits (the transceive
          convention), and a click NETS THE DECODER (pskNet — engine RX state), never the
          rig. Hiding it hands its height to the stream frame, the shell's only grower. */}
      {psk && shown('scope') && (
        <Waterfall
          theme={theme}
          active={active}
          rowMs={50} // live band instrument — rig-scope cadence (the RTTY value)
          // NO `txBlanks` — RTTY's ruling: a latched over runs to the 10-minute
          // ceiling and the dark band would read as a dead waterfall.
          transmitting={snap?.radio.transmitting ?? false}
          rxOffsetHz={centerHz}
          txOffsetHz={0}
          cursors={[{ hz: centerHz, color: '#3ddc8c', label: 'RX' }]}
          hint={t('psk.waterfall.hint')}
          onTune={(hz) => void pskNet(hz).then(setPsk).catch(() => {})}
        />
      )}

      {psk?.keyerError && (
        <div className="cw-keyer-warn" role="alert">
          ⚠ {psk.keyerError}
        </div>
      )}

      {/* THE ONE CONTENT PANE — RTTY's region-less shape: a single CockpitPaneFrame as
          the shell's grower, transcript scrolling inside. Every control in the head is a
          DECODER control (arm / re-acquire / clear) — none touches PTT. */}
      {shown('stream') && (
        <CockpitPaneFrame title={t('psk.pane.stream.title')} paneId="stream">
          <div className="cw-decode psk-stream" title={t('psk.stream.title')}>
            <div className="cw-decode-head">
              <span className="cw-decode-label">{RX_PLATE}</span>
              <button
                type="button"
                className={`rtty-arm${armed ? ' on' : ''}`}
                aria-pressed={armed}
                onClick={toggleArm}
                title={armed ? t('psk.arm.on.title') : t('psk.arm.off.title')}
              >
                {armed ? t('psk.arm.on.label') : t('psk.arm.off.label')}
              </button>
              {armed && psk && (
                <span
                  className={`rtty-afc-pill${psk.signal ? ' locked' : ''}`}
                  title={psk.signal ? t('psk.carrier.on.title') : t('psk.carrier.off.title')}
                >
                  {psk.signal ? '● ' : '○ '}
                  {fmtAfc(psk.afcHz)}
                </span>
              )}
              {armed && (
                <button
                  type="button"
                  className="rtty-arm"
                  onClick={() => {
                    void pskAfcReset()
                      .then(setPsk)
                      .catch(() => {})
                  }}
                  title={t('psk.afcReset.title')}
                >
                  {t('psk.afcReset.label')}
                </button>
              )}
              <button
                className="cw-decode-clear"
                onClick={() => {
                  void pskClear()
                    .then(setPsk)
                    .catch(() => {})
                  // A wipe re-pins (the RTTY/Operate rule): the emptied pane must
                  // follow the next copy even if the operator had scrolled up.
                  streamPin.repin()
                }}
                title={t('psk.clear.title')}
              >
                {t('psk.clear.label')}
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
                  {armed ? t('psk.stream.listening') : t('psk.stream.idle')}
                </span>
              )}
            </div>
          </div>
        </CockpitPaneFrame>
      )}

      {/* TX DOCK — the macros (each a one-click transmit), the continuous-TX latch, Stop
          and the compose bar, pinned OUTSIDE any pane so nothing can scroll them out of
          reach. None of these has an id in the PSK panel vocabulary — for the Esc/Stop
          macro and the header's Stop TX / TX-arm that is THE STOP LINE (they render
          outside every ⊞-removable pane; the sweep in stop-line.test.tsx drives it); for
          the macros and the compose bar it is this cockpit's own choice.

          THE CONTINUOUS-TX (TX) BUTTON IS A SENDER, NOT A STOP — RTTY's ruling verbatim:
          clicking it off lets what was typed finish keying. The immediate cuts each also
          drop the latch: Stop TX, the Esc/Stop macro (disabled={!(sending || latched)},
          live from the instant the latch goes up), the TX-enable latch, and Esc
          (keyboard-only, census-only). A FIFTH stop reaches a latched over with no
          control pressed: the engine's per-tick gate re-check, which unkeys within one
          tick on a section change, a QSY out of privileges, a tune, or a radio handoff. */}
      <div className="cockpit-txdock">
      <div className="cw-macros psk-macros" role="group" aria-label={t('psk.macros.aria')}>
        <input
          className="settings-input rtty-hiscall"
          value={hisCall}
          onChange={(e) => setHisCall(e.target.value.toUpperCase())}
          placeholder={t('psk.hisCall.placeholder')}
          aria-label={t('psk.hisCall.aria')}
          autoComplete="off"
          spellCheck={false}
        />
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
          className={`cw-macro rtty-tx-latch psk-tx-latch${latched ? ' on' : ''}`}
          aria-pressed={latched}
          onClick={toggleLatch}
          title={
            latched
              ? 'Continuous TX ON — the transmitter stays keyed and idles on PSK reversals; type and it goes out as you type. Click to stop transmitting once what you have typed has gone out. (Stop TX or Esc cuts immediately.)'
              : 'Continuous TX — key up and stay keyed, then type into the live transmission (the classic PSK31 ragchew flow), instead of pressing Enter for every line'
          }
        >
          <span className="cw-macro-key">TX</span>
          <span className="cw-macro-label">{latched ? 'On air' : 'Continuous'}</span>
        </button>
        {/* ⚠️ NOT MIGRATED — the Esc/Stop macro is on PSK's stop-line census and is found by
            ACCESSIBLE NAME by components/stop-line.test.tsx (/^esc\s*stop$/i). Both spans and
            the tooltip move in the transmit-path batch, with that sweep re-run. */}
        <button
          type="button"
          className="cw-macro rtty-stop psk-stop"
          onClick={stop}
          // `sending || latched`, NOT `sending` alone — RTTY's rule: this is on
          // the PSK stop-line census, and it must be live from the instant the
          // latch goes up, before the first chunk is keyed.
          disabled={!(sending || latched)}
          title="Stop PSK — abort the transmission in progress, drop anything queued, unkey"
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
          onChange={(e) => {
            if (!latched) setText(e.target.value)
          }}
          onKeyDown={(e) => {
            if (e.key !== 'Enter') return
            e.preventDefault()
            if (latched) {
              // Latched, Enter is a NEW LINE on the air (CR LF — both are in
              // the varicode table), not a send: the transmitter is already up.
              setText('')
              void withErrorToast(() => pskType('\r\n'), t('psk.type.failed')).then((s) => {
                if (s) setPsk(s)
              })
            } else {
              sendTyped()
            }
          }}
          placeholder={
            latched ? t('psk.compose.placeholder.latched') : t('psk.compose.placeholder')
          }
          autoComplete="off"
          spellCheck={false}
          aria-label={t('psk.compose.aria')}
        />
        <button
          type="button"
          className="cw-send-btn"
          onClick={sendTyped}
          disabled={latched || !text.trim()}
        >
          {t('psk.compose.send.label')}
        </button>
      </div>

      {/* THE DRIVE HINT (the plan's ALC sentence): PSK31's shaped envelope IS the
          signal — a rig's ALC flattening it back to constant amplitude regenerates
          the splatter (IMD) the shaping removed. Nexus already transmits at a modest
          default drive; this line is the operator's half. No auto-ALC, on purpose. */}
      <div className="psk-drive-hint" title={t('psk.drive.title')}>
        {t('psk.drive.text')}
      </div>
      </div>
    </main>
  )
}
