import { describe, it, expect } from 'vitest'
import { aprsDecodeStatus } from './components/AprsCockpit'
import type { AprsHealth } from './api'

// WHY THIS FILE EXISTS (original): only frames that passed the AX.25 checksum ever reached the UI,
// so an empty APRS screen was the single answer to several unrelated questions. These tests pin
// that the readout distinguishes them.
//
// ⭐ ON-AIR CORRECTION, 0.21.1, IC-9700 on 144.390. The first cut of this got the most common
// state on an FM channel WRONG, and said so in the most misleading possible way. A squelched USB
// codec streams a CONTINUOUS run of digital ZEROS — samples keep arriving, so nothing goes stale;
// only the LEVEL is zero. The old logic tested staleness OR level in one branch and called both
// "no audio is reaching the decoder — check your input device". So an operator with a perfectly
// correct device, sitting on an idle channel between packets, was told for ~90% of the time that
// their audio routing was broken, and went hunting a fault that did not exist.
//
// Those are opposite diagnoses and must never share a state again:
//   - NOTHING ARRIVING  -> the capture device really is dead/wrong. A fault.
//   - ARRIVING, SILENT  -> squelch closed. The normal resting state of an FM channel.

const NOW = 1_700_000_000

function health(over: Partial<AprsHealth> = {}): AprsHealth {
  return {
    arm: 'explicit',
    audioPeak: 0.4,
    lastAudioUnix: NOW,
    drains: 500,
    framesSeen: 0,
    framesDecoded: 0,
    lastDecodeUnix: null,
    ...over,
  }
}

describe('a squelched channel is not a broken audio device', () => {
  it('samples arriving at zero level reads as SILENT, not as a capture fault', () => {
    // Scenario (b), measured against the engine: peak 0.0, lastAudioUnix FRESH.
    const s = aprsDecodeStatus(health({ audioPeak: 0, lastAudioUnix: NOW }), NOW)
    expect(s.state).toBe('silent')
  })

  it('and it blames the SQUELCH first, not the input device', () => {
    const s = aprsDecodeStatus(health({ audioPeak: 0, lastAudioUnix: NOW }), NOW)
    // Squelch has to come before any mention of the device — it is the overwhelmingly common
    // cause, and sending the operator to Settings first is what wasted their time on air.
    const squelchAt = s.detail.toLowerCase().indexOf('squelch')
    const deviceAt = s.detail.toLowerCase().indexOf('device')
    expect(squelchAt, 'the squelch must be named').toBeGreaterThan(-1)
    expect(squelchAt, 'squelch must be mentioned before the input device').toBeLessThan(
      deviceAt === -1 ? Number.MAX_SAFE_INTEGER : deviceAt,
    )
  })

  it('does not read as an alarm — an idle FM channel is a normal resting state', () => {
    const s = aprsDecodeStatus(health({ audioPeak: 0 }), NOW)
    expect(s.detail).toMatch(/normal|idle|between packets/i)
  })

  it('no samples arriving at all is a SEPARATE state that does point at the device', () => {
    // Scenario (a): lastAudioUnix never set, and the thread has polled plenty of times.
    const s = aprsDecodeStatus(health({ audioPeak: 0, lastAudioUnix: null, drains: 500 }), NOW)
    expect(s.state).toBe('nocapture')
    expect(s.detail).toMatch(/input/i)
  })

  it('capture that STOPS arriving also reads as nocapture', () => {
    const s = aprsDecodeStatus(health({ lastAudioUnix: NOW - 60, drains: 500 }), NOW)
    expect(s.state).toBe('nocapture')
  })

  it('does not cry nocapture before the decode thread has had a chance to drain', () => {
    // Arming resets health. The cockpit re-reads it immediately, so for the first fraction of a
    // second lastAudioUnix is legitimately null — that must not flash "check your input device"
    // every single time the operator arms Monitor.
    const s = aprsDecodeStatus(health({ audioPeak: 0, lastAudioUnix: null, drains: 0 }), NOW)
    expect(s.state).not.toBe('nocapture')
  })

  it('still tolerates the ordinary gap between drains', () => {
    expect(aprsDecodeStatus(health({ lastAudioUnix: NOW - 2 }), NOW).state).toBe('listening')
  })
})

describe('the level the decoder is hearing is a number on screen', () => {
  it('reports a dBFS level whenever audio is flowing', () => {
    // 0.4 peak ~= -8 dBFS. The operator should not have to infer the level from a word.
    expect(aprsDecodeStatus(health({ audioPeak: 0.4 }), NOW).detail).toMatch(/-?\d+\s*dBFS/i)
  })

  it('reports the level on a silent input too — that is the number being judged', () => {
    const s = aprsDecodeStatus(health({ audioPeak: 0 }), NOW)
    expect(s.detail).toMatch(/dBFS|silent/i)
  })

  it('hiss reads as a real level, well above silence', () => {
    const s = aprsDecodeStatus(health({ audioPeak: 0.03 }), NOW)
    expect(s.state).toBe('listening')
    expect(s.detail).toMatch(/-3\d\s*dBFS/i) // ~-30 dBFS
  })
})

describe('failed checksums are honest about partial bursts', () => {
  it('says part-heard bursts are expected rather than reading as a hard fault', () => {
    const s = aprsDecodeStatus(health({ framesSeen: 12 }), NOW)
    expect(s.state).toBe('unreadable')
    // Squelch opening mid-packet eats the opening flags — routine, not a misconfiguration.
    expect(s.detail).toMatch(/squelch|part|partial/i)
    expect(s.detail).toMatch(/normal|expected/i)
  })

  it('still names the real faults worth checking when NOTHING ever decodes', () => {
    const s = aprsDecodeStatus(health({ framesSeen: 12 }), NOW)
    expect(s.detail).toMatch(/144\.390|clipping/i)
  })

  it('a decode outranks a later quiet drain — the chip must not flap back to an alarm', () => {
    // The operator's exact complaint: it sat on a fault message and flicked between states. Once
    // packets are decoding, a squelched gap between them is not news.
    const s = aprsDecodeStatus(
      health({ audioPeak: 0, framesSeen: 5, framesDecoded: 5, lastDecodeUnix: NOW - 4 }),
      NOW,
    )
    expect(s.state).toBe('decoding')
  })

  it('frames seen but none decoded outranks a squelched gap', () => {
    const s = aprsDecodeStatus(health({ audioPeak: 0, framesSeen: 3 }), NOW)
    expect(s.state).toBe('unreadable')
  })
})

describe('the states that were already right stay right', () => {
  it('disarmed says so rather than blaming the radio', () => {
    expect(aprsDecodeStatus(health({ arm: 'off' }), NOW).state).toBe('off')
    expect(aprsDecodeStatus(null, NOW).state).toBe('off')
  })

  it('audio flowing with nothing heard is a quiet channel, not a fault', () => {
    expect(aprsDecodeStatus(health({ audioPeak: 0.3 }), NOW).state).toBe('listening')
  })

  it('successful decodes report a count and how long ago', () => {
    const s = aprsDecodeStatus(
      health({ framesSeen: 20, framesDecoded: 18, lastDecodeUnix: NOW - 30 }),
      NOW,
    )
    expect(s.state).toBe('decoding')
    expect(s.label).toContain('18')
    expect(s.detail).toContain('30s')
  })
})
