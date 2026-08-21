// One needed × workable-now DXpedition card: need tier (color + glyph), the
// modelled likelihood word (color), a live-spots confirmation, beam/distance,
// YOUR modelled best-shot window (engine-badged, expandable to the 24h×band
// grid), how-to-call, and the ★ chase toggle (alert when the window opens).
import { useState } from 'react'
import { Check } from 'lucide-react'
import type { DxpedWindow, WorkableCard } from '../../types'
import { needMeta, workabilityVar, bandTiming } from '../../propViz'
import { azimuthLabel, azimuthTitle, backendAzimuth } from '../../grid'
import { LikelihoodHeatmap } from './LikelihoodHeatmap'
import { t } from '../../i18n'

/** The ITU recommendation's number — a citation, not a word. */
const ENGINE_P533 = 'P.533'

export function WorkNowCard({
  card,
  onWork,
  window: win,
  chasing = false,
  onToggleChase,
}: {
  card: WorkableCard
  /** "Work" button → the app's atomic work path (rig jumps band+mode+freq).
   * Omitted = display-only card. */
  onWork?: (card: WorkableCard) => void
  /** Modelled best-shot window for this expedition (get_dxped_windows). */
  window?: DxpedWindow
  /** Chase state + toggle (★ = alert me when my window opens). */
  chasing?: boolean
  onToggleChase?: (call: string) => void
}) {
  const need = needMeta(card.need)
  const az = backendAzimuth(card.bearingDeg, card.distanceKm)
  const [details, setDetails] = useState(false)
  // Headline this card with ITS OWN band. `win.best`/`win.outlook[0]` rank every HF band
  // on the PATH, so they are a cross-band answer on a per-band card: a 20m card read
  // "Best shot: 60m" (field report 2026-08-05) — a band this expedition may never have
  // announced, since the window sweep never receives the announced list. The sibling
  // cards already cover the other bands, so the cross-band line was redundant too.
  // If our band is absent (the caller truncates the outlook to 4), fall back to the
  // backend's own per-band `windowHint` — never show another band's window under this
  // band's heading, which is the reported bug in miniature.
  const own = win?.outlook.find((o) => o.band === card.band)
  const timing = own ? bandTiming(own.hourly, Date.now()) : ''
  return (
    <div className={`worknow-card${card.status === 'WorkNow' ? ' is-worknow' : ''}`}>
      <div className="wn-top">
        <b className="wn-call">{card.call}</b>
        <span className="wn-entity">{card.entity}</span>
        <span className="wn-need" style={{ color: `var(${need.cssVar})` }} title={need.label}>
          <span aria-hidden="true">{need.glyph}</span> {card.need}
        </span>
        {onToggleChase && (
          <button
            type="button"
            className={`wn-chase${chasing ? ' active' : ''}`}
            onClick={() => onToggleChase(card.call)}
            title={
              chasing ? t('dxped.chase.toggle.on.title') : t('dxped.chase.toggle.off.title')
            }
            aria-pressed={chasing}
          >
            {chasing ? '★' : '☆'}
          </button>
        )}
      </div>
      <div className="wn-mid">
        <span className="wn-band">{card.band}</span>
        <span className="wn-like" style={{ color: workabilityVar(card.likelihood) }}>
          {card.likelihood}
        </span>
        {card.liveConfirmed && (
          <span className="wn-live" title={t('dxped.card.live.title')}>
            <Check size={12} strokeWidth={3} aria-hidden="true" /> {t('dxped.card.live.label')}
          </span>
        )}
        {/* The octant alone ("NE") was never a beam heading. The degrees were already
            in this payload — measured backend-side from the operator's grid to the
            announced one — and simply never rendered. */}
        <span className="wn-geo" title={az ? azimuthTitle(az, card.entity) : undefined}>
          {t('dxped.card.geo', {
            octant: card.octant,
            az: az ? ` ${azimuthLabel(az)}` : '',
            km: Math.round(card.distanceKm).toLocaleString(),
          })}
        </span>
      </div>
      {win && own ? (
        <div className="wn-window">
          {t('dxped.card.bestShot', {
            band: own.band,
            workability: own.workability,
            window: own.window,
          })}
          {timing ? ` · ${timing}` : ''}
          <span className="cp-engine">
            {win.engine === 'p533' ? ENGINE_P533 : t('dxped.engine.modelled')}
          </span>
          <button
            type="button"
            className="wn-details"
            onClick={() => setDetails((d) => !d)}
            title={t('dxped.card.details.title')}
          >
            {details ? t('dxped.card.details.hide') : t('dxped.card.details.show')}
          </button>
        </div>
      ) : (
        <div className="wn-window">{card.windowHint}</div>
      )}
      {details && win && <LikelihoodHeatmap outlook={win.outlook} />}
      <div className="wn-how">{card.howToCall}</div>
      {onWork && (
        <button
          type="button"
          className="wn-work"
          onClick={() => onWork(card)}
          title={t('dxped.card.work.title', { band: card.band })}
        >
          {t('dxped.card.work.label', { band: card.band })}
        </button>
      )}
    </div>
  )
}
