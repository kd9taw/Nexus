// The Chase pane — "work THIS now". The operator-anchored need alerts, each fused with its
// band's modeled openness + best window, so the elite chaser sees at a glance which needed
// stations are workable this minute vs which have a later window. Dual-audience: Basic shows
// the plain "call now" / "best 1400Z" action per row; Expert adds the entity + who heard it.
// Clicking a row selects it on the map; ▶ Work QSYs the rig and opens the cockpit.
import type { PaneContext } from '../connect/paneContext'
import { NEED_CHIP } from '../connect/paneFormat'
import { buildChaseTargets, type ChaseTarget } from '../../features/chase'
import { azimuthLabel, azimuthTitle, azimuthTo } from '../../grid'
import { t } from '../../i18n'

function ageLabel(secs: number | null): string {
  if (secs == null) return ''
  return secs < 60
    ? t('chase.age.secs', { secs })
    : t('chase.age.mins', { mins: Math.round(secs / 60) })
}

/** openness → a short plain phrase + a css state class for the row's accent. The
 * workability word and the window are the backend's own, interpolated verbatim. */
function openPhrase(target: ChaseTarget): { text: string; cls: string } {
  if (target.openNow)
    return {
      text: t('chase.open.now', { band: target.band, workability: target.workability }),
      cls: 'open',
    }
  if (target.workability === 'Marginal')
    return {
      text: `${t('chase.open.marginal', { band: target.band })}${
        target.window ? t('chase.open.best', { window: target.window }) : ''
      }`,
      cls: 'marginal',
    }
  if (target.window)
    return {
      text: t('chase.open.closed', { band: target.band, window: target.window }),
      cls: 'closed',
    }
  return { text: target.band, cls: 'unknown' }
}

export function ChasePane({ ctx }: { ctx: PaneContext }) {
  // Freshness is re-derived on each snapshot-driven re-render; no per-second ticking needed.
  const targets = buildChaseTargets(ctx.needAlerts, ctx.bandOutlook, Date.now())
  if (targets.length === 0) return null // PaneFrame falls back to the basic() line

  return (
    <section className="chase-pane panel">
      <ul className="chase-list">
        {targets.slice(0, 12).map((target) => {
          const chip = target.tags[0] ? NEED_CHIP[target.tags[0]] : null
          const op = openPhrase(target)
          return (
            <li key={`${target.call}-${target.band}`} className={`chase-row is-${op.cls}`}>
              <div
                className="chase-main"
                onClick={() => ctx.onSelectCall(target.call)}
                title={t('chase.row.show.title', { call: target.call })}
              >
                <div className="chase-head">
                  {chip && <span className={`need-chip need-${chip.cls}`}>{chip.label}</span>}
                  <b className="chase-call">{target.call}</b>
                  {ctx.onPoint && (
                    <button
                      type="button"
                      className="np-point"
                      title={t('chase.row.point.title', { call: target.call })}
                      onClick={(e) => {
                        e.stopPropagation()
                        ctx.onPoint!(target.call)
                      }}
                    >
                      ↗
                    </button>
                  )}
                  <span className="chase-entity">{target.entity}</span>
                  {/* The heading beside the entity — this is the pane with a
                      point-the-antenna button on the same row, so the number the
                      button is about should be readable without pressing it. */}
                  {(() => {
                    const az = azimuthTo(ctx.myGrid, null, target.entity, ctx.entityCentroids)
                    return az ? (
                      <span className="chase-az" title={azimuthTitle(az, target.entity)}>
                        {azimuthLabel(az)}
                      </span>
                    ) : null
                  })()}
                  {target.ageSecs != null && <span className="chase-age">{ageLabel(target.ageSecs)}</span>}
                </div>
                <div className={`chase-open o-${op.cls}`}>{op.text}</div>
                {target.evidence && <div className="chase-evi">{target.evidence}</div>}
              </div>
              {ctx.onWorkSpot && (
                <button
                  type="button"
                  className="chase-work"
                  onClick={() =>
                    ctx.onWorkSpot!({
                      call: target.call,
                      band: target.band,
                      mode: target.mode,
                      freqMhz: target.freqMhz,
                    })
                  }
                  title={t('chase.row.work.title')}
                >
                  {t('chase.row.work.label')}
                </button>
              )}
            </li>
          )
        })}
      </ul>
    </section>
  )
}
