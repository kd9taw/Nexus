import { useState, type ReactNode } from 'react'
import type { ModeRequest, QsoStatus, RadioStatus } from '../types'

interface Props {
  qso: QsoStatus | null
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
  /** TX controls consolidated beside CQ/S&P (operator request: one cluster,
   * no mousing to the top bar). Present only in the digital cockpit. */
  radio?: RadioStatus
  onSetTxEnabled?: (on: boolean) => void
  onSetTune?: (on: boolean) => void
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
}

function reportLabel(rx: number | null | undefined): string | null {
  if (rx === null || rx === undefined) return null
  return `${rx > 0 ? '+' : ''}${rx} dB`
}

/**
 * The ONE always-visible operating strip of the Operate cockpit (~36 px one row
 * at xl; two stable rows below): Call CQ / S&P and the TX cluster (TX On / Tune /
 * Stop TX / Hold Tx) anchored FIRST so they never move as a QSO runs, then the
 * live TX/RX + sequencer readout with the DX call, "Now sending" with Resend,
 * the compact Tx5 free-text field, the transmit-period + Skip Tx1 controls, the
 * next-slot countdown and the fixed-width TX telemetry cell — so you work a
 * station and watch it sequence WITHOUT leaving the waterfall. Merging the old
 * status row in here is ARRANGEMENT, not hiding: every control stays permanently
 * visible, and none of them has an id in any panel vocabulary.
 */
