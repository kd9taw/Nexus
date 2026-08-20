// ⚠️ THIS FILE IS **PARTIAL** ON THE i18n LIST (i18n/hardcoded-strings.test.ts), and what is
// deferred is the whole reason this batch exists: THE TX-ENABLE LATCH, TUNE, ATU AND STOP TX
// stay written here. This one header draws them for SIX cockpits — Phone, CW, RTTY, PSK, SSTV
// and Operate — so a typo in any of the four labels below is a six-cockpit safety regression,
// and three of them are what `components/stop-line.test.tsx` matches by accessible name
// (/^stop tx$/i, /^tune$|^tuning…$/i, /^▼ tx on$|^■ tx off$/i). ATU keys the rig's own tuning
// carrier exactly as SetupHealth's Prove TX does. Transmit-path controls and their accessible
// names move in their own batch, with the stop-line sweeps re-run.
//
// Everything else in the header is migrated under `cockpit.header.*`: the wheel-tune tooltips,
// the band-edge toast, the power slider (a CONFIGURATION control on the transmit path, which
// moves exactly as PTT Method and the drive slider did) and the CAT pill's two states.
import { useRef } from 'react'
import type { ReactNode } from 'react'
import type { AppSnapshot } from '../types'
import { bandLabelForMhz, bandRangeForLabel } from '../band'
import { pushToast } from '../toast'
import { FrequencyReadout } from './FrequencyReadout'
import { useWheelTune } from '../useWheelTune'
import { t } from '../i18n'

/** The header's annunciator plates — state, not prose: the CAT link's ✓/✗ pill and the three
 *  readings of the TX/RX pill. The pill is the PASSIVE rendering of the TX-enable latch below,
 *  which this batch defers, so its words stay here with the button's for the same reason: the
 *  two must never read as different controls. */
const CAT_OK = 'CAT ✓'
const CAT_BAD = 'CAT ✗'
const TX_KEYING = '▲ KEYING'
const TX_RX = '▼ RX'
const TX_OFF = '■ TX off'

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
  /** Run the RADIO's own built-in ATU (discussion #19 — WSJT-X fires it from a right-click on
   * Tune). Belongs HERE, beside Tune, and not in a cockpit's DSP row: NB/NR/Notch are receive
   * filters, while an ATU tune-up keys the transmitter — so it sits with the other TX controls,
   * carries the same `txAllowed` lockout, and reports the backend's refusal instead of going
   * quiet. Rendered only when the rig actually reports a tuner (`radio.atu != null`). */
  onAtuTune?: () => void
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

/** What to say when a wheel burst PARKS on a band edge, naming the edge it stopped at — the
 *  direction of travel picks which end of the CURRENT band to quote. Empty when the dial is
 *  already off-plan (there is no "current band" to name an edge of, and nothing was stopped).
 *
 *  ⚠️ NOT A REFUSAL ANY MORE, and the wording carries the difference. It used to be one: the
 *  wheel would not write a target off the band plan, so "outside the band plan" at `error`
 *  severity described what had just happened. Since the 2026-08-13 ruling — listening off the
 *  ham bands is first-class — the wheel tunes anywhere; what remains is the runaway guard,
 *  which stops a fast burst ON the edge so a single flick cannot carry the dial off the band
 *  by accident. One more deliberate gesture goes straight past. Calling that an error told the
 *  operator they had been refused when they had not, and `error` speaks it into the assertive
 *  live region on top. */
