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
// catalog (`i18n/en.ts`); a hardcoded one fails CI. What does NOT come from the catalog is the
// <option> values below — see the invariant-token rule in `i18n/index.ts`.

import type { ReactNode } from 'react'
import { t } from '../i18n'
import type { MessageKey } from '../i18n'
import type { BandChannel, ExperienceLevel, Settings } from '../types'
import { EXPERIENCE_LEVELS } from '../settings/experience'
import { FrequencyControl } from './FrequencyControl'
import { StationProfiles } from './StationProfiles'
import { drivesAnotherStation } from '../features/durableStore'

type FieldKey = keyof Settings

interface StationField {
  key: FieldKey
  labelKey: MessageKey
  hintKey: MessageKey
  /** What an empty box says, when it says anything: words about what goes in it, never a
   *  sample value — a sample in grey reads as a value already filled in. */
  placeholderKey?: MessageKey
}

const STATION_FIELDS: StationField[] = [
  {
    key: 'mycall',
    labelKey: 'settings.station.callsign.label',
    hintKey: 'settings.station.callsign.hint',
  },
  {
    key: 'mygrid',
    labelKey: 'settings.station.grid.label',
    hintKey: 'settings.station.grid.hint',
  },
  {
    key: 'opName',
    labelKey: 'settings.station.opName.label',
    hintKey: 'settings.station.opName.hint',
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
    placeholderKey: 'settings.station.fdOperator.placeholder',
  },
  {
    key: 'opState',
    labelKey: 'settings.station.opState.label',
    hintKey: 'settings.station.opState.hint',
  },
]

/** The persisted licence-class VALUES are tokens; only the labels are prose. */
const LICENSE_CLASSES: { value: string; labelKey: MessageKey }[] = [
  { value: 'novice', labelKey: 'settings.station.licenseClass.novice' },
  { value: 'technician', labelKey: 'settings.station.licenseClass.technician' },
  { value: 'general', labelKey: 'settings.station.licenseClass.general' },
  { value: 'advanced', labelKey: 'settings.station.licenseClass.advanced' },
  { value: 'extra', labelKey: 'settings.station.licenseClass.extra' },
  { value: 'open', labelKey: 'settings.station.licenseClass.open' },
]

interface Props {
  readOnly?: boolean
  form: Settings
  /** The panel's live save error. Drives the invalid styling on an empty callsign. */
  error: string | null
  /** Band-plan channels for the frequency control. */
  bandPlan: BandChannel[]
  onUpdate: (key: FieldKey, raw: string) => void
  onSetFreq: (dialMhz: number, band: string, mode: string) => void
  /** Units is on the Remote allow-list even though the rest of this section is not, so a
   *  browser may still change it (#248 moved it here from Digital). Undefined off Remote. */
  unitsLocked?: boolean
  /** From a computer driving the station: a setting the save from there keeps as the station has
   *  it shows read-only, and `setAtStation` says so. Absent at the station. */
  locked?: (key: FieldKey) => boolean
  setAtStation?: (key: FieldKey) => ReactNode
  /** Saves a new experience answer through its one writer (and re-picks the starting features).
   *  Absent: the select is not shown. */
  onSetExperience?: (level: ExperienceLevel) => void
  /** After a "My station" profile was saved or removed through its own writer: the panel re-reads
   *  the station's fields. Absent: the profiles are not shown. */
  onStationProfilesChanged?: () => void
}

