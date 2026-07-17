import { describe, expect, it } from 'vitest'
import { dueNetReminders, nextNetStart, reminderKey, untilPhrase } from './nets'
import { type Memory, type NetInfo } from './memories'

const net = (over: Partial<NetInfo> = {}): NetInfo => ({
  days: [1], // Monday
  utcTime: '14:00',
  alertEnabled: true,
  alertLeadMin: 10,
  ...over,
})

const netMem = (over: Partial<Memory> = {}): Memory => ({
  id: 'n1',
  name: 'Test Net',
  kind: 'hfnet',
  rxMhz: 14.3,
  mode: 'USB',
  groups: [],
  favorite: false,
  source: 'curated',
  net: net(),
  ...over,
})

// Monday 2026-01-05 13:55 UTC (net starts 14:00 that day).
const MON_1355 = Date.UTC(2026, 0, 5, 13, 55)

describe('nextNetStart', () => {
  it('finds the same-day occurrence when the time is still ahead', () => {
    const start = nextNetStart(net(), MON_1355)
    expect(start).toBe(Date.UTC(2026, 0, 5, 14, 0))
  })

  it('rolls to next week when today’s time has passed', () => {
    const afterMon = Date.UTC(2026, 0, 5, 14, 1)
    expect(nextNetStart(net(), afterMon)).toBe(Date.UTC(2026, 0, 12, 14, 0))
  })

  it('picks the nearest of multiple days', () => {
    // Mon + Wed net; from Monday afternoon → Wednesday.
    const start = nextNetStart(net({ days: [1, 3] }), Date.UTC(2026, 0, 5, 14, 1))
    expect(start).toBe(Date.UTC(2026, 0, 7, 14, 0))
  })

  it('returns null for an empty or malformed schedule', () => {
    expect(nextNetStart(net({ days: [] }), MON_1355)).toBeNull()
    expect(nextNetStart(net({ utcTime: 'nope' }), MON_1355)).toBeNull()
    expect(nextNetStart(net({ utcTime: '25:00' }), MON_1355)).toBeNull()
  })
})

describe('dueNetReminders', () => {
  it('fires within the lead window and not before', () => {
    // 5 min out, 10-min lead → due.
    expect(dueNetReminders([netMem()], MON_1355)).toHaveLength(1)
    // 20 min out, 10-min lead → not yet.
    expect(dueNetReminders([netMem()], Date.UTC(2026, 0, 5, 13, 40))).toHaveLength(0)
  })

  it('ignores nets that are not opted in', () => {
    expect(dueNetReminders([netMem({ net: net({ alertEnabled: false }) })], MON_1355)).toHaveLength(0)
  })

  it('reminderKey is stable per occurrence', () => {
    const [r] = dueNetReminders([netMem()], MON_1355)
    expect(reminderKey(r)).toBe(`n1:${Date.UTC(2026, 0, 5, 14, 0)}`)
  })
})

describe('untilPhrase', () => {
  it('phrases the lead time', () => {
    expect(untilPhrase(MON_1355 + 5 * 60_000, MON_1355)).toBe('in 5 min')
    expect(untilPhrase(MON_1355 + 60_000, MON_1355)).toBe('in 1 min')
    expect(untilPhrase(MON_1355, MON_1355)).toBe('now')
  })
})
