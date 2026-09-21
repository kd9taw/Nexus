// @vitest-environment jsdom
//
// THE CONTEST LOG STRIP'S OWN-DUPE SENTENCE MUST NAME ONLY WHAT ITS RULESET KEYS ON.
//
// `contestDupe` applies exactly the components a ruleset's rule names — that is correct, and
// it is what makes this reachable. The sentence did not follow: it interpolated the CURRENT
// band and the CURRENT mode class unconditionally, so under any rule that drops one of them
// the strip told the operator to go and look somewhere the contact is not, and the warning
// that was RIGHT got read as broken.
//
// ⚠️ IT IS NOT ONLY SWEEPSTAKES. Of the nine shipped rulesets whose verdict reaches this
// sentence at all (the other eight key on an exchange slot, where `contestDupe` returns
// 'none'), SEVEN name fewer than three components:
//
//   arrlfd, wfd                                        by_band ✓  by_mode_class ✓   — correct today
//   cqww_cw, cqww_ssb, cqww_rtty, cqwpx_cw, cqwpx_ssb  by_band ✓  by_mode_class ✗   — named a mode
//   arrlss_cw, arrlss_ssb                              by_band ✗  by_mode_class ✗   — named both
//
// `by_band` and `by_mode_class` are independent booleans, so there are FOUR sentences and all
// four are tested here. The fourth (mode but not band) ships in no ruleset today, but the type
// and the rules installer both permit it, and a sentence that is wrong only for a ruleset
// nobody has installed yet is still a sentence that is wrong.
//
// ⚠️ EVERY ARM SETS BOTH FLAGS EXPLICITLY. An arm that merely omitted one would be read as
// LEGACY_TRIPLE (see below) and would silently re-test the (band ✓ mode ✓) branch, so the pair
// would prove nothing. The one arm that omits the rule does so ON PURPOSE and says why.
//
// THE CLUB SENTENCE IS DELIBERATELY UNTOUCHED, and the control at the bottom holds it that
// way: `contestDupe` matches a club key RAW — `c === typed && b === band && m === modeClass`,
// with no `rule.byBand`/`rule.byModeClass` guard anywhere near it — so a club dupe really is
// on the band AND in the mode class being looked at, whatever the ruleset says. It is the one
// verdict here whose three components are always true.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import type { AppSnapshot, DupeRule, FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  fdLogManual: vi.fn(() => Promise.resolve({})),
  logQso: vi.fn(() => Promise.resolve({})),
  getLog: vi.fn(() => Promise.resolve([])),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  resolveEntity: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
}))

/** The rig, for every arm: 20 m, phone. Whatever band and mode class a wording names, it can
 *  only have come from here — the contest log rows below are deliberately somewhere else. */
const snap = {
  radio: { band: '20m', dialMhz: 14.2 },
  hunt: null,
} as unknown as AppSnapshot

function rule(byBand: boolean, byModeClass: boolean): DupeRule {
  return {
    byCall: true,
    byBand,
    byModeClass,
    byFields: [],
    bySentFields: [],
    modeClassGroups: [],
    logDupes: false,
  }
}

/** A running contest with one contact in this position's log, under `dupeRule`.
 *  `dupeRule` omitted = a station too old to send one, which must read as LEGACY_TRIPLE. */
function contest(logBand: string, logMode: string, dupeRule?: DupeRule): FieldDayStatus {
  return {
    composing: [
      { key: 'CLASS', raw: '3A' },
      { key: 'SECTION', raw: 'WI', domain: 'fd_sections' },
    ],
    running: true,
    state: '',
    qsoCount: 1,
    sections: 1,
    points: 1,
    log: [{ call: 'W1AW', class: '1D', section: 'CT', band: logBand, mode: logMode, submode: '' }],
    ...(dupeRule ? { dupeRule } : {}),
  } as unknown as FieldDayStatus
}

/** Type W1AW into a strip running `fieldDay`, and return the own-dupe sentence.
 *
 *  It is found by the alert that names the call rather than by its text, so a wording change
 *  cannot make this silently find nothing — and the count is asserted, because the strip's
 *  exchange verdict is an alert too and reading the wrong one would make every `not.toContain`
 *  below pass vacuously. */
function dupeHint(fieldDay: FieldDayStatus): string {
  render(
    <LogEntry
      snap={snap}
      mode="PH"
      defaultRst="59"
      exchange="terrestrial"
      fieldDay={fieldDay}
      fdMode="PH"
    />,
  )
  fireEvent.change(screen.getByPlaceholderText('W1AW'), { target: { value: 'w1aw' } })
  const hits = screen
    .queryAllByRole('alert')
    .map((el) => el.textContent ?? '')
    .filter((s) => s.includes('W1AW'))
  expect(hits, 'expected exactly one own-dupe alert naming W1AW').toHaveLength(1)
  return hits[0]
}

