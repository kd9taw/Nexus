// The map legends, shared by the 2-D map AND the 3-D globe so the two surfaces
// explain their dots identically (2D↔3D parity, operator report 2026-07-21 — the
// globe showed the data with no key to read it by).
//
// ⚠️ ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The band names on the MUF
// ticks are technical tokens and stay here; every word is a catalog entry.
import { useMemo } from 'react'
import { sampleLut } from '../colormaps'
import { t } from '../i18n'

export function MapLegend() {
  const stops = useMemo(() => {
    return Array.from({ length: 6 }, (_, i) => {
      const [r, g, b] = sampleLut('inferno', i / 5)
      return `rgb(${r}, ${g}, ${b}) ${(i / 5) * 100}%`
    }).join(', ')
  }, [])
  return (
    <div className="map-legend" aria-hidden="true">
      <span className="map-legend-dot" style={{ background: '#ff5d8f' }} />
      <span>{t('map.legend.newDxcc')}</span>
      <span className="map-legend-dot" style={{ background: '#f5a524' }} />
      <span>{t('map.legend.newBand')}</span>
      <span className="map-legend-dot" style={{ background: '#b07cff' }} />
      <span>{t('map.legend.zoneMode')}</span>
      <span className="map-legend-dot" style={{ background: '#4ea3ff' }} />
      <span>{t('map.legend.confirm')}</span>
      <span className="map-legend-dot worked" />
      <span>{t('map.legend.worked')}</span>
      <span className="map-legend-sep" />
      <span>{t('map.legend.opening')}</span>
      <span className="map-legend-bar" style={{ background: `linear-gradient(90deg, ${stops})` }} />
      <span className="map-legend-sep" />
      <span title={t('map.legend.heat.title')}>{t('map.legend.heat.label')}</span>
    </div>
  )
}

export function MufLegend() {
  const stops = Array.from({ length: 6 }, (_, i) => {
    const t = i / 5
    return `hsl(${(210 - 210 * t).toFixed(0)}, 85%, 55%) ${Math.round(t * 100)}%`
  }).join(', ')
  return (
    <div className="muf-legend" aria-hidden="true">
      <span className="muf-legend-title">{t('map.legend.muf.title')}</span>
      <span className="muf-legend-bar" style={{ background: `linear-gradient(90deg, ${stops})` }} />
      <span className="muf-legend-ticks">
        <span>40m</span>
        <span>20m</span>
        <span>15m</span>
        <span>10m</span>
      </span>
    </div>
  )
}
