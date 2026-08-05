import { useRef } from 'react'
import type { ReactNode } from 'react'
import type { AppSnapshot } from '../types'
import { bandLabelForMhz, bandRangeForLabel } from '../band'
import { pushToast } from '../toast'
import { FrequencyReadout } from './FrequencyReadout'
import { useWheelTune } from '../useWheelTune'

/**
 * Shared cockpit header for the Phone, Digital (FT8/FT4) and CW cockpits.
 *
 * The design goal (operator request): the BASE cross-mode controls — the big frequency readout,
 * power, Tune, Stop TX, and the CAT status — render in the SAME screen position in every cockpit,
 * so an operator switching modes finds them where they left them. The MODE-SPECIFIC controls stay
 * unique per mode and are injected through slots (`modeIndicator`, `bandControl`, `frequencyExtras`,
 * and `children` for the rest) — never forced to look identical, only positioned consistently.
 *
 * Layout regions (left→right, wrapping): identity · frequency(+extras+band) · mode-extras(elastic)
 * · actions(power·TX/RX·Tune·Stop·CAT, pinned right). Every region wraps + has min-width:0 so
 * nothing clips off-screen at a non-maximized width or 110–125% UI zoom.
 */
export interface CockpitHeaderPower {
  /** 0..1 when unit='drive' (FT8 TX drive), 0..100 when unit='%' (Phone RF power). */
  value: number
  unit: '%' | 'drive'
  onChange: (v: number) => void
  label?: string
  title?: string
  onPointerDown?: () => void
  onPointerUp?: () => void
}

export interface CockpitHeaderProps {
  snap: AppSnapshot
  onSnap?: (s: AppSnapshot) => void
  /** Mode/tier indicator, fixed left region (FT8/FT4 tier · Phone AUTO/USB/LSB/FM · CW badge). */
  modeIndicator: ReactNode
  /** Band control, fixed band region (BandPicker for Phone/CW, band-plan select for FT8). */
  bandControl: ReactNode
  /** Commit a typed dial (MHz). Omit ⇒ the readout is display-only. */
  onCommitDial?: (mhz: number) => void
  /** Enable mouse-wheel tuning over the readout (Phone/CW). */
  wheelTune?: boolean
  /** PER-DIGIT wheel tuning on the readout (operator request): hover the 100 Hz digit and one
   * notch moves 100 Hz, hover the 1 MHz digit and one notch moves 1 MHz, carrying like a real
   * VFO. Opt-in per cockpit so it reaches the five MAIN dials and nothing else — the readouts
   * that live inside scrollable lists (TopBar, Settings, the memory rows) reach FrequencyReadout
   * through FrequencyControl instead and must keep their page scroll. */
  digitTune?: boolean
  /** Wheel step (Hz), shared with the scope wheel-tune selector. */
  wheelStepHz?: number
  /** Wheel sensitivity (Settings). */
  wheelSensitivity?: number
  /** Extra tuning affordances right of the readout (Phone/CW pass <TuningStrip showReadout={false}/>). */
  frequencyExtras?: ReactNode
  /** Mode-specific control cluster (consistent middle / second-row location). */
  children?: ReactNode
  /** Extra controls pinned into the actions cluster, BEFORE the power/TX/Tune/Stop
   *  group — the cluster is `margin-left:auto`, so adding at the front never moves the
   *  base controls out from under the operator. */
  actions?: ReactNode
  /** RF/drive power — OMIT for CW (no RF power); the region collapses. */
  power?: CockpitHeaderPower
  /** Show the compact TX/RX pill in the actions cluster. Default true. */
  txState?: boolean
  /** Label shown on the pill while transmitting (default '▲ KEYING'; Phone passes '▲ TX'). */
  txActiveLabel?: string
  /** Tune (key a steady carrier). */
  onTune?: (on: boolean) => void
  /** Stop TX / abort (CW passes its combined stopCw()+haltTx()). */
  onStopTx?: () => void
  /** Arm/disarm TX (WSJT-X "Enable Tx"). When provided, the TX/RX pill becomes a clickable
   * arm control — the RTTY/SSTV cockpits have no other Enable-Tx affordance (the TopBar's is
   * hidden with the digital chrome), so the send gate would sit at "TX is off" with no way to
   * arm from those screens. Omit ⇒ display-only pill (Phone/CW/FT8 arm elsewhere). */
  onSetTxEnabled?: (on: boolean) => void
  /** Override the derived CAT ✓/✗ pill. */
  catStatus?: ReactNode
}

