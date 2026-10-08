// The status-lane item for a slot over that lost its audio part way through: the station ended it
// and halted TX, and the lane says so, and when, until TX is on again.
import { describe, expect, it } from 'vitest'
import { slotAudioLostLane } from './slotAudioLost'
import { EN } from '../i18n/en'

describe('the status lane after a slot over lost its audio', () => {
  // 2026-10-08 14:32:05 UTC
  const AT = Date.UTC(2026, 9, 8, 14, 32, 5) / 1000

  it('says nothing while no over lost its audio', () => {
    expect(slotAudioLostLane(null)).toBeNull()
    expect(slotAudioLostLane(undefined)).toBeNull()
  })

  it('says the over was ended, when, and that TX is off', () => {
    const lane = slotAudioLostLane({ at: AT })
    expect(lane).not.toBeNull()
    expect(lane!.tier).toBe('critical')
    expect(lane!.message).toBe(EN['shell.lane.slotAudioLost.message'])
    expect(lane!.detail).toBe(EN['shell.lane.slotAudioLost.detail'].replace('{{time}}', '14:32:05'))
  })
})
