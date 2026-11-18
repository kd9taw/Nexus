// The station's audio-error line in the status lane. The station writes one line for several
// different problems and names the kind of each beside its sentence; the lane heads the sentence
// with words for that kind, tiers it by what it costs the operator, and keeps the sentence whole
// for the tooltip. A refused PTT used to read "RADIO STOPPED" while the dial and CAT both worked.
//
// The words are pinned here as text, not read back from the catalog, so a catalog edit that made a
// refused PTT read "RADIO STOPPED" again, or two kinds read alike, goes red here.
import { describe, expect, it } from 'vitest'
import { audioErrorLane } from './audioError'
import type { AudioErrorKind } from '../types'

const PTT = "The rig didn't accept PTT — check your PTT method and CAT/port."
const PTT_BRIEF = 'PTT NOT ACCEPTED: check PTT method & port (Settings ▸ Radio ▸ Rig & CAT)'
const PTT_UNCONFIRMED = 'The rig did not answer the PTT key in time — check the CAT link to the radio.'

// Each kind the station names (crates/tempo-app/src/dto.rs, AudioErrorKind, by its wire name):
// a sentence its writer puts on the line, the headline, and the tier.
const KINDS: [AudioErrorKind, string, string, 'critical' | 'warning'][] = [
  [
    'engineStopped',
    'RADIO ENGINE STOPPED — TX/RX is dead until you restart Nexus (no usable sound card)',
    'RADIO ENGINE STOPPED',
    'critical',
  ],
  [
    'soundCard',
    'Sound card stopped — capture stream: device no longer available. Reopening…',
    'SOUND CARD FAILED',
    'critical',
  ],
  ['noReceiveAudio', 'Audio device reopened, waiting for samples…', 'NO RECEIVE AUDIO', 'critical'],
  ['ptt', PTT, 'PTT NOT ACCEPTED', 'critical'],
  ['pttUnconfirmed', PTT_UNCONFIRMED, 'PTT NOT CONFIRMED', 'warning'],
  [
    'flexAudio',
    'Native Flex audio is selected but no audio is arriving — switched back to the sound card.',
    'NO FLEX AUDIO',
    'warning',
  ],
  [
    'flexAddress',
    'Flex native panadapter is switched on but no Flex radio IP is set — nothing will start.',
    'NO FLEX RADIO IP',
    'warning',
  ],
  ['monitor', 'Headphone monitor could not open: device busy', 'HEADPHONE MONITOR OFF', 'warning'],
  [
    'voiceMic',
    'Voice mic could not open: device busy — recording from the shared input instead',
    'VOICE MIC FAILED',
    'warning',
  ],
  ['recording', 'Could not start QSO recording: permission denied', 'RECORDING FAILED', 'warning'],
  [
    'decodeCrash',
    'A Boundary decode crashed and was contained — receive continues. This is a bug; please report it.',
    'DECODE CRASHED',
    'warning',
  ],
]

describe('the status lane for the station audio-error line', () => {
  it.each(KINDS)('%s: heads the sentence with its own words and tier, the sentence whole', (kind, sentence, words, tier) => {
    // Only the refusal's own sentence carries the TX strip's shorter line (below).
    const brief = sentence === PTT ? { brief: PTT_BRIEF } : {}
    expect(audioErrorLane(sentence, kind)).toEqual({ tier, message: words, detail: sentence, ...brief })
  })

  // The TX strip's one line read "PTT NOT ACCEPTED — The rig didn't accept PTT — check your…", and the
  // ellipsis cut the advice. It leads with the advice and where the setting is; any other sentence the
  // station puts on the PTT line, and a refusal under any other kind, gets no shorter line.
  it('a refused PTT carries the advice first for the one-line cell, and only that sentence does', () => {
    expect(audioErrorLane(PTT, 'ptt')?.brief).toBe(PTT_BRIEF)
    // A station that knows why appends the radio's answer to the same sentence: the same shorter line, and
    // the answer kept whole in the detail (the hover).
    for (const answer of ['Hamlib answered: Feature not available (RPRT -11).', 'The PTT port said: Access is denied.']) {
      const lane = audioErrorLane(`${PTT} ${answer}`, 'ptt')
      expect(lane?.brief, answer).toBe(PTT_BRIEF)
      expect(lane?.detail, answer).toBe(`${PTT} ${answer}`)
    }
    expect(audioErrorLane('Not keyed: Split Operation could not put the radio’s TX VFO back on the dial', 'ptt')?.brief).toBeUndefined()
    expect(audioErrorLane(PTT, null)?.brief).toBeUndefined()
    expect(audioErrorLane(PTT_UNCONFIRMED, 'pttUnconfirmed')?.brief).toBeUndefined()
  })

  it('a PTT the rig did not accept reads PTT NOT ACCEPTED, not RADIO STOPPED', () => {
    const lane = audioErrorLane(PTT, 'ptt')
    expect(lane?.message).toBe('PTT NOT ACCEPTED')
    expect(lane?.message).not.toBe('RADIO STOPPED')
    expect(lane?.detail).toBe(PTT)
  })

  // A key the rig did not answer in time is not one it refused: it may still be keying, so the
  // headline must not say "not accepted", and it is a warning rather than a stopped transmitter.
  it('a PTT with no answer in time reads PTT NOT CONFIRMED, never PTT NOT ACCEPTED', () => {
    const lane = audioErrorLane(PTT_UNCONFIRMED, 'pttUnconfirmed')
    expect(lane?.message).toBe('PTT NOT CONFIRMED')
    expect(lane?.message).not.toBe(audioErrorLane(PTT, 'ptt')?.message)
    expect(lane?.tier).toBe('warning')
    expect(lane?.detail).toBe(PTT_UNCONFIRMED)
  })

  it('gives every kind words of its own', () => {
    const words = KINDS.map(([kind, sentence]) => audioErrorLane(sentence, kind)?.message)
    expect(new Set(words).size).toBe(KINDS.length)
    expect(words).not.toContain(audioErrorLane(PTT, null)?.message)
  })

  it('heads a line from a station too old to name the kind with plain words, critical', () => {
    for (const kind of [null, undefined]) {
      expect(audioErrorLane(PTT, kind)).toEqual({ tier: 'critical', message: 'RADIO ALERT', detail: PTT })
    }
  })

  it('heads a kind this page does not know the same way, whatever its name', () => {
    for (const kind of ['somethingNew', 'toString', 'constructor', '__proto__', 'hasOwnProperty']) {
      expect(audioErrorLane(PTT, kind as AudioErrorKind), kind).toEqual({
        tier: 'critical',
        message: 'RADIO ALERT',
        detail: PTT,
      })
    }
  })

  it('says nothing while there is no line, whatever kind a snapshot still carries', () => {
    expect(audioErrorLane(null, null)).toBeNull()
    expect(audioErrorLane(undefined, undefined)).toBeNull()
    expect(audioErrorLane(null, 'ptt')).toBeNull()
    expect(audioErrorLane('', 'soundCard')).toBeNull()
  })
})
