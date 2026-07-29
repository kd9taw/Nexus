import { describe, it, expect } from 'vitest'
import { aprsDecodeStatus } from './components/AprsCockpit'
import type { AprsHealth } from './api'

// THE BUG THIS EXISTS FOR: only frames that passed the AX.25 checksum ever reached the UI, so an
// empty APRS screen was the single answer to three unrelated questions — the app listening to the
// wrong sound card, a channel arriving too corrupted to check, and a genuinely quiet band. Two of
// those three are faults the operator fixes in seconds once told which one it is. These tests pin
// that the readout actually distinguishes them.

const NOW = 1_700_000_000

function health(over: Partial<AprsHealth> = {}): AprsHealth {
  return {
    armed: true,
    audioPeak: 0.4,
    framesSeen: 0,
    framesDecoded: 0,
    lastDecodeUnix: null,
    ...over,
  }
}

describe('APRS decode health tells the three empty-screen cases apart', () => {
  it('armed but fed silence reads as DEAF and points at the input device', () => {
    const s = aprsDecodeStatus(health({ audioPeak: 0 }), NOW)
    expect(s.state).toBe('deaf')
    expect(s.detail).toMatch(/input/i)
    // The heart of the report: hearing it on the speaker says nothing about what is captured.
    expect(s.detail).toMatch(/hear/i)
  })

  it('audio arriving with no packets reads as a QUIET BAND, not a fault', () => {
    const s = aprsDecodeStatus(health({ audioPeak: 0.3 }), NOW)
    expect(s.state).toBe('listening')
  })

  it('frames seen but none passing the checksum reads as UNREADABLE', () => {
    const s = aprsDecodeStatus(health({ framesSeen: 12 }), NOW)
    expect(s.state).toBe('unreadable')
    expect(s.label).toContain('12')
    expect(s.detail).toMatch(/144\.390|clipping/i)
  })

  it('successful decodes read as DECODING with a count', () => {
    const s = aprsDecodeStatus(
      health({ framesSeen: 20, framesDecoded: 18, lastDecodeUnix: NOW - 30 }),
      NOW,
    )
    expect(s.state).toBe('decoding')
    expect(s.label).toContain('18')
    expect(s.detail).toContain('30s')
  })

  it('a decode outranks a later drain that saw nothing', () => {
    // Packets are bursty: the drain right after a decode is usually empty. That must not flip the
    // readout back to "quiet band" and hide the fact that the decoder is working.
    const s = aprsDecodeStatus(
      health({ audioPeak: 0.2, framesSeen: 3, framesDecoded: 3, lastDecodeUnix: NOW - 5 }),
      NOW,
    )
    expect(s.state).toBe('decoding')
  })

  it('disarmed says so rather than blaming the radio', () => {
    expect(aprsDecodeStatus(health({ armed: false }), NOW).state).toBe('off')
    expect(aprsDecodeStatus(null, NOW).state).toBe('off')
  })
})
