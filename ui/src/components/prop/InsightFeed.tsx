// The predictive insight feed — dual-audience: the plain sentence is always shown; the
// technical detail rides inline in Expert mode, or behind a per-row expander in Simple
// mode. Each row is colour-keyed by level and icon-keyed by kind; rows with a band link
// to the map highlight.
import {
  TrendingUp,
  Sun,
  Activity,
  Zap,
  Sunrise,
  Radio,
  Gauge,
  Rocket,
  ArrowLeftRight,
  Wind,
  type LucideIcon,
} from 'lucide-react'
import type { Insight, InsightKind } from '../../types'
import { sortInsights, insightLevelVar } from '../../propViz'
import { t } from '../../i18n'

const KIND_ICON: Record<InsightKind, LucideIcon> = {
  mufTrend: TrendingUp,
  solarFlux: Sun,
  geomagnetic: Activity,
  flare: Zap,
  greyline: Sunrise,
  esWatch: Radio,
  bandHeadroom: Gauge,
  openingMomentum: Rocket,
  reciprocity: ArrowLeftRight,
  solarWind: Wind,
}

export function InsightFeed({
  insights,
  onBandClick,
}: {
  insights: Insight[]
  onBandClick?: (band: string) => void
}) {
  const rows = sortInsights(insights)
  if (rows.length === 0) return null
  return (
    <div className="insight-feed" role="list" aria-label={t('prop.insightFeed.aria')}>
      {rows.map((ins, i) => (
        <InsightRow key={`${ins.kind}-${i}`} ins={ins} onBandClick={onBandClick} />
      ))}
    </div>
  )
}

function InsightRow({
  ins,
  onBandClick,
}: {
  ins: Insight
  onBandClick?: (band: string) => void
}) {
  // ?? Activity, not a bare lookup: `kind` arrives from the backend, and an unknown
  // one (version skew — a TV page polling a newer or older Nexus is exactly where it
  // happens) made `Icon` undefined, which crashed the WHOLE view with React #130.
  // Found by pointing a browser at the TV page with a mismatched payload: one bad row
  // black-screened everything. A generic icon is honest; a dead screen is not.
  const Icon = KIND_ICON[ins.kind] ?? Activity
  const clickable = !!ins.band && !!onBandClick
  return (
    <div
      className={`insight-row${clickable ? ' is-clickable' : ''}`}
      role="listitem"
      style={{ borderLeftColor: insightLevelVar(ins.level) }}
      onClick={clickable ? () => onBandClick!(ins.band!) : undefined}
      title={clickable ? t('prop.focusBand.title', { band: ins.band! }) : undefined}
    >
      <span className="if-icon" style={{ color: insightLevelVar(ins.level) }}>
        <Icon size={14} />
      </span>
      <div className="if-body">
        <span className="if-plain">{ins.plain}</span>
        {/* Always shown: Connect's Basic detail level was removed 2026-07-26, and with it the
            per-row expand control that used to reveal this. */}
        <span className="if-tech">{ins.technical}</span>
      </div>
    </div>
  )
}