export function OperateQsoStrip({
  qso,
  onSetMode,
  onCallCq,
  onResend,
  onFreetext,
  onLog,
  radio,
  onSetTxEnabled,
  onSetTune,
  onHaltTx,
  onSetHoldTxFreq,
  rxOnly,
  beacon,
  onSetTxEven,
  onSetTxCycleAuto,
  skipTx1,
  onSkipTx1,
  nextSlotSec,
  specialOpBadge,
  rotor,
  telemetry,
}: Props) {
  const running = qso?.running ?? false
  const dxcall = qso?.dxcall ?? null
  const state = qso?.state ?? 'Idle'
  const txNow = qso?.txNow ?? null
  const stalled = qso?.stalled ?? false
  const txCount = qso?.txCount ?? 0
  const rpt = reportLabel(qso?.rxReport)
  const noTx = rxOnly ?? false
  const noTxWhy = 'This mode is receive-only in Nexus — it decodes but does not transmit'
  const isBeacon = beacon ?? false
  const beaconWhy =
    'This is a beacon mode — it transmits your callsign, grid and power on a schedule. There is no QSO sequence. Set the transmit % and power in Settings ▸ Beacons (WSPR & FST4W).'
  // No QSO to run on a beacon, and nothing to send on a receive-only tier.
  const noQso = noTx || isBeacon
  const noQsoWhy = isBeacon ? beaconWhy : noTxWhy

  const [free, setFree] = useState('')
  const sendFree = () => {
    const t = free.trim()
    if (!t) return
    onFreetext(t)
    setFree('')
  }

  return (
    <section className={`cockpit-qso panel${radio ? (radio.transmitting ? ' tx' : ' rx') : ''}`}>
      {/* POSITION-STABILITY ORDER (OperateQsoStrip.geometry.test): the two button
          clusters render FIRST, so the six panic-adjacent controls are anchored at
          the strip's start. Every state-variable readout member (state cap,
          AUTO-CQ pill, sequencer state, DX call, report) renders AFTER them —
          before the fix, Stop TX drifted ~270 px as a QSO populated and again on
          every 15 s TX transition. */}
      <div className="cq-roles" role="group" aria-label="Sequencer role">
        <button
          type="button"
          className={`cq-role cq-call${running ? ' active' : ''}`}
          aria-pressed={running}
          onClick={() => (onCallCq ? onCallCq() : onSetMode('qso-run'))}
          disabled={noQso}
          title={
            noQso
              ? noQsoWhy
              : 'Auto CQ — call CQ continuously, work each station that answers with the normal FT8/FT4 sequence, then return to CQ automatically'
          }
        >
          Call CQ
        </button>
        <button
          type="button"
          className={`cq-role${!running ? ' active' : ''}`}
          aria-pressed={!running}
          onClick={() => onSetMode('qso-monitor')}
          disabled={noQso}
          title={noQso ? noQsoWhy : 'Monitor — search & pounce'}
        >
          S&amp;P
        </button>
      </div>
      {radio && (
        <div className="op-controls cq-txctl" role="group" aria-label="Transmit controls">
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
          <button
            type="button"
            className="op-btn stop"
            onClick={() => onHaltTx?.()}
            title="Stop transmitting immediately — cuts even an over already in flight"
          >
            Stop TX
          </button>
          <button
            type="button"
            className={`op-btn hold${radio.holdTxFreq ? ' on' : ''}`}
            aria-pressed={radio.holdTxFreq}
            onClick={() => onSetHoldTxFreq?.(!radio.holdTxFreq)}
            title="Hold Tx Freq: keep your TX offset fixed when you click the waterfall to set RX"
          >
            Hold Tx
          </button>
        </div>
      )}
      {radio && (
        <span className="cq-statecap">
          {radio.transmitting ? '▲ TRANSMITTING' : radio.txEnabled ? '▼ Receiving' : '■ TX off'}
        </span>
      )}
      {running && (
        <span
          className="cq-autocq"
          title="Auto CQ is running — calling CQ continuously, working each station that answers, then returning to CQ for the next one. Click S&P to stop."
        >
          <span className="cq-autocq-dot" aria-hidden="true" />
          AUTO&#8288;-&#8288;CQ
        </span>
      )}
      {/* role=status: the sequencer state is THE QSO progress signal — a
          screen reader hears "AwaitReport… RR73… Done" as the exchange runs. */}
      <span className={`cq-state${running ? ' running' : ''}`} role="status">
        {state}
      </span>
      {dxcall && <span className="cq-dx mono">{dxcall}</span>}
      {rpt && <span className="cq-rpt" title="Report received about your signal">{rpt}</span>}

      {/* Deliberate wrap point: below xl the strip breaks into a stable second row
          HERE (buttons + state readout above; sending/free-text/period/telemetry
          below) instead of a content-dependent wrap mid-QSO. lg includes the
          1366x768 laptop (85 % auto-fit → effective 1607). */}
      <span className="cq-break" aria-hidden="true" />

      <div className={`cq-now${stalled ? ' stalled' : ''}`} role="status">
        <span className="cq-now-label">{stalled ? 'Stalled' : 'TX'}</span>
        <span className="cq-now-msg mono">
          {noTx
            ? '— receive-only, not transmitting'
            : isBeacon
              ? '— beacon: transmits on schedule'
              : (txNow ?? '— listening')}
        </span>
        {txCount > 1 && (
          <span className="cq-attempts" title={`Sent ${txCount} times — calling repeatedly`}>
            ×{txCount}
          </span>
        )}
        <button
          type="button"
          className="cq-resend"
          onClick={onResend}
          disabled={!txNow}
          title="Re-arm and re-send this message"
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
          placeholder="Free text (Tx5)"
          aria-label="In-QSO free text"
          onChange={(e) => setFree(e.target.value.toUpperCase())}
        />
        <button
          type="submit"
          disabled={!free.trim() || noQso}
          title={noQso ? noQsoWhy : 'Send on the next over'}
        >
          Send
        </button>
        <button
          type="button"
          className="cq-log"
          onClick={onLog}
          disabled={!dxcall}
          title="Log this QSO now"
        >
          Log
        </button>
      </form>

      <span className="cq-spacer" />
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
          title="Transmit cycle — click to cycle Auto → Tx 1st → Tx 2nd. Auto picks the opposite cycle of the station you answer; the station you work must be on the OPPOSITE period."
        >
          {radio.txCycleAuto
            ? `TX AUTO / ${radio.txEven ? '1st' : '2nd'}`
            : radio.txEven
              ? 'TX 1st / even'
              : 'TX 2nd / odd'}
        </button>
      )}
      {onSkipTx1 && (
        <button
          type="button"
          className={`cq-skiptx1${skipTx1 ? ' on' : ''}`}
          aria-pressed={skipTx1 ?? false}
          onClick={() => onSkipTx1(!skipTx1)}
          title="Skip Tx1 — when you answer a CQ, open with your signal report (Tx2) instead of your grid (Tx1), saving a cycle. Standard callsigns only (a compound call still sends its grid). Resets each launch, like WSJT-X."
        >
          Skip Tx1
        </button>
      )}
      {nextSlotSec != null && (
        <span className="cq-next" title="Time to the next slot">
          next {nextSlotSec}s
        </span>
      )}
      {telemetry != null && <div className="cq-telemetry">{telemetry}</div>}
    </section>
  )
}
