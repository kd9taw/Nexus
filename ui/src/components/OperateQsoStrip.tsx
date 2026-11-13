import { FtStopControl } from './FtStopControl'
// ⚠️ THIS FILE IS **PARTIAL** ON THE i18n LIST (i18n/hardcoded-strings.test.ts), and the part
// that is deferred is deliberate: STOP TX, TUNE, ATU and the TX On/Off tooltip stay written
// here. The first three cut or key a carrier — Stop TX and Tune are on this cockpit's
// stop-line census (features/panelState.ts), ATU keys the rig's own tuning carrier exactly as
// SetupHealth's Prove TX does — and the fourth is the control whose wording states the abort
// semantics ("an FT over already in flight finishes"). Transmit-path controls and their
// accessible names move in their own batch, with the stop-line sweeps re-run. Everything else
// in this strip is migrated.
//
// The readings are all data: the sequencer state, the DX call, the report, the message on the
// air, the rig's mode and filter width, and every count.
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from 'react'
import { t } from '../i18n'
import { catSaysTxOff } from '../features/catDetail'
import { requestWindowAttention } from '../api'
import type { ModeRequest, QsoStatus, RadioStatus } from '../types'
import { isOnAir } from '../types'
import { modeMismatch } from './TopBar'
import { RadioTuneNote } from './RadioTuneNote'
import { AtuSwitchOutMenu, useAtuSwitchOutMenu } from './AtuSwitchOutMenu'
import { upperTyped } from '../typedInput'
import { useTxMeterStyle } from '../txMeterStyle'

/** `TX` on the now-sending label and `AUTO-CQ` on the run pill are annunciator tokens, not
 *  words — `CQ` is a Q-code, and the word joiners keep the pill from breaking mid-token in a
 *  narrow strip. Named so the catalog guard can see they were a decision. */
const TX_LABEL = 'TX'
const AUTO_CQ_BADGE = 'AUTO⁠-⁠CQ'

/** Below this the rig's RX filter is too narrow for an FT8/FT4 window and decodes are being
 * thrown away wholesale — a CW or RTTY filter left in, or a SmartSDR slice narrowed at the
 * radio. Deliberately NOT set at the SSB 2.4 kHz mark: a 2.4 kHz filter is the ordinary,
 * perfectly good case and flagging it would nag every station on every band. 1 kHz is where the
 * loss stops being a matter of taste. */
const NARROW_FILTER_HZ = 1000

