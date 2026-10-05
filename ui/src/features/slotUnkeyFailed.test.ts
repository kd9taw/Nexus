// The status-lane item for a slot over whose unkey the radio did not take: the station halted TX,
// and the lane says so, when, and with the radio's own answer, until TX is on again.
import { describe, expect, it } from 'vitest'
import { slotUnkeyFailedLane } from './slotUnkeyFailed'
import { EN } from '../i18n/en'

describe('the status lane after a failed slot unkey', () => {
  // 2026-10-05 14:32:18 UTC
  const AT = Date.UTC(2026, 9, 5, 14, 32, 18) / 1000
  const WHY = 'rigctld PTT error: "RPRT -1\\n"'

  it('says nothing while no unkey failed', () => {
    expect(slotUnkeyFailedLane(null)).toBeNull()
    expect(slotUnkeyFailedLane(undefined)).toBeNull()
  })

  it('says TX stopped, when, and what the radio answered', () => {
    const lane = slotUnkeyFailedLane({ at: AT, why: WHY })
    expect(lane).not.toBeNull()
    expect(lane!.tier).toBe('critical')
    expect(lane!.message).toBe(EN['shell.lane.slotUnkeyFailed.message'])
    expect(lane!.detail).toContain('14:32:18')
    expect(lane!.detail).toContain(WHY)
    expect(lane!.detail).not.toMatch(/\{\{/)
  })
})
