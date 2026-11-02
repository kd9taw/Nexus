// The "now" line above a modelled outlook: a flare, a proton event, a NOAA watch — what is
// happening that the climatology below it does not include, each with its source and time
// (features/pathNow `spaceNowLines`; the facts come from the shack's one generator,
// propagation::pathnow::space_now, so the shack, the Remote and the TV page say the same).
// Renders nothing when nothing is happening.
import type { SpaceNow } from '../../types'
import { t } from '../../i18n'
import { spaceNowLines } from '../../features/pathNow'

export function SpaceNowLine({ now, nowSec, className }: { now: SpaceNow | null | undefined; nowSec: number; className?: string }) {
  const lines = spaceNowLines(now, nowSec)
  if (lines.length === 0) return null
  return (
    <ul
      className={`space-now${className ? ` ${className}` : ''}`}
      role="status"
      aria-label={t('spaceNow.aria')}
      title={t('spaceNow.title')}
    >
      {lines.map((l) => (
        <li key={l}>{l}</li>
      ))}
    </ul>
  )
}
