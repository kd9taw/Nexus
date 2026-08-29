// The Amplifier pane — a READ-ONLY status readout for a linear on its own serial port.
//
// ⛔ NOTHING HERE STOPS A TRANSMISSION, AND NOTHING HERE MAY EVER BE MADE TO. Putting an
// amplifier in standby is not a way to stop an over — the exciter keeps keying and the drive
// passes straight through — so this pane has no control of any kind, appears on no cockpit's
// stop-line census, and must never be added to a sweep's `stopControls`. It renders numbers.
//
// TWO STATES, and keeping them apart is the whole honesty argument (settings.rs, `amp_port`:
// "unconfigured shows nothing, configured-and-silent shows '—'"):
//   • no amplifier CONFIGURED → `amp == null` → return null, and PaneFrame falls back to the
//     Basic hint. A readout with no amplifier behind it would be an ornament.
//   • configured and NOT ANSWERING → the pane STAYS, every reading '—', with the reason named.
//     RotorPane.tsx records what the other choice costs: `if (az == null) return null` deleted
//     the rose, the slew and the STOP button the moment a readback failed.
//
// ⚠️ THIS FILE IS ON THE **MIGRATED** LIST (i18n/hardcoded-strings.test.ts): every sentence is
// in the catalog under `amp.*`. What stays in the code is the instrument's own vocabulary —
// the unit symbols W, V, A, ° and the `:1` of an SWR ratio, and the three named constants
// below. Every NUMBER is built here and rounded before display: `invariantNumber` is
// `String(n)`, so a raw f32 SWR of 1.2000000476837158 would interpolate verbatim.
import type { AmpStatus } from '../../types'
import { t } from '../../i18n'

/** The instrument's own marks — technical tokens exactly as a meter name is, gathered here so
 *  the catalog guard reads them as the deliberate constants they are (TxMeters.tsx's pattern). */
const SWR = 'SWR'
const ATU = 'ATU'
const VDC = 'Vdc'
/** Absence. Not prose — the em dash IS the reading here, exactly as it is on the S-meter. */
const DASH = '—'

/** Wire tag → the sentence for it, each as a THUNK.
 *
 *  Thunks, not resolved strings: this is a module constant read during render, so calling `t`
 *  at import time would freeze whichever locale loaded first — the same reason the Connect
 *  pane registry resolves its titles through getters.
 *
 *  ⭐ EVERY LOOKUP IS TOTAL, AND THE FALLBACK IS THE FAULT. An alarm code a later firmware
 *  ships arrives with the tag `unknown` and must reach the operator as "an alarm we cannot
 *  name", never as silence — the failure direction of a status decoder in front of a kilowatt
 *  is toward reporting a fault. The backend flattens its enums to these flat tags precisely so
 *  this stays one lookup instead of a TS union that falls through on a shape it did not expect.
 */
const REASON: Record<string, () => string> = {
  portBusy: () => t('amp.reason.portBusy'),
  noAnswer: () => t('amp.reason.noAnswer'),
  wrongModel: () => t('amp.reason.wrongModel'),
  malformed: () => t('amp.reason.malformed'),
}
const ALARM: Record<string, () => string> = {
  swrExceedingLimits: () => t('amp.alarm.swrExceedingLimits'),
  amplifierProtection: () => t('amp.alarm.amplifierProtection'),
  inputOverdriving: () => t('amp.alarm.inputOverdriving'),
  excessOverheating: () => t('amp.alarm.excessOverheating'),
  combinerFault: () => t('amp.alarm.combinerFault'),
  fault: () => t('amp.alarm.fault'),
}
const WARNING: Record<string, () => string> = {
  alarmAmplifier: () => t('amp.warning.alarmAmplifier'),
  noSelectedAntenna: () => t('amp.warning.noSelectedAntenna'),
  swrAntenna: () => t('amp.warning.swrAntenna'),
  noValidBand: () => t('amp.warning.noValidBand'),
  powerLimitExceeded: () => t('amp.warning.powerLimitExceeded'),
  overheating: () => t('amp.warning.overheating'),
  atuNotAvailable: () => t('amp.warning.atuNotAvailable'),
  tuningWithNoPower: () => t('amp.warning.tuningWithNoPower'),
  atuBypassed: () => t('amp.warning.atuBypassed'),
  powerSwitchHeldByRemote: () => t('amp.warning.powerSwitchHeldByRemote'),
  combinerOverheating: () => t('amp.warning.combinerOverheating'),
  combinerFault: () => t('amp.warning.combinerFault'),
}

/** Watts, rounded — never a raw float. */
const fmtWatts = (w: number) => `${Math.round(w)} W`
/** An SWR ratio, one decimal, in the form a rig's own meter prints it. */
const fmtSwr = (s: number) => `${s.toFixed(1)}:1`

