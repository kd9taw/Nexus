// The shared contest dupe verdict, tested at the seam rather than through a component, so
// the two surfaces that render it (the FD log strip and the FT cockpit's callsign card)
// are asserting against a checked answer rather than each other.
//
// Every case below is paired with the case that must NOT fire — a dupe check that only ever
// says "dupe" proves nothing, and the band/mode/call axes are exactly where a wrong answer
// costs the operator a contact.
import { describe, it, expect } from 'vitest'
import { contestDupe } from './contestDupe'
import type { FieldDayStatus } from '../types'

/** A contest session: W1AW worked on 20m PH by THIS position, K1ABC by another position. */
const fd = (over: Partial<FieldDayStatus> = {}): FieldDayStatus =>
  ({
    running: true,
    log: [{ call: 'W1AW', class: '1D', section: 'CT', band: '20m', mode: 'PH', submode: '' }],
    club: {
      syncState: 'synced',
      dupes: [
        ['K1ABC', '20m', 'PH'],
        ['N0XYZ', '40m', 'PH'],
      ],
      board: [],
    },
    ...over,
  }) as unknown as FieldDayStatus

describe('contestDupe — the own-log hard block', () => {
  it('says own for a call this position already worked on this band and mode class', () => {
    expect(contestDupe(fd(), 'W1AW', '20m', 'PH')).toBe('own')
  })

  it('is case- and whitespace-insensitive about the typed call', () => {
    expect(contestDupe(fd(), '  w1aw  ', '20m', 'PH')).toBe('own')
  })

  // The three axes that must each be able to clear the verdict on their own.
  it('does NOT fire on another band', () => {
    expect(contestDupe(fd(), 'W1AW', '40m', 'PH')).toBe('none')
  })

  it('does NOT fire in another mode class, when the contest counts them separately', () => {
    expect(contestDupe(fd(), 'W1AW', '20m', 'CW')).toBe('none')
  })

  it('does NOT fire for a call that is not in the log', () => {
    expect(contestDupe(fd(), 'W9XYZ', '20m', 'PH')).toBe('none')
  })
})

describe('contestDupe — a bird is its own band (ARRL Field Day 7.3.8)', () => {
  /** The same session, but W1AW was worked through RS-44 rather than terrestrially —
   *  same call, same band, same mode class, under a rule that names the satellite. */
  const viaBird = (rule: Record<string, unknown> = {}) =>
    fd({
      log: [
        {
          call: 'W1AW',
          class: '1D',
          section: 'CT',
          band: '20m',
          mode: 'PH',
          submode: '',
          sat: 'RS-44',
        },
      ],
      dupeRule: {
        byCall: true,
        byBand: true,
        byModeClass: true,
        byFields: [],
        bySentFields: [],
        modeClassGroups: [],
        logDupes: false,
        satelliteIsABand: true,
        fmSatelliteOnce: true,
        ...rule,
      },
    } as unknown as Partial<FieldDayStatus>)

  it('does NOT call a fresh terrestrial contact a dupe of a PASS contact on that band', () => {
    // ARRL: "Satellite QSOs also count for regular QSO credit. Show them listed
    // separately on the summary sheet as a separate 'band.'" The engine keys it that
    // way, so a badge that said DUPE here would talk the operator out of a contact the
    // log is about to accept — the over-reporting direction this module refuses.
    expect(contestDupe(viaBird(), 'W1AW', '20m', 'PH')).toBe('none')
  })

  it('CONTROL — the same row under a rule with no satellite dimension still fires', () => {
    // Without this the test above proves nothing: it would pass on a verdict that had
    // simply stopped matching the row for some other reason.
    expect(contestDupe(viaBird({ satelliteIsABand: false }), 'W1AW', '20m', 'PH')).toBe('own')
  })

  it('CONTROL — a terrestrial row in the same log still fires under the same rule', () => {
    // And the dimension must not switch the whole verdict off: `fd()`'s own row carries
    // no bird, so it is still this contact.
    expect(contestDupe(fd({ dupeRule: viaBird().dupeRule }), 'W1AW', '20m', 'PH')).toBe('own')
  })
})

describe('contestDupe — mode classes a contest counts as ONE', () => {
  // ILQP: "once per band and mode (phone and CW/digital)" — CW and DIG are one class there.
  const ilqp = fd({
    log: [{ call: 'W1AW', class: '1D', section: 'IL', band: '20m', mode: 'CW' }],
    dupeModeGroups: [['CW', 'DIG']],
  } as unknown as Partial<FieldDayStatus>)

  it('folds DIG onto a CW contact when the ruleset groups them', () => {
    expect(contestDupe(ilqp, 'W1AW', '20m', 'DIG')).toBe('own')
  })

  it('still separates PH, which that grouping does not name', () => {
    expect(contestDupe(ilqp, 'W1AW', '20m', 'PH')).toBe('none')
  })

  // The positive control for the fold itself: the SAME contact and the SAME query, with the
  // grouping removed, must come back clean. Without this, the case above would pass on a
  // build where the mode comparison had been dropped altogether.
  it('does NOT fold when the ruleset names no groups', () => {
    const separate = fd({
      log: [{ call: 'W1AW', class: '1D', section: 'IL', band: '20m', mode: 'CW' }],
    } as unknown as Partial<FieldDayStatus>)
    expect(contestDupe(separate, 'W1AW', '20m', 'DIG')).toBe('none')
  })
})

