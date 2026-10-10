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
    expect(audioErrorLane(sentence, kind)).toEqual({ tier, message: words, detail: sentence })
  })

  it('a PTT the rig did not accept reads PTT NOT ACCEPTED, not RADIO STOPPED', () => {
    const lane = audioErrorLane(PTT, 'ptt')
    expect(lane?.message).toBe('PTT NOT ACCEPTED')
    expect(lane?.message).not.toBe('RADIO STOPPED')
    expect(lane?.detail).toBe(PTT)
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
