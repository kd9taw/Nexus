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
  pskStop,
  pskType,
} from '../api'
import { bandLabelForMhz } from '../band'
import { pushToast, withErrorToast } from '../toast'
import { IS_MAC, FN_KEY_HINT } from '../platform'
import { usePinnedScroll } from '../usePinnedScroll'
import { confidenceRuns } from '../transcript'
import { PSK_MODES, PSK_MODE_BY_SLUG } from '../pskModes'

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

/** Display labels for the PSK removable panels (the ⊞ Panels menu). */
const PSK_PANEL_LABELS: Record<PskPanelId, string> = {
  // Same id, cockpit-local label — the RTTY/SSTV convention (see SCOPE_PANEL_ID).
  scope: 'Waterfall',
  stream: 'Decoded Text',
}

/** Standard casual PSK31 F-key set — mixed case on purpose (full-ASCII
 * varicode is the point over Baudot; lowercase-heavy text also runs SHORTER
 * on the wire, the frequency-ordered code table). The engine re-validates
 * every gate (TX-enable, privileges, the Keyboard section) on each send. */
const MACROS: { key: string; label: string; text: string }[] = [
  { key: 'F1', label: 'CQ', text: 'CQ CQ CQ de {MYCALL} {MYCALL} pse k' },
  { key: 'F2', label: 'Answer', text: '{CALL} de {MYCALL} {MYCALL} k' },
  { key: 'F3', label: 'Exchange', text: '{CALL} de {MYCALL} ur 599 599 btu k' },
  { key: 'F4', label: '73', text: '{CALL} de {MYCALL} tnx qso 73 sk' },
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
    ? panelHost(panels, { menu: PSK_PANEL_IDS, side: [], main: 'stream', labels: PSK_PANEL_LABELS })
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
      .catch(() => pushToast('Could not switch the PSK decoder', 'error'))
  }

  // Sub-mode selector — COCKPIT STATE, not settings (the sstvModes pattern).
  // One entry today; QPSK31 slots in at Phase 3 as a second row in pskModes.ts.
  const [modeSlug, setModeSlug] = useState('psk31')
  const mode = PSK_MODE_BY_SLUG[modeSlug] ?? PSK_MODES[0]

  // Licensed PSK watering holes (built-in band plan, WSJT-X-style) — same
  // source the RTTY cockpit uses, filtered to data privileges.
  const [plan, setPlan] = useState<BandChannel[]>([])
  useEffect(() => {
    void getLicensedBandPlan('psk').then(setPlan).catch(() => {})
  }, [])

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
    if (snapRef.current && !snapRef.current.radio.txAllowed) {
      pushToast('TX locked — this frequency is outside your license privileges', 'info', 3500)
      return
    }
    const expanded = t
      .replace(/\{MYCALL\}/g, mycall)
      .replace(/\{CALL\}/g, hisCall.trim().toUpperCase())
    void withErrorToast(() => pskSend(expanded), 'PSK send failed').then((s) => {
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
    void withErrorToast(() => pskSetLatched(!latched), 'Continuous TX refused').then((s) => {
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
        const t = ev.data
        setText((prev) => prev + t)
        void withErrorToast(() => pskType(t), 'PSK typing refused').then((s) => {
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
          modeIndicator={
            <>
              <span
                className="cw-mode-badge"
                title={`${mode.name} — ${mode.hint}`}
              >
                {mode.name} · {mode.baud} Bd
              </span>
              {PSK_MODES.length > 1 ? (
                <select
                  className="settings-input psk-mode-select"
                  value={modeSlug}
                  onChange={(e) => setModeSlug(e.target.value)}
                  aria-label="PSK sub-mode"
                >
                  {PSK_MODES.map((m) => (
                    <option key={m.slug} value={m.slug} title={m.hint}>
                      {m.name}
                    </option>
                  ))}
                </select>
              ) : null}
              {sending && (
                <span className="rtty-tx-pill" title="PSK transmission on the air (Stop TX aborts)">
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
          hint="click nets the decoder"
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
        <CockpitPaneFrame title="Decoded text" paneId="stream">
          <div
            className="cw-decode psk-stream"
            title="Decoded PSK31 text — faint characters are low-confidence copy (the demodulator's phase-margin metric)"
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
                    ? 'RX armed — decoding the receive audio (RX only, never keys the rig). Click to stop; stopping is remembered for this session.'
                    : 'Arm RX — start decoding PSK31 from the receive audio (RX only, never keys the rig)'
                }
              >
                {armed ? 'RX armed' : 'Arm RX'}
              </button>
              {armed && psk && (
                <span
                  className={`rtty-afc-pill${psk.signal ? ' locked' : ''}`}
                  title={
                    psk.signal
                      ? 'Carrier — the decoder reads a PSK signal at its cursor; the AFC offset from the netted frequency is shown (slew-limited, never more than ±25 Hz)'
                      : 'No carrier at the cursor yet — click a trace on the waterfall to net the decoder onto it'
                  }
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
                  title="Re-acquire — drop and rebuild the demodulator for a fresh AFC pull from the netted frequency (use when it pulled onto a neighbor)"
                >
                  Re-acquire
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
                  {armed
                    ? 'listening… click a PSK trace on the waterfall to net the decoder'
                    : 'Arm RX to decode PSK31 from the receive audio'}
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
      <div className="cw-macros psk-macros" role="group" aria-label="PSK macros">
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
            title={`${m.text
              .replace(/\{MYCALL\}/g, snap?.mycall ?? '{MYCALL}')
              .replace(/\{CALL\}/g, hisCall.trim().toUpperCase() || '{CALL}')}${
              IS_MAC ? `\n${FN_KEY_HINT}` : ''
            }`}
          >
            <span className="cw-macro-key">{m.key}</span>
            <span className="cw-macro-label">{m.label}</span>
          </button>
        ))}
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
              void withErrorToast(() => pskType('\r\n'), 'PSK typing refused').then((s) => {
                if (s) setPsk(s)
              })
            } else {
              sendTyped()
            }
          }}
          placeholder={latched ? 'Typing on the air…' : 'Type PSK31 to send… (Enter)'}
          autoComplete="off"
          spellCheck={false}
          aria-label="PSK compose"
        />
        <button
          type="button"
          className="cw-send-btn"
          onClick={sendTyped}
          disabled={latched || !text.trim()}
        >
          Send
        </button>
      </div>

      {/* THE DRIVE HINT (the plan's ALC sentence): PSK31's shaped envelope IS the
          signal — a rig's ALC flattening it back to constant amplitude regenerates
          the splatter (IMD) the shaping removed. Nexus already transmits at a modest
          default drive; this line is the operator's half. No auto-ALC, on purpose. */}
      <div
        className="psk-drive-hint"
        title="PSK31 is an amplitude-shaped mode: if the rig's ALC is compressing, the signal splatters into the neighbors (IMD). Nexus keys at a modest drive by default — set TX audio / power so the rig's ALC meter barely moves."
      >
        Keep the rig's ALC near zero — an overdriven PSK31 signal splatters (IMD). Lower TX
        audio until the ALC meter barely moves.
      </div>
      </div>
    </main>
  )
}
