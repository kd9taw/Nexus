// Settings ▸ Station ▸ My station profiles: Home, a portable kit, a club call. Each is saved the
// moment it is saved here, through its own writer (`saveStationProfile`), never by the panel's
// Save: a profile is the call the station transmits under, and a whole-settings save keeps the
// live list (`Engine::apply_settings`). The top bar's picker switches between them.
//
// ⚠️ ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts): every operator-visible string comes
// from the catalog. The call, grid, park and IOTA references are tokens and stay as typed.
import { useState } from 'react'
import { t } from '../i18n'
import type { Settings, StationProfile } from '../types'
import { HOME_STATION_PROFILE } from '../types'
import { removeStationProfile, saveStationProfile } from '../api'
import { withErrorToast } from '../toast'

/** The name a profile shows: Home's in the reader's language, the others as the operator named
 *  them. */
export function stationProfileName(p: { id: string; name: string }): string {
  return p.id === HOME_STATION_PROFILE && !p.name ? t('settings.stationProfiles.home') : p.name
}

/** A new profile: the station as it is now, so only what differs needs typing. */
function fromStation(form: Settings): StationProfile {
  return {
    id: '',
    name: '',
    call: form.mycall ?? '',
    grid: form.mygrid ?? '',
    state: form.opState ?? '',
    county: '',
    cqZone: null,
    ituZone: null,
    iota: '',
    park: '',
    powerW: form.stationPowerW ?? null,
    rig: '',
    antenna: '',
  }
}

type TextKey = 'name' | 'call' | 'grid' | 'state' | 'county' | 'iota' | 'park' | 'rig' | 'antenna'
type NumberKey = 'cqZone' | 'ituZone' | 'powerW'

// Each label is read through a literal key, so the catalog guards can check every call site.
const TEXT_FIELDS: { key: TextKey; label: () => string; hint?: () => string }[] = [
  { key: 'name', label: () => t('settings.stationProfiles.name') },
  { key: 'call', label: () => t('settings.stationProfiles.call'), hint: () => t('settings.stationProfiles.call.hint') },
  { key: 'grid', label: () => t('settings.stationProfiles.grid') },
  { key: 'state', label: () => t('settings.stationProfiles.state') },
  { key: 'county', label: () => t('settings.stationProfiles.county'), hint: () => t('settings.stationProfiles.county.hint') },
  { key: 'iota', label: () => t('settings.stationProfiles.iota') },
  { key: 'park', label: () => t('settings.stationProfiles.park'), hint: () => t('settings.stationProfiles.park.hint') },
  { key: 'rig', label: () => t('settings.stationProfiles.rig') },
  { key: 'antenna', label: () => t('settings.stationProfiles.antenna') },
]

const NUMBER_FIELDS: { key: NumberKey; label: () => string }[] = [
  { key: 'cqZone', label: () => t('settings.stationProfiles.cqZone') },
  { key: 'ituZone', label: () => t('settings.stationProfiles.ituZone') },
  { key: 'powerW', label: () => t('settings.stationProfiles.power') },
]

interface Props {
  form: Settings
  /** From a computer driving the station, or a read-only form: shown, not edited. */
  readOnly: boolean
  /** After a profile was saved or removed: the panel re-reads the station's fields. */
  onChanged: () => void
}

export function StationProfiles({ form, readOnly, onChanged }: Props) {
  const [editing, setEditing] = useState<StationProfile | null>(null)
  const profiles = form.stationProfiles ?? []
  const active = form.activeStationProfile ?? ''

  const edit = (p: StationProfile) =>
    // The profile on the air shows the station's call, grid, state and power: they are its.
    setEditing(
      p.id === active
        ? { ...p, call: form.mycall, grid: form.mygrid, state: form.opState ?? '', powerW: form.stationPowerW ?? null }
        : { ...p },
    )

  const save = async () => {
    if (!editing) return
    const done = await withErrorToast(() => saveStationProfile(editing), t('settings.stationProfiles.failed'))
    if (!done) return
    setEditing(null)
    onChanged()
  }

  const remove = async (id: string) => {
    const done = await withErrorToast(() => removeStationProfile(id), t('settings.stationProfiles.failed'))
    if (done) onChanged()
  }

  return (
    <div className="settings-featgroup" data-testid="station-profiles">
      <span className="settings-featgroup-title">{t('settings.stationProfiles.title')}</span>
      <span className="settings-hint">{t('settings.stationProfiles.hint')}</span>
      {profiles.map((p) => (
        <div className="settings-field" key={p.id}>
          <span className="settings-label">
            {stationProfileName(p)} · {p.id === active ? form.mycall : p.call}
            {p.id === active && <> · {t('settings.stationProfiles.active')}</>}
          </span>
          {!readOnly && (
            <span>
              <button type="button" className="settings-linkbtn" onClick={() => edit(p)}>
                {t('settings.stationProfiles.edit')}
              </button>
              {p.id !== HOME_STATION_PROFILE && p.id !== active && (
                <>
                  {' '}
                  <button type="button" className="settings-linkbtn" onClick={() => void remove(p.id)}>
                    {t('settings.stationProfiles.remove')}
                  </button>
                </>
              )}
            </span>
          )}
        </div>
      ))}
      {readOnly && <span className="settings-hint">{t('settings.stationProfiles.atStation')}</span>}
      {!readOnly && !editing && (
        <div className="settings-field">
          <button type="button" className="settings-linkbtn" onClick={() => setEditing(fromStation(form))}>
            {t('settings.stationProfiles.add')}
          </button>
        </div>
      )}
      {!readOnly && editing && (
        <div className="settings-grid" role="group" aria-label={t('settings.stationProfiles.title')}>
          {TEXT_FIELDS.filter((f) => !(f.key === 'name' && editing.id === HOME_STATION_PROFILE)).map((f) => (
            <label className="settings-field" key={f.key}>
              <span className="settings-label">{f.label()}</span>
              <input
                className="settings-input"
                type="text"
                value={editing[f.key]}
                onChange={(e) => setEditing({ ...editing, [f.key]: e.target.value })}
                autoComplete="off"
                spellCheck={false}
              />
              {f.hint && <span className="settings-hint">{f.hint()}</span>}
            </label>
          ))}
          {NUMBER_FIELDS.map((f) => (
            <label className="settings-field" key={f.key}>
              <span className="settings-label">{f.label()}</span>
              <input
                className="settings-input"
                type="number"
                min={1}
                value={editing[f.key] ?? ''}
                onChange={(e) => {
                  const n = e.target.value.trim() === '' ? null : Number(e.target.value)
                  setEditing({ ...editing, [f.key]: n !== null && Number.isNaN(n) ? editing[f.key] : n })
                }}
              />
            </label>
          ))}
          <div className="settings-field">
            <span>
              <button type="button" className="settings-linkbtn" onClick={() => void save()}>
                {t('settings.stationProfiles.save')}
              </button>{' '}
              <button type="button" className="settings-linkbtn" onClick={() => setEditing(null)}>
                {t('settings.stationProfiles.cancel')}
              </button>
            </span>
          </div>
        </div>
      )}
    </div>
  )
}
