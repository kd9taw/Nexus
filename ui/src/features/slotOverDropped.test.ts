// The status lane for an FT8 or FT4 over the radio keyed only after its period had ended: the station
// dropped that over and left TX on, however many in a row, and the lane says so, when and how late, in a
// lane of its own.
import { describe, expect, it } from 'vitest'
import { slotOverDroppedLane } from './slotOverDropped'
import { EN } from '../i18n/en'

describe('the status lane after a dropped FT8/FT4 over', () => {
  // 2026-10-15 14:32:05 UTC
  const AT = Date.UTC(2026, 9, 15, 14, 32, 5) / 1000

  it('says nothing while no over was dropped', () => {
    expect(slotOverDroppedLane(null)).toBeNull()
    expect(slotOverDroppedLane(undefined)).toBeNull()
  })

  it('says the over was dropped, when and how late, and that TX stayed on: a warning, not a stop', () => {
    const lane = slotOverDroppedLane({ at: AT, late: '1.2 s', count: 1 })
    expect(lane).not.toBeNull()
    expect(lane!.tier).toBe('warning')
    expect(lane!.message).toBe(EN['shell.lane.slotOverDropped.message'])
    expect(lane!.message).not.toContain('STOPPED')
    expect(lane!.detail).toBe(
      EN['shell.lane.slotOverDropped.detail'].replace('{{time}}', '14:32:05').replace('{{late}}', '1.2 s'),
    )
    expect(lane!.detail).toContain('1.2 s after your period had ended (slow CAT)')
    expect(lane!.detail).toContain('left TX on')
    expect(lane!.detail).not.toMatch(/\{\{/)
  })

  it('reads the same however many in a row: a warning, TX being on', () => {
    for (const count of [2, 12, 24]) {
      const lane = slotOverDroppedLane({ at: AT, late: '0.4 s', count })
      expect(lane!.tier).toBe('warning')
      expect(lane!.message).toBe(EN['shell.lane.slotOverDropped.message'])
      expect(lane!.detail).toContain('0.4 s after your period had ended')
      expect(lane!.detail).toContain('left TX on')
    }
  })
})
