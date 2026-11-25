// Settings ▸ Station ▸ My station profiles: Home, a portable kit, a club call. Each is saved the
// moment it is saved here, through its own writer (`saveStationProfile`), never by the panel's
// Save: a profile is the call the station transmits under, and a whole-settings save keeps the
// live list (`Engine::apply_settings`). The top bar's picker switches between them.
//
// ⚠️ ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts): every operator-visible string comes
// from the catalog. The call, grid, park and IOTA references are tokens and stay as typed.
import { useEffect, useState } from 'react'
import { t } from '../i18n'
import type { Settings, StationProfile } from '../types'
import { HOME_STATION_PROFILE } from '../types'
import {
  qrzTestConnection,
  removeStationProfile,
  saveStationProfile,
  setStationProfileQrzKey,
  stationProfileQrzKeyStored,
} from '../api'
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
    lotwLocation: '',
  }
}

type TextKey = 'name' | 'call' | 'grid' | 'state' | 'county' | 'iota' | 'park' | 'rig' | 'antenna' | 'lotwLocation'
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
  {
    key: 'lotwLocation',
    label: () => t('settings.stationProfiles.lotwLocation'),
    hint: () => t('settings.stationProfiles.lotwLocation.hint'),
  },
]

const NUMBER_FIELDS: { key: NumberKey; label: () => string }[] = [
  { key: 'cqZone', label: () => t('settings.stationProfiles.cqZone') },
  { key: 'ituZone', label: () => t('settings.stationProfiles.ituZone') },
  { key: 'powerW', label: () => t('settings.stationProfiles.power') },
]

/** #503: a saved profile's own QRZ Logbook API key. Kept in the system keychain through its own
 *  writer, never in the profile the form saves; masked, never read back. Without one, the
 *  profile's contacts go up with the key in Settings ▸ Services. */
function ProfileQrzKey({ id }: { id: string }) {
  const [key, setKey] = useState('')
  const [own, setOwn] = useState<boolean | null>(null)
  const [test, setTest] = useState<{ ok: boolean; msg: string } | null>(null)
  // Bumped after a write, so whether a key is stored is read again.
  const [written, setWritten] = useState(0)
  useEffect(() => {
    stationProfileQrzKeyStored(id)
      .then(setOwn)
      .catch(() => setOwn(null))
  }, [id, written])
  const write = async (value: string) => {
    const ok = await withErrorToast(async () => {
      await setStationProfileQrzKey(id, value)
      return true
    }, t('settings.stationProfiles.qrzKey.failed'))
    if (!ok) return
    setKey('')
    setTest(null)
    setWritten((n) => n + 1)
  }
  const runTest = () => {
    setTest({ ok: true, msg: t('settings.connections.test.testing') })
    qrzTestConnection(id)
      .then((detail) => setTest({ ok: true, msg: t('settings.connections.qrz.test.ok', { detail }) }))
      .catch((e) => setTest({ ok: false, msg: t('settings.stationProfiles.qrzKey.testFail', { detail: String(e) }) }))
  }
  return (
    <div className="settings-field">
      <span className="settings-label">{t('settings.stationProfiles.qrzKey')}</span>
      <div className="settings-input-row">
        <input
          className="settings-input"
          type="password"
          aria-label={t('settings.stationProfiles.qrzKey')}
          value={key}
          onChange={(e) => setKey(e.target.value)}
          autoComplete="off"
          spellCheck={false}
        />
        <button type="button" className="settings-refresh" onClick={() => void write(key)} disabled={!key.trim()}>
          {t('settings.stationProfiles.qrzKey.save')}
        </button>
        {own && (
          <button type="button" className="settings-refresh" onClick={() => void write('')}>
            {t('settings.stationProfiles.qrzKey.clear')}
          </button>
        )}
        <button type="button" className="settings-refresh" onClick={runTest}>
          {t('settings.connections.qrz.test.action')}
        </button>
      </div>
      {own !== null && (
        <span className="settings-hint">
          {own ? t('settings.stationProfiles.qrzKey.own') : t('settings.stationProfiles.qrzKey.none')}
        </span>
      )}
      {test && (
        <span className="settings-hint" role="status">
          {test.msg}
        </span>
      )}
      <span className="settings-hint">{t('settings.stationProfiles.qrzKey.hint')}</span>
    </div>
  )
}

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
          {editing.id !== '' && <ProfileQrzKey id={editing.id} />}
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
