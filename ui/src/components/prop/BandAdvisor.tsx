// The band advisor: ranked rows with a score bar (width + tier color), the
// "count people not physics" evidence, region/bearing, confidence, and the
// plain-language reason. Closed bands recede.
//
// Two views (when worldwide data is present): "Best for you" ranks bands by
// OPERATOR-REACHABLE activity (own-call + near-region); "Worldwide" ranks by the
// global cluster/RBN firehose. The toggle teaches the chaser the difference
// between workable-for-me and merely-busy-somewhere.
import { useState } from 'react'
import type { BandReport } from '../../types'
import { tierVar, modeledVar, dualStateLabel } from '../../propViz'
import { t } from '../../i18n'

export function BandAdvisor({
  bands,
  worldwideBands,
  onBandClick,
  activeBand,
}: {
  bands: BandReport[]
  /** "Worldwide activity" ranking (the global firehose). When provided, a
   * For-you / Worldwide toggle appears; absent = single (for-you) view. */
  worldwideBands?: BandReport[] | null
  /** Click a row → focus that band on the map ("where IS this opening?").
   * Omitted = display-only rows (the standalone Propagation layout). */
  onBandClick?: (band: string) => void
  /** The currently-focused band (highlighted; click again to clear). */
  activeBand?: string | null
}) {
  const [view, setView] = useState<'you' | 'world'>('you')
  const hasWorld = !!worldwideBands && worldwideBands.length > 0
  const showWorld = hasWorld && view === 'world'
  const rows = showWorld ? worldwideBands! : bands

  return (
    <section className="band-advisor panel" aria-label={t('prop.bands.aria')}>
      <h2 className="ba-head">
        <span>{showWorld ? t('prop.bands.head.world') : t('prop.bands.head.you')}</span>
        {hasWorld && (
          <span className="ba-view" role="tablist" aria-label={t('prop.bands.view.aria')}>
            <button
              type="button"
              role="tab"
              aria-selected={!showWorld}
              className={`ba-view-btn${!showWorld ? ' active' : ''}`}
              onClick={() => setView('you')}
              title={t('prop.bands.view.you.title')}
            >
              {t('prop.bands.view.you.label')}
            </button>
            <button
              type="button"
              role="tab"
              aria-selected={showWorld}
              className={`ba-view-btn${showWorld ? ' active' : ''}`}
              onClick={() => setView('world')}
              title={t('prop.bands.view.world.title')}
            >
              {t('prop.bands.view.world.label')}
            </button>
          </span>
        )}
        {activeBand && onBandClick && (
          <button
            type="button"
            className="ba-clear"
            onClick={() => onBandClick(activeBand)}
            title={t('prop.bands.clearFocus.title')}
          >
            {t('prop.bands.focused', { band: activeBand })}
          </button>
        )}
      </h2>
      <p className="ba-caption">
        {showWorld ? t('prop.bands.caption.world') : t('prop.bands.caption.you')}
      </p>
      <div className="ba-rows">
        {rows.map((b) => {
          // Dual state: MODELED openness (physics) is the dominant word; the OBSERVED
          // tier rides as a sub-note. An open-but-unheard band reads "Open · none heard",
          // never a dead "Quiet" — the core fix. Only genuinely modeled-closed bands recede.
          const ds = dualStateLabel(b.modeled, b.tier)
          const stateColor = b.modeled ? modeledVar(b.modeled) : tierVar(b.tier)
          return (
            <div
              className={`ba-row${ds.word === 'Closed' ? ' is-closed' : ''}${onBandClick ? ' is-clickable' : ''}${activeBand === b.band ? ' is-active' : ''}`}
              key={b.band}
              onClick={onBandClick ? () => onBandClick(b.band) : undefined}
              role={onBandClick ? 'button' : undefined}
              title={
                onBandClick
                  ? t('prop.focusBand.title', { band: b.band })
                  : b.modeledReason
                    ? t('prop.bands.modelled.title', { reason: b.modeledReason })
                    : undefined
              }
            >
              <span className="ba-band">{b.band}</span>
              <span className="ba-meter" aria-hidden="true">
                <span
                  className="ba-meter-fill"
                  style={{ width: `${Math.round(b.score * 100)}%`, background: tierVar(b.tier) }}
                />
              </span>
              <span className="ba-state">
                <span className="ba-modeled" style={{ color: stateColor }}>
                  {ds.word}
                </span>
                {ds.sub && <span className="ba-observed">{ds.sub}</span>}
              </span>
              <span className="ba-dir">
                {b.bestRegion ? `${b.bestRegion.octant} · ${b.bestRegion.region}` : '—'}
              </span>
              <span className="ba-people" title={t('prop.bands.people.title')}>
                {b.nHearMe}↓ {b.nIHear}↑
              </span>
              <span className="ba-reason">{b.reason}</span>
            </div>
          )
        })}
      </div>
    </section>
  )
}
