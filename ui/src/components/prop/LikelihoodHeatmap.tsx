// Band × UTC-hour contact-likelihood heatmap (the flagship planning viz). Each
// row is an announced band; each cell an hour, colored by the model's hourly
// score via the perceptual inferno LUT (dark=low, bright=high). A NOW hairline
// marks the current UTC hour; hovering a cell shows the exact band/hour/score.
import type { CSSProperties } from 'react'
import type { BandOutlook } from '../../types'
import { heatColor, fmtZ, nowUtcHour, stateInkKind, workabilityVar } from '../../propViz'
import { Tooltip, TooltipProvider } from '../ui/Tooltip'
import { t } from '../../i18n'

const HOURS = Array.from({ length: 24 }, (_, h) => h)
const TICKS = [0, 6, 12, 18]

export function LikelihoodHeatmap({
  outlook,
  /** Sit the ramp back against the panel — for lists the operator SCROLLS rather
   * than studies. See `heatColor`'s alpha note. */
  muted = false,
}: {
  outlook: BandOutlook[]
  muted?: boolean
}) {
  if (outlook.length === 0) return null
  const nowH = nowUtcHour()
  return (
    <TooltipProvider>
      <div className="heatmap" role="img" aria-label={t('prop.heatmap.aria')}>
        <div className="heatmap-axis" aria-hidden="true">
          <span className="heatmap-corner" />
          {HOURS.map((h) => (
            <span key={h} className={`heatmap-tick${TICKS.includes(h) ? ' major' : ''}`}>
              {TICKS.includes(h) ? h : ''}
            </span>
          ))}
        </div>
        {outlook.map((o) => (
          <div className="heatmap-row" key={o.band}>
            <span
              className="heatmap-band"
              // The workability colour rides as `--state-ink`, lettered per theme by the sheet; the
              // name is its own span so the light themes' underline does not run under the percentage.
              data-state-ink={stateInkKind(workabilityVar(o.workability)) ?? undefined}
              style={{ '--state-ink': workabilityVar(o.workability) } as CSSProperties}
              title={t('prop.heatmap.band.title', {
                band: o.band,
                workability: o.workability,
                pct: Math.round(o.reliability),
              })}
            >
              <span className="heatmap-name">{o.band}</span>
              <span className="heatmap-rel">{Math.round(o.reliability)}%</span>
            </span>
            {HOURS.map((h) => {
              const s = o.hourly[h] ?? 0
              return (
                <Tooltip key={h} side="top" content={`${o.band} ${fmtZ(h)} — ${pct(s)}`}>
                  <span
                    className={`heatmap-cell${h === nowH ? ' now' : ''}`}
                    style={{ background: heatColor(s, muted ? 0.5 : 1) }}
                  />
                </Tooltip>
              )
            })}
          </div>
        ))}
        <div className="heatmap-legend" aria-hidden="true">
          <span>00Z</span>
          <span className="heatmap-scale" />
          <span>{t('prop.heatmap.legend')}</span>
          <span>23Z</span>
        </div>
      </div>
    </TooltipProvider>
  )
}

function pct(s: number): string {
  return `${Math.round(s * 100)}%`
}
