import { useLayoutEffect, useRef, type RefObject } from 'react'
import { haltTx } from '../api'
import { withErrorToast } from '../toast'
import { useStationControl, useStationStopControl, useStationStopProgress } from '../stationAccess'
import { t } from '../i18n'
import type { AppSnapshot, RadioStatus } from '../types'
import { isOnAir } from '../types'
import { RadioTuneNote } from './RadioTuneNote'
// ⚠️ THIS FILE IS **PARTIAL** ON THE i18n LIST (i18n/hardcoded-strings.test.ts), for the same
// deferral `CockpitHeader` carried until these controls moved here: THE TX-ENABLE LATCH (and its
// read-only rendering), TUNE, ATU AND STOP TX stay written here, labels and tooltips. One strip
// draws them for EIGHT screens, so a typo in one of the four labels is an eight-screen safety
// regression, and three of them are what `components/stop-line.test.tsx` matches by accessible
// name (/^stop tx$/i, /^tune$/i, /^tx on$|^tx off$/i). Transmit-path controls and their accessible
// names move in their own batch, with the stop-line sweeps re-run. Everything else here — the
// group's name, the TX state, Hold Tx, the remote stop's progress — is catalogued.

export interface CockpitTxStripProps {
  radio: RadioStatus
  /** Takes the snapshot a REMOTE Stop TX answers with. */
  onSnap?: (s: AppSnapshot) => void
  /** Arm/disarm TX (WSJT-X "Enable Tx"). Omit ⇒ the first slot shows the latch READ-ONLY (Phone
   *  and CW arm it themselves: the mode change arms it, Phone's PTT offers to). */
  onSetTxEnabled?: (on: boolean) => void
  /** Tune (key a steady carrier). */
  onTune?: (on: boolean) => void
  /** Run the RADIO's own built-in ATU. Rendered only when the rig reports a tuner (`radio.atu`). */
  onAtuTune?: () => void
  /** Stop TX — the screen's own stop (CW passes stopCw()+haltTx(), RTTY rttyStop()+haltTx()). */
  onStopTx?: () => void
  /** Hold Tx, the slot modes' fifth control (Tempo; FT draws its own strip). */
  hold?: { on: boolean; onChange: (on: boolean) => void; disabled?: boolean }
}

/**
 * THE TRANSMIT STRIP — FT's transmit cluster, in FT's order and FT's look, on every operating
 * screen (2026-10-01: "FT's strip under the scope in every mode, FT's order,
 * staying in view; Stop TX at the same spot on all 8 screens"). TX On/Off · Tune · ATU · Stop TX
 * (· Hold Tx), then the TX state. Each screen keeps its own send area in its dock.
 *
 * A SHELL CHILD, NEVER A PANE: it renders as a direct child of the cockpit shell, right under the
 * scope, with no id in any ⊞ vocabulary — so hiding or moving it is unrepresentable, as for the
 * dock (THE STOP LINE, `cockpit-panes.css`). It is STICKY on both edges (`.cockpit-txstrip`), so
 * when the shell's deficit valve scrolls, the strip parks at the top of the window, or just above
 * the sticky dock, and never leaves the window: at the 150–175 % pins the header Stop TX it
 * replaces sat under the dock or below the window.
 *
 * THE FIRST SLOT IS THE LATCH, AND KEEPS ONE WIDTH: a button where the screen arms TX here (RTTY,
 * PSK, JS8, SSTV, Tempo, APRS), its read-only twin where the screen arms it itself (Phone, CW), so
 * Tune, ATU and Stop TX sit at the same x on every screen. The button keys on the SLOT flag, as
 * the header's did: in RTTY and SSTV the latch is a STOP (set_tx_enabled(false) arms the mode's
 * abort) and must stay a button through their overs, which `radio.transmitting` never marks.
 */