export function SettingsStation({ readOnly=false, form, error, bandPlan, onUpdate, onSetFreq, unitsLocked, locked, setAtStation, onSetExperience, onStationProfilesChanged }: Props) {
  return (
    // The SECTION is never disabled; the groups inside it are. A form control inside a disabled
    // <fieldset> is disabled whatever its own `disabled` says, and a nested fieldset cannot take
    // that back — so the one control a Remote browser may write (Units) has to render OUTSIDE the
    // disabled group, not merely with `disabled={false}`. The id and class stay here so the
    // settings registry, the deep link and the generated manual still resolve the section.
    <fieldset className="settings-section" id="settings-operator-radio">
      <legend>{t('settings.station.legend')}</legend>
      <fieldset disabled={readOnly} className="settings-fieldgroup">
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
                placeholder={f.placeholderKey ? t(f.placeholderKey) : undefined}
                onChange={(e) => onUpdate(f.key, e.target.value)}
                disabled={locked?.(f.key)}
                aria-invalid={invalid && !!error}
                autoComplete="off"
                spellCheck={false}
              />
              <span className="settings-hint">{t(f.hintKey)}</span>
              {/* From afar, a field the station keeps as it has it: the call, while the station has
                  "My station" profiles (`stationOnlySettings`). */}
              {setAtStation?.(f.key)}
            </label>
          )
        })}
        <label className="settings-field">
          <span className="settings-label">{t('settings.station.licenseClass.label')}</span>
          <select disabled={locked?.('licenseClass')} data-setting="licenseClass"
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
          {setAtStation?.('licenseClass')}
          <span className="settings-hint">{t('settings.station.licenseClass.hint')}</span>
        </label>
        {/* The first-run wizard's question, changeable here. Saved on change (its own writer),
            not on Save. "Not set" is offered only while it is: an existing station that updated
            has never been asked, and nothing changes until it picks one. */}
        {onSetExperience && (
          <label className="settings-field">
            <span className="settings-label">{t('settings.station.experience.label')}</span>
            <select
              aria-label={t('settings.station.experience.label')}
              className="settings-input"
              value={form.experienceLevel ?? ''}
              onChange={(e) => e.target.value && onSetExperience(e.target.value as ExperienceLevel)}
            >
              {!form.experienceLevel && (
                <option value="" disabled>
                  {t('settings.station.experience.notSet')}
                </option>
              )}
              {EXPERIENCE_LEVELS.map((l) => (
                <option key={l.value} value={l.value}>
                  {l.label}
                </option>
              ))}
            </select>
            <span className="settings-hint">{t('settings.station.experience.hint')}</span>
          </label>
        )}
      </div>
      </fieldset>
      {/* "My station" profiles: saved as they are saved, by their own writer, so they sit outside
          the form's disabled group and take `readOnly` themselves; from a computer driving the
          station they are shown, and set at the station only. */}
      {onStationProfilesChanged && (
        <StationProfiles form={form} readOnly={readOnly || drivesAnotherStation()} onChanged={onStationProfilesChanged} />
      )}
      {/* #248: km or miles, °C or °F, for the whole app. It sat under Digital ▸ Station
          Housekeeping, where nobody looking for miles would look. The option VALUES are
          persisted tokens; only the labels are prose. A separate <label htmlFor> keeps the
          select's accessible name exactly "Units" (a wrapping label would fold the hint in).
          It renders here, between the two disabled groups, because it is the one setting in
          this section on the Remote writable allow-list: inside either group it would be dead
          however `unitsLocked` read. */}
      <div className="settings-grid settings-grid--lone">
        <div className="settings-field">
          <label className="settings-label" htmlFor="units">
            {t('settings.digital.units.label')}
          </label>
          <select
            id="units"
            disabled={unitsLocked ?? readOnly}
            className="settings-input"
            value={form.units ?? 'auto'}
            onChange={(e) => onUpdate('units', e.target.value)}
          >
            <option value="auto">{t('settings.digital.units.auto')}</option>
            <option value="metric">{t('settings.digital.units.metric')}</option>
            <option value="imperial">{t('settings.digital.units.imperial')}</option>
          </select>
          <span className="settings-hint">{t('settings.digital.units.hint')}</span>
        </div>
      </div>
      <fieldset disabled={readOnly} className="settings-fieldgroup">
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
    </fieldset>
  )
}
