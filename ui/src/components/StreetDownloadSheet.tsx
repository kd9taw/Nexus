// ⚠️ ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Sizes and distances (MB, GB, km) and
// grid squares are invariant tokens, built by features/streetMaps.ts; "© OpenStreetMap" is the
// data's own credit and is never translated.
//
// THE STREET MAP'S DOWNLOAD SHEET (operator rulings 2026-10-04: D2, D3, D6). One dialog, three
// choices: WHERE (around my station, from the grid; or around the map's centre), HOW BIG (a square
// 50, 100, 200 or 400 km across, 200 by default) and DETAIL (All streets, the default, or Main
// roads). Then the EXACT size, measured before anything downloads (`street_map_size`), the free
// disk, and the licence credit. Download refuses where the disk has less than twice the download
// free (the Rust side's rule, shown here before it is pressed).
//
// The download runs on in the background (features/streetMaps.ts): this sheet can close, and the
// map picker's chip keeps counting. One that stopped, because the app closed or the network went,
// is offered here as Resume, and a pause after the retries ran out as Retry.
//
// Nothing here reaches the network until the sheet is open: the size is the one read it makes.
import { useEffect, useMemo, useRef, useState } from 'react'
import { Dialog } from './ui/Dialog'
import { streetMapSize } from '../api'
import { gridToLatLon, type LatLon } from '../grid'
import { t } from '../i18n'
import { T } from '../i18n/T'
import { locatorAt } from '../features/streetOverlay'
import type { StreetPack } from '../features/streetPack'
import {
  DEFAULT_KM,
  STREET_KMS,
  asStreetError,
  cancelStreetDownload,
  clearStreetStop,
  gb,
  kmText,
  mb,
  removeStreetMap,
  startStreetDownload,
  useStreetMaps,
  type StreetArea,
  type StreetDetail,
  type StreetError,
  type StreetKm,
  type StreetProgress,
  type StreetSize,
  type StreetUnfinished,
} from '../features/streetMaps'

/** The data's credit, linked to its licence page (externalLinks.ts opens it in the browser). */
export const OSM = '© OpenStreetMap'
export const OSM_COPYRIGHT = 'https://www.openstreetmap.org/copyright'

/** What each refusal says, by kind (the Rust message is English diagnostics). Spelled out, one
 *  literal key per kind, so the catalog guards can see every one. */
export function streetErrorText(e: StreetError): string {
  switch (e.kind) {
    case 'invalidArea':
      return t('map.street.error.invalidArea')
    case 'badRequest':
      return t('map.street.error.badRequest')
    case 'unknownPack':
      return t('map.street.error.unknownPack')
    case 'notFound':
      return t('map.street.error.notFound')
    case 'busy':
      return t('map.street.error.busy')
    case 'diskSpace':
      return t('map.street.error.diskSpace')
    case 'network':
      return t('map.street.error.network')
    case 'server':
      return t('map.street.error.server')
    case 'paused':
      return t('map.street.error.paused')
    case 'cancelled':
      return t('map.street.error.cancelled')
    case 'buildGone':
    case 'buildChanged':
      return t('map.street.error.buildGone')
    case 'rangeIgnored':
      return t('map.street.error.rangeIgnored')
    case 'invalidManifest':
      return t('map.street.error.invalidManifest')
    case 'invalidArchive':
      return t('map.street.error.invalidArchive')
    default:
      return t('map.street.error.io')
  }
}

/** An area's name as the packs are named: its centre's 6-character square and its size. */
export const areaName = (a: { lat: number; lon: number; km: number }): string => `${locatorAt(a.lat, a.lon, 6)} ${kmText(a.km)}`

/** A running download in words. */
function progressText(p: StreetProgress | null): string {
  if (!p) return t('map.street.sheet.running.starting')
  switch (p.phase) {
    case 'assets':
      return t('map.street.sheet.running.assets', { done: mb(p.done), total: mb(p.total) })
    case 'tiles':
      return p.etaSecs == null
        ? t('map.street.sheet.running.tiles', { done: mb(p.done), total: mb(p.total) })
        : t('map.street.sheet.running.tilesEta', {
            done: mb(p.done),
            total: mb(p.total),
            speed: mb(p.bytesPerSec),
            mins: Math.max(1, Math.ceil(p.etaSecs / 60)),
          })
    case 'retrying':
      return t('map.street.sheet.running.retrying', { secs: p.waitSecs, n: p.attempt })
    case 'verifying':
      return t('map.street.sheet.running.verifying')
  }
}

