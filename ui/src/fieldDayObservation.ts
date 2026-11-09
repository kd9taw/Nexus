import type { FieldDayStatus, Settings } from './types'
import type { FdRulesetDto } from './api'

/** The Field Day settings a display needs. The two objective lists are present only for a
 *  contest that scores by objectives (Winter Field Day): every other capture keeps the four
 *  keys every published Remote page accepts. */
export type FdDisplaySettings = Pick<
  Settings,
  'fdOperator' | 'fdPowerMult' | 'fdBonuses' | 'fdBonusesPlanned' | 'fdObjectives' | 'fdObjectivesPlanned'
> &
  // The station's own screen only: no Remote capture carries these two, so an observed
  // view counts a claimed per-unit box as one unit (its score is the snapshot's).
  Partial<Pick<Settings, 'fdBonusCounts' | 'fdClass'>>
export interface FieldDayObservation {
  active: boolean
  fieldDay: FieldDayStatus | null
  settings: FdDisplaySettings
  ruleset: FdRulesetDto
}
