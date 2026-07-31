// The satellite VFO-mapping enumeration — ONE copy, shared by Settings ▸ Radio
// (the canonical store) and the Satellites readiness rail (a live mirror of the
// same setting). Two drifting copies of a list that decides WHERE THE RADIO
// TRANSMITS would be a wrong-uplink generator, so the labels live here.
//
// Off first — it is the default and the only value that writes nothing to the
// radio. The labels name BOTH legs because naming only one is how an operator
// ends up transmitting on the downlink.
import type { Settings } from '../types'

export const SAT_VFO_MAPS: { value: NonNullable<Settings['satVfoMap']>; label: string }[] = [
  { value: 'off', label: 'Off' },
  { value: 'downlink-only', label: 'Downlink only (receive)' },
  { value: 'uplink-only', label: 'Uplink only (transmit)' },
  { value: 'a-down-b-up', label: 'VFO A = downlink, VFO B = uplink' },
  { value: 'a-up-b-down', label: 'VFO A = uplink, VFO B = downlink' },
  { value: 'main-down-sub-up', label: 'Main = downlink, Sub = uplink (IC-9700 full duplex)' },
  { value: 'main-up-sub-down', label: 'Main = uplink, Sub = downlink' },
]

/** The human label for a mapping value ("Main = downlink, Sub = uplink…"). */
export function satVfoLabel(v: Settings['satVfoMap']): string {
  return SAT_VFO_MAPS.find((m) => m.value === (v ?? 'off'))?.label ?? 'Off'
}
