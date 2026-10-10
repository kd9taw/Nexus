// The Chase pane — "work THIS now". The operator-anchored need alerts, each fused with its
// band's modeled openness + best window, so the elite chaser sees at a glance which needed
// stations are workable this minute vs which have a later window. Dual-audience: Basic shows
// the plain "call now" / "best 1400Z" action per row; Expert adds the entity + who heard it.
// Clicking a row selects it on the map; ▶ Work QSYs the rig and opens the cockpit.
//
// A row's heading is the station's answer for where its ↗ turns the antenna (features/callBearing),
// never the centre of the entity worked out here: the station's grid or callbook position when Nexus
// knows one, `~` and the centre of its country when it does not. When the station cannot give one the
// row says so in words, on a line of its own under the call (in the heading's place they took the
// whole line from the entity in a 200 px box, measured in Chrome).
import { useRef } from 'react'
import type { PaneContext } from '../connect/paneContext'
import { NEED_CHIP } from '../connect/paneFormat'
import { useChaseSplit } from './chaseSplit'
import { buildChaseTargets, type ChaseTarget } from '../../features/chase'
import { useCallBearings, type CallAim } from '../../features/callBearing'
import { azimuthLabel, azimuthTitle } from '../../grid'
import { t } from '../../i18n'
import { fmtKmTokens } from '../../units'

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

/** A row's heading, drawn as every board draws one (`azimuthLabel`/`azimuthTitle`: `~` and a
 *  rough-heading tooltip when it is only the centre of the country). No answer: nothing. */
function Heading({ aim }: { aim: CallAim | undefined }) {
  if (!aim || !('bearing' in aim)) return null
  const { pointed } = aim.bearing
  const az = { deg: Math.round(pointed.bearing) % 360, approx: pointed.to === 'country' }
  return (
    <span className="chase-az" title={azimuthTitle(az, pointed.country)}>
      {azimuthLabel(az)}
    </span>
  )
}

/** Why a row has no heading, in words and never a 0°: the station cannot place the call, or the
 *  operator's own grid is not set. Nothing while there is no answer yet, or for a failure the station
 *  did not name. */
function NoHeading({ aim }: { aim: CallAim | undefined }) {
  if (!aim || !('why' in aim)) return null
  if (aim.why === 'unknownStation') return <div className="chase-why">{t('rotor.pane.aim.unknown')}</div>
  if (aim.why === 'noGrid') return <div className="chase-why">{t('rotor.pane.aim.noGrid')}</div>
  return null
}

export function ChasePane({ ctx }: { ctx: PaneContext }) {
  // Freshness is re-derived on each snapshot-driven re-render; no per-second ticking needed.
  const targets = buildChaseTargets(ctx.needAlerts, ctx.bandOutlook, Date.now())
  const rows = targets.slice(0, 12)
  const aims = useCallBearings(rows.map((target) => target.call))
  // A row whose first line has no room for the entity gives it the line under it (chaseSplit).
  const list = useRef<HTMLUListElement>(null)
  useChaseSplit(list, targets.length > 0)
  // Nothing needed and heard: the box's one line, drawn here — the frame never sees a null from
  // this component, so returning one would leave the box empty.
  if (targets.length === 0) return <p className="pane-basic">{t('chase.empty')}</p>

  return (
    <section className="chase-pane panel">
      <ul className="chase-list" ref={list}>
        {rows.map((target) => {
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
                  <span className="chase-where">
                    <span className="chase-entity">{target.entity}</span>
                    {/* The heading beside the entity — this is the pane with a
                        point-the-antenna button on the same row, so the number the
                        button is about should be readable without pressing it. */}
                    <Heading aim={aims.get(target.call)} />
                  </span>
                  {target.ageSecs != null && <span className="chase-age">{ageLabel(target.ageSecs)}</span>}
                </div>
                <NoHeading aim={aims.get(target.call)} />
                <div className={`chase-open o-${op.cls}`}>{op.text}</div>
                {/* "heard by K9LC (EN52, 26 km)": its distances in the operator's units. */}
                {target.evidence && (
                  <div className="chase-evi">{fmtKmTokens(target.evidence, ctx.units)}</div>
                )}
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
