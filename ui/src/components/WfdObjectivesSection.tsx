import { useMemo } from 'react'
import type { FieldDayQso } from '../types'
import { t } from '../i18n'
import {
  WFD_OBJECTIVES,
  WFD_QRP_OBJECTIVE,
  wfdEarnedIds,
  wfdLogHints,
  wfdObjectiveState,
  wfdObjectiveTally,
} from '../features/wfdObjectives'

/** The objectives the log can show evidence for, and how much evidence each asks for. */
const BAND_OBJECTIVES: Record<string, number> = { 'wfd-six-bands': 6, 'wfd-twelve-bands': 12 }
const MODES_OBJECTIVE = 'wfd-multiple-modes'

interface Props {
  /** `settings.fdObjectives` — the completed objectives, what the score multiplies by. */
  earned: string[]
  /** `settings.fdObjectivesPlanned` — the plan, which nothing that scores reads. */
  planned: string[]
  log: FieldDayQso[]
  /** The ENGINE's objective multiplier — the number the claimed score was made with. */
  om: number
  /** `settings.contestCategoryPower`, on the desktop only (a Remote observation carries no
   *  power setting, so it shows no note it cannot check). */
  power?: string
  readOnly: boolean
  open: boolean
  onToggleOpen: () => void
  onToggleEarned: (id: string) => void
  onTogglePlanned: (id: string) => void
}

/**
 * ⭐ **Winter Field Day's objectives, in place of ARRL's bonus checklist.**
 *
 * The sponsor's thirteen (2027 rules, p.6-7 and the worksheet on p.11), each worth an
 * objective multiplier, and the claimed score is QSO points × (OM + 1). Ticked and planned
 * work as the bonuses do: ticked is the score, planned is the plan. An objective a ticked
 * one brings with it (100% alternative power brings station equipment on alternative power;
 * twelve bands brings six) shows ticked and counts once.
 *
 * The three objectives a log can show evidence for — six bands, twelve bands, multiple modes
 * — carry a hint from the log, and the operator ticks them, as the sponsor's submission form
 * asks. Nothing is ticked for them.
 */
export function WfdObjectivesSection(p: Props) {
  const tally = useMemo(() => wfdObjectiveTally(p.earned, p.planned), [p.earned, p.planned])
  const hints = useMemo(() => wfdLogHints(p.log), [p.log])
  const qrp = WFD_OBJECTIVES.find((o) => o.id === WFD_QRP_OBJECTIVE)
  const qrpEarned = wfdEarnedIds(p.earned).has(WFD_QRP_OBJECTIVE)
  const declared = (p.power ?? '').trim().toUpperCase()
  // The QRP objective and the declared power disagree: one of the two is wrong, and the
  // Cabrillo file's CATEGORY-POWER is the declared one.
  const qrpNote =
    p.power === undefined || !qrp
      ? null
      : qrpEarned && declared !== 'QRP'
        ? t('fieldDay.objectives.qrp.undeclared', { objective: qrp.label })
        : !qrpEarned && declared === 'QRP'
          ? t('fieldDay.objectives.qrp.unticked', { objective: qrp.label, om: qrp.multiplier })
          : null
  return (
    <div className="fd-bonuses-section">
      <button
        type="button"
        className="fd-bonuses-toggle"
        onClick={p.onToggleOpen}
        aria-expanded={p.open}
      >
        <span>{t('fieldDay.objectives.head')}</span>
        <span className="fd-bonuses-count">
          {t('fieldDay.objectives.count', {
            done: tally.earnedCount,
            total: WFD_OBJECTIVES.length,
            om: p.om,
          })}
        </span>
        {tally.plannedCount > 0 && (
          <span className="fd-bonuses-planned-count">
            {t('fieldDay.objectives.planned.count', { count: tally.plannedCount, om: tally.plannedOm })}
          </span>
        )}
        <span className="fd-bonuses-chevron">{p.open ? '▲' : '▼'}</span>
      </button>
      {p.open && (
        <div className="fd-bonuses-body">
          {qrpNote && (
            <p className="fd-power-hint" role="note">
              {qrpNote}
            </p>
          )}
          <div className="fd-chase" role="group" aria-label={t('fieldDay.objectives.chase.aria')}>
            <div className="fd-chase-tile earned">
              <span className="fd-chase-val">{t('fieldDay.objectives.chase.earned', { om: p.om })}</span>
              <span className="fd-chase-note">{t('fieldDay.bonuses.chase.earned.note')}</span>
            </div>
            <div className="fd-chase-tile planned">
              <span className="fd-chase-val">{t('fieldDay.objectives.chase.planned', { om: tally.plannedOm })}</span>
              <span className="fd-chase-note">{t('fieldDay.bonuses.chase.planned.note')}</span>
            </div>
            <div className="fd-chase-tile potential">
              <span className="fd-chase-val">
                {t('fieldDay.objectives.chase.potential', { om: p.om + tally.plannedOm })}
              </span>
            </div>
          </div>
          <div className="fd-bonuses-list" role="group" aria-label={t('fieldDay.objectives.aria')}>
            {WFD_OBJECTIVES.map((o) => {
              const state = wfdObjectiveState(o.id, p.earned, p.planned)
              const done = state === 'earned' || state === 'implied'
              const onPlan = p.planned.includes(o.id)
              const need = BAND_OBJECTIVES[o.id]
              const hint =
                need !== undefined
                  ? t('fieldDay.objective.hint.bands', { count: hints.bands, need })
                  : o.id === MODES_OBJECTIVE
                    ? t('fieldDay.objective.hint.modes', { count: hints.modes })
                    : null
              return (
                <div
                  key={o.id}
                  className={`fd-bonus-row${done ? ' checked' : ''}${state === 'planned' ? ' planned' : ''}`}
                  data-objective-state={state}
                >
                  <input
                    id={`fd-objective-${o.id}`}
                    type="checkbox"
                    checked={done}
                    disabled={p.readOnly || state === 'implied'}
                    onChange={() => p.onToggleEarned(o.id)}
                    aria-label={t('fieldDay.objective.aria', { label: o.label, om: o.multiplier })}
                  />
                  <label className="fd-bonus-label" htmlFor={`fd-objective-${o.id}`}>
                    {o.label}
                    {state === 'implied' && (
                      <span className="fd-power-hint"> {t('fieldDay.objective.implied')}</span>
                    )}
                    {hint && <span className="fd-power-hint"> {hint}</span>}
                  </label>
                  <button
                    type="button"
                    className={`fd-bonus-plan${onPlan ? ' on' : ''}`}
                    aria-pressed={onPlan}
                    disabled={p.readOnly}
                    title={t('fieldDay.bonus.plan.title')}
                    aria-label={t('fieldDay.objective.plan.aria', { label: o.label })}
                    onClick={() => p.onTogglePlanned(o.id)}
                  >
                    {onPlan ? t('fieldDay.bonus.plan.on') : t('fieldDay.bonus.plan.off')}
                  </button>
                  <span className="fd-bonus-pts">{t('fieldDay.objective.om', { om: o.multiplier })}</span>
                </div>
              )
            })}
          </div>
        </div>
      )}
    </div>
  )
}