interface Props {
  qso: QsoStatus | null
  /** A contest's own run (`snap.fieldDay.running`): Call CQ starts it while a contest runs, and
   * the snapshot carries it there, not in `qso`. */
  contestRunning?: boolean
  /** Switch the sequencer role (Call CQ / Monitor S&P). */
  onSetMode: (mode: ModeRequest) => void
  /** Start a PLAIN CQ run (clears any sticky directed token — the labelled
   * "Call CQ" button must never silently transmit a leftover "CQ DX"). The
   * DIRECTED machine lives in the Tx panel's editable Tx6. */
  onCallCq?: () => void
  /** Re-arm the current message (re-transmit a stalled/uncopied step). */
  onResend: () => void
  /** Send in-QSO free text (WSJT-X Tx5). */
  onFreetext: (text: string) => void
  /** Log the active QSO now (inline "Log QSO" button). */
  onLog: () => void
  /** How many finished contacts wait for Log QSO with Auto-log and the popup both off
   *  (`snap.logQsoWaiting`): counted on the Log button's corner, out of the layout, and Log
   *  stays live while any wait, with or without a station on screen. */
  logWaiting?: number
  /** TX controls consolidated beside CQ/S&P (operator request: one cluster,
   * no mousing to the top bar). Present only in the digital cockpit. */
  radio?: RadioStatus
  onSetTxEnabled?: (on: boolean) => void
  onSetTune?: (on: boolean) => void
  /** Run the RADIO's own built-in ATU (Phone/CW/SSTV have carried this since #19; the FT
   * cockpit missed it — operator report 2026-08-17). Button renders only when the rig
   * reports a tuner (`radio.atu` non-null) — an ATU button on a radio with no ATU is
   * worse than no button (CockpitHeader's rule, kept identical here). */
  onAtuTune?: () => void
  /** Switch the radio's tuner OUT of line — "Switch tuner out" in the ATU button's menu (#344); a
   *  click always runs a tune-up. See `CockpitTxStrip`. */
  onAtuBypass?: () => void
  onHaltTx?: () => void
  onSetHoldTxFreq?: (on: boolean) => void
  /** The active tier decodes but cannot transmit. NO SHIPPED TIER IS RX-ONLY TODAY —
   * `RX_ONLY_TIERS` (types.ts) is empty, so this is always false and the mechanism is
   * dormant. It is kept because it is the lever a mode gets pinned to when its TX path
   * breaks: JT65 sat in that list for 0.19.17 only, to contain a Windows crash on Call
   * CQ. Do not re-list Q65 / MSK144 / FST4 / FST4W / JT65 / WSPR here on the strength
   * of an old comment — they all transmit.
   * The engine already refuses to arm TX or start a CQ run on an rx-only tier;
   * this disables the controls so the strip does not OFFER a run it
   * cannot make happen — the failure that made "Call CQ" on MSK144 look like it
   * was transmitting. Stop TX stays live: disarming is always allowed. */
  rxOnly?: boolean
  /** The active tier is a BEACON (WSPR / FST4W): it transmits on a schedule and
   * has no QSO sequence. Distinct from `rxOnly` — TX controls stay live, because
   * the mode does key the radio, but Call CQ / S&P / free text / Log have nothing
   * to act on. The engine refuses these entry points with a message either way;
   * this stops the strip offering them. */
  beacon?: boolean
  // ── Absorbed from the deleted .cockpit-status row (decode-first rebuild) ──
  // All optional: hosts that pass none (tests, future surfaces) get the strip
  // exactly as before. NOTE: the relocated period/Skip-Tx1 controls keep their
  // pre-move enablement — they did NOT disable on rx-only tiers outside the
  // strip, and relocation must not change behaviour.
  /** Transmit-period control (TX AUTO / 1st / 2nd) — rendered when BOTH handlers
   *  and `radio` are present. */
  onSetTxEven?: (even: boolean) => void
  onSetTxCycleAuto?: (auto: boolean) => void
  /** Skip Tx1 (WSJT-X parity) — rendered when the handler is present. */
  skipTx1?: boolean
  onSkipTx1?: (v: boolean) => void
  /** Live next-slot countdown (seconds). */
  nextSlotSec?: number
  /** Active DXpedition badge (HOUND), rotor strip, and the fixed-width TX
   *  telemetry cell — slotted nodes owned by the cockpit. */
  specialOpBadge?: ReactNode
  rotor?: ReactNode
  telemetry?: ReactNode
  /** THE TRANSMITTER ALARM CELL (`TxAlarmBanner`), on the strip's second line after Log (2026-11-07):
   *  always drawn at one minimum width, so an alarm or a critical lane item coming or going moves nothing.
   *  OperateCockpit passes it (`snap.txAlarms` is not in `radio`); stop-line.test.tsx checks FT. */
  alarm?: ReactNode
}

/** STOP TX IS ON SCREEN AS FT OPENS (`.cockpit-ftstrip`, cockpit-panes.css). The strip parks at the foot of the shell's
 *  scrollport where its own place is below it, but only its FIRST LINE: the TX cluster, Stop TX in it. Parked whole, a
 *  strip taller than the scrollport (300–400 px on a small window at 175 %, where the shell has 120–250) put its top,
 *  and Stop TX, above the window. So the rest of the strip under that line is written here as a placement input
 *  (`--cockpit-ftstrip-rest`, layout px; CSS cannot read where a child's line ends), and the strip parks that far past
 *  the scrollport's foot.
 *  · THE ALARM CELL'S LINE PARKS TOO, where the strip draws one (`.ch-txalarm`, the transmitter alarm, on the
 *    strip's second line) and the two lines fit the scrollport: a critical alarm stands with Stop TX, and a
 *    `scrollIntoView` of it is not spent on the sticky offset (the strip, and the cell with it, did not move). The cell
 *    has one height empty or full, so an alarm coming or going moves nothing. Where the two do not fit, only the first
 *    line parks: Stop TX must never be pushed above the window to make room for the words.
 *  · WHAT PARKS IS CLEARED BY A FOCUS: its rendered height goes on the shell (`--cockpit-ftstrip-park-h`, read by the
 *    shells' `scroll-padding-bottom`, styles.css `.layout.single`), so Tab never stops on a control behind the parked line
 *    (36 controls at 900×600 / 175 %, measured). RENDERED, for the TX strip's reason: Chrome applies `scroll-padding`
 *    unzoomed.
 *  Re-read on every render as well as on a resize of the strip or the shell. */
