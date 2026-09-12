import type { FieldDayStatus, Settings } from './types'
import type { FdRulesetDto } from './api'

export type FdDisplaySettings = Pick<Settings, 'fdOperator' | 'fdPowerMult' | 'fdBonuses' | 'fdBonusesPlanned'>
export interface FieldDayObservation {
  active: boolean
  fieldDay: FieldDayStatus | null
  settings: FdDisplaySettings
  ruleset: FdRulesetDto
}
