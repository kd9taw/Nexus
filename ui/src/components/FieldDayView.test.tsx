import { describe, it, expect } from 'vitest'
import { annotate, buildSummaryText } from './FieldDayView'
import type { FieldDayQso } from '../types'

function qso(call: string, band: string, mode: string): FieldDayQso {
  return { call, class: '1A', section: 'IL', band, mode }
}

describe('annotate() FD dupe detection', () => {
  it('does NOT flag the same call worked on two different bands as a dupe', () => {
    // FD permits working the same station once per band per mode.
    const rows = annotate([qso('W1AW', '20m', 'CW'), qso('W1AW', '40m', 'CW')])
    expect(rows.map((r) => r.isDupe)).toEqual([false, false])
  })

  it('does NOT flag the same call worked in two different modes on one band', () => {
    const rows = annotate([qso('W1AW', '20m', 'CW'), qso('W1AW', '20m', 'DIG')])
    expect(rows.map((r) => r.isDupe)).toEqual([false, false])
  })

  it('flags an exact (call, band, mode) repeat as a dupe', () => {
    const rows = annotate([qso('W1AW', '20m', 'CW'), qso('W1AW', '20m', 'CW')])
    expect(rows.map((r) => r.isDupe)).toEqual([true, true])
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
