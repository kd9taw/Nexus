import type { FieldDayStatus, Settings } from './types'
import type { FdRulesetDto } from './api'

/** The Field Day settings a display needs. The two objective lists are present only for a
 *  contest that scores by objectives (Winter Field Day): every other capture keeps the four
 *  keys every published Remote page accepts. */
export type FdDisplaySettings = Pick<
  Settings,
  'fdOperator' | 'fdPowerMult' | 'fdBonuses' | 'fdBonusesPlanned' | 'fdObjectives' | 'fdObjectivesPlanned'
>
export interface FieldDayObservation {
  active: boolean
  fieldDay: FieldDayStatus | null
  settings: FdDisplaySettings
  ruleset: FdRulesetDto
}