afterEach(() => cleanup())

describe('the log strip names only the dupe components its ruleset keys on', () => {
  it('SWEEPSTAKES (by_band ✗, by_mode_class ✗) — the sentence names neither', () => {
    // Rule 2.2: a station is worked ONCE, regardless of band or mode. The contact is on 40 m
    // CW and the rig is on 20 m PH, so the hint is correct and is UNCHECKABLE on the band and
    // mode the operator is looking at. Saying "on 20m PH" sent him to the one place it is not.
    const hint = dupeHint(contest('40m', 'CW', rule(false, false)))
    expect(hint, 'named the band its own ruleset ignores').not.toContain('20m')
    expect(hint, 'named the mode its own ruleset ignores').not.toContain('PH')
    expect(hint).toContain('regardless of band or mode')
  })

  it('CQ WW / WPX (by_band ✓, by_mode_class ✗) — the band, and not the mode', () => {
    // Five shipped rulesets. One contact per band either mode, so the 20 m CW contact rightly
    // raises the hint while the operator is in PH — and the hint then said "PH", which is the
    // same defect a band wider. The band is real and must stay.
    const hint = dupeHint(contest('20m', 'CW', rule(true, false)))
    expect(hint, 'dropped the band its ruleset does key on').toContain('20m')
    expect(hint, 'named the mode its own ruleset ignores').not.toContain('PH')
    expect(hint).toContain('once per band, any mode')
  })

  it('(by_band ✗, by_mode_class ✓) — the mode, and not the band', () => {
    // No shipped ruleset yet; `DupeRule` and the installer both allow it. The contact is on
    // 40 m PH: the mode class is what matched and the band is what did not.
    const hint = dupeHint(contest('40m', 'PH', rule(false, true)))
    expect(hint, 'named the band its own ruleset ignores').not.toContain('20m')
    expect(hint, 'dropped the mode its ruleset does key on').toContain('PH')
    expect(hint).toContain('once per mode, any band')
  })

  it('CONTROL — FIELD DAY (by_band ✓, by_mode_class ✓) still names both', () => {
    // arrlfd and wfd. Without this arm every assertion above passes just as well on a strip
    // that stopped naming a band anywhere, which is the regression that would hurt most: Field
    // Day is the flagship and "which band" is the useful half of the sentence there.
    const hint = dupeHint(contest('20m', 'PH', rule(true, true)))
    expect(hint, 'a per-band, per-mode ruleset stopped naming the band').toContain('20m')
    expect(hint, 'a per-band, per-mode ruleset stopped naming the mode').toContain('PH')
    expect(hint).not.toContain('regardless of')
    expect(hint).not.toContain('any mode')
    expect(hint).not.toContain('any band')
  })

  it('CONTROL — NO dupeRule at all reads as the legacy triple, not as band-free', () => {
    // A station too old to send a rule meant `(call, band, mode class)`, which is what
    // `contestDupe` falls back to (LEGACY_TRIPLE) and what this strip has always shown. This
    // is the arm that fails on `=== true` written where `!== false` belongs: an absent rule
    // must read as BOTH named, or every pre-rule station loses the band overnight.
    const hint = dupeHint(contest('20m', 'PH'))
    expect(hint, 'an absent rule stopped naming the band').toContain('20m')
    expect(hint, 'an absent rule stopped naming the mode').toContain('PH')
    expect(hint).not.toContain('regardless of')
  })

  it('CONTROL — the CLUB sentence keeps its band and mode under a band-free ruleset', () => {
    // `contestDupe` matches a club key RAW, ungated by the rule, so this one really is on the
    // band and in the mode class being looked at even under Sweepstakes. It must NOT follow
    // the own-dupe sentence band-free — that would be the inverse of this defect.
    const fd = {
      ...contest('40m', 'CW', rule(false, false)),
      log: [],
      club: {
        syncState: 'synced',
        queued: 0,
        offlineSinceUnix: 0,
        hosting: false,
        event: 'FD',
        hostCall: 'W9ABC',
        score: 0,
        qsos: 1,
        sections: 1,
        skewSecs: 0,
        dupes: [['K1ABC', '20m', 'PH']],
        board: [],
      },
    } as unknown as FieldDayStatus
    render(
      <LogEntry snap={snap} mode="PH" defaultRst="59" exchange="terrestrial" fieldDay={fd} fdMode="PH" />,
    )
    fireEvent.change(screen.getByPlaceholderText('W1AW'), { target: { value: 'k1abc' } })
    expect(
      screen.getByText(
        'Club dupe: another position already worked K1ABC on 20m PH — logging is allowed but adds no points',
      ),
    ).toBeTruthy()
  })
})
