// The status-lane item for a computer-microphone over that ended, or never keyed, for a reason the
// operator did not choose: it says so, when, and why, in the operator's words.
import { describe, expect, it } from 'vitest'
import { pcMicLane } from './pcMic'
import { EN } from '../i18n/en'
import type { PcMicCause } from '../types'

describe('the status lane after a computer-microphone over ended', () => {
  // 2026-10-27 14:32:05 UTC
  const AT = Date.UTC(2026, 9, 27, 14, 32, 5) / 1000

  it('says nothing while no over ended that way', () => {
    expect(pcMicLane(null)).toBeNull()
    expect(pcMicLane(undefined)).toBeNull()
  })

  it.each<[PcMicCause, string]>([
    ['noDevice', 'shell.lane.pcMic.noDevice'],
    ['sameAsReceive', 'shell.lane.pcMic.sameAsReceive'],
    ['openFailed', 'shell.lane.pcMic.openFailed'],
    ['deviceLost', 'shell.lane.pcMic.deviceLost'],
    ['noAudio', 'shell.lane.pcMic.noAudio'],
    ['ceiling', 'shell.lane.pcMic.ceiling'],
    ['watchdog', 'shell.lane.pcMic.watchdog'],
    ['busy', 'shell.lane.pcMic.busy'],
    ['flexMic', 'shell.lane.pcMic.flexMic'],
    ['radioReceive', 'shell.lane.pcMic.radioReceive'],
    ['loopback', 'shell.lane.pcMic.loopback'],
    ['gateDown', 'shell.lane.pcMic.gateDown'],
    ['interrupted', 'shell.lane.pcMic.interrupted'],
    ['keyRefused', 'shell.lane.pcMic.keyRefused'],
    ['unsure', 'shell.lane.pcMic.unsure'],
  ])('names %s, with the time', (cause, key) => {
    const lane = pcMicLane({ at: AT, cause })
    expect(lane).not.toBeNull()
    expect(lane!.tier).toBe('critical')
    expect(lane!.message).toBe(EN['shell.lane.pcMic.message'])
    expect(lane!.detail).toBe(
      EN['shell.lane.pcMic.detail']
        .replace('{{time}}', '14:32:05')
        .replace('{{why}}', EN[key as keyof typeof EN] as string),
    )
  })
})