/**
 * ⚠️ NEVER ROUTE A TEMPERATURE THROUGH `units.ts`. `fmtTempF` assumes FAHRENHEIT input and
 * would render an SPE reading of 33 as `1°C` on a metric install.
 *
 * The scale letter is licensed by `tempCelsius` and by nothing else. Elecraft's `^TM` is
 * documented Celsius, so the KPA gets `52 °C`. SPE's §5 says "Temp in °C or F" — the amplifier
 * reports whatever its own front panel is set to and the wire does not say which — so it gets
 * a bare `41°` with a degree sign and NO letter, plus the tooltip below. A guessed °C is a
 * false statement half the time.
 */
const fmtTemp = (deg: number, celsius: boolean) => (celsius ? `${deg} °C` : `${deg}°`)

/** One label/value cell. `null` value renders the em dash — absence stays absent. */
function Cell({
  k,
  v,
  title,
  tone,
}: {
  k: string
  v: string | null
  title?: string
  tone?: 'alarm' | 'warn'
}) {
  return (
    <div className="amp-cell" title={title}>
      <span className="amp-k">{k}</span>
      <span className={tone ? `amp-v amp-${tone}` : 'amp-v'}>{v ?? DASH}</span>
    </div>
  )
}

export function AmpPane({ amp }: { amp: AmpStatus | null | undefined }) {
  // No amplifier configured on the active radio. The Basic hint takes over.
  if (amp == null) return null

  const linked = amp.linked
  // ⭐ EVERY READING IS GATED ON `linked`, not merely on its own presence. The backend already
  // clears each one on the first failed poll — a stale wattage in front of a kilowatt is a
  // fabrication — and this is the second half of the same rule: nothing from a dead link
  // reaches the screen even if a field somehow survived it.
  const val = <T,>(x: T | null | undefined): T | null => (linked && x != null ? x : null)

  const watts = val(amp.outputWatts)
  const swr = val(amp.swr)
  const swrAtu = val(amp.swrAtu)
  const volts = val(amp.volts)
  const amps = val(amp.amps)
  const temp = val(amp.temp)
  const operate = val(amp.operate)

  // ⭐ COLOUR FROM THE AMPLIFIER'S OWN JUDGEMENT, NEVER FROM A THRESHOLD OF OURS. `alarmRaised`
  // counts an alarm letter no firmware we know ships — the decoder's failure direction is
  // toward reporting a fault, not toward silence — and a tag comparison here would undo that.
  // And the temperature is deliberately NOT thresholded or coloured: 60 °C and 60 °F are not
  // the same amplifier state, and we do not know which one an SPE is reporting.
  const alarm = linked && amp.alarmRaised
  const warning = linked && amp.warningRaised

  return (
    <div className="amp-pane">
      <div className="amp-head">
        <span className={linked ? 'amp-link' : 'amp-link amp-down'}>
          {linked ? t('amp.link.up') : (REASON[amp.reason] ?? REASON.noAnswer)()}
        </span>
        {/* The amplifier's own model id, raw. Empty for a KPA, which reports none. */}
        {amp.model !== '' && <span className="amp-model">{amp.model}</span>}
        {operate != null && (
          <span className="amp-state">{operate ? t('amp.operate') : t('amp.standby')}</span>
        )}
      </div>

      <div className="amp-grid">
        <Cell k={t('amp.k.power')} v={watts == null ? null : fmtWatts(watts)} />
        <Cell k={SWR} v={swr == null ? null : fmtSwr(swr)} title={t('amp.swr.title')} />
        {/* Pre-ATU SWR is SPE-only; the cell is simply absent for a KPA rather than an
            eternal '—' that reads as a broken reading. */}
        {amp.swrAtu != null && (
          <Cell
            k={`${SWR} ${ATU}`}
            v={swrAtu == null ? null : fmtSwr(swrAtu)}
            title={t('amp.swrAtu.title')}
          />
        )}
        <Cell
          k={t('amp.k.temp')}
          v={temp == null ? null : fmtTemp(temp, amp.tempCelsius)}
          // The one place the missing unit is explained, and it is explained rather than
          // guessed at.
          title={amp.tempCelsius ? undefined : t('amp.temp.unknownScale')}
        />
        <Cell k={VDC} v={volts == null ? null : `${volts.toFixed(1)} V`} />
        <Cell k={t('amp.k.current')} v={amps == null ? null : `${amps.toFixed(1)} A`} />
      </div>

      {/* FAULTS ARE RENDERED AS FAULTS, including the `unknown` tag — an alarm letter a later
          firmware ships must reach the operator, not go quiet. */}
      {alarm && (
        <p className="amp-fault amp-alarm">
          {(ALARM[amp.alarm] ?? (() => t('amp.alarm.unknown')))()}
          {amp.kpaFault != null && amp.kpaFault !== 0 ? ` (${amp.kpaFault})` : ''}
        </p>
      )}
      {warning && (
        <p className="amp-fault amp-warn">
          {(WARNING[amp.warning] ?? (() => t('amp.warning.unknown')))()}
        </p>
      )}
    </div>
  )
}
