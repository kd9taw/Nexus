// ⚠️ ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Pack names (a grid square and a size),
// sizes, dates and the folder are invariant tokens; "© OpenStreetMap" is the data's own credit.
//
// SETTINGS ▸ APPEARANCE ▸ MAP & GLOBE ▸ STREET MAPS (operator ruling 2026-10-04, D5: the map picker
// offers Street, and Update and Remove live in Settings, in the section beside the Logbook-globe
// toggle). Hidden, like the picker's Street choice, until the street map is offered
// (features/streetMaps.ts), and never on the Remote page.
//
// Each installed pack: its area, detail, size and data date, and Remove (confirmed, saying the space
// it frees; the map pick stays, and Street offers its download again). Check for updates reads the
// host's index only when pressed, and Update downloads only when pressed (D8). Download another area
// opens the sheet the map picker opens. The folder is named so the maps can be removed by hand: an
// uninstaller does not know about them.
//
// THE BENCH AID, shown only when this computer runs with NEXUS_STREET_MAP=1: "Install a street map
// file…" installs a map file the operator already has, so Street can be tried before any host
// serves packs. Never shown to users.
import { useState } from 'react'
import { confirmDialog } from '../confirm'
import { t } from '../i18n'
import { T } from '../i18n/T'
import type { StreetPack } from '../features/streetPack'
import {
  asStreetError,
  checkStreetUpdates,
  installStreetMapFile,
  mb,
  removeStreetMap,
  startStreetDownload,
  updateStreetMap,
  useStreetMaps,
  type StreetDetail,
  type StreetUpdate,
} from '../features/streetMaps'
import { areaName, OSM, OSM_COPYRIGHT, StreetDownloadSheet, streetErrorText } from './StreetDownloadSheet'

const detailText = (d: StreetDetail): string =>
  d === 'streets' ? t('map.street.sheet.detail.streets') : t('map.street.sheet.detail.roads')

export function SettingsStreetMaps({ myGrid }: { myGrid: string }) {
  const { offered, packs, unfinished, download, folder, bench } = useStreetMaps()
  const [sheet, setSheet] = useState(false)
  const [note, setNote] = useState<string | null>(null)
  const [updates, setUpdates] = useState<StreetUpdate[]>([])
  const [checking, setChecking] = useState(false)
  if (!offered) return null

  const running = download.state === 'running'
  const say = (e: unknown) => setNote(streetErrorText(asStreetError(e)))
  const remove = async (p: StreetPack) => {
    const yes = await confirmDialog({
      title: t('settings.streetMaps.remove.title', { name: p.name }),
      body: t('settings.streetMaps.remove.body', { size: mb(p.bytes) }),
      confirmLabel: t('settings.streetMaps.remove.confirm'),
      danger: true,
    })
    if (!yes) return
    try {
      const freed = await removeStreetMap(p.id)
      setNote(t('settings.streetMaps.removed', { name: p.name, size: mb(freed) }))
      setUpdates((u) => u.filter((x) => x.packId !== p.id))
    } catch (e) {
      say(e)
    }
  }
  const check = async () => {
    setChecking(true)
    setNote(null)
    try {
      const found = await checkStreetUpdates()
      setUpdates(found)
      if (found.length === 0) setNote(t('settings.streetMaps.updates.none'))
    } catch (e) {
      say(e)
    } finally {
      setChecking(false)
    }
  }
  const update = async (u: StreetUpdate) => {
    setNote(null)
    const pack = await updateStreetMap(u)
    if (pack) {
      setUpdates((list) => list.filter((x) => x.packId !== u.packId))
      setNote(t('settings.streetMaps.updated', { name: pack.name, date: pack.dataDate }))
    }
  }
  const install = async () => {
    setNote(null)
    try {
      const done = await installStreetMapFile()
      if (done) {
        setNote(
          done.kind === 'pack'
            ? t('settings.streetMaps.bench.installed.pack', { name: done.pack.name })
            : t('settings.streetMaps.bench.installed.assets'),
        )
      }
    } catch (e) {
      say(e)
    }
  }

  return (
    <div className="settings-field street-maps">
      <span className="settings-label">{t('settings.streetMaps.label')}</span>
      <div className="street-maps-body">
        {packs && packs.length === 0 && <p className="settings-hint">{t('settings.streetMaps.none')}</p>}
        {packs && packs.length > 0 && (
          <ul className="street-maps-list">
            {packs.map((p) => {
              const u = updates.find((x) => x.packId === p.id)
              return (
                <li key={p.id} className="street-maps-row">
                  <span className="street-maps-name">{p.name}</span>
                  <span className="street-maps-facts">
                    {t('settings.streetMaps.pack', { detail: detailText(p.detail), size: mb(p.bytes), date: p.dataDate || '?' })}
                  </span>
                  {u && (
                    <button type="button" className="settings-refresh" disabled={running} onClick={() => void update(u)}>
                      {t('settings.streetMaps.update', { date: u.newDataDate })}
                    </button>
                  )}
                  <button type="button" className="settings-refresh danger" disabled={running} onClick={() => void remove(p)}>
                    {t('settings.streetMaps.remove')}
                  </button>
                </li>
              )
            })}
          </ul>
        )}
        {!running &&
          unfinished.map((u) => (
            <p key={u.packId} className="street-maps-row">
              <span className="street-maps-facts">
                {t('map.street.sheet.unfinished', { name: areaName(u.area), done: mb(u.doneBytes), total: mb(u.totalBytes) })}
              </span>
              <button type="button" className="settings-refresh" onClick={() => void startStreetDownload(u.area)}>
                {t('map.street.sheet.resume')}
              </button>
            </p>
          ))}
        {running && (
          <p className="settings-hint" role="status">
            {t('settings.streetMaps.downloading', { name: areaName(download.area), pct: download.percent })}
          </p>
        )}
        <div className="settings-input-row">
          <button type="button" className="settings-refresh" disabled={running} onClick={() => setSheet(true)}>
            {packs && packs.length > 0 ? t('settings.streetMaps.another') : t('settings.streetMaps.download')}
          </button>
          <button
            type="button"
            className="settings-refresh"
            disabled={checking || running || !packs || packs.length === 0}
            onClick={() => void check()}
          >
            {checking ? t('settings.streetMaps.updates.checking') : t('settings.streetMaps.updates.check')}
          </button>
          {bench && (
            <button type="button" className="settings-refresh" disabled={running} onClick={() => void install()}>
              {t('settings.streetMaps.bench.install')}
            </button>
          )}
        </div>
        {note && (
          <p className="settings-hint" role="status">
            {note}
          </p>
        )}
        {bench && <p className="settings-hint">{t('settings.streetMaps.bench.hint')}</p>}
        <p className="settings-hint">
          <T k="settings.streetMaps.folder" tags={{ path: <code className="street-maps-folder" /> }} vals={{ folder }} />
        </p>
        <p className="settings-hint">
          <T k="map.street.sheet.credit" tags={{ osm: <a href={OSM_COPYRIGHT} target="_blank" rel="noopener noreferrer" /> }} vals={{ osm: OSM }} />
        </p>
      </div>
      <StreetDownloadSheet open={sheet} onClose={() => setSheet(false)} myGrid={myGrid} mapCentre={null} />
    </div>
  )
}
