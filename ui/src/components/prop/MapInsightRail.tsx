// The floating, collapsible insight overlay pinned to the map's right edge (mirrors
// `.map-path`). Exploits the empty map real-estate WITHOUT covering the globe and
// persists across canvas zoom. Hosts the MUF ceiling readout, the hamqsl-style
// band-condition strip, the predictive insight feed, and (Expert) the band×hour
// modelled heatmap. Every section degrades gracefully when its data is absent.
import { useState } from 'react'
import { ChevronRight, ChevronLeft } from 'lucide-react'
import type { PropagationSnapshot, PathPrediction } from '../../types'
import { BandConditionStrip } from './BandConditionStrip'
import { InsightFeed } from './InsightFeed'
import { LikelihoodHeatmap } from './LikelihoodHeatmap'
import { mufCeilingBand, trendArrow, trendVar } from '../../propViz'
import { surfaceGet, surfaceSet } from '../../features/windowScope'
import { t, type MessageKey } from '../../i18n'

/** PER-SURFACE: a rail collapsed to reclaim space in THIS window is pure layout. */
const COLLAPSE_KEY = 'nexus.connect.insights.collapsed'

/** `MUF` is the acronym for Maximum Usable Frequency — a technical token, not prose. What
 * it MEANS is the tooltip beside it, and that is a catalog entry. */
const MUF_LABEL = 'MUF'

/** Three whole accessible names rather than a stem plus a direction word. */
const MUF_TREND_ARIA: Record<'rising' | 'falling' | 'steady', { ariaKey: MessageKey }> = {
  rising: { ariaKey: 'map.insights.muf.aria.rising' },
  falling: { ariaKey: 'map.insights.muf.aria.falling' },
  steady: { ariaKey: 'map.insights.muf.aria.steady' },
}

export function MapInsightRail({
  prop,
  outlook,
  onBandClick,
  activeBand,
}: {
  prop: PropagationSnapshot
  /** The current path/general outlook (selected station's path, else the no-selection
   * band outlook), for the MUF ceiling + modelled heatmap. */
  outlook?: PathPrediction | null
  onBandClick?: (band: string) => void
  activeBand?: string | null
}) {
  const [collapsed, setCollapsed] = useState(() => surfaceGet(COLLAPSE_KEY) === '1')
  const toggle = () =>
    setCollapsed((v) => {
      const nv = !v
      surfaceSet(COLLAPSE_KEY, nv ? '1' : '0')
      return nv
    })

  if (collapsed) {
    return (
      <button
        type="button"
        className="map-insights collapsed"
        onClick={toggle}
        title={t('map.insights.collapsed.title')}
      >
        <ChevronLeft size={14} />
        <span className="mi-pill-label">{t('map.insights.pill')}</span>
      </button>
    )
  }

  const bands = prop.advisory?.bands ?? []
  const insights = prop.insights ?? []
  const muf = outlook?.mufNow ?? 0
  const mufBand = mufCeilingBand(muf)
  const mufDir = prop.wxTrend?.muf.dir ?? 'steady'
  const heatBands = (outlook?.bands ?? []).filter((b) => b.workability !== 'Closed').slice(0, 8)

  return (
    <aside className="map-insights" aria-label={t('map.insights.aria')}>
      <div className="mi-head">
        <span className="mi-title">{t('map.insights.title')}</span>
        <button type="button" className="mi-collapse" onClick={toggle} title={t('map.insights.collapse.title')}>
          <ChevronRight size={14} />
        </button>
      </div>

      {muf > 0 && (
        <div className="mi-muf" title={t('map.insights.muf.title')}>
          <span className="mi-muf-label">{MUF_LABEL}</span>
          <strong>{t('map.insights.muf.value', { muf: muf.toFixed(1) })}</strong>
          {mufBand && <span className="mi-muf-band">≈ {mufBand}</span>}
          <span
            className="mi-muf-trend"
            style={{ color: trendVar(mufDir) }}
            aria-label={t(MUF_TREND_ARIA[mufDir].ariaKey)}
          >
            {trendArrow(mufDir)}
          </span>
        </div>
      )}

      {bands.length > 0 && (
        <div className="mi-card">
          <h4 className="mi-card-h">{t('map.insights.bands.head')}</h4>
          <BandConditionStrip bands={bands} onBandClick={onBandClick} activeBand={activeBand} />
        </div>
      )}

      {insights.length > 0 && (
        <div className="mi-card">
          <h4 className="mi-card-h">{t('map.insights.outlook.head')}</h4>
          <InsightFeed insights={insights} onBandClick={onBandClick} />
        </div>
      )}

      {heatBands.length > 0 && (
        <div className="mi-card">
          <h4 className="mi-card-h">{t('map.insights.heatmap.head')}</h4>
          <LikelihoodHeatmap outlook={heatBands} />
        </div>
      )}
    </aside>
  )
}
