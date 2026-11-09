// Best times, the chart side (the pane only): band by UTC hour, highest band on top, in the path
// strip's grid and colour ramp (PathDayStrip's `.pd-*` classes). Each cell is the hour's WORD for the
// asked mode, coloured by its place on the ramp — never a figure, and its hover says only the word
// and the route (▸ short, ◂ long). A band the licence keeps the operator off in this mode is dimmed.
// Under the bands: the grey-line row and the Kp ribbon, as the strip draws them.
//
// Display only: nothing here is a control.
import type { CSSProperties } from 'react'
import type { BestTimes, Workability } from '../../types'
import { t } from '../../i18n'
import { heatColor } from '../../propViz'
import { kpCell, ROUTE_GLYPH } from '../../features/pathDay'
import { cellTitle } from '../../features/bestTimes'

const HOURS = Array.from({ length: 24 }, (_, h) => h)
const MAJOR = [0, 6, 12, 18]
const GREY_GLYPH = '◐'
const KP_LABEL = 'Kp'

/** Where on the ramp each word sits: a colour position, not a figure (none is shown). */
const RAMP: Record<Workability, number> = { Closed: 0.04, Marginal: 0.22, Fair: 0.45, Good: 0.68, Excellent: 0.92 }

export function BestTimesChart({ answer }: { answer: BestTimes }) {
  const now = answer.nowHour
  const col = (h: number) => (h === now ? ' now' : '')
  return (
    <div className="bt-chart">
      <div className="pd-grid" role="group" aria-label={t('bestTimes.chart.aria')}>
        <span className="pd-corner" />
        {HOURS.map((h) => (
          <span key={h} className={`pd-hour${MAJOR.includes(h) ? ' major' : ''}${col(h)}`}>
            {MAJOR.includes(h) ? h : ''}
          </span>
        ))}
        {answer.chart.map((b) => (
          <div key={b.band} className={`pd-row${b.licensed ? '' : ' bt-unlicensed'}`}>
            <span className="pd-label">{b.band}</span>
            {b.cells.map((c, h) => (
              <span
                key={h}
                className={`pd-cell${col(h)}`}
                style={{ background: heatColor(RAMP[c.word]) } as CSSProperties}
                title={cellTitle(b.band, h, c.word, c.route, b.licensed)}
              >
                {c.route ? ROUTE_GLYPH[c.route] : ''}
              </span>
            ))}
          </div>
        ))}
        {answer.hours.length === 24 && (
          <>
            <div className="pd-row pd-grey">
              <span className="pd-label">{GREY_GLYPH}</span>
              {answer.hours.map((x, h) => (
                <span key={h} className={`pd-greycell${col(h)}`}>
                  {x.greyMe || x.greyDx ? GREY_GLYPH : ''}
                </span>
              ))}
            </div>
            <div className="pd-row pd-kp">
              <span className="pd-label">{KP_LABEL}</span>
              {answer.hours.map((x, h) => {
                const k = kpCell(x)
                return <span key={h} className={`pd-kpcell ${k.fill}${k.tone ? ` ${k.tone}` : ''}${col(h)}`} />
              })}
            </div>
          </>
        )}
      </div>
      <div className="pd-legend">
        <span>
          {ROUTE_GLYPH.short} {t('pathDay.legend.short')}
        </span>
        <span>
          {ROUTE_GLYPH.long} {t('pathDay.legend.long')}
        </span>
        <span>{t('pathDay.legend.kp')}</span>
        <span className="pd-modelled">{t('bestTimes.chart.legend')}</span>
      </div>
    </div>
  )
}
