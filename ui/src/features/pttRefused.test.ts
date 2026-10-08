// The status-lane item for a PTT press Nexus kept off the air because the radio still took its
// transmit audio from the DAX Nexus had set, not its mic: it says so, when, and in what mode, with
// none of the PTT and CAT advice a refused key otherwise gets.
import { describe, expect, it } from 'vitest'
import { pttRefusedLane } from './pttRefused'
import { EN } from '../i18n/en'

describe('the status lane after a PTT press kept off the air for the radio DAX', () => {
  // 2026-10-08 14:32:05 UTC
  const AT = Date.UTC(2026, 9, 8, 14, 32, 5) / 1000

  it('says nothing while no press was kept off the air', () => {
    expect(pttRefusedLane(null)).toBeNull()
    expect(pttRefusedLane(undefined)).toBeNull()
  })

  it('says Nexus did not key the over, when, in what mode, and why, without PTT advice', () => {
    const lane = pttRefusedLane({ at: AT, mode: 'USB' })
    expect(lane).not.toBeNull()
    expect(lane!.tier).toBe('critical')
    expect(lane!.message).toBe(EN['shell.lane.pttRefused.message'])
    expect(lane!.detail).toBe(
      EN['shell.lane.pttRefused.detail'].replace('{{mode}}', 'USB').replace('{{time}}', '14:32:05'),
    )
    expect(lane!.detail).not.toContain('CAT/port')
  })
})
