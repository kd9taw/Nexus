// What a P.533 answer rests on, said beside it: the transmit power it assumes and where that
// figure came from, and the storm correction laid over it when there is one. The heuristic
// ignores power, so for its answers only a storm line could apply, and it never makes one.
import type { AssumedPower, StormNote } from '../../types'
import { t } from '../../i18n'

/** The Kp from which the line says "storm": NOAA's G1. */
const STORM_KP = 5

/** Watts as the line says them: whole from 10 W, tenths below. */
function wattsText(w: number): string {
  return w >= 10 ? String(Math.round(w)) : String(Math.round(w * 10) / 10)
}

export function modelBasisLines(engine: string | undefined, power?: AssumedPower, storm?: StormNote): string[] {
  const lines: string[] = []
  if (storm) {
    // Decided on the Kp as shown, so the words never say "unsettled, Kp up to 5.0". Below 5,
    // NOAA's G1, the same correction reads as unsettled conditions, not a storm.
    const kp = storm.kp.toFixed(1)
    lines.push(Number(kp) >= STORM_KP ? t('prop.storm.applied', { kp }) : t('prop.storm.unsettled', { kp }))
    if (storm.hoursWithoutKp) lines.push(t('prop.storm.someHours'))
  }
  if (engine === 'p533' && power) {
    const watts = wattsText(power.watts)
    if (power.source === 'setting') lines.push(t('prop.power.setting', { watts }))
    else if (power.source === 'rig') lines.push(t('prop.power.rig', { watts }))
    else if (power.source === 'amplifier') lines.push(t('prop.power.amplifier', { watts }))
    else {
      lines.push(t('prop.power.default'))
      if (power.amplifier) lines.push(t('prop.power.amplifierSetUp'))
    }
  }
  return lines
}

export function ModelBasis({
  engine,
  power,
  storm,
  className = 'cp-muf',
}: {
  engine: string | undefined
  power?: AssumedPower
  storm?: StormNote
  className?: string
}) {
  const lines = modelBasisLines(engine, power, storm)
  if (lines.length === 0) return null
  return <p className={`${className} model-basis`}>{lines.join(' ')}</p>
}