function useParkLine(ref: RefObject<HTMLElement | null>) {
  const apply = () => {
    const strip = ref.current
    if (!strip) return
    const shell = strip.closest<HTMLElement>('main')
    const line = strip.querySelector<HTMLElement>(':scope > .cq-txctl')
    const q = strip.getBoundingClientRect()
    if (!line || strip.offsetHeight === 0 || q.height === 0) {
      strip.style.removeProperty('--cockpit-ftstrip-rest')
      shell?.style.removeProperty('--cockpit-ftstrip-park-h')
      return
    }
    const cs = getComputedStyle(strip)
    // The strip's own zoom where the engine reports it (Chromium's `currentCSSZoom`); the ratio of its boxes elsewhere.
    // Not the ratio here: `offsetHeight` is a whole number, so the ratio carries its rounding into every length below.
    const cz = (strip as HTMLElement & { currentCSSZoom?: number }).currentCSSZoom
    const z = typeof cz === 'number' && cz > 0 ? cz : q.height / strip.offsetHeight
    // How far down the strip parks, layout px from its top: to the foot of a line, and the strip's own bottom padding
    // and border under it.
    const tail = (parseFloat(cs.paddingBottom) || 0) + (parseFloat(cs.borderBottomWidth) || 0)
    const reachOf = (el: HTMLElement) => (el.getBoundingClientRect().bottom - q.top) / z + tail
    let reach = reachOf(line)
    const alarm = strip.querySelector<HTMLElement>(':scope > .ch-txalarm')
    if (alarm && shell && alarm.getBoundingClientRect().height > 0) {
      const ss = getComputedStyle(shell)
      const room = shell.clientHeight - (parseFloat(ss.paddingTop) || 0) - (parseFloat(ss.paddingBottom) || 0)
      const both = reachOf(alarm)
      if (both > reach && both <= room) reach = both
    }
    // EXACT, not rounded: the strip's own unrounded height (its rendered box over the zoom, not `offsetHeight`) less the
    // reach. Rounded to a whole px, a line under Stop TX's that grew (the TX meters' first readings) moved the rounding
    // error, and Stop TX with it: 0.8 px at 1366×768 at 175 % on the first transmit (measured in Chrome; 0.1 now).
    const px = `${Math.max(0, Math.round((q.height / z - reach) * 100) / 100)}px`
    if (strip.style.getPropertyValue('--cockpit-ftstrip-rest') !== px) strip.style.setProperty('--cockpit-ftstrip-rest', px)
    const park = `${Math.ceil(reach * z)}px`
    if (shell && shell.style.getPropertyValue('--cockpit-ftstrip-park-h') !== park) shell.style.setProperty('--cockpit-ftstrip-park-h', park)
  }
  useLayoutEffect(apply)
  useLayoutEffect(() => {
    const strip = ref.current
    if (!strip || typeof ResizeObserver === 'undefined') return
    const shell = strip.closest<HTMLElement>('main')
    const observer = new ResizeObserver(apply)
    observer.observe(strip)
    // The guard reads the shell's room: a shorter window may leave room for the first line only.
    if (shell) observer.observe(shell)
    return () => {
      observer.disconnect()
      shell?.style.removeProperty('--cockpit-ftstrip-park-h')
    }
    // `apply` reads only the ref and the DOM; the observer is set up once per mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ref])
}

function reportLabel(rx: number | null | undefined): string | null {
  if (rx === null || rx === undefined) return null
  return `${rx > 0 ? '+' : ''}${rx} dB`
}

/**
 * The ONE always-visible operating strip of the Operate cockpit (~36 px one row
 * at xl; two stable rows below): the TX cluster (TX On / Tune / Stop TX / Hold Tx,
 * in the other screens' order) and Call CQ / S&P anchored FIRST so they never move
 * as a QSO runs, then the
 * live TX/RX + sequencer readout with the DX call, "Now sending" with Resend,
 * the compact Tx5 free-text field, the transmit-period + Skip Tx1 controls, the
 * next-slot countdown and the fixed-width TX telemetry cell — so you work a
 * station and watch it sequence WITHOUT leaving the waterfall. Merging the old
 * status row in here is ARRANGEMENT, not hiding: every control stays permanently
 * visible, and none of them has an id in any panel vocabulary.
 */
export function OperateQsoStrip({
  qso,
  contestRunning,
  onSetMode,
  onCallCq,
  onResend,
  onFreetext,
  onLog,
  logWaiting,
  radio,
  onSetTxEnabled,
  onSetTune,
  onAtuTune,
  onAtuBypass,
  onHaltTx,
  onSetHoldTxFreq,
  rxOnly,
  beacon,
  alarm,
  onSetTxEven,
  onSetTxCycleAuto,
  skipTx1,
  onSkipTx1,
  nextSlotSec,
  specialOpBadge,
  rotor,
  telemetry,
}: Props) {
  const meterStyle = useTxMeterStyle()
  const stripRef = useRef<HTMLElement>(null)
  useParkLine(stripRef)
  // ⚠️ cqRunning, NOT qso.running. `running` is also true through a directed S&P call (the
  // engine's own comment at call_station_ctx says so) and nothing clears it after the QSO —
  // so this strip lit Call CQ solid through every S&P contact and forever after, and the
  // AUTO-CQ pill claimed a CQ run that was not happening (operator-relayed report,
  // 2026-08-16). cqRunning is the flag resume_cq and the sequencer actually key off.
  // In a contest there is no `qso`: the contest's own run is the one Call CQ started.
  const running = qso?.cqRunning ?? contestRunning ?? false
  // A contest's Running stops after each contact: TX goes off and nothing is called until the
  // operator calls CQ again, so its AUTO-CQ pill shows only while TX is on.
  const contestRun = qso == null && (contestRunning ?? false)
  const autoCq = running && (!contestRun || (radio?.txEnabled ?? false))
  const dxcall = qso?.dxcall ?? null
  const waiting = logWaiting ?? 0
  const state = qso?.state ?? 'Idle'
  const txNow = qso?.txNow ?? null
  const stalled = qso?.stalled ?? false
  // The TX watchdog ended this call (WSJT-X: a red "Runaway Tx watchdog" and a window alert).
  const watchdogStopped = qso?.watchdogStopped ?? false
  const txCount = qso?.txCount ?? 0
  const rpt = reportLabel(qso?.rxReport)
  const noTx = rxOnly ?? false
  // ATU ON (#344): `CockpitTxStrip`'s rule — a click always tunes; the menu switches the tuner out.
  const atuMenu = useAtuSwitchOutMenu(radio?.atu === true && radio?.atuSwitchOut === true && !!onAtuBypass)
  const atuTuneBlocked = noTx || !!radio?.atuStartTuneUnsupported
  // WHY a mode cannot transmit / cannot run a QSO — prose about the MODE, shown on every
  // control it disables, so it moves with the strip rather than with the TX controls.
  const noTxWhy = t('operate.strip.rxOnly.why')
  const isBeacon = beacon ?? false
  const beaconWhy = t('operate.strip.beacon.why')
  // No QSO to run on a beacon, and nothing to send on a receive-only tier.
  const noQso = noTx || isBeacon
  const noQsoWhy = isBeacon ? beaconWhy : noTxWhy

  // ⚠️ THE RADIO MOVED AND NOTHING SAID SO. The mode read-back is display-only by contract
  // (`observe_rig_mode` never overwrites the commanded sideband) and the steady-state mode push
  // only fires when the APP's own target changes — so a mode or filter changed at the radio, in
  // SmartSDR or on the front panel, simply stands, and in Operate it was invisible: App hides the
  // TopBar readout group here, and the mismatch pill lives inside it. Every other cockpit that
  // could show it does. 2026-08-17 Flex audit, wave-1 #54 (and wave-2 #64 for the filter half,
  // which is shown NOWHERE in this cockpit today).
  //
  // Same verdict function as the TopBar — imported, not re-derived, so the two can never drift.
  const rigDiverged = radio ? modeMismatch(radio.rigMode, radio.sideband, radio.rigConfirmed) : null
  // The strip's ON AIR wash and "▲ TRANSMITTING" read the arbiter the header's sign reads, never
  // the FT slot flag alone: that said "▼ Receiving" through this cockpit's own Tune, a key held at
  // the radio and an over another screen started. Paint only — no control here keys, enables or
  // routes on it.
  const onAir = radio ? isOnAir(radio) : false
  const filterHz = radio?.filterWidthHz ?? null
  const narrowFilter = filterHz != null && filterHz > 0 && filterHz < NARROW_FILTER_HZ

  // Flash the window ONCE, on the snapshot that brings the trip, so an operator in another
  // program learns the call stopped. Seeded with the value at mount: a trip already standing
  // when this strip appeared is old news, and is on screen anyway.
  const wasWatchdogStopped = useRef(watchdogStopped)
  useEffect(() => {
    if (watchdogStopped && !wasWatchdogStopped.current) {
      void requestWindowAttention().catch(() => {})
    }
    wasWatchdogStopped.current = watchdogStopped
  }, [watchdogStopped])

  const [free, setFree] = useState('')
  const sendFree = () => {
    const t = free.trim()
    if (!t) return
    onFreetext(t)
    setFree('')
  }

  // A CAT outage turned transmit off: said in the TX state, after the controls (`catSaysTxOff`).
  const catOff = radio && !radio.txEnabled ? catSaysTxOff(radio.catDetail) : null

  return (
    <section ref={stripRef} className={`cockpit-qso cockpit-ftstrip panel${radio ? (onAir ? ' tx' : ' rx') : ''}`}>
      {/* POSITION-STABILITY ORDER (OperateQsoStrip.geometry.test): the two button
          clusters render FIRST, so the six panic-adjacent controls are anchored at
          the strip's start. Every state-variable readout member (state cap,
          AUTO-CQ pill, sequencer state, DX call, report) renders AFTER them —
          before the fix, Stop TX drifted ~270 px as a QSO populated and again on
          every 15 s TX transition. The TX cluster comes first of the two, as it
          opens every other screen's TX strip (CockpitTxStrip), so Tune sits right
          after TX On/Off in the same spot on all eight screens (#287). */}
      {radio && (
        <div className="op-controls cq-txctl" role="group" aria-label={t('operate.strip.txControls.aria')}>
          {/* ⚠️ DEFERRED, all four of the controls below: TX On/Off's tooltip states the abort
              semantics, Tune and ATU key a carrier, and Stop TX is the one control in this
              cockpit that cuts an over in flight. They move in the transmit-path batch with
              the stop-line sweeps re-run — see this file's header. */}
          <button
            type="button"
            className={`op-btn monitor${radio.txEnabled ? ' on' : ''}`}
            aria-pressed={radio.txEnabled}
            onClick={() => onSetTxEnabled?.(!radio.txEnabled)}
            disabled={noTx}
            title={
              noTx
                ? noTxWhy
                : radio.txEnabled
                ? 'Transmit ENABLED — your queued message will go out. Click to disable: an FT over already in flight finishes, then TX stays off (Stop TX is the immediate halt). Receive keeps decoding either way.'
                : 'Transmit DISABLED — receive keeps decoding. Click to enable transmit (WSJT-X "Enable Tx").'
            }
          >
            {radio.txEnabled ? 'TX On' : 'TX Off'}
          </button>
          <button
            type="button"
            className={`op-btn tune${radio.tuning ? ' keyed' : ''}`}
            aria-pressed={radio.tuning}
            onClick={() => onSetTune?.(!radio.tuning)}
            disabled={noTx}
            title={noTx ? noTxWhy : 'Key a tune carrier'}
          >
            Tune
          </button>
          {/* ⭐ DISABLED, NOT HIDDEN, when this CAT path cannot START a tune — see the same
              button in `CockpitHeader` for why (Hamlib's Icom and Kenwood backends clamp
              `set_func TUNER 2` to "tuner in line"; `icom.c:7085`). */}
          {onAtuTune && radio.atu != null && (
            // ATU ON (#344) — `CockpitTxStrip`'s rule: a click always runs a tune-up; the menu
            // (right-click / long press) switches the tuner out where that path can.
            <button
              type="button"
              ref={atuMenu.buttonRef}
              className={`op-btn atu${radio.atu ? ' on' : ''}`}
              aria-pressed={radio.atu}
              aria-haspopup={atuMenu.offered ? 'menu' : undefined}
              onClick={atuMenu.click(atuTuneBlocked ? undefined : onAtuTune)}
              disabled={!atuMenu.offered && atuTuneBlocked}
              aria-disabled={atuMenu.offered && atuTuneBlocked ? true : undefined}
              {...atuMenu.handlers}
              title={`${
                noTx
                  ? noTxWhy
                  : `${
                      radio.atuStartTuneUnsupported
                        ? "This CAT connection can't start the radio's tuner — press TUNER on the radio itself."
                        : "Run the radio's built-in antenna tuner (it transmits its own carrier for a second or two)."
                    } ${radio.atu ? 'The tuner is switched in.' : 'The tuner is currently bypassed.'}`
              }${atuMenu.offered ? ' Right-click or long-press to switch it out.' : ''}`}
            >
              {radio.atu ? 'ATU ON' : 'ATU'}
              {atuMenu.offered && onAtuBypass && (
                <AtuSwitchOutMenu
                  open={atuMenu.open}
                  onOpenChange={atuMenu.setOpen}
                  onSwitchOut={onAtuBypass}
                  buttonRef={atuMenu.buttonRef}
                />
              )}
            </button>
          )}
          <FtStopControl onHaltTx={onHaltTx} />
          <button
            type="button"
            className={`op-btn hold${radio.holdTxFreq ? ' on' : ''}`}
            aria-pressed={radio.holdTxFreq}
            onClick={() => onSetHoldTxFreq?.(!radio.holdTxFreq)}
            title={t('operate.strip.holdTx.title')}
          >
            {t('operate.strip.holdTx.label')}
          </button>
        </div>
      )}
      <div className="cq-roles" role="group" aria-label={t('operate.strip.roles.aria')}>
        <button
          type="button"
          className={`cq-role cq-call${running ? ' active' : ''}`}
          aria-pressed={running}
          onClick={() => (onCallCq ? onCallCq() : onSetMode('qso-run'))}
          disabled={noQso}
          title={noQso ? noQsoWhy : t('operate.strip.callCq.title')}
        >
          {t('operate.strip.callCq.label')}
        </button>
        <button
          type="button"
          className={`cq-role${!running ? ' active' : ''}`}
          aria-pressed={!running}
          onClick={() => onSetMode('qso-monitor')}
          disabled={noQso}
          title={noQso ? noQsoWhy : t('operate.strip.sandp.title')}
        >
          {t('operate.strip.sandp.label')}
        </button>
      </div>
      {/* A connection that transmits nothing says so, with its reason on hover, as the other
          screens' TX strip does (`CockpitTxStrip`), rather than "Receiving" over a Call CQ that
          cannot key. What is on the air is still shown first. */}
      {radio && (
        <span
          // #482: the state rides on the caption itself, as on every other screen's strip
          // (CockpitTxStrip): the strip's `rx` wash means "not on the air", TX off included, so
          // painting the caption from it made "TX off" green here and white everywhere else.
          className={`cq-statecap ${onAir ? 'tx' : radio.txRefusal ? 'off' : radio.txEnabled ? 'rx' : 'off'}`}
          title={
            !onAir && radio.txRefusal
              ? radio.txRefusal
              : !onAir && catOff === 'lost'
                ? t('settings.rigControl.catSilentTxOff')
                : !onAir && catOff === 'back'
                  ? t('settings.rigControl.catBackTxOff')
                  : undefined
          }
        >
          {onAir
            ? t('operate.strip.state.transmitting')
            : radio.txRefusal
              ? t('operate.strip.state.connectionNoTx')
              : radio.txEnabled
                ? t('operate.strip.state.receiving')
                : t('operate.strip.state.txOff')}
        </span>
      )}
      {/* THE CONTACT'S WORDS, ONE BOX (2026-11-12): the rig-mode and filter chips, the AUTO-CQ pill, the sequencer
          state, the DX call and the report change with the contact. As loose members on Stop TX's line a narrow strip
          (1366×768 at 150–175 %) wrapped them onto a line of their own when Auto-CQ came on and again when the call
          and report filled in, and the strip, parked at the foot of the shell, grew upward and moved Stop TX 29–35 px
          (1.24.0, measured in Chrome). Wherever the strip breaks, the box is that line, always there at one height
          (styles.css `.cq-status`), so nothing the contact does changes the strip's height and every word stands
          whole; on the one-row xl strip it is sized by its words, as they were. */}
      <span className="cq-status">
        {/* The rig has been moved out from under us — stated in the strip that is already always
            visible, styled as the Phone/TopBar mismatch chip (one visual vocabulary, no new pane,
            no structural size of its own: see cockpit-panes.css's contract). */}
        {rigDiverged && (
          <span
            className="cq-rigdiverge"
            title={t('operate.strip.rigDiverge.title', {
              rigMode: rigDiverged,
              mode: radio?.sideband ?? '',
            })}
          >
            {t('operate.strip.rigDiverge.label', { mode: rigDiverged })}
          </span>
        )}
        {narrowFilter && (
          <span
            className="cq-rigdiverge"
            title={t('operate.strip.narrowFilter.title', { hz: filterHz })}
          >
            {t('operate.strip.narrowFilter.label', { hz: filterHz })}
          </span>
        )}
        {autoCq && (
          <span
            className="cq-autocq"
            title={contestRun ? t('operate.strip.autoCq.contestTitle') : t('operate.strip.autoCq.title')}
          >
            <span className="cq-autocq-dot" aria-hidden="true" />
            {AUTO_CQ_BADGE}
          </span>
        )}
        {/* role=status: the sequencer state is THE QSO progress signal — a
            screen reader hears "AwaitReport… RR73… Done" as the exchange runs. */}
        <span
          className={`cq-state${running ? ' running' : ''}${watchdogStopped ? ' watchdog' : ''}`}
          role="status"
          title={watchdogStopped ? t('operate.strip.state.watchdog.title') : state}
        >
          {watchdogStopped ? t('operate.strip.state.watchdog') : state}
        </span>
        {dxcall && <span className="cq-dx mono" title={dxcall}>{dxcall}</span>}
        {rpt && <span className="cq-rpt" title={t('operate.strip.report.title')}>{rpt}</span>}
      </span>

      {/* Deliberate wrap point: below xl the strip breaks into a stable second row
          HERE (buttons + state readout above; sending/free-text/period/telemetry
          below) instead of a content-dependent wrap mid-QSO. lg includes the
          1366x768 laptop (85 % auto-fit → effective 1607). */}
      <span className="cq-break" aria-hidden="true" />

      <div className={`cq-now${stalled ? ' stalled' : ''}`} role="status">
        <span className="cq-now-label">{stalled ? t('operate.strip.now.stalled') : TX_LABEL}</span>
        <span className="cq-now-msg mono">
          {noTx
            ? t('operate.strip.now.rxOnly')
            : isBeacon
              ? t('operate.strip.now.beacon')
              : (txNow ?? t('operate.strip.now.listening'))}
        </span>
        {txCount > 1 && (
          <span className="cq-attempts" title={t('operate.strip.attempts.title', { count: txCount })}>
            ×{txCount}
          </span>
        )}
        <button
          type="button"
          className="cq-resend"
          onClick={onResend}
          disabled={!txNow}
          title={t('operate.strip.resend.title')}
        >
          ↻
        </button>
      </div>

      <form
        className="cq-free"
        onSubmit={(e) => {
          e.preventDefault()
          sendFree()
        }}
      >
        <input
          type="text"
          value={free}
          maxLength={13}
          placeholder={t('operate.strip.freetext.placeholder')}
          aria-label={t('operate.strip.freetext.aria')}
          onChange={(e) => setFree(upperTyped(e.target))}
        />
        <button
          type="submit"
          disabled={!free.trim() || noQso}
          title={noQso ? noQsoWhy : t('operate.strip.send.title')}
        >
          {t('operate.strip.send.label')}
        </button>
        <button
          type="button"
          className="cq-log"
          onClick={onLog}
          disabled={!dxcall && waiting === 0}
          title={
            waiting > 0
              ? t('operate.strip.log.titleWaiting', { count: waiting })
              : t('operate.strip.log.title')
          }
          aria-label={
            waiting > 0 ? t('operate.strip.log.ariaWaiting', { count: waiting }) : undefined
          }
        >
          {t('operate.strip.log.label')}
        </button>
        {/* On the button's corner and out of the flow, so a count never moves or wraps Log or
            anything beside it. It takes no clicks (Log is under its edge), so it carries no
            tooltip of its own: Log's says it. Hidden from a screen reader, which hears it in
            Log's name and from the status below. */}
        {waiting > 0 && (
          <span className="cq-log-waiting" aria-hidden="true">
            {waiting}
          </span>
        )}
        {/* Always mounted, so the count's first appearance is announced (a live region added
            with its text is often not), and politely, as a sentence rather than a bare number. */}
        <span className="sr-only" role="status" aria-live="polite">
          {waiting > 0 ? t('operate.strip.log.waiting', { count: waiting }) : ''}
        </span>
      </form>
      {/* THE TRANSMITTER ALARM CELL (`TxAlarmBanner`): on the strip's second line, right after the next-over box, Free
          text, Send and Log, in the room the line leaves (it grows beside the absorber), always drawn at one minimum
          width. On the second line, a line the break always starts, so nothing on the first line can push it to a
          line of its own: the first line's words change with the contact (the sequencer state, the DX call, the
          report, the rig-mode chip), and a cell at its end was pushed off it and grew the strip, moving Stop TX.
          Not the row's last cell either: where the strip is taller than the room the shell gives it (FT, ru, 900x600
          pinned) its last line is below the fold, and a critical alarm shown only there was on screen nowhere. Not
          FIRST on the line either: an empty cell there stood before the next-over box and took half the line's
          free room, so the box started 600 px in on a wide window (1.24.0). */}
      {alarm}

      {/* The strip's one absorber. Why transmit is off after a CAT outage rides in it: it is already a
          zero-basis grower, so the words can never wrap the strip or move a control (`.cq-catnote`). */}
      {catOff && !onAir && !radio?.txRefusal ? (
        <span
          className="cq-spacer cq-catnote cat-txoff-note"
          title={catOff === 'lost' ? t('settings.rigControl.catSilentTxOff') : t('settings.rigControl.catBackTxOff')}
        >
          {catOff === 'lost' ? t('operate.strip.state.txOffCatLost') : t('operate.strip.state.txOffCatBack')}
        </span>
      ) : (
        <span className="cq-spacer" />
      )}
      {/* Active DXpedition mode badge — prominent, next to the period controls. */}
      {specialOpBadge}
      {rotor}
      {radio && onSetTxEven && onSetTxCycleAuto && (
        <button
          type="button"
          className={`cq-period${radio.txCycleAuto ? ' is-auto' : ''}`}
          onClick={() => {
            // Cycle: Auto → lock 1st → lock 2nd → Auto.
            if (radio.txCycleAuto) onSetTxEven(true)
            else if (radio.txEven) onSetTxEven(false)
            else onSetTxCycleAuto(true)
          }}
          title={t('operate.strip.period.title')}
        >
          {radio.txCycleAuto
            ? radio.txEven
              ? t('operate.strip.period.auto.first')
              : t('operate.strip.period.auto.second')
            : radio.txEven
              ? t('operate.strip.period.first')
              : t('operate.strip.period.second')}
        </button>
      )}
      {onSkipTx1 && (
        <button
          type="button"
          className={`cq-skiptx1${skipTx1 ? ' on' : ''}`}
          aria-pressed={skipTx1 ?? false}
          onClick={() => onSkipTx1(!skipTx1)}
          title={t('operate.strip.skipTx1.title')}
        >
          {t('operate.strip.skipTx1.label')}
        </button>
      )}
      {nextSlotSec != null && (
        <span className="cq-next" title={t('operate.strip.nextSlot.title')}>
          {t('operate.strip.nextSlot.label', { secs: nextSlotSec })}
        </span>
      )}
      {/* `has-temps`: the radio or an amplifier reports a temperature, so the meters have a third row. Stable through a
          contact (the rig's model and the amplifier's setup), so the cell's one height never changes mid-over. */}
      {telemetry != null && (
        <div className={`cq-telemetry${meterStyle === 'needle' ? ' needle' : ''}${radio && (radio.paTempC != null || radio.paTempMeter != null || radio.amp != null) ? ' has-temps' : ''}`}>
          {telemetry}
        </div>
      )}
      {/* What the radio reports beside Tune while Tune is the Flex radio's own carrier: the last
          child, on a line of its own, so it never moves the clusters or the readouts. */}
      {radio && <RadioTuneNote radio={radio} />}
    </section>
  )
}
