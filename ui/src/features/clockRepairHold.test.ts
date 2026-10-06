// The status lane while a clock repair holds transmit: it says so for as long as the hold lasts,
// and nothing otherwise.
import { describe, expect, it } from 'vitest'
import { clockRepairHoldLane } from './clockRepairHold'
import { EN } from '../i18n/en'

describe('the status lane while a clock repair holds transmit', () => {
  it('says nothing while no repair holds transmit', () => {
    expect(clockRepairHoldLane(false)).toBeNull()
    expect(clockRepairHoldLane(undefined)).toBeNull()
  })

  it('says transmit is held while a repair runs, and why', () => {
    expect(clockRepairHoldLane(true)).toEqual({
      tier: 'warning',
      message: EN['shell.lane.clockRepairHold.message'],
      detail: EN['shell.lane.clockRepairHold.detail'],
    })
  })
})
