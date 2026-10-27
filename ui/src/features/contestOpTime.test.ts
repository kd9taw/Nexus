import { describe, it, expect } from 'vitest'
import { operatingTime, formatOpTime, offTimeRule, OFF_TIME_MIN_SEC } from './contestOpTime'

const at = (...mins: number[]) => mins.map((m) => ({ whenUnix: 1_800_000_000 + m * 60 }))

describe('operatingTime', () => {
  it('counts a break of 30 minutes or more as off time, the rest from first to last as on time', () => {
    // 0 → 10 → 40 (a 30-minute gap: off) → 69 (29 minutes: on) → 169 (100 minutes: off)
    expect(operatingTime(at(0, 10, 40, 69, 169))).toEqual({ onSec: (10 + 29) * 60, offSec: (30 + 100) * 60, breaks: 2 })
  })

  it('reads the contacts in time order, whatever order the log holds them in', () => {
    expect(operatingTime(at(40, 0, 10))).toEqual({ onSec: 10 * 60, offSec: 30 * 60, breaks: 1 })
  })

  it('takes a contest definition’s own minimum break', () => {
    expect(operatingTime(at(0, 40, 99), 60 * 60)).toEqual({ onSec: 99 * 60, offSec: 0, breaks: 0 })
    expect(operatingTime(at(0, 40, 100), 60 * 60)).toEqual({ onSec: 40 * 60, offSec: 60 * 60, breaks: 1 })
    expect(OFF_TIME_MIN_SEC).toBe(30 * 60)
  })

  it('is nothing before the first contact, and no time at all with one', () => {
    expect(operatingTime([])).toBeNull()
    expect(operatingTime([{ whenUnix: 0 }, {}])).toBeNull()
    expect(operatingTime(at(5))).toEqual({ onSec: 0, offSec: 0, breaks: 0 })
  })

  it('leaves out a row with no time', () => {
    expect(operatingTime([...at(0, 10), { whenUnix: 0 }])).toEqual({ onSec: 600, offSec: 0, breaks: 0 })
  })
})

describe('formatOpTime', () => {
  it('writes hours and minutes', () => {
    expect(formatOpTime(0)).toBe('0:00')
    expect(formatOpTime(59)).toBe('0:00')
    expect(formatOpTime(65 * 60)).toBe('1:05')
    expect(formatOpTime(36 * 3600 + 7 * 60)).toBe('36:07')
  })
})

describe('offTimeRule — the contest’s own minimum break', () => {
  it('is 60 minutes in CQ WW and CQ WPX, 30 in Sweepstakes, and 30 not from the rules elsewhere', () => {
    for (const e of ['cqww_cw', 'cqww_ssb', 'cqww_rtty', 'cqwpx_cw', 'cqwpx_ssb']) {
      expect(offTimeRule(e), e).toEqual({ minOffSec: 3600, fromRules: true })
    }
    for (const e of ['arrlss_cw', 'arrlss_ssb']) expect(offTimeRule(e), e).toEqual({ minOffSec: 1800, fromRules: true })
    for (const e of ['ilqp', 'arrlfd', 'wfd', 'arrlvhf_jun', '', undefined]) {
      expect(offTimeRule(e), String(e)).toEqual({ minOffSec: OFF_TIME_MIN_SEC, fromRules: false })
    }
  })

  it('in CQ WW, a 45-minute gap is on time and a 61-minute gap is a break', () => {
    const { minOffSec } = offTimeRule('cqww_cw')
    expect(operatingTime(at(0, 45), minOffSec)).toEqual({ onSec: 45 * 60, offSec: 0, breaks: 0 })
    expect(operatingTime(at(0, 61), minOffSec)).toEqual({ onSec: 0, offSec: 61 * 60, breaks: 1 })
    // CONTROL: the same 45-minute gap is a break under the 30-minute default.
    expect(operatingTime(at(0, 45), offTimeRule('ilqp').minOffSec)).toEqual({ onSec: 0, offSec: 45 * 60, breaks: 1 })
  })
})
