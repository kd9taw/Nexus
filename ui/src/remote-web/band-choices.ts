import type { BandChannel } from '../types'
import { finite, object, rows, text } from './display-validation'
import { stationAction } from './station-operation'

/** Derived station-owned choices, carried by the existing Settings read. Older
 * stations simply omit this extension. Never construct default dials here. */
export function readBandChoices(settings: unknown, mode: 'cw' | 'phone'): BandChannel[] {
  if (!settings || typeof settings !== 'object' || !('bandChoices' in settings)) throw Error('stationUpdateRequired')
  const plans = object(settings.bandChoices, ['cw', 'phone'])
  const plan = rows(plans[mode], 32), bands = new Set<string>()
  return plan.map(raw => {
    const c = object(raw, ['band', 'group', 'dialMhz', 'mode', 'label', 'note', 'tx'])
    stationAction({ action: 'radio.band', band: c.band, mode })
    if (!text(c.band, 16) || bands.has(c.band) || !text(c.group, 16) || !finite(c.dialMhz) || c.dialMhz <= 0 || c.dialMhz > 250000 ||
      typeof c.mode !== 'string' || !['USB', 'LSB', 'FM', 'AM'].includes(c.mode) || !text(c.label, 128) || !text(c.note, 512) || typeof c.tx !== 'boolean') throw Error('invalidStationDisplay')
    bands.add(c.band)
    return c as unknown as BandChannel
  })
}
