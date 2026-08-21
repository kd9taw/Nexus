// Loud 6 m/VHF opening alerts — the under-served win, given the highest-salience
// treatment. Only rendered when openings exist.
import { Zap } from 'lucide-react'
import type { OpeningView } from '../../types'
import { modeClass } from './OpeningsLogPane'
import { t } from '../../i18n'

function agoLabel(secs: number): string {
  if (secs <= 0) return ''
  const m = Math.round(secs / 60)
  return m < 1
    ? t('prop.opening.ago.justNow')
    : m < 60
      ? t('prop.opening.ago.mins', { mins: m })
      : t('prop.opening.ago.hours', { hours: Math.round(m / 60) })
}

export function OpeningStrip({
  openings,
  onBandClick,
}: {
  openings: OpeningView[]
  /** Click an opening → focus its band on the map. Omitted = display-only. */
  onBandClick?: (band: string) => void
}) {
  if (openings.length === 0) return null
  return (
    <div className="opening-strips">
      {openings.map((o, i) => {
        const ago = agoLabel(o.onsetSecs)
        return (
          <div
            className={`opening-strip${onBandClick ? ' is-clickable' : ''}`}
            key={i}
            onClick={onBandClick ? () => onBandClick(o.band) : undefined}
            role={onBandClick ? 'button' : undefined}
            title={onBandClick ? t('prop.opening.focus.title', { band: o.band }) : undefined}
          >
            <span className="opening-band">
              <Zap size={15} strokeWidth={2.25} aria-hidden="true" />
              {t('prop.opening.bandOpen', { band: o.band })}
            </span>
            {o.isNew && <span className="opening-new">{t('prop.opening.new')}</span>}
            <span className={`opening-mode opening-mode--${modeClass(o.mode)}`}>{o.mode}</span>
            <span className="opening-detail">
              {t('prop.opening.detail', {
                octant: o.octant,
                km: Math.round(o.maxKm).toLocaleString(),
                stations: o.stations,
                reciprocal:
                  o.reciprocalPairs > 0
                    ? t('prop.opening.reciprocal', { count: o.reciprocalPairs })
                    : '',
                confidence: o.confidence,
                opened: ago ? t('prop.opening.opened', { ago }) : '',
              })}
            </span>
            {o.note && <span className="opening-note">{o.note}</span>}
          </div>
        )
      })}
    </div>
  )
}
