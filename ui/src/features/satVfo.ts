// The satellite VFO-mapping enumeration — ONE copy, shared by Settings ▸ Radio
// and the Satellites readiness rail. Two drifting copies of a list that decides
// WHERE THE RADIO TRANSMITS would be a wrong-uplink generator, so the labels
// live here. LABELS ONLY: the consent write itself is the backend verb
// (`confirmSatUplink` in api.ts → `Engine::confirm_sat_uplink`) — the pair is
// engine-owned live state a settings payload cannot carry, so the
// change-retires-other-consents rule has exactly one home, in the language the
// engine reads it in (settings.rs).
//
// Off first — it is the default: no uplink mapping, so nothing is written to a
// transmit VFO. (The downlink needs no mapping and is corrected without one.)
// The labels name BOTH legs because naming only one is how an operator ends up
// transmitting on the downlink.
import type { Settings } from '../types'

export const SAT_VFO_MAPS: { value: NonNullable<Settings['satVfoMap']>; label: string }[] = [
  { value: 'off', label: 'Not set — downlink only' },
  { value: 'downlink-only', label: 'Downlink only (receive)' },
  { value: 'uplink-only', label: 'Uplink only (transmit)' },
  { value: 'a-down-b-up', label: 'VFO A = downlink, VFO B = uplink' },
  { value: 'a-up-b-down', label: 'VFO A = uplink, VFO B = downlink' },
  { value: 'main-down-sub-up', label: 'Main = downlink, Sub = uplink (IC-9700 full duplex)' },
  { value: 'main-up-sub-down', label: 'Main = uplink, Sub = downlink' },
]

/** The human label for a mapping value ("Main = downlink, Sub = uplink…"). */
export function satVfoLabel(v: Settings['satVfoMap']): string {
  return SAT_VFO_MAPS.find((m) => m.value === (v ?? 'off'))?.label ?? 'Not set — downlink only'
}

/** The mapping named for a SENTENCE — the label without its parenthetical
 * example ("Main = downlink, Sub = uplink"). The select needs the example to
 * help an operator find their rig; a sentence that already names the radio
 * does not, and would read as "your IC-9700 … (IC-9700 full duplex)". */
export function satVfoPair(v: Settings['satVfoMap']): string {
  return satVfoLabel(v).replace(/\s*\(.*\)$/, '')
}

