// The status-lane item for a slot over the radio would not key: the station halted TX, and the lane
// says so, when, and with the radio's own answer, until TX is on again.
import { describe, expect, it } from 'vitest'
import { slotKeyRefusedLane } from './slotKeyRefused'
import { EN } from '../i18n/en'

describe('the status lane after a refused slot key', () => {
  // 2026-10-05 14:32:05 UTC
  const AT = Date.UTC(2026, 9, 5, 14, 32, 5) / 1000
  const WHY = 'rigctld PTT error: "RPRT -1\\n"'

  it('says nothing while no key was refused', () => {
    expect(slotKeyRefusedLane(null)).toBeNull()
    expect(slotKeyRefusedLane(undefined)).toBeNull()
  })

  it('says TX stopped, when, and what the radio answered', () => {
    const lane = slotKeyRefusedLane({ at: AT, why: WHY })
    expect(lane).not.toBeNull()
    expect(lane!.tier).toBe('critical')
    expect(lane!.message).toBe(EN['shell.lane.slotKeyRefused.message'])
    expect(lane!.detail).toContain('14:32:05')
    expect(lane!.detail).toContain(WHY)
    expect(lane!.detail).not.toMatch(/\{\{/)
  })

  it('says Nexus kept the key off the air itself for the Flex audio route, without PTT advice', () => {
    const lane = slotKeyRefusedLane({
      at: AT,
      why: 'not keying a DIGU over: the radio takes its transmit audio from its mic input',
      flexAudio: { mode: 'DIGU', cause: 'notYetDax' },
    })
    expect(lane!.tier).toBe('critical')
    expect(lane!.message).toBe(EN['shell.lane.slotKeyRefused.message'])
    expect(lane!.detail).toBe(
      EN['shell.lane.slotKeyRefused.flex.notYetDax']
        .replace('{{mode}}', 'DIGU')
        .replace('{{time}}', '14:32:05'),
    )
    expect(lane!.detail).not.toContain('CAT/port')
  })

  it('says the transmit audio would not open, in the sound card’s words, without PTT advice', () => {
    const why = 'playback stream: Device or resource busy'
    const lane = slotKeyRefusedLane({ at: AT, why, txAudioFailed: true })
    expect(lane!.tier).toBe('critical')
    expect(lane!.message).toBe(EN['shell.lane.slotKeyRefused.message'])
    expect(lane!.detail).toBe(
      EN['shell.lane.slotKeyRefused.txAudio'].replace('{{time}}', '14:32:05').replace('{{why}}', why),
    )
    expect(lane!.detail).not.toContain('CAT/port')
  })

  it('says the radio did not take the split frequency, in the rig link’s words, without PTT advice', () => {
    const why = 'rigctld error for "I 14073000\\n": "RPRT -11\\n"'
    const lane = slotKeyRefusedLane({ at: AT, why, splitFailed: true })
    expect(lane!.tier).toBe('critical')
    expect(lane!.message).toBe(EN['shell.lane.slotKeyRefused.message'])
    expect(lane!.detail).toBe(
      EN['shell.lane.slotKeyRefused.split'].replace('{{time}}', '14:32:05').replace('{{why}}', why),
    )
    expect(lane!.detail).toContain('Fake It')
    expect(lane!.detail).not.toContain('CAT/port')
  })

  it('says Nexus could not confirm the radio’s SPLIT is off and how to clear it, not to change Split operation', () => {
    const why = "Nexus could not confirm the radio's SPLIT is off"
    const lane = slotKeyRefusedLane({ at: AT, why, splitFailed: true, splitOffUnconfirmed: true })
    expect(lane!.tier).toBe('critical')
    expect(lane!.message).toBe(EN['shell.lane.slotKeyRefused.message'])
    expect(lane!.detail).toBe(EN['shell.lane.slotKeyRefused.splitOff'].replace('{{time}}', '14:32:05'))
    expect(lane!.detail).toContain('disconnect and reconnect the radio')
    expect(lane!.detail).not.toContain('Fake It')
  })

  it('says the radio was still on the DAX Nexus set once native audio was off, in its own sentence', () => {
    const lane = slotKeyRefusedLane({
      at: AT,
      why: 'not keying a DIGU over: native audio is off, and the radio still takes its transmit audio from the DAX Nexus set, which nothing feeds until its mic input is back',
      flexAudio: { mode: 'DIGU', cause: 'daxUnfed' },
    })
    expect(lane!.tier).toBe('critical')
    expect(lane!.message).toBe(EN['shell.lane.slotKeyRefused.message'])
    expect(lane!.detail).toBe(
      EN['shell.lane.slotKeyRefused.flex.daxUnfed']
        .replace('{{mode}}', 'DIGU')
        .replace('{{time}}', '14:32:05'),
    )
  })
})
