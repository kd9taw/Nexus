import { describe, it, expect } from 'vitest'
import { aprsDecodeStatus, aprsRadioNote, radioCoversMhz } from './components/AprsCockpit'
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
    lastFrameSeenUnix: null,
    framePeak: 0,
    maxFramePeak: 0,
    frameClippedSamples: 0,
    radioName: 'FT-991A',
    bandRadioCount: 1,
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
    const s = aprsDecodeStatus(health({ framesSeen: 12, lastFrameSeenUnix: NOW - 5 }), NOW)
    expect(s.state).toBe('unreadable')
    // Squelch opening mid-packet eats the opening flags — routine, not a misconfiguration.
    expect(s.detail).toMatch(/squelch|part|partial/i)
    expect(s.detail).toMatch(/normal|expected/i)
  })

  it('still names the real faults worth checking when NOTHING ever decodes', () => {
    const s = aprsDecodeStatus(health({ framesSeen: 12, lastFrameSeenUnix: NOW - 5 }), NOW)
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
    const s = aprsDecodeStatus(health({ audioPeak: 0, framesSeen: 3, lastFrameSeenUnix: NOW - 5 }), NOW)
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

// ⭐ SECOND ON-AIR REPORT, same session. FT8 was decoding beautifully on 2 m at the moment the
// APRS chip claimed there was no audio — which proves the capture path end to end and means the
// radio was simply parked on the FT8 frequency in USB. One receiver, one dial: APRS's 144.390 FM
// was never being received at all.
//
// No audio-level message can ever say that. The app KNOWS the dial and mode from CAT, so when the
// radio is not where APRS lives, that fact outranks every inference drawn from the audio.
const RADIO_FT8 = { dialMhz: 144.174, sideband: 'USB' }
const RADIO_APRS = { dialMhz: 144.39, sideband: 'FM' }
const APRS_DIAL = 144.39

describe('the chip names who owns the dial', () => {
  it('says the radio is on the wrong frequency instead of blaming the audio', () => {
    const s = aprsDecodeStatus(health({ audioPeak: 0 }), NOW, RADIO_FT8, APRS_DIAL)
    expect(s.state).toBe('wrongfreq')
    expect(s.detail).toMatch(/144\.174/)
    expect(s.detail).toMatch(/144\.390/)
  })

  it('names the MODE when the dial is right but the rig is not in FM', () => {
    const s = aprsDecodeStatus(
      health(),
      NOW,
      { dialMhz: 144.39, sideband: 'USB' },
      APRS_DIAL,
    )
    expect(s.state).toBe('wrongfreq')
    expect(s.detail).toMatch(/FM/)
  })

  it('is quiet when the radio IS on the APRS channel', () => {
    expect(aprsDecodeStatus(health(), NOW, RADIO_APRS, APRS_DIAL).state).toBe('listening')
  })

  it('judges against the SELECTED APRS frequency, not a hardcoded North American one', () => {
    // 144.800 is the European channel. A European operator correctly tuned there must not be
    // told they are on the wrong frequency.
    const eu = { dialMhz: 144.8, sideband: 'FM' }
    expect(aprsDecodeStatus(health(), NOW, eu, 144.8).state).toBe('listening')
    // ...and the same radio IS wrong if the operator picked the North American channel.
    expect(aprsDecodeStatus(health(), NOW, eu, 144.39).state).toBe('wrongfreq')
  })

  it('does not invent a verdict when the rig mode is unknown', () => {
    const s = aprsDecodeStatus(health(), NOW, { dialMhz: 144.39, sideband: '' }, APRS_DIAL)
    expect(s.state).not.toBe('wrongfreq')
  })

  it('tolerates small dial offsets rather than nagging about rounding', () => {
    const s = aprsDecodeStatus(health(), NOW, { dialMhz: 144.3901, sideband: 'FM' }, APRS_DIAL)
    expect(s.state).not.toBe('wrongfreq')
  })
})

describe('the state ladder never hides a higher fact behind a lower guess', () => {
  it('a wrong dial outranks a dead capture — the dial is the dispositive fact', () => {
    const s = aprsDecodeStatus(
      health({ audioPeak: 0, lastAudioUnix: null, drains: 500 }),
      NOW,
      RADIO_FT8,
      APRS_DIAL,
    )
    expect(s.state).toBe('wrongfreq')
  })

  it('a wrong dial outranks a silent input', () => {
    const s = aprsDecodeStatus(health({ audioPeak: 0 }), NOW, RADIO_FT8, APRS_DIAL)
    expect(s.state).toBe('wrongfreq')
  })

  it('a wrong dial outranks frames failing their checksum', () => {
    // Exactly the operator's 10%: FT8 audio through an AFSK demodulator throws up occasional
    // flag patterns that never pass a checksum. Blaming the signal there is misleading.
    const s = aprsDecodeStatus(health({ framesSeen: 9 }), NOW, RADIO_FT8, APRS_DIAL)
    expect(s.state).toBe('wrongfreq')
  })

  it('a dead capture still outranks a silent input when the dial is fine', () => {
    const s = aprsDecodeStatus(
      health({ audioPeak: 0, lastAudioUnix: null, drains: 500 }),
      NOW,
      RADIO_APRS,
      APRS_DIAL,
    )
    expect(s.state).toBe('nocapture')
  })

  it('still works with no radio information at all', () => {
    // Callers without CAT data must keep the old behaviour rather than crash or over-claim.
    expect(aprsDecodeStatus(health({ audioPeak: 0 }), NOW).state).toBe('silent')
  })
})

describe('the wrong-frequency message names WHICH thing is wrong', () => {
  it('mode-only: says the dial is right and the mode is not', () => {
    // The operator asked outright "should APRS be FM or USB?" — so the message has to answer
    // that, not just restate the frequency they are already on.
    const s = aprsDecodeStatus(health(), NOW, { dialMhz: 144.39, sideband: 'USB' }, 144.39)
    expect(s.state).toBe('wrongfreq')
    expect(s.label).toMatch(/mode/i)
    expect(s.detail).toMatch(/needs FM/i)
    // Explains WHY, so it reads as a reason rather than a rule.
    expect(s.detail).toMatch(/garbl|demodulat/i)
  })

  it('frequency-only: names the dial, not the mode', () => {
    const s = aprsDecodeStatus(health(), NOW, { dialMhz: 144.174, sideband: 'FM' }, 144.39)
    expect(s.state).toBe('wrongfreq')
    expect(s.detail).toMatch(/144\.174/)
    expect(s.detail).toMatch(/144\.390/)
  })

  it('both wrong: names both', () => {
    const s = aprsDecodeStatus(health(), NOW, { dialMhz: 144.174, sideband: 'USB' }, 144.39)
    expect(s.detail).toMatch(/144\.174 USB/)
    expect(s.detail).toMatch(/144\.390 FM/)
  })

  it('accepts data-FM submodes as FM — PKTFM is still FM on the air', () => {
    expect(aprsDecodeStatus(health(), NOW, { dialMhz: 144.39, sideband: 'PKTFM' }, 144.39).state)
      .toBe('listening')
  })
})

// ⭐ THIRD ON-AIR REPORT (0.21.2). The dBFS readout landed and immediately caught the next
// defect, in a sentence that contradicted itself:
//
//   "2 packets were heard but none passed the checksum... Input level: peak -99 dBFS."
//
// Nothing is "heard" at -99 dBFS. The counters are CUMULATIVE SINCE ARMING; the level is LIVE.
// Two frame candidates from minutes ago — real bursts, or the deframer's flag-hunt locking onto
// dither — latched the chip in `unreadable` forever, and the sentence then led with the stale
// evidence and trailed with the live level as though both described now.
//
// Facts of different ages must never be rendered in one present tense.

describe('a stale frame count decays instead of latching', () => {
  it('frames from minutes ago no longer claim the present', () => {
    const s = aprsDecodeStatus(
      health({ audioPeak: 0.00001, framesSeen: 2, lastFrameSeenUnix: NOW - 360 }),
      NOW,
    )
    expect(s.state).toBe('silent')
  })

  it('recent frames DO hold the unreadable state — it is still the present tense', () => {
    const s = aprsDecodeStatus(
      health({ audioPeak: 0.4, framesSeen: 2, lastFrameSeenUnix: NOW - 10 }),
      NOW,
    )
    expect(s.state).toBe('unreadable')
  })

  it('decays to whatever the LIVE level says, not always to silent', () => {
    // Audio present at hiss level with no recent frames: a quiet channel, not a fault.
    const s = aprsDecodeStatus(
      health({ audioPeak: 0.03, framesSeen: 2, lastFrameSeenUnix: NOW - 600 }),
      NOW,
    )
    expect(s.state).toBe('listening')
  })

  it('a frame count with no timestamp at all cannot claim the present', () => {
    const s = aprsDecodeStatus(
      health({ audioPeak: 0.00001, framesSeen: 2, lastFrameSeenUnix: null }),
      NOW,
    )
    expect(s.state).not.toBe('unreadable')
  })
})

describe('every claim says WHEN it was true', () => {
  it('the unreadable message dates the count and keeps the level separate', () => {
    const s = aprsDecodeStatus(
      health({ audioPeak: 0.4, framesSeen: 2, lastFrameSeenUnix: NOW - 20 }),
      NOW,
    )
    expect(s.detail).toMatch(/since arming/i)
    expect(s.detail).toMatch(/last one 20s ago/i)
  })

  it('the decode message dates the last decode rather than an eternal count', () => {
    const s = aprsDecodeStatus(
      health({ framesSeen: 20, framesDecoded: 18, lastDecodeUnix: NOW - 720 }),
      NOW,
    )
    expect(s.state).toBe('decoding')
    expect(s.detail).toMatch(/since arming/i)
    expect(s.detail).toMatch(/12m ago/i)
  })

  it('the level says WHAT WINDOW it measured, so the number is readable', () => {
    const s = aprsDecodeStatus(health({ audioPeak: 0.4 }), NOW)
    // It is the peak of the most recent ~100 ms drain, not a decaying meter.
    expect(s.detail).toMatch(/most recent|0\.1 s|100 ms/i)
  })
})

// ⭐ FOURTH ON-AIR ROUND. Operator heard a burst, the app detected it, CRC failed, live peak read
// -62 dBFS — read NINE SECONDS after the burst, so that number was the gap, not the packet. Then
// they raised the path and read -15 dBFS of open-squelch hiss, with 4 bursts and 0 CRC passes.
//
// Two lessons baked in here. First: advice about burst level must be measured AT BURST TIME, never
// from the live drain peak. Second — and this is the one that changes what we SAY — measurement
// shows level is NOT why frames fail. The Bell-202 discriminator compares mark energy against
// space energy, so absolute level cancels: decode holds to -140 dBFS clean, to ~9 dB SNR over the
// measured -99 dBFS dither floor, and through 30 dB of hard clipping. So the chip reports level as
// HEADROOM, and must not claim it is the cause of a CRC failure.

const QUIET_BURST = 0.0008 // -62 dBFS, the operator's measured burst
const HEALTHY_BURST = 0.1 // -20 dBFS
const HOT_BURST = 0.995 // ~0 dBFS

describe('burst level advice is measured at BURST time, not gap time', () => {
  it('reports the burst peak, not the live gap peak', () => {
    // The exact field trap: live drain is quiet hiss, the burst was louder.
    const s = aprsDecodeStatus(
      health({
        audioPeak: 0.00001,
        framePeak: HEALTHY_BURST,
        maxFramePeak: HEALTHY_BURST,
        framesSeen: 1,
        lastFrameSeenUnix: NOW - 9,
      }),
      NOW,
    )
    expect(s.state).toBe('unreadable')
    expect(s.detail).toMatch(/burst/i)
    expect(s.detail).toMatch(/-20 dBFS/)
  })

  it('flags a burst far below the healthy band, with the number and where to fix it', () => {
    const s = aprsDecodeStatus(
      health({ framePeak: QUIET_BURST, maxFramePeak: QUIET_BURST, framesSeen: 1, lastFrameSeenUnix: NOW - 5 }),
      NOW,
    )
    expect(s.detail).toMatch(/-62 dBFS/)
    expect(s.detail).toMatch(/USB AF Output/i)
  })

  it('does NOT claim low level is why the checksum failed — measurement says otherwise', () => {
    const s = aprsDecodeStatus(
      health({ framePeak: QUIET_BURST, maxFramePeak: QUIET_BURST, framesSeen: 1, lastFrameSeenUnix: NOW - 5 }),
      NOW,
    )
    expect(s.detail).toMatch(/headroom|margin/i)
    expect(s.detail).not.toMatch(/too quiet to decode|cannot decode because/i)
  })

  it('flags a CLIPPING burst and says to turn it down', () => {
    const s = aprsDecodeStatus(
      health({
        framePeak: HOT_BURST,
        maxFramePeak: HOT_BURST,
        frameClippedSamples: 400,
        framesSeen: 4,
        lastFrameSeenUnix: NOW - 5,
      }),
      NOW,
    )
    expect(s.detail).toMatch(/clipping/i)
    expect(s.detail).toMatch(/lower|reduce/i)
  })

  it('says nothing about level when the burst sits in the healthy band', () => {
    const s = aprsDecodeStatus(
      health({ framePeak: HEALTHY_BURST, maxFramePeak: HEALTHY_BURST, framesSeen: 2, lastFrameSeenUnix: NOW - 5 }),
      NOW,
    )
    expect(s.detail).not.toMatch(/USB AF Output/i)
    expect(s.detail).not.toMatch(/clipping/i)
  })

  it('states the healthy band and a hiss target the operator can actually watch', () => {
    const s = aprsDecodeStatus(health({ audioPeak: 0.03 }), NOW)
    expect(s.detail).toMatch(/-30/)
    expect(s.detail).toMatch(/hiss/i)
  })
})

// ⭐ THIRD ON-AIR REPORT, 0.21.x, Yaesu FTdx10 — an HF/50 MHz radio with no 2 m at all.
// Opening the APRS cockpit auto-tuned 144.390, the radio refused it, and the CAT link was dead
// until Nexus restarted. The chip's job here is narrow but important: say the honest thing, and
// do NOT offer a [Tune] button whose only possible outcome is another refusal.
//
// The fail-open rule is the load-bearing part. Coverage is UNKNOWN far more often than it is
// absent — no CAT, first poll not in yet, a rigctld whose \dump_state we cannot parse — and a
// chip that read "unknown" as "no VHF radio" would tell IC-9700 operators their radio can't do
// APRS. Only a positively-parsed range list may ever say no.
const HF_ONLY: [number, number][] = [[0.03, 60]] // FTdx10 (Hamlib rigs/yaesu/ftdx10.c): 30 kHz – 60 MHz receive
const VHF_UHF: [number, number][] = [
  [144, 148],
  [430, 450],
]

describe('an HF-only radio is told the truth instead of being made to look broken', () => {
  it('says the radio has no 2 m rather than blaming the frequency, mode or audio', () => {
    const s = aprsDecodeStatus(
      health({ audioPeak: 0 }),
      NOW,
      { dialMhz: 14.25, sideband: 'USB', rxRangesMhz: HF_ONLY },
      APRS_DIAL,
    )
    expect(s.state).toBe('norf')
    // Crucially NOT 'wrongfreq' — that state offers a Tune button, and tuning cannot work here.
    expect(s.state).not.toBe('wrongfreq')
  })

  it('names what is needed AND the path that still works on this station', () => {
    const s = aprsDecodeStatus(
      health(),
      NOW,
      { dialMhz: 14.25, sideband: 'USB', rxRangesMhz: HF_ONLY },
      APRS_DIAL,
    )
    expect(s.detail).toMatch(/VHF/)
    // The APRS-IS feed is genuinely useful on an HF-only station: the view must not read as dead.
    expect(s.detail).toMatch(/internet/i)
    expect(s.detail).toMatch(/144\.390/)
  })

  it('outranks even "Monitor off" — arming the decoder cannot help', () => {
    const s = aprsDecodeStatus(
      health({ arm: 'off' }),
      NOW,
      { dialMhz: 14.25, sideband: 'USB', rxRangesMhz: HF_ONLY },
      APRS_DIAL,
    )
    expect(s.state).toBe('norf')
  })

  it('and survives a null health (before the first poll)', () => {
    const s = aprsDecodeStatus(
      null,
      NOW,
      { dialMhz: 14.25, sideband: 'USB', rxRangesMhz: HF_ONLY },
      APRS_DIAL,
    )
    expect(s.state).toBe('norf')
  })

  it('FAILS OPEN: unknown coverage never claims the radio lacks VHF', () => {
    for (const ranges of [undefined, [] as [number, number][]]) {
      const s = aprsDecodeStatus(
        health(),
        NOW,
        { dialMhz: 144.174, sideband: 'USB', rxRangesMhz: ranges },
        APRS_DIAL,
      )
      expect(s.state, `ranges=${JSON.stringify(ranges)}`).not.toBe('norf')
      // The ordinary CAT-aware verdict still applies — nothing is lost by not knowing.
      expect(s.state).toBe('wrongfreq')
    }
  })

  it('a radio that DOES cover 2 m is judged normally', () => {
    const covered = { dialMhz: 144.39, sideband: 'FM', rxRangesMhz: VHF_UHF }
    expect(aprsDecodeStatus(health(), NOW, covered, APRS_DIAL).state).toBe('listening')
    // …and it still gets the wrong-frequency verdict when parked on FT8.
    const ft8 = { dialMhz: 144.174, sideband: 'USB', rxRangesMhz: VHF_UHF }
    expect(aprsDecodeStatus(health(), NOW, ft8, APRS_DIAL).state).toBe('wrongfreq')
  })

  it('judges the SELECTED channel, so a European pick on an HF rig is also out of range', () => {
    const s = aprsDecodeStatus(
      health(),
      NOW,
      { dialMhz: 14.25, sideband: 'USB', rxRangesMhz: HF_ONLY },
      144.8,
    )
    expect(s.state).toBe('norf')
    expect(s.detail).toMatch(/144\.800/)
  })

  it('a 6 m-capable HF rig is correctly told it covers 50 MHz but not 144', () => {
    // Guards the range arithmetic itself: the FTdx10 really does reach 6 m (50 MHz).
    expect(radioCoversMhz(HF_ONLY, 50.313)).toBe(true)
    expect(radioCoversMhz(HF_ONLY, 144.39)).toBe(false)
    expect(radioCoversMhz(VHF_UHF, 144.39)).toBe(true)
    expect(radioCoversMhz(VHF_UHF, 50.313)).toBe(false)
    // Inclusive bounds, and unknown stays unknown.
    expect(radioCoversMhz([[144, 148]], 144)).toBe(true)
    expect(radioCoversMhz([[144, 148]], 148)).toBe(true)
    expect(radioCoversMhz(undefined, 144.39)).toBe(null)
    expect(radioCoversMhz([], 144.39)).toBe(null)
  })
})

// ⭐ FIFTH instance today of the same class: silence with nothing named. A merge changed which of
// two 2 m-capable radios an APRS activation resolves to, the tap followed the new one whose audio
// was configured for FT8, and the section went quiet with nothing on screen saying which radio it
// was listening to. Wrong-radio silence and a dead band are indistinguishable without the name.
//
// Named ONLY when there is genuine ambiguity — a single capable radio has nothing to disambiguate,
// and saying so would be noise on a calm cockpit.

describe('the chip names its radio when more than one could be listening', () => {
  it('names the radio when two radios cover the band', () => {
    const n = aprsRadioNote(health({ radioName: 'FT-991A', bandRadioCount: 2 }))
    expect(n).not.toBeNull()
    expect(n!.label).toContain('FT-991A')
  })

  it('explains that routing decides it, and where to change that', () => {
    const n = aprsRadioNote(health({ radioName: 'FT-991A', bandRadioCount: 3 }))
    expect(n!.detail).toMatch(/3 of your radios/i)
    expect(n!.detail).toMatch(/active radio/i)
    expect(n!.detail).toMatch(/routing/i)
    expect(n!.detail).toMatch(/Settings/i)
  })

  it('stays QUIET when only one radio covers the band', () => {
    expect(aprsRadioNote(health({ radioName: 'IC-9700', bandRadioCount: 1 }))).toBeNull()
  })

  it('stays quiet on a single-radio station', () => {
    expect(aprsRadioNote(health({ radioName: 'IC-9700', bandRadioCount: 0 }))).toBeNull()
  })

  it('stays quiet rather than showing an empty name', () => {
    // A roster with no profile for the active radio must not render "on ".
    expect(aprsRadioNote(health({ radioName: '', bandRadioCount: 2 }))).toBeNull()
  })

  it('is silent while the decoder is not armed — nothing is listening to name', () => {
    expect(aprsRadioNote(health({ arm: 'off', radioName: 'FT-991A', bandRadioCount: 2 }))).toBeNull()
  })
})
