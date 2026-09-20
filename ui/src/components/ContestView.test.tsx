import { describe, it, expect } from 'vitest'
import { annotate, buildSummaryText } from './ContestView'
import type { FieldDayQso } from '../types'

function qso(call: string, band: string, mode: string, dupe?: boolean): FieldDayQso {
  return { call, class: '1A', section: 'IL', band, mode, ...(dupe === undefined ? {} : { dupe }) }
}

// ⭐ THE ENGINE DECIDES WHICH ROW IS A DUPE, and the table reports its answer.
//
// `annotate` used to re-derive the verdict from a fourth copy of the dupe key
// (`call|band|mode`, counted, >1 = dupe). Three things were wrong with that, and the first
// two are the ones an operator sees:
//
//   * COUNTING marks the ORIGINAL as well as the duplicate — a real, scoring contact was
//     styled as a dupe purely because the call came back later;
//   * the triple is the key for two of the seventeen shipped rulesets. Sweepstakes works a
//     station once on ANY band, so its cross-band dupe went unmarked; a QSO party counts a
//     new county as a new contact, so two legal contacts were both marked;
//   * it could not see a row the ENGINE logged as a dupe, which is the only thing that
//     actually determines whether the row scores.
//
// Now that the cross-checked contests LOG a dupe rather than refusing it, the log contains
// zero-scoring rows and guessing at them is no longer merely imprecise.
describe('annotate() reports the engine\'s dupe flag, and never guesses one', () => {
  it('marks the row the engine flagged, and only that row', () => {
    const rows = annotate([qso('W1AW', '20m', 'CW'), qso('W1AW', '20m', 'CW', true)])
    expect(rows.map((r) => r.isDupe)).toEqual([false, true])
  })

  it('marks nothing when the engine flagged nothing, however the calls repeat', () => {
    // Under Sweepstakes' own rule these two would BOTH be one contact and the second a
    // dupe — but that is the engine's call, and here it has said neither is.
    const rows = annotate([qso('W1AW', '20m', 'CW'), qso('W1AW', '40m', 'CW')])
    expect(rows.map((r) => r.isDupe)).toEqual([false, false])
  })

  it('marks a cross-band dupe the old triple could never see (Sweepstakes)', () => {
    const rows = annotate([qso('W1AW', '20m', 'CW'), qso('W1AW', '40m', 'PH', true)])
    expect(rows.map((r) => r.isDupe)).toEqual([false, true])
  })

  it('treats an absent flag as false, so a build older than the field marks nothing', () => {
    const rows = annotate([qso('W1AW', '20m', 'CW'), qso('W1AW', '20m', 'CW')])
    expect(rows.map((r) => r.isDupe)).toEqual([false, false])
  })
})

describe('the Score Summary rules line (design 3f)', () => {
  const args = (rulesYear: number, rulesGenerated: string) => ({
    eventName: 'ARRL Field Day',
    isWfd: false,
    rulesYear,
    rulesGenerated,
    myClass: '1A',
    mySection: 'IL',
    log: [],
    modes: { dig: 0, cw: 0, ph: 0 },
    workedSet: new Set<string>(),
    powerMult: 2,
    qsoPts: 0,
    poweredPoints: 0,
    bonusPoints: 0,
    totalScore: 0,
    claimedBonuses: [],
  })

  it('names the rules vintage scoring the document', () => {
    const text = buildSummaryText(args(2026, '2026-08-29T00:00:00Z'))
    expect(text).toContain('Scored under 2026 rules (data 2026-08-29)')
  })

  it('skips the line on an older backend with no rules stamp', () => {
    const text = buildSummaryText(args(0, ''))
    expect(text).not.toContain('Scored under')
  })
})
