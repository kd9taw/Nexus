// Short path or long path on the Path card and the selected station card: the polar chip, then
// the heading hint (features/pathRoute.ts), with the long-path label wherever the long path is in
// the answer and the numbers behind it in the hover. Advice only — nothing here turns a rotator.
// Renders nothing when the engine modelled no long path (the heuristic) or both routes are closed;
// near the antipode it says why no route is called.
import type { PathPrediction } from '../../types'
import { t } from '../../i18n'
import { nearAntipode, polarChip, polarChipTitle, routeHint, routeHintDetail, routeHintLine } from '../../features/pathRoute'

export function PathRoute({
  pred,
  focusBand,
  className,
  short = false,
}: {
  pred: PathPrediction | null | undefined
  focusBand?: string | null
  /** The host's row class (`cp-route` on the Path card, `station-route` on the card). */
  className: string
  /** A narrow host: the reasons move to the hover, so the long-path label is never clamped off. */
  short?: boolean
}) {
  if (nearAntipode(pred)) {
    // No route call from noise: say why there is none.
    return (
      <span className={className}>
        <span className="route-hint">{t('pathWhy.sentence', { text: t('pathRoute.antipode') })}</span>
      </span>
    )
  }
  const hint = routeHint(pred, focusBand)
  const full = routeHintLine(hint)
  const line = routeHintLine(hint, short)
  if (!hint || !full || !line) return null
  const route = hint.route === 'long' ? 'long' : 'short'
  const chip = polarChip(pred, route)
  return (
    <span className={className} title={[full, ...routeHintDetail(hint)].join('\n')}>
      {chip && (
        <span className="route-polar-chip" title={polarChipTitle(pred, route) ?? undefined}>
          {chip}
        </span>
      )}
      <span className="route-hint">{line}</span>
      {hint.route !== 'short' && <span className="route-lp-badge">{t('pathRoute.lpBadge')}</span>}
    </span>
  )
}
