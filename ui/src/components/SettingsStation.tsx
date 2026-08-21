// Settings ▸ Station — the Operator & Radio fieldset.
//
// EXTRACTED FROM SettingsPanel.tsx (2026-08-18) as the i18n pilot. The extraction is not
// cosmetic: the guard that keeps migrated surfaces from drifting back to hardcoded English
// (`i18n/hardcoded-strings.test.ts`) works on whole files, and an 8,900-line panel with one
// migrated fieldset in it cannot be a unit of anything. The DOM this renders is byte-identical
// to what the panel rendered — same fieldset, same `id`, same class names, same order — so the
// deep-link anchor, the width guards and the registry guard all still find it.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST. Every operator-visible string here comes from the
// catalog (`i18n/en.ts`); a hardcoded one fails CI. What does NOT come from the catalog is
// listed in STATION_EXAMPLES and in the <option> values below — see the invariant-token rule
// in `i18n/index.ts`. `KD9TAW` is a callsign, `EN52xa` is a grid, `WI` is a section/state
// code; they are the same characters in every language, and a translator who "localises" one
// has broken an example of a wire format.

import { t } from '../i18n'
import type { MessageKey } from '../i18n'
import type { BandChannel, Settings } from '../types'
import { FrequencyControl } from './FrequencyControl'

type FieldKey = keyof Settings

/**
 * The example shown in an empty field. The two cases are NOT interchangeable and the union
 * exists to make an author choose:
 *   `token` — a value from a technical namespace (callsign, grid, section code). Invariant.
 *   `prose` — a human example (a first name, an instruction). A catalog key.
 */
type Example = { token: string } | { prose: MessageKey }

interface StationField {
  key: FieldKey
  labelKey: MessageKey
  hintKey: MessageKey
  example: Example
}

/**
 * The invariant example values this surface shows, gathered so the guard can prove they never
 * became catalog entries. Do not inline them — a bare string in a `placeholder=` is exactly
 * what the guard refuses, and rightly: it is the moment someone has to decide which category
 * the value is in.
 */
export const STATION_EXAMPLES = {
  callsign: 'KD9TAW',
  grid: 'EN52xa',
  state: 'WI',
} as const

const STATION_FIELDS: StationField[] = [
  {
    key: 'mycall',
    labelKey: 'settings.station.callsign.label',
    hintKey: 'settings.station.callsign.hint',
    example: { token: STATION_EXAMPLES.callsign },
  },
  {
    key: 'mygrid',
    labelKey: 'settings.station.grid.label',
    hintKey: 'settings.station.grid.hint',
    example: { token: STATION_EXAMPLES.grid },
  },
  {
    key: 'opName',
    labelKey: 'settings.station.opName.label',
    hintKey: 'settings.station.opName.hint',
    example: { prose: 'settings.station.opName.placeholder' },
  },
  // #25 — who is AT THE KEY, as opposed to whose station it is. Two different questions, and
  // ADIF has two fields because the answers differ: STATION_CALLSIGN is the callsign above,
  // OPERATOR is this. Set it and every contact you log carries it, so a two-person activation
  // can be split by operator afterwards instead of hand-edited. This setting already existed
  // but only inside the Field Day view, where a POTA pair would never find it — and it only
  // ever reached the N3FJP feed, never the operator's own log.
  {
    key: 'fdOperator',
    labelKey: 'settings.station.fdOperator.label',
    hintKey: 'settings.station.fdOperator.hint',
    example: { prose: 'settings.station.fdOperator.placeholder' },
  },
  {
    key: 'opState',
    labelKey: 'settings.station.opState.label',
    hintKey: 'settings.station.opState.hint',
    example: { token: STATION_EXAMPLES.state },
  },
]

/** The persisted licence-class VALUES are tokens; only the labels are prose. */
const LICENSE_CLASSES: { value: string; labelKey: MessageKey }[] = [
  { value: 'technician', labelKey: 'settings.station.licenseClass.technician' },
  { value: 'general', labelKey: 'settings.station.licenseClass.general' },
  { value: 'extra', labelKey: 'settings.station.licenseClass.extra' },
  { value: 'open', labelKey: 'settings.station.licenseClass.open' },
]

const placeholderFor = (e: Example): string => ('token' in e ? e.token : t(e.prose))

interface Props {
  form: Settings
  /** The panel's live save error. Drives the invalid styling on an empty callsign. */
  error: string | null
  /** Band-plan channels for the frequency control. */
  bandPlan: BandChannel[]
  onUpdate: (key: FieldKey, raw: string) => void
  onSetFreq: (dialMhz: number, band: string, mode: string) => void
}

export function SettingsStation({ form, error, bandPlan, onUpdate, onSetFreq }: Props) {
  return (
    <fieldset className="settings-section" id="settings-operator-radio">
      <legend>{t('settings.station.legend')}</legend>
      <div className="settings-grid">
        {STATION_FIELDS.map((f) => {
          const value = form[f.key]
          const invalid = f.key === 'mycall' && !String(value).trim()
          return (
            <label className="settings-field" key={f.key}>
              <span className="settings-label">{t(f.labelKey)}</span>
              <input
                className={`settings-input${invalid && error ? ' invalid' : ''}`}
                type="text"
                value={String(value)}
                placeholder={placeholderFor(f.example)}
                onChange={(e) => onUpdate(f.key, e.target.value)}
                aria-invalid={invalid && !!error}
                autoComplete="off"
                spellCheck={false}
              />
              <span className="settings-hint">{t(f.hintKey)}</span>
            </label>
          )
        })}
        <label className="settings-field">
          <span className="settings-label">{t('settings.station.licenseClass.label')}</span>
          <select
            className="settings-input"
            value={String(form.licenseClass ?? 'open')}
            onChange={(e) => onUpdate('licenseClass', e.target.value)}
          >
            {LICENSE_CLASSES.map((c) => (
              <option key={c.value} value={c.value}>
                {t(c.labelKey)}
              </option>
            ))}
          </select>
          <span className="settings-hint">{t('settings.station.licenseClass.hint')}</span>
        </label>
      </div>
      <div className="settings-freq">
        <span className="settings-label">{t('settings.station.frequency.label')}</span>
        <FrequencyControl
          channels={bandPlan}
          dialMhz={form.dialMhz}
          band={form.band}
          mode={form.sideband}
          variant="full"
          onSet={onSetFreq}
        />
        <span className="settings-hint">{t('settings.station.frequency.hint')}</span>
      </div>
    </fieldset>
  )
}
