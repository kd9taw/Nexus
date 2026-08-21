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
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The
// LABEL is prose; the `value` beside it is the persisted token the settings
// file and the engine read, and the two are not the same string. The words
// resolve LAZILY, through a getter, for the reason `needVisuals.ts` documents:
// this array is a module constant that two components read during render, so
// resolving at import time would freeze whichever locale loaded first.
import { t, type MessageKey } from '../i18n'
import type { Settings } from '../types'

type SatVfoMapValue = NonNullable<Settings['satVfoMap']>

/** Wire value → the key of the words that describe it. */
const SAT_VFO_MAP_KEYS: { value: SatVfoMapValue; labelKey: MessageKey }[] = [
  { value: 'off', labelKey: 'sat.vfoMap.off' },
  { value: 'downlink-only', labelKey: 'sat.vfoMap.downlinkOnly' },
  { value: 'uplink-only', labelKey: 'sat.vfoMap.uplinkOnly' },
  { value: 'a-down-b-up', labelKey: 'sat.vfoMap.aDownBUp' },
  { value: 'a-up-b-down', labelKey: 'sat.vfoMap.aUpBDown' },
  { value: 'main-down-sub-up', labelKey: 'sat.vfoMap.mainDownSubUp' },
  { value: 'main-up-sub-down', labelKey: 'sat.vfoMap.mainUpSubDown' },
]

export const SAT_VFO_MAPS: { value: SatVfoMapValue; label: string }[] = SAT_VFO_MAP_KEYS.map(
  (m): { value: SatVfoMapValue; label: string } => ({
    value: m.value,
    get label() {
      return t(m.labelKey)
    },
  }),
)

/** The human label for a mapping value ("Main = downlink, Sub = uplink…"). */
export function satVfoLabel(v: Settings['satVfoMap']): string {
  return SAT_VFO_MAPS.find((m) => m.value === (v ?? 'off'))?.label ?? t('sat.vfoMap.off')
}

/** The mapping named for a SENTENCE — the label without its parenthetical
 * example ("Main = downlink, Sub = uplink"). The select needs the example to
 * help an operator find their rig; a sentence that already names the radio
 * does not, and would read as "your IC-9700 … (IC-9700 full duplex)".
 *
 * It strips the parenthetical from whatever the catalog gave it rather than
 * carrying a second entry per mapping: a translated label keeps the example in
 * the same parentheses, so one rule holds for every locale. */
export function satVfoPair(v: Settings['satVfoMap']): string {
  return satVfoLabel(v).replace(/\s*\(.*\)$/, '')
}