export function CockpitTxStrip({ radio, onSnap, onSetTxEnabled, onTune, onAtuTune, onStopTx, hold }: CockpitTxStripProps) {
  const control = useStationControl()
  // Stop's own authority, never ordinary control freshness — see the Stop TX button below.
  const remoteStop = useStationStopControl()
  // How far a remote Stop has got. ACCEPTANCE IS NOT RF: see `useStationStopProgress`.
  const stopProgress = useStationStopProgress(radio)
  const onAir = isOnAir(radio)
  // THE SIGN WAITS WITH AN ARMED OVER (the operator's pick "Header ON AIR waits too",
  // 2026-09-28; the sign moved here from the header with the rest of the strip). A streamed
  // operator's PTT, whichever they hold, arms an over that owns the transmitter from the arm, but
  // nothing is on the air until the voice keys it, so while the station reports it armed
  // (`streamMic`, which says so only while that over owns the transmitter) the sign reads as it
  // does before a key. A key read back from the radio is not one of the arbiter's owners, and the
  // FT slot flag and the tune carrier are checked here too, for a station that reports `armed`
  // whatever else holds the transmitter: any of them lights it. DISPLAY ONLY: `onAir` stays the
  // arbiter's answer, and the header's amplifier strip reads it; Stop TX is not touched.
  const lit =
    onAir &&
    !(radio.streamMic === 'armed' && !radio.transmitting && !radio.tuning && radio.rigKeyed !== true)
  const ref = useRef<HTMLDivElement>(null)
  useDockClearance(ref)

  return (
    // The hosted OBSERVER's strip scrolls with its cockpit, as its dock does (`.remote-observer-dock`):
    // its controls are disabled, and a parked strip of disabled buttons covered the contact the
    // observer was reading on a phone. With station control it is pinned like the dock.
    <div className={`cockpit-txstrip${control ? '' : ' remote-observer-strip'}`} ref={ref}>
      <div className="op-controls cq-txctl" role="group" aria-label={t('operate.strip.txControls.aria')}>
        {/* ⚠️ DEFERRED (i18n), the four controls below — see this file's header. */}
        {onSetTxEnabled && !radio.transmitting ? (
          <button disabled={!control}
            type="button"
            className={`op-btn monitor${radio.txEnabled ? ' on' : ''}`}
            aria-pressed={radio.txEnabled}
            onClick={() => onSetTxEnabled(!radio.txEnabled)}
            title={
              radio.txEnabled
                ? 'Transmit ARMED (WSJT-X "Enable Tx") — sends will key the rig. Click to disable.'
                : 'Transmit is OFF — click to ARM so this screen’s sends can key the rig (WSJT-X "Enable Tx").'
            }
          >
            {radio.txEnabled ? 'TX On' : 'TX Off'}
          </button>
        ) : (
          <span className={`op-btn monitor readonly${radio.txEnabled ? ' on' : ''}`}
            title={
              !radio.transmitting && radio.txBusyReason
                ? radio.txBusyReason
                : radio.txEnabled
                  ? 'Transmit is enabled (WSJT-X "Enable Tx"). Stop TX is the immediate halt.'
                  : 'Transmit is off (WSJT-X "Enable Tx"). This screen arms it from its own transmit control.'
            }
          >
            {radio.txEnabled ? 'TX On' : 'TX Off'}
          </span>
        )}
        {onTune && (
          <button
            type="button"
            className={`op-btn tune${radio.tuning ? ' keyed' : ''}`}
            aria-pressed={radio.tuning}
            onClick={() => onTune(!radio.tuning)}
            disabled={!control || !radio.txAllowed}
            title={radio.txRefusal ?? "Key a steady carrier to tune an ATU/amp (auto-stops on the tune watchdog). Click again to stop."}
          >
            Tune
          </button>
        )}
        {/* ⭐ DISABLED, NOT HIDDEN, when this CAT path cannot START a tune (Hamlib's Icom and
            Kenwood backends clamp `set_func TUNER 2` to "tuner in line"; `icom.c:7085`): hiding it
            answers "where did my ATU go?" with nothing. It is the ACTION that is unavailable.
            Disabled on the licence lockout exactly like Tune; every other refusal (TX off, busy)
            comes back from the backend with its reason. */}
        {onAtuTune && radio.atu != null && (
          <button
            type="button"
            className="op-btn atu"
            onClick={onAtuTune}
            disabled={!control || !radio.txAllowed || !!radio.atuStartTuneUnsupported}
            title={`${
              radio.atuStartTuneUnsupported
                ? "This CAT connection can't start the radio's tuner — press TUNER on the radio itself."
                : "Run the radio's built-in antenna tuner (it transmits its own carrier for a second or two)."
            } ${radio.atu ? 'The tuner is switched in.' : 'The tuner is currently bypassed.'}`}
          >
            ATU
          </button>
        )}
        {/* THE stop control of every screen that draws this strip; its label is the accessible
            name every stop-line sweep looks for (/^stop tx$/i).

            REMOTE (operator decision 2026-09-14): in the browser this is the station's one remote
            stop, the same path Operate's FtStopControl uses — halt_tx → the hosted control
            transport → the operation client's `stopTransmit`. Its authority is
            useStationStopControl (connected + a station-issued stop token), so stale readings, a
            pending command or the busy banner never disable it. The screen's own handler is
            local-only: its extra verbs (stop_cw, rtty_stop, psk_stop) have no remote route. The
            station stops ANY transmission for it, however it started. */}
        {onStopTx && (
          <button disabled={!(control || remoteStop)} type="button" className="op-btn stop"
            data-remote-stop={(!control && remoteStop) || undefined}
            onClick={control ? onStopTx : () => void withErrorToast(() => haltTx(), t('shell.halt.failed')).then(s => { if (s) onSnap?.(s) })}
            title="Stop TX (Esc)">
            Stop TX
          </button>
        )}
        {hold && (
          <button disabled={hold.disabled}
            type="button"
            className={`op-btn hold${hold.on ? ' on' : ''}`}
            aria-pressed={hold.on}
            onClick={() => hold.onChange(!hold.on)}
            title={t('topbar.holdTx.title')}
          >
            {t('topbar.holdTx.label')}
          </button>
        )}
      </div>
      {/* ⚠️ NEVER "stopped" on acceptance. The station answers an accepted Stop before the halt has
          run — when its Engine is held it runs afterwards on its own thread — so this reads SENT
          until the station's own transmitter reading goes free, and STOPPED only then. AFTER the
          cluster, so it never moves the controls. */}
      {onStopTx && stopProgress !== 'idle' && (
        <span className={`cockpit-stopstate${stopProgress === 'stopped' ? ' done' : ''}`} role="status"
          title={stopProgress === 'stopped' ? t('remote.stop.stopped.title') : stopProgress === 'sent' ? t('remote.stop.sent.title') : undefined}>
          {stopProgress === 'stopped' ? t('remote.stop.stopped') : stopProgress === 'sent' ? t('remote.stop.sent') : t('remote.stop.sending')}
        </span>
      )}
      {/* The ON AIR sign reads the arbiter (`isOnAir`), never the FT slot flag alone, so a voice
          over, CW, RTTY, a tune or a key held at the radio all light it; an over armed through the
          stream lights it once the voice keys it (`lit`, above). Paint only. */}
      {/* A connection that transmits nothing says so here, with its reason on hover, rather
          than "receiving" over a TX that cannot key. A key the radio itself holds still lights
          the sign: what is on the air is shown first. */}
      <span className={`cq-statecap ${lit ? 'tx' : radio.txRefusal ? 'off' : radio.txEnabled ? 'rx' : 'off'}`}
        title={!lit && radio.txRefusal ? radio.txRefusal : undefined}>
        {lit
          ? t('operate.strip.state.transmitting')
          : radio.txRefusal
            ? t('operate.strip.state.connectionNoTx')
            : radio.txEnabled
              ? t('operate.strip.state.receiving')
              : t('operate.strip.state.txOff')}
      </span>
      {/* What the radio reports beside Tune while Tune is the Flex radio's own carrier: the last
          child, on a line of its own, so it never moves the controls or the TX state. */}
      {onTune && <RadioTuneNote radio={radio} />}
    </div>
  )
}

