// The status-lane item for an ATU press that came to nothing after the button accepted it: it says
// which of the three things happened, when, and for a dropped press why, by value.
import { describe, expect, it } from 'vitest'
import { atuFailedLane } from './atuFailed'
import { EN } from '../i18n/en'

describe('the status lane after an ATU press that came to nothing', () => {
  // 2026-10-14 18:07:42 UTC
  const AT = Date.UTC(2026, 9, 14, 18, 7, 42) / 1000

  it('says nothing while no press failed', () => {
    expect(atuFailedLane(null)).toBeNull()
    expect(atuFailedLane(undefined)).toBeNull()
  })

  it('says a dropped press was dropped, why, and to press again', () => {
    expect(
      atuFailedLane({ at: AT, cause: 'dropped', why: 'TX is off — enable TX first', stoodDown: false }),
    ).toEqual({
      tier: 'warning',
      message: 'ATU PRESS DROPPED',
      detail: 'ATU press dropped at 18:07:42 UTC: TX is off — enable TX first; press again.',
    })
  })

  it('says the radio refused the tune-up and points at the tuner selection', () => {
    expect(atuFailedLane({ at: AT, cause: 'refused', stoodDown: false })).toEqual({
      tier: 'warning',
      message: 'ATU REFUSED',
      detail:
        'The radio refused the tune-up at 18:07:42 UTC (check its tuner is set to the internal tuner).',
    })
  })

  it('says no answer came, and that TX stood down only where it did', () => {
    expect(atuFailedLane({ at: AT, cause: 'noAnswer', stoodDown: true })).toEqual({
      tier: 'warning',
      message: EN['shell.lane.atuFailed.noAnswer.message'],
      detail: 'No answer from the radio to the tune-up at 18:07:42 UTC; TX stood down in case it is tuning.',
    })
    expect(atuFailedLane({ at: AT, cause: 'noAnswer', stoodDown: false })?.detail).toBe(
      'No answer from the radio to the tune-up at 18:07:42 UTC; it may be tuning.',
    )
  })
})
