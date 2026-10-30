// The path "why" line on the Path card: one plain sentence first (features/pathWhy.ts), the
// numbers it stands on one click away in a disclosure. Renders nothing when the outlook has no
// geometry or there is nothing worth saying.
import type { PathPrediction } from '../../types'
import { t } from '../../i18n'
import { pathWhyDetail, pathWhyFacts, pathWhyLine } from '../../features/pathWhy'

export function PathWhy({ pred }: { pred: PathPrediction | null | undefined }) {
  const facts = pathWhyFacts(pred)
  const line = pathWhyLine(facts)
  if (!line || !pred?.geometry) return null
  const rows = pathWhyDetail(facts, pred.geometry.maxGmlatDeg)
  return (
    <details className="cp-why">
      <summary>
        <span className="cp-why-line">{line}</span>{' '}
        <span className="cp-why-more">{t('pathWhy.detail.summary')}</span>
      </summary>
      <ul className="cp-why-detail">
        {rows.map((r) => (
          <li key={r}>{r}</li>
        ))}
      </ul>
    </details>
  )
}