describe('contestDupe — the club warning', () => {
  it('says club for a key another position logged, on this band and mode', () => {
    expect(contestDupe(fd(), 'K1ABC', '20m', 'PH')).toBe('club')
  })

  it('does NOT fire for a club key on another band', () => {
    expect(contestDupe(fd(), 'N0XYZ', '20m', 'PH')).toBe('none')
  })

  it('own beats club when both would match — the hard block is the one to show', () => {
    const both = fd({
      club: { syncState: 'synced', dupes: [['W1AW', '20m', 'PH']], board: [] },
    } as unknown as Partial<FieldDayStatus>)
    expect(contestDupe(both, 'W1AW', '20m', 'PH')).toBe('own')
  })
})

describe('contestDupe — when there is nothing to say', () => {
  it('is none with no contest running', () => {
    expect(contestDupe(null, 'W1AW', '20m', 'PH')).toBe('none')
    expect(contestDupe(undefined, 'W1AW', '20m', 'PH')).toBe('none')
  })

  it('is none for an empty call, so a cleared box clears the badge', () => {
    expect(contestDupe(fd(), '', '20m', 'PH')).toBe('none')
    expect(contestDupe(fd(), '   ', '20m', 'PH')).toBe('none')
  })

  it('survives a snapshot from a build with no log or club block at all', () => {
    expect(contestDupe({} as unknown as FieldDayStatus, 'W1AW', '20m', 'PH')).toBe('none')
  })
})

// ---------------------------------------------------------------------------
// THE RULESET'S OWN KEY, not a hardcoded triple.
//
// `contestDupe` compared (call, band, mode class) whatever contest was running. That is the
// key for TWO of the seventeen shipped rulesets — both Field Days. Counted from
// fd_rules.seed.json: eight key on an exchange slot as well (five QSO parties on QTH, three
// ARRL VHF runnings on GRID), and seven more drop a component (Sweepstakes keys on the CALL
// ALONE, rule 2.2; CQ WW and WPX drop the mode class).
//
// The two directions are NOT symmetric, and `contest/dupe.rs` says which one hurts:
// "Under-reporting a dupe costs one duplicate contact that scores zero; over-reporting
// refuses a legal contact." So a rule this helper cannot evaluate answers 'none' rather than
// guessing — exactly as `FieldDayLog::worked_key` does on the Rust side for the same reason.
// ---------------------------------------------------------------------------
const ruled = (
  rule: Partial<{
    byCall: boolean; byBand: boolean; byModeClass: boolean
    byFields: string[]; bySentFields: string[]; modeClassGroups: string[][]
  }>,
  over: Partial<FieldDayStatus> = {},
): FieldDayStatus =>
  fd({
    dupeRule: {
      byCall: true, byBand: true, byModeClass: true,
      byFields: [], bySentFields: [], modeClassGroups: [],
      ...rule,
    },
    ...over,
  } as unknown as Partial<FieldDayStatus>)

describe('contestDupe — the rule decides which components are the key', () => {
  // ⭐ THE REPORTED DEFECT. An ARRL VHF running keys on the GRID as well, in both
  // directions. A rover reappears from a new grid — a LEGAL, scoring contact — and the card
  // said already-worked, so the operator skipped it. This helper is not given the grid, so
  // it must decline rather than answer from a prefix of the key.
  it('declines a rule that keys on a received exchange slot it was not given', () => {
    const vhf = ruled({ byFields: ['GRID'], bySentFields: ['GRID'] })
    expect(contestDupe(vhf, 'W1AW', '20m', 'PH')).toBe('none')
  })

  it('declines a rule that keys on a SENT slot too — being the mobile, the other direction', () => {
    const qsoParty = ruled({ byFields: ['QTH'], bySentFields: ['QTH'] })
    expect(contestDupe(qsoParty, 'W1AW', '20m', 'PH')).toBe('none')
  })

  // CONTROL for both of the above: the SAME contact under a rule that names no slot is
  // still a dupe. Without this, "declines" could be the function simply never firing.
  it('still fires for a rule that names no exchange slot at all', () => {
    expect(contestDupe(ruled({}), 'W1AW', '20m', 'PH')).toBe('own')
  })

  // ⭐ THE OTHER DIRECTION, and it was wrong too. Sweepstakes works a station ONCE, period:
  // band and mode class are not in its key, so the same call on any band is a dupe. The
  // triple said 'none' and the log would then refuse the contact after the over.
  it('fires across bands and modes when the rule keys on the call alone (Sweepstakes)', () => {
    const ss = ruled({ byBand: false, byModeClass: false })
    expect(contestDupe(ss, 'W1AW', '40m', 'CW')).toBe('own')
    expect(contestDupe(ss, 'W1AW', '20m', 'PH')).toBe('own')
  })

  it('…and still clears for a call Sweepstakes has never worked', () => {
    expect(contestDupe(ruled({ byBand: false, byModeClass: false }), 'K9XYZ', '20m', 'PH')).toBe('none')
  })

  // CQ WW and WPX: one band, either mode class.
  it('ignores the mode class when the rule does (CQ WW), but still honours the band', () => {
    const cqww = ruled({ byModeClass: false })
    expect(contestDupe(cqww, 'W1AW', '20m', 'CW')).toBe('own')
    expect(contestDupe(cqww, 'W1AW', '40m', 'CW')).toBe('none')
  })

  // A station older than the field sends no rule. What that build meant is the triple every
  // surface hardcoded, so absence must behave exactly as before rather than declining.
  it('falls back to the legacy triple when the station sends no rule', () => {
    expect(contestDupe(fd(), 'W1AW', '20m', 'PH')).toBe('own')
    expect(contestDupe(fd(), 'W1AW', '40m', 'PH')).toBe('none')
    expect(contestDupe(fd(), 'W1AW', '20m', 'CW')).toBe('none')
  })
})