export interface StreetDownloadSheetProps {
  open: boolean
  onClose: () => void
  /** The operator's grid: "Around my station". */
  myGrid: string
  /** The map's centre when the sheet was opened from the map; null elsewhere (Settings). */
  mapCentre: LatLon | null
  /** A download this sheet started finished while it was open. */
  onInstalled?: (pack: StreetPack) => void
}

export function StreetDownloadSheet({ open, onClose, myGrid, mapCentre, onInstalled }: StreetDownloadSheetProps) {
  const { download, unfinished } = useStreetMaps()
  // A download that ends after the sheet closed must not change the map under the operator.
  const openRef = useRef(open)
  openRef.current = open
  const station = useMemo(() => gridToLatLon(myGrid), [myGrid])
  const [where, setWhere] = useState<'station' | 'centre'>(station ? 'station' : 'centre')
  const [km, setKm] = useState<StreetKm>(DEFAULT_KM)
  const [detail, setDetail] = useState<StreetDetail>('streets')
  const centre = where === 'station' ? station : mapCentre
  const area: StreetArea | null = centre ? { lat: centre.lat, lon: centre.lon, km, detail } : null
  const areaKey = area ? `${area.lat},${area.lon},${km},${detail}` : ''

  // THE EXACT SIZE, read from the host's directories for this area (1 to 3 s). Asked again on every
  // change of the three choices, and only while the sheet is open; a late answer for an earlier
  // choice is dropped.
  const [sized, setSized] = useState<{ key: string; size?: StreetSize; error?: StreetError } | null>(null)
  const running = download.state === 'running'
  useEffect(() => {
    if (!open || !area || running) return
    let live = true
    setSized({ key: areaKey })
    const timer = window.setTimeout(() => {
      streetMapSize(area)
        .then((size) => live && setSized({ key: areaKey, size }))
        .catch((e: unknown) => live && setSized({ key: areaKey, error: asStreetError(e) }))
    }, 250)
    return () => {
      live = false
      window.clearTimeout(timer)
    }
    // `area` is rebuilt every render; its key is what changes.
  }, [open, areaKey, running]) // eslint-disable-line react-hooks/exhaustive-deps
  const size = sized?.key === areaKey ? sized.size : undefined
  const sizeError = sized?.key === areaKey ? sized.error : undefined

  const begin = async (a: StreetArea) => {
    clearStreetStop()
    const pack = await startStreetDownload(a)
    if (pack && openRef.current) onInstalled?.(pack)
  }
  // The unfinished download this sheet offers to resume (the newest, while nothing runs).
  const resumable: StreetUnfinished | undefined = running ? undefined : unfinished[unfinished.length - 1]
  const stopped = download.state === 'stopped' ? download : null
  const need = size ? 2 * (size.downloadBytes + size.assetsBytes) - size.resumeBytes : 0

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) onClose()
      }}
      title={t('map.street.sheet.title')}
      description={t('map.street.sheet.desc')}
      className="street-sheet"
    >
      {resumable && (
        <div className="street-sheet-resume" role="status">
          <span>
            {t('map.street.sheet.unfinished', {
              name: areaName(resumable.area),
              done: mb(resumable.doneBytes),
              total: mb(resumable.totalBytes),
            })}
          </span>
          <button type="button" className="settings-save" onClick={() => void begin(resumable.area)}>
            {t('map.street.sheet.resume')}
          </button>
          <button type="button" className="settings-refresh" onClick={() => void removeStreetMap(resumable.packId).catch(() => 0)}>
            {t('map.street.sheet.discard')}
          </button>
        </div>
      )}

      <div className="street-sheet-row">
        <span className="street-sheet-label">{t('map.street.sheet.where')}</span>
        <div className="theme-switcher" role="group" aria-label={t('map.street.sheet.where')}>
          <button
            type="button"
            className={`theme-chip${where === 'station' ? ' active' : ''}`}
            aria-pressed={where === 'station'}
            disabled={!station || running}
            onClick={() => setWhere('station')}
          >
            {t('map.street.sheet.where.station')}
            {station && <span className="street-sheet-token">{locatorAt(station.lat, station.lon, 6)}</span>}
          </button>
          <button
            type="button"
            className={`theme-chip${where === 'centre' ? ' active' : ''}`}
            aria-pressed={where === 'centre'}
            disabled={!mapCentre || running}
            onClick={() => setWhere('centre')}
          >
            {t('map.street.sheet.where.centre')}
            {mapCentre && <span className="street-sheet-token">{locatorAt(mapCentre.lat, mapCentre.lon, 6)}</span>}
          </button>
        </div>
        {!station && <span className="settings-hint">{t('map.street.sheet.where.noGrid')}</span>}
      </div>

      <div className="street-sheet-row">
        <span className="street-sheet-label">{t('map.street.sheet.size')}</span>
        <div className="theme-switcher" role="group" aria-label={t('map.street.sheet.size')}>
          {STREET_KMS.map((k) => (
            <button
              key={k}
              type="button"
              className={`theme-chip${km === k ? ' active' : ''}`}
              aria-pressed={km === k}
              disabled={running}
              onClick={() => setKm(k)}
            >
              {kmText(k)}
            </button>
          ))}
        </div>
        <span className="settings-hint">{t('map.street.sheet.size.hint')}</span>
      </div>

      <div className="street-sheet-row">
        <span className="street-sheet-label">{t('map.street.sheet.detail')}</span>
        <div className="theme-switcher" role="group" aria-label={t('map.street.sheet.detail')}>
          <button
            type="button"
            className={`theme-chip${detail === 'streets' ? ' active' : ''}`}
            aria-pressed={detail === 'streets'}
            disabled={running}
            onClick={() => setDetail('streets')}
          >
            {t('map.street.sheet.detail.streets')}
          </button>
          <button
            type="button"
            className={`theme-chip${detail === 'roads' ? ' active' : ''}`}
            aria-pressed={detail === 'roads'}
            disabled={running}
            onClick={() => setDetail('roads')}
          >
            {t('map.street.sheet.detail.roads')}
          </button>
        </div>
      </div>

      {!running && area && (
        <div className="street-sheet-size" role="status" aria-live="polite">
          {sizeError ? (
            <p className="street-sheet-error">{streetErrorText(sizeError)}</p>
          ) : !size ? (
            <p>{t('map.street.sheet.measure.running')}</p>
          ) : (
            <>
              <p className="street-sheet-figure">
                {size.assetsBytes > 0
                  ? t('map.street.sheet.measure.sizeAssets', { size: mb(size.downloadBytes), assets: mb(size.assetsBytes) })
                  : t('map.street.sheet.measure.size', { size: mb(size.downloadBytes) })}
              </p>
              {size.freeBytes != null && <p>{t('map.street.sheet.measure.free', { free: gb(size.freeBytes) })}</p>}
              {size.resumeBytes > 0 && <p>{t('map.street.sheet.measure.resume', { done: mb(size.resumeBytes) })}</p>}
              {size.installed && <p>{t('map.street.sheet.measure.installed')}</p>}
              {!size.enoughSpace && (
                <p className="street-sheet-error">
                  {t('map.street.sheet.measure.noSpace', { need: gb(need), free: gb(size.freeBytes ?? 0) })}
                </p>
              )}
            </>
          )}
        </div>
      )}

      {running && (
        <div className="street-sheet-progress" role="status" aria-live="polite">
          <progress max={100} value={download.percent} aria-label={t('map.street.sheet.progress.aria')} />
          <p>{progressText(download.progress)}</p>
          <p className="settings-hint">{t('map.street.sheet.running.background')}</p>
        </div>
      )}
      {stopped && (
        <p className="street-sheet-error" role="alert">
          {t('map.street.sheet.stopped', { reason: streetErrorText(stopped.error) })}
        </p>
      )}

      <p className="street-sheet-credit">
        <T
          k="map.street.sheet.credit"
          tags={{ osm: <a href={OSM_COPYRIGHT} target="_blank" rel="noopener noreferrer" /> }}
          vals={{ osm: OSM }}
        />
      </p>

      <div className="confirm-actions">
        {running ? (
          <>
            <button type="button" className="settings-refresh danger" onClick={() => void cancelStreetDownload()}>
              {t('map.street.sheet.stop')}
            </button>
            <button type="button" className="settings-refresh" onClick={onClose}>
              {t('map.street.sheet.close')}
            </button>
          </>
        ) : (
          <>
            <button type="button" className="settings-refresh" onClick={onClose}>
              {t('map.street.sheet.cancel')}
            </button>
            {stopped?.retry ? (
              <button type="button" className="settings-save" onClick={() => void begin(stopped.area)}>
                {t('map.street.sheet.retry')}
              </button>
            ) : (
              <button
                type="button"
                className="settings-save"
                disabled={!area || !size || !size.enoughSpace || size.installed}
                onClick={() => area && void begin(area)}
              >
                {t('map.street.sheet.download')}
              </button>
            )}
          </>
        )}
      </div>
    </Dialog>
  )
}
