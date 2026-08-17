import { useEffect, useMemo, useRef, useState } from 'react'
import type { AppSnapshot, PskState } from '../types'
import { CockpitHeader } from './CockpitHeader'
import { CockpitPaneFrame } from './panes/CockpitPaneFrame'
import { PanelsMenu } from './PanelsMenu'
import { panelHost } from '../features/panelHost'
import { PSK_PANEL_IDS, type PskPanelId, type PanelLayoutApi } from '../features/panelState'
import { Waterfall } from './Waterfall'
import { getPskState, pskAfcReset, pskArm, pskAutoArm, pskClear, pskNet } from '../api'
import { bandLabelForMhz } from '../band'
import { pushToast } from '../toast'
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
  /** Commit a typed/scrolled dial (the shared App setFrequency path) — a QSY,
   * never TX. Omit ⇒ the readout is display-only. */
  onSetFrequency?: (dialMhz: number, band: string, mode: string) => void
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

/** "+12 Hz" (signed) AFC readout. */
function fmtAfc(hz: number): string {
  const r = Math.round(hz)
  return `${r >= 0 ? '+' : ''}${r} Hz`
}

/**
 * PSK operating cockpit (Digital rail: FT · Tempo · RTTY · PSK · SSTV · APRS) —
 * Keyboard Modes Phase 1, RECEIVE ONLY. Arm the decoder (it auto-arms on view
 * entry, with the APRS/SSTV decline memory), click a trace on the waterfall to
 * net the DECODER onto it (single-signal click-to-tune, the CW/RTTY precedent
 * — the click never moves the rig), and read the varicode transcript with
 * per-character confidence fading plus the slew-limited AFC readout.
 *
 * THE STOP LINE holds here BY CONSTRUCTION (features/panelState.ts, the PSK
 * vocabulary comment): no PSK TX path exists in the engine this phase, so this
 * cockpit renders NO control that starts a transmission — no send box, no
 * macros, no TX-enable latch, no TX dock — and therefore needs no stop
 * control. Do not add one of either kind here without building the Phase 2
 * census (dock + latch + Esc + sweep entry) around it first.
 *
 * Mounted in a keep-alive host (like RTTY/SSTV) so the decoded stream keeps
 * accumulating while the operator is on another section.
 */
export function PskCockpit({ snap, onSnap, active = true, onSetFrequency, theme = 'dark', wheelSensitivity, panels }: Props) {
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
  // ⚠️ RECEIVE-ONLY, and the ENGINE is what guarantees that: `psk_auto_arm`
  // only upgrades from off, refuses once the operator has explicitly stopped
  // the decoder this session (the decline memory), and honours the Settings
  // opt-out. The policy lives there rather than in this ref so it cannot be
  // lost to a remount — the APRS armed-desync lesson.
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
          // RX-only cockpit: no TX/RX pill, no Stop TX, no Tune, no TX-enable
          // latch, no power — nothing here transmits (THE STOP LINE holds by
          // construction; see the component doc).
          txState={false}
          modeIndicator={
            <>
              <span
                className="cw-mode-badge"
                title={`${mode.name} — ${mode.hint}. Receive-only for now; transmit is on the roadmap.`}
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
              <span
                className="psk-rx-pill"
                title="This screen is receive-only: nothing on it can key the rig. PSK transmit arrives in a later release."
              >
                RX only
              </span>
            </>
          }
          bandControl={
            <span className="cockpit-ph-pill" title="Showing the rig's current band">
              {bandLabelForMhz(snap.radio.dialMhz) || '— band —'}
            </span>
          }
          // Typed/scrolled dial entry (a QSY — never TX). Off the ham bands is
          // first-class listening, same as RTTY's commitDial.
          onCommitDial={
            onSetFrequency
              ? (mhz) => onSetFrequency(mhz, bandLabelForMhz(mhz), snap.radio.sideband || 'USB')
              : undefined
          }
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
          marks where the decoder listens, and a click NETS THE DECODER (pskNet — engine
          RX state), never the rig. Hiding it hands its height to the stream frame, the
          shell's only grower. */}
      {psk && shown('scope') && (
        <Waterfall
          theme={theme}
          active={active}
          rowMs={50} // live band instrument — rig-scope cadence (the RTTY value)
          transmitting={snap?.radio.transmitting ?? false}
          rxOffsetHz={centerHz}
          txOffsetHz={0}
          cursors={[{ hz: centerHz, color: '#3ddc8c', label: 'RX' }]}
          hint="click nets the decoder"
          onTune={(hz) => void pskNet(hz).then(setPsk).catch(() => {})}
        />
      )}

      {/* THE ONE CONTENT PANE — RTTY's region-less shape: a single CockpitPaneFrame as
          the shell's grower (the `--cockpit-fill-min` knob on `.psk-cockpit` floors it),
          transcript scrolling inside. Every control in the head is a DECODER control
          (arm / re-acquire / clear) — none touches PTT, pinned by the structure test. */}
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
    </main>
  )
}