function edgeMessage(dialMhz: number, targetMhz: number): string {
  const label = bandLabelForMhz(dialMhz)
  const range = label ? bandRangeForLabel(label) : null
  if (!range) return ''
  const up = targetMhz > dialMhz
  const edge = up ? range.hi : range.lo
  // Two WHOLE sentences, one per direction — never a stem with `up`/`down` glued on the end,
  // which no language with a different word order could take. The band name and the edge
  // frequency are invariant tokens and are formatted here, not by the catalog. Each key is
  // written out so the catalog guard can see both of them.
  const vals = { band: label, edge: edge.toFixed(4) }
  return up ? t('cockpit.header.bandEdge.up', vals) : t('cockpit.header.bandEdge.down', vals)
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
  txActiveLabel = TX_KEYING,
  onTune,
  onAtuTune,
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
      !radio.rigKeyed && // radio-side keying (mic/straight key, #57) — same live-over rule
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
    // Stopping in silence was right for a 100 Hz creep and wrong for a 1 MHz digit notch, which
    // reaches the edge on the first click — so say where the dial parked, and NAME THE EDGE:
    // "stopped at 7.3000" alone leaves the operator guessing which wall they met. `info`, not
    // `error`: nothing was refused, and an error toast is routed to the assertive live region
    // (i.e. spoken), which would announce a supported destination as a failure.
    onEdge: (mhz) => {
      const said = edgeMessage(dial, mhz)
      if (said) pushToast(said, 'info', 3000)
    },
    onSnap,
  })

  // ON AIR is the ARBITER's answer, not the FT slot flag: `transmitting` is written only
  // by the slot/beacon TX path, so a voice over, CW sending, the tune carrier, a held mic
  // key and SSTV all read as "RX" through it — the #57 report (FTdx10, Phone/CW showed no
  // TX). `txBusyReason` ships whenever ANY of the seven owners holds the transmitter; the
  // same rule the wheel-tune gate above already follows.
  const onAir = radio.transmitting || radio.txBusyReason != null || radio.rigKeyed === true
  const txPill = onAir ? txActiveLabel : radio.txEnabled ? TX_RX : TX_OFF

  return (
    <div className="cockpit-header">
      <div className="ch-identity">{modeIndicator}</div>

      <div className="ch-freq">
        <div
          className="ch-readout"
          ref={readoutRef}
          title={
            digitTune
              ? t('cockpit.header.readout.digitTune.title')
              : wheelTune
                ? t('cockpit.header.readout.wheelTune.title')
                : undefined
          }
        >
          <FrequencyReadout
            dialMhz={dial}
            size="hero"
            editable={onCommitDial != null}
            disabled={!catOk}
            // TX-red means "you must not key here", so it is the LICENCE's answer, not the UI
            // band table's. Derived from `!bandLabelForMhz(dial)` it alarm-coloured the readout
            // for a whole off-band session — and receiving off the ham bands (WWV, shortwave,
            // CB, marine, the gaps between band edges) is a supported use case, operator ruling
            // 2026-08-13. `radio.txAllowed` is the backend's privileges answer for this dial AND
            // mode: it knows the non-US `Open` class, and the no-privilege gaps INSIDE a band
            // that a frequency table cannot see (a General on 14.010 read as fine before).
            txBlocked={!radio.txAllowed}
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
            title={
              power.title ??
              t('cockpit.header.power.title', {
                label: power.label ?? t('cockpit.header.power.label'),
              })
            }
          >
            <span>{power.label ?? t('cockpit.header.power.label')}</span>
            <input
              type="range"
              min={0}
              max={power.unit === '%' ? 100 : 1}
              step={power.unit === '%' ? 1 : 0.01}
              // Drive-unit sliders (tx_level, 0-1) use a square-law curve between slider
              // POSITION and the underlying level: position^2 -> level, sqrt(level) ->
              // position. The real hardware-usable drive range (0 up to just past where ALC
              // engages) sits in only the bottom ~15-20% of a linear slider, so this spreads
              // that critical region across most of the travel. The '%' RF-power sliders
              // (Phone/SSTV) are a different backend value entirely and stay linear. What
              // tx_level itself means is completely unchanged - only how far the slider
              // travels to reach a given level. Rounded to the 0.01 step grid: an unrounded
              // sqrt() rarely lands on a step boundary, and a step-mismatched range input
              // value fails HTML5 constraint validation.
              value={
                power.unit === 'drive'
                  ? Math.round(Math.sqrt(power.value) * 100) / 100
                  : power.value
              }
              onChange={(e) => {
                const raw = Number(e.target.value)
                power.onChange(power.unit === 'drive' ? raw ** 2 : raw)
              }}
              onPointerDown={power.onPointerDown}
              onPointerUp={power.onPointerUp}
              aria-label={power.label ?? t('cockpit.header.power.label')}
            />
            <span className="cockpit-pwr-val">
              {power.unit === '%' ? `${Math.round(power.value)}%` : `${Math.round(power.value * 100)}%`}
            </span>
          </label>
        )}

        {/* ⚠️ THE BRANCH CONDITION IS THE SLOT FLAG ON PURPOSE — stop line. In RTTY/SSTV
            the TX-enable latch is a STOP control and must stay a BUTTON through an over
            (`radio.transmitting` is the slot-TX indicator alone, so a soundcard over never
            flips this branch). Gating it on the arbiter's `onAir` would replace the latch
            with a passive pill exactly while an over is keying — removing a stop control.
            Only the PASSIVE pill below (Phone/CW, which pass no onSetTxEnabled) reads the
            arbiter.

            ⚠️ DEFERRED (i18n): the latch's two labels are the accessible name
            `stop-line.test.tsx` matches for RTTY and SSTV, where this button IS a stop
            control. It moves in the transmit-path batch — see this file's header. */}
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
            <span
              className={`cockpit-txstate${onAir ? ' on' : ''}`}
              title={!radio.transmitting && radio.txBusyReason ? radio.txBusyReason : undefined}
            >
              {txPill}
            </span>
          ))}

        {/* ⚠️ DEFERRED (i18n): Tune keys a carrier and is on the Phone, CW, Operate, RTTY and
            PSK stop-line censuses; `stop-line.test.tsx` finds it by accessible name. */}
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

        {/* The RADIO's own ATU, beside the carrier Tune it is so often confused with. Shown ONLY
            when the rig reports a tuner (`radio.atu` non-null) — an ATU button on a radio with no
            ATU is worse than no button. Disabled on the licence lockout exactly like Tune; every
            other refusal (TX off, transmitter busy) comes back from the backend WITH ITS REASON
            and is shown, because a control that keys the transmitter must never fail silently.

            ⚠️ DEFERRED (i18n): it keys the rig's own tuning carrier, exactly as SetupHealth's
            Prove TX does. */}
        {onAtuTune && radio.atu != null && (
          <button
            type="button"
            className="cockpit-tune"
            onClick={onAtuTune}
            disabled={!radio.txAllowed}
            title={
              radio.atu
                ? "Run the radio's built-in antenna tuner (it transmits its own carrier for a second or two). The tuner is switched in."
                : "Run the radio's built-in antenna tuner (it transmits its own carrier for a second or two). The tuner is currently bypassed."
            }
          >
            ATU
          </button>
        )}

        {/* ⚠️ DEFERRED (i18n): THE stop control of six cockpits. Its label is the accessible
            name every stop-line sweep looks for (/^stop tx$/i). */}
        {onStopTx && (
          <button type="button" className="cockpit-stoptx" onClick={onStopTx} title="Stop TX (Esc)">
            Stop TX
          </button>
        )}

        {catStatus ?? (
          <span
            className={`cockpit-cat ${catOk ? 'ok' : 'bad'}`}
            // `catDetail` is the backend's own report and passes through verbatim.
            title={
              radio.catDetail ||
              (catOk ? t('cockpit.header.cat.ok.title') : t('cockpit.header.cat.bad.title'))
            }
          >
            {catOk ? CAT_OK : CAT_BAD}
          </span>
        )}
      </div>
    </div>
  )
}
