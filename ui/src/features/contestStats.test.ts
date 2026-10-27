import { describe, it, expect } from 'vitest'
import { contestStats, distanceStats, entityStats, type StatsPlace } from './contestStats'
import type { FieldDayQso } from '../types'

/** 2026-11-28 00:00 UTC: a whole hour and a whole day. */
const T0 = 1_795_824_000
const q = (call: string, min: number, extra: Partial<FieldDayQso> = {}): FieldDayQso => ({
  call,
  class: '',
  section: '',
  band: '20m',
  mode: 'CW',
  whenUnix: T0 + min * 60,
  ...extra,
})

describe('contestStats — operating time and rates', () => {
  it('counts a break under the minimum as on time and one at or over it as a break', () => {
    // 0 → 10 → 39 (29 min: on) → 99 (60 min: a break at a 60-minute minimum) → 110
    const log = [q('K1A', 0), q('K1B', 10), q('K1C', 39), q('K1D', 99), q('K1E', 110)]
    const s60 = contestStats(log, { nowUnix: T0 + 120 * 60, minOffSec: 3600 })
    expect(s60.op).toEqual({ onSec: (10 + 29 + 11) * 60, offSec: 60 * 60, breaks: 1 })
    // The same log under a 30-minute minimum: the 29-minute gap is still on time.
    const s30 = contestStats(log, { nowUnix: T0 + 120 * 60, minOffSec: 1800 })
    expect(s30.op).toEqual({ onSec: (10 + 29 + 11) * 60, offSec: 60 * 60, breaks: 1 })
    // …and a 61-minute minimum takes the hour back as on time.
    const s61 = contestStats(log, { nowUnix: T0 + 120 * 60, minOffSec: 61 * 60 })
    expect(s61.op).toEqual({ onSec: 110 * 60, offSec: 0, breaks: 0 })
    // Overall rate: 5 contacts over 50 minutes on = 6/h.
    expect(s60.overallPerHour).toBeCloseTo(6, 6)
  })

  it('finds the best 60 minutes, by contacts inside any hour-long stretch', () => {
    // Three at 0–2, then six between 70 and 129 (inside one hour), then one at 200.
    const log = [
      q('A1', 0), q('A2', 1), q('A3', 2),
      q('B1', 70), q('B2', 80), q('B3', 90), q('B4', 100), q('B5', 110), q('B6', 129),
      q('C1', 200),
    ]
    const s = contestStats(log, { nowUnix: T0 + 300 * 60 })
    expect(s.best60).toEqual({ count: 6, startUnix: T0 + 70 * 60 })
    // Exactly 60 minutes apart is two different hours, not one.
    expect(contestStats([q('X', 0), q('Y', 60)], { nowUnix: T0 + 3600 * 2 }).best60).toEqual({
      count: 1,
      startUnix: T0,
    })
  })

  it('leaves dupes and banned-mode rows out of the counts', () => {
    const log = [q('K1A', 0), q('K1A', 1, { dupe: true }), q('K1B', 2, { banned: true }), q('K1C', 3)]
    const s = contestStats(log, { nowUnix: T0 + 600 })
    expect(s.contacts).toBe(2)
    expect(s.bands).toEqual([{ key: '20m', count: 2 }])
  })

  it('counts today in UTC and says when the last contact was', () => {
    const log = [q('K1A', -30), q('K1B', 5), q('K1C', 20)]
    const s = contestStats(log, { nowUnix: T0 + 50 * 60 })
    expect(s.today).toBe(2)
    expect(s.lastContactUnix).toBe(T0 + 20 * 60)
  })
})

describe('contestStats — hourly buckets', () => {
  it('buckets by UTC clock hour over the contest window, empty hours included', () => {
    const log = [q('A', 5), q('B', 59), q('C', 60), q('D', 185)]
    const s = contestStats(log, { nowUnix: T0 + 4 * 3600, windowStart: T0, windowEnd: T0 + 4 * 3600 })
    expect(s.hours).toEqual([
      { startUnix: T0, count: 2 },
      { startUnix: T0 + 3600, count: 1 },
      { startUnix: T0 + 7200, count: 0 },
      { startUnix: T0 + 10800, count: 1 },
    ])
    expect(s.outsideWindow).toBe(0)
  })

  it('counts a contact outside the window apart rather than drawing it', () => {
    const log = [q('TEST', -90), q('A', 5)]
    const s = contestStats(log, { nowUnix: T0 + 3600, windowStart: T0, windowEnd: T0 + 3600 })
    expect(s.hours).toEqual([{ startUnix: T0, count: 1 }])
    expect(s.outsideWindow).toBe(1)
  })

  it('without a window, runs from the first contact’s hour to the last’s', () => {
    const s = contestStats([q('A', 30), q('B', 150)], { nowUnix: T0 + 4 * 3600 })
    expect(s.hours.map((h) => h.count)).toEqual([1, 0, 1])
    expect(s.hours[0].startUnix).toBe(T0)
  })
})

