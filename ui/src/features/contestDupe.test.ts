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
