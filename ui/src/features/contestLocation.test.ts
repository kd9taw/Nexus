// THE W/VE LOCATION WARNING, worded. The engine says WHAT is wrong (what the operator typed as
// their contest state, and the listed codes it most likely means); these are the words the strip,
// Settings and the contest-start notice show for it.
import { describe, it, expect } from 'vitest'
import { contestStartWarning, locationWarningText } from './contestLocation'
import type { AppSnapshot, ContestLocationWarning } from '../types'

const BLANK =
  'Your call is in the US or Canada, but no contest state or province is set, so Nexus will send the DX exchange with no QTH. Set yours in Settings › Contesting, or ignore this if you are operating from outside the US and Canada.'

describe('the W/VE location warning', () => {
  it('names a blank state, an unlisted value and its hint, and offers both of two hints', () => {
    expect(locationWarningText({ typed: '', hints: [] })).toBe(BLANK)
    expect(locationWarningText({ typed: 'EMA', hints: ['MA'] })).toBe(
      'EMA is not a state or province this contest lists, so Nexus will send the DX exchange with no QTH. Set yours in Settings › Contesting, or ignore this if you are operating from outside the US and Canada. Did you mean MA?',
    )
    // Ambiguous: both are offered, neither is chosen.
    expect(locationWarningText({ typed: 'NL', hints: ['NF', 'LB'] })).toMatch(/ Did you mean NF \/ LB\?$/)
    // No hint, no question.
    expect(locationWarningText({ typed: 'ZZ', hints: [] })).not.toMatch(/Did you mean/)
  })

  it('is the notice when a contest starts with it set, and only then', () => {
    const snap = (w?: ContestLocationWarning) =>
      ({ fieldDay: w ? { locationWarning: w } : {} }) as unknown as AppSnapshot
    expect(contestStartWarning('fieldday-sp', snap({ typed: '', hints: [] }))).toBe(BLANK)
    expect(contestStartWarning('fieldday-run', snap({ typed: '', hints: [] }))).toBe(BLANK)
    // CONTROLS: another mode, a contest without the warning, no snapshot.
    expect(contestStartWarning('chat', snap({ typed: '', hints: [] }))).toBeNull()
    expect(contestStartWarning('fieldday-sp', snap())).toBeNull()
    expect(contestStartWarning('fieldday-sp', null)).toBeNull()
  })
})