describe('contestStats — bands, modes and grids', () => {
  it('counts bands and modes, the busiest first, and names the actual mode where it is known', () => {
    const log = [
      q('A', 0, { band: '40m', mode: 'PH', submode: 'LSB' }),
      q('B', 1, { band: '40m', mode: 'CW' }),
      q('C', 2, { band: '20m', mode: 'DIG', submode: 'FT8' }),
      q('D', 3, { band: '40m', mode: 'CW' }),
    ]
    const s = contestStats(log, { nowUnix: T0 + 600 })
    expect(s.bands).toEqual([{ key: '40m', count: 3 }, { key: '20m', count: 1 }])
    expect(s.modes).toEqual([{ key: 'CW', count: 2 }, { key: 'FT8', count: 1 }, { key: 'LSB', count: 1 }])
  })

  it('counts the distinct grid squares received, or says the contest receives none', () => {
    const log = [q('A', 0, { rcvd: ['FN31pr'] }), q('B', 1, { rcvd: ['fn31'] }), q('C', 2, { rcvd: ['EM12'] }), q('D', 3, { rcvd: ['xx'] })]
    expect(contestStats(log, { nowUnix: T0 + 600, gridSlot: 0 }).grids).toEqual(['EM12', 'FN31'])
    expect(contestStats(log, { nowUnix: T0 + 600 }).grids).toBeNull()
  })
})

describe('distanceStats', () => {
  const places = new Map<string, StatsPlace | null>([
    ['DL1ABC', { entity: 'Fed. Rep. of Germany', lat: 51, lon: 10 }],
    ['W1AW', { entity: 'United States', lat: 37.53, lon: -91.67 }],
    ['ZZ9ZZ', null],
  ])
  // EN52 (northern Illinois).
  const home = { lat: 42.5, lon: -89 }

  it('measures to the entity centre, finds the longest and averages the rest', () => {
    const log = [q('DL1ABC', 0), q('W1AW', 1), q('ZZ9ZZ', 2)]
    const d = distanceStats(log, places, home, null)
    expect(d.measured).toBe(2)
    expect(d.longest?.call).toBe('DL1ABC')
    expect(d.longest?.entity).toBe('Fed. Rep. of Germany')
    // Great-circle EN52-ish → (51, 10) is about 7,040 km; → (37.53, -91.67) about 600 km.
    expect(d.longest!.km).toBeGreaterThan(6900)
    expect(d.longest!.km).toBeLessThan(7200)
    expect(d.averageKm!).toBeCloseTo((d.longest!.km + d.shortestKm!) / 2, 6)
  })

  it('measures to the grid the contact sent when the exchange carries one', () => {
    // W1AW sends FN31: Connecticut, ~1,450 km from EN52 — not the entity centre in Missouri.
    const d = distanceStats([q('W1AW', 0, { rcvd: ['FN31'] })], places, home, 0)
    expect(d.longest!.km).toBeGreaterThan(1300)
    expect(d.longest!.km).toBeLessThan(1600)
  })

  it('measures nothing without the station’s own position', () => {
    const d = distanceStats([q('DL1ABC', 0)], places, null, null)
    expect(d).toEqual({ measured: 0, longest: null, averageKm: null, shortestKm: null })
  })
})

describe('entityStats', () => {
  it('counts contacts per entity, the busiest first, and the DX among them', () => {
    const places = new Map<string, StatsPlace | null>([
      ['DL1ABC', { entity: 'Fed. Rep. of Germany', lat: 51, lon: 10 }],
      ['DL2XYZ', { entity: 'Fed. Rep. of Germany', lat: 51, lon: 10 }],
      ['W1AW', { entity: 'United States', lat: 37.53, lon: -91.67 }],
      ['JA1ABC', { entity: 'Japan', lat: 36, lon: 138 }],
    ])
    const log = [q('DL1ABC', 0), q('DL2XYZ', 1), q('W1AW', 2), q('JA1ABC', 3), q('W1AW', 4, { dupe: true }), q('NOPE', 5)]
    const e = entityStats(log, places, 'United States')
    expect(e.entities).toEqual([
      { entity: 'Fed. Rep. of Germany', count: 2 },
      { entity: 'Japan', count: 1 },
      { entity: 'United States', count: 1 },
    ])
    expect(e.dx).toBe(3)
    expect(e.unplaced).toBe(1)
    // Without the station's own country, DX is not known.
    expect(entityStats(log, places, null).dx).toBeNull()
  })
})