/** THE STICKY EDGES' TWO PLACEMENT INPUTS, measured because CSS cannot read a sibling's box.
 *  · `--cockpit-txstrip-bottom` on the strip: the dock's height. The dock is sticky at `bottom: 0`,
 *    so a strip parked at 0 would park UNDER it — Stop TX covered by the PTT row, the exact defect
 *    at the 150–175 % pins. A dock that is not sticky (the hosted observer's) clears nothing.
 *  · `--cockpit-txstrip-h` on the SHELL: the strip's rendered height while it is sticky, which the
 *    shell's `scroll-padding-top` reads so whatever is scrolled to the start lands below a strip
 *    parked at the top. RENDERED, not offsetHeight: under the UI zoom Chrome applies
 *    `scroll-padding` unzoomed (measured: at 175 % a 42 px offsetHeight padded 42 screen px under a
 *    74 px strip), and more padding than the strip only scrolls a target a little lower.
 *  Re-read on every render as well as on a resize: station control arriving turns the observer's
 *  dock and strip sticky with no change of size for an observer to see. */
const isDock = (c: Element): c is HTMLElement =>
  c instanceof HTMLElement && (c.classList.contains('cockpit-txdock') || c.classList.contains('sstv-tx-bar'))

/** The cockpit shell the strip stands in: its parent — or, where layout wrappers stand between them
 *  (Phone's stage and the left side's row, 2026-10-03), the nearest ancestor holding the dock, up to
 *  the cockpit's own `<main>`. With no dock that far up, the parent, as before. */
function shellOf(strip: HTMLElement): HTMLElement | null {
  for (let p = strip.parentElement; p; p = p.parentElement) {
    if (Array.from(p.children).some(isDock)) return p
    if (p.tagName === 'MAIN') break
  }
  return strip.parentElement
}

function useDockClearance(ref: RefObject<HTMLDivElement | null>) {
  const apply = () => {
    const strip = ref.current
    const shell = strip && shellOf(strip)
    if (!strip || !shell) return
    const dock = Array.from(shell.children).find(isDock)
    strip.style.setProperty('--cockpit-txstrip-bottom',
      dock && getComputedStyle(dock).position === 'sticky' ? `${dock.offsetHeight}px` : '0px')
    shell.style.setProperty('--cockpit-txstrip-h',
      getComputedStyle(strip).position === 'sticky' ? `${Math.ceil(strip.getBoundingClientRect().height)}px` : '0px')
  }
  useLayoutEffect(apply)
  useLayoutEffect(() => {
    const strip = ref.current
    const shell = strip && shellOf(strip)
    if (!strip || !shell) return
    const dock = Array.from(shell.children).find(isDock)
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(apply)
    observer?.observe(strip)
    if (dock) observer?.observe(dock)
    return () => {
      observer?.disconnect()
      shell.style.removeProperty('--cockpit-txstrip-h')
    }
    // `apply` reads only refs and the DOM; the observer is set up once per mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ref])
}
