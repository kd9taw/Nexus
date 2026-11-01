// The polar-cap chip and the aurora chip — the words beside the proton cap and the aurora oval, on the
// 2-D map and the 3-D globe alike, in the space-weather chip stack (`.wx-chip-stack`).
//
// Every number carries its source and time (capsNow.ts has the rules):
//   · the cap: NOAA's S-scale from the GOES protons at their sample time; the picture from NOAA's
//     D-RAP grid at its valid time, or "modelled" with the cap edge called an approximation (and
//     "unknown" without a measured Kp); NOAA's rule of thumb for how long it lasts, dropped when the
//     proton sample is old;
//   · the aurora: why the layer turned itself on (the Kp with its word, or NOAA's watch); OVATION's
//     forecast time; or that no oval is drawn because OVATION is out of date (for when) or missing.

import { t } from '../../i18n'
import { agoLabel } from './SpaceWxGauges'
import { sScale, PROTONS_STALE_SECS, type AuroraPicture, type CapPicture } from '../../capsNow'
import type { PcaView } from '../../types'

/** `HH:MMZ`. */
function hm(unix: number): string {
  const d = new Date(unix * 1000)
  return `${String(d.getUTCHours()).padStart(2, '0')}:${String(d.getUTCMinutes()).padStart(2, '0')}Z`
}

export function PcaChip({ pca, pic, nowMs }: { pca: PcaView; pic: CapPicture; nowMs: number }) {
  const s = sScale(pca)
  const db = pic.maxDb.toFixed(1)
  // Shown only during a held proton event (S1+, capsNow `capShown`).
  const head = pic.points.length > 0 ? t('map.pca.head', { scale: s, db }) : t('map.pca.headWeak', { scale: s })
  const at = pca.protonsAt ?? null
  const age = at != null ? Math.max(0, nowMs / 1000 - at) : null
  const stale = age != null && age >= PROTONS_STALE_SECS
  const protons =
    at == null
      ? t('map.pca.protonsUndated')
      : stale
        ? t('map.pca.protonsOld', { time: hm(at), ago: agoLabel(age!) })
        : t('map.pca.protons', { time: hm(at) })
  const src =
    pic.source === 'noaa'
      ? t('map.pca.src.noaa', { time: hm(pic.validAt!), protons })
      : pca.cutoffDeg != null
        ? t('map.pca.src.model', { protons, deg: Math.round(pca.cutoffDeg) })
        : t('map.pca.src.modelNoKp', { protons })
  const hours = !stale && pca.sepRecoveryH != null ? Math.round(pca.sepRecoveryH) : null
  return (
    <div
      className="flare-chip pca-chip"
      role="status"
      data-source={pic.source}
      data-stale={stale ? '' : undefined}
      title={t('map.pca.title')}
    >
      <div className="caps-chip-head">{head}</div>
      <div className="flare-chip-line flare-chip-src" data-source={pic.source === 'noaa' ? 'noaa' : 'goes'}>
        {src}
      </div>
      {hours != null && hours >= 1 && <div className="flare-chip-line">{t('map.pca.recovery', { h: hours })}</div>}
    </div>
  )
}

export function AuroraChip({ pic, stormWhy }: { pic: AuroraPicture; stormWhy: string | null }) {
  const line =
    pic.mode === 'ovation'
      ? t('map.aurora.chip.ovation', { time: hm(pic.forecastAt) })
      : pic.why === 'stale'
        ? t('map.aurora.chip.stale', { time: hm(pic.forecastAt) })
        : pic.why === 'undated'
          ? t('map.aurora.chip.undated')
          : t('map.aurora.chip.none')
  return (
    <div
      className="flare-chip aurora-chip"
      role="status"
      data-mode={pic.mode}
    >
      {stormWhy != null && <div className="caps-chip-head">{t('map.aurora.chip.storm', { why: stormWhy })}</div>}
      <div className={`flare-chip-line flare-chip-src${stormWhy == null ? ' caps-chip-head' : ''}`} data-source={pic.mode === 'ovation' ? 'noaa' : 'goes'}>
        {line}
      </div>
    </div>
  )
}