/** The refusal message for a wheeled target off the band plan, naming the edge it hit — the
 *  direction of travel picks which end of the CURRENT band to quote. Falls back to the bare
 *  refusal when the dial is already off-plan (there is no "current band" to name an edge of). */
function edgeMessage(dialMhz: number, targetMhz: number): string {
  const bare = `${targetMhz.toFixed(4)} MHz is outside the band plan`
  const label = bandLabelForMhz(dialMhz)
  const range = label ? bandRangeForLabel(label) : null
  if (!range) return bare
  const edge = targetMhz > dialMhz ? range.hi : range.lo
  return `${bare} — ${label} ${targetMhz > dialMhz ? 'ends' : 'starts'} at ${edge.toFixed(4)} MHz`
}

export function CockpitHeader({
  snap,
  onSnap,
  modeIndicator,
  bandControl,
  onCommitDial,
  wheelTune = false,
  digitTune = false,
  wheelStepHz = 100,
  wheelSensitivity,
  frequencyExtras,
  children,
  actions,
  power,
  txState = true,
  txActiveLabel = '▲ KEYING',
  onTune,
  onStopTx,
  onSetTxEnabled,
  catStatus,
}: CockpitHeaderProps) {
  const radio = snap.radio
  const catOk = radio.catOk === true
  const dial = radio.dialMhz
  const readoutRef = useRef<HTMLDivElement>(null)

  // Wheel tuning over the readout (Phone/CW hunting) AND per-digit tuning, on ONE listener.
  //
  // WHY ONE. The digit hit regions render inside `.ch-readout`, the element this listener is on,
  // so a handler of their own would see every event twice — and with two optimistic targets and
  // two coalescers the two writes race, latest wins, and the digit's move is the one silently
  // discarded (measured). Here the digit under the pointer only chooses the STEP for that event,
  // so there is still one accumulator, one target and one flush: double-handling is
  // unrepresentable rather than guarded, and the selected step keeps driving everything it drives
  // today (the nudge buttons, the scope wheel, and the non-digit parts of the readout).
  //
  // Disabled while CAT is down or the transmitter is up — never move the VFO under a live over.
  //
  // ⚠️ `radio.txBusyReason` IS THE GATE, and `transmitting`/`tuning` are kept only as a
  // belt-and-braces for a snapshot too old to carry it. Those two flags alone were the bug
  // (2026-08-05): they reimplement `Engine::tx_owner()` and know 2 of its 7 owners, because
  // `transmitting` is written ONLY by the FT slot-TX path. A held mic key, the voice keyer, CW,
  // RTTY and SSTV all read as "transmitter free" through them. The soundcard modes happen to be
  // caught downstream by `tx_until_ms`; manual PTT is DELIBERATELY not (`tempo_audio::service`:
  // "a section/mode change must always reach the rig"), so this gate was the only thing there
  // and it was open. One notch on the 1 MHz digit then moves the VFO under a keyed
  // transmitter — and if it carries across a band edge, halts the over and re-commands mode too.
  //
  // Do NOT re-derive this from flags. Ask the arbiter; it ships the reason with the answer.
  const tuneBy = useWheelTune(readoutRef, {
    dialMhz: dial,
    sideband: radio.sideband || 'USB',
    enabled:
      (wheelTune || digitTune) &&
      catOk &&
      !radio.txBusyReason &&
      !radio.transmitting &&
      !radio.tuning,
    stepHz: wheelStepHz,
    sensitivity: wheelSensitivity,
    resolveStepHz: digitTune
      ? (t) => {
          const dec = (t as Element | null)?.closest?.('[data-decade]')?.getAttribute('data-decade')
          // A digit: exactly its decade, so `target += 10**n` gives VFO carry for free.
          if (dec != null) return 10 ** Number(dec)
          // Not a digit (the '.', the MHz unit, the band chip, the wrapper's own padding): the
          // selected step where the operator already has it, and DECLINE otherwise so the
          // cockpits without uniform wheel-tune keep their page scroll everywhere but the digits.
          return wheelTune ? wheelStepHz : null
        }
      : undefined,
    // The operator accepted VFO carry past a band edge ON THE CONDITION that the edge is visible.
    // Refusing in silence was right for a 100 Hz creep and wrong for a 1 MHz digit notch, which
    // lands off the plan on the first click — say it, as the nudge buttons and typed entry already
    // do (Radix routes an error toast to the assertive live region, so it is spoken too), and NAME
    // THE EDGE he hit: "you cannot go to 15.0740" alone leaves him guessing where the wall is.
    onEdge: (mhz) => pushToast(edgeMessage(dial, mhz), 'error', 3000),
    onSnap,
  })

  const txPill = radio.transmitting ? txActiveLabel : radio.txEnabled ? '▼ RX' : '■ TX off'

  return (
    <div className="cockpit-header">
      <div className="ch-identity">{modeIndicator}</div>

      <div className="ch-freq">
        <div
          className="ch-readout"
          ref={readoutRef}
          title={
            digitTune ? 'Scroll a digit to tune it' : wheelTune ? 'Scroll to tune' : undefined
          }
        >
          <FrequencyReadout
            dialMhz={dial}
            size="hero"
            editable={onCommitDial != null}
            disabled={!catOk}
            txBlocked={!bandLabelForMhz(dial)}
            onCommit={onCommitDial}
            digitTune={digitTune}
            onTuneHz={tuneBy}
          />
        </div>
        {frequencyExtras && <div className="ch-freq-extras">{frequencyExtras}</div>}
        <div className="ch-band">{bandControl}</div>
      </div>

      {children != null && <div className="ch-mode-extras">{children}</div>}

      <div className="ch-actions">
        {actions}

        {power && (
          <label
            className={`cockpit-pwr${power.unit === '%' ? ' ph-power' : ''}`}
            title={power.title ?? `${power.label ?? 'Power'} — trim so your rig's ALC is just zero`}
          >
            <span>{power.label ?? 'Power'}</span>
            <input
              type="range"
              min={0}
              max={power.unit === '%' ? 100 : 1}
              step={power.unit === '%' ? 1 : 0.01}
              value={power.value}
              onChange={(e) => power.onChange(Number(e.target.value))}
              onPointerDown={power.onPointerDown}
              onPointerUp={power.onPointerUp}
              aria-label={power.label ?? 'Power'}
            />
            <span className="cockpit-pwr-val">
              {power.unit === '%' ? `${Math.round(power.value)}%` : `${Math.round(power.value * 100)}%`}
            </span>
          </label>
        )}

        {txState &&
          (onSetTxEnabled && !radio.transmitting ? (
            <button
              type="button"
              className={`cockpit-txstate cockpit-txarm${radio.txEnabled ? ' armed' : ''}`}
              aria-pressed={radio.txEnabled}
              onClick={() => onSetTxEnabled(!radio.txEnabled)}
              title={
                radio.txEnabled
                  ? 'Transmit ARMED (WSJT-X "Enable Tx") — sends will key the rig. Click to disable.'
                  : 'Transmit is OFF — click to ARM so RTTY/SSTV sends can key the rig (WSJT-X "Enable Tx").'
              }
            >
              {radio.txEnabled ? '▼ TX On' : '■ TX Off'}
            </button>
          ) : (
            <span className={`cockpit-txstate${radio.transmitting ? ' on' : ''}`}>{txPill}</span>
          ))}

        {onTune && (
          <button
            type="button"
            className={`cockpit-tune${radio.tuning ? ' keyed' : ''}`}
            aria-pressed={radio.tuning}
            onClick={() => onTune(!radio.tuning)}
            disabled={!radio.txAllowed}
            title="Key a steady carrier to tune an ATU/amp (auto-stops on the tune watchdog). Click again to stop."
          >
            {radio.tuning ? 'TUNING…' : 'Tune'}
          </button>
        )}

        {onStopTx && (
          <button type="button" className="cockpit-stoptx" onClick={onStopTx} title="Stop TX (Esc)">
            Stop TX
          </button>
        )}

        {catStatus ?? (
          <span
            className={`cockpit-cat ${catOk ? 'ok' : 'bad'}`}
            title={radio.catDetail || (catOk ? 'CAT link OK' : 'No CAT link')}
          >
            {catOk ? 'CAT ✓' : 'CAT ✗'}
          </span>
        )}
      </div>
    </div>
  )
}
