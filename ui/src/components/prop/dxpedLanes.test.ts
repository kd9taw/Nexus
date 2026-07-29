// Calendar geometry — the failures here are the silent kind. A bar one column
// off, a lane that shifts under a run, or a capped week that swallows an
// operation all render as a perfectly plausible calendar and quietly mislead the
// operator about when an entity is on the air.
import { describe, it, expect } from 'vitest'
import type { CalendarEntry } from '../../types'
import {
  buildWeeks,
  layoutWeek,
  layoutCalendar,
  dxpedColorIndex,
  compactBands,
  DXPED_COLOR_COUNT,
} from './dxpedLanes'

const DAY = 86_400
/** Monday 2026-07-27 00:00 UTC — week rows here run Mon–Sun. */
const MON = Date.UTC(2026, 6, 27) / 1000
const WEEK = [0, 1, 2, 3, 4, 5, 6].map((i) => MON + i * DAY)

function entry(over: Partial<CalendarEntry> & { call: string }): CalendarEntry {
  return {
    entity: 'Test Land',
    region: 'OC',
    startUnix: MON,
    endUnix: MON + DAY,
    bands: ['20m'],
    modes: ['FT8'],
    octant: 'SW',
    bearingDeg: 240,
    distanceKm: 12000,
    outlook: [],
    best: '',
    ...over,
  }
}

describe('layoutWeek — span geometry', () => {
  it('spans one bar across the days of the run instead of one chip per day', () => {
    // Wed 00:00 → Sat 00:00 is Wed/Thu/Fri: three columns, ONE segment.
    const e = entry({ call: 'VK9CM', startUnix: MON + 2 * DAY, endUnix: MON + 5 * DAY })
    const { segments } = layoutWeek([e], WEEK, 4)
    expect(segments).toHaveLength(1)
    expect(segments[0].startCol).toBe(2)
    expect(segments[0].span).toBe(3)
  })

  it('does not paint the day a run ends exactly at midnight on', () => {
    // The boundary that a naive `endUnix -> column` conversion gets wrong: an
    // operation ending 00:00 Thursday is off the air all of Thursday.
    const e = entry({ call: 'T33T', startUnix: MON, endUnix: MON + 3 * DAY })
    const [seg] = layoutWeek([e], WEEK, 4).segments
    expect(seg.startCol).toBe(0)
    expect(seg.span).toBe(3) // Mon/Tue/Wed, NOT Thu
  })

  it('clips a run to the week and flags both continuation edges', () => {
    const e = entry({ call: 'LONG', startUnix: MON - 3 * DAY, endUnix: MON + 30 * DAY })
    const [seg] = layoutWeek([e], WEEK, 4).segments
    expect(seg.startCol).toBe(0)
    expect(seg.span).toBe(7)
    expect(seg.contPrev).toBe(true)
    expect(seg.contNext).toBe(true)
  })

  it('marks a self-contained run as neither continuation', () => {
    const e = entry({ call: 'SHORT', startUnix: MON + DAY, endUnix: MON + 3 * DAY })
    const [seg] = layoutWeek([e], WEEK, 4).segments
    expect(seg.contPrev).toBe(false)
    expect(seg.contNext).toBe(false)
  })
})

describe('layoutWeek — lane packing', () => {
  it('stacks overlapping operations into separate lanes', () => {
    const a = entry({ call: 'AAA', startUnix: MON, endUnix: MON + 4 * DAY })
    const b = entry({ call: 'BBB', startUnix: MON + DAY, endUnix: MON + 3 * DAY })
    const { segments } = layoutWeek([a, b], WEEK, 4)
    const lanes = new Map(segments.map((s) => [s.call, s.lane]))
    expect(lanes.get('AAA')).toBe(0)
    expect(lanes.get('BBB')).toBe(1)
  })

  it('shares one lane between operations whose days do not overlap', () => {
    // Two short operations side by side on one row is correct packing, not a
    // collision — burning a whole lane on each would waste the grid.
    const a = entry({ call: 'AAA', startUnix: MON, endUnix: MON + 2 * DAY })
    const b = entry({ call: 'BBB', startUnix: MON + 4 * DAY, endUnix: MON + 6 * DAY })
    const { segments, laneRows } = layoutWeek([a, b], WEEK, 4)
    expect(segments.every((s) => s.lane === 0)).toBe(true)
    expect(laneRows).toBe(1)
  })

  it('keeps a multi-week operation in the SAME lane every week it runs', () => {
    // The whole point of a span bar: the eye tracks one row. If a neighbouring
    // operation starting and ending can bump the long one between rows, the bar
    // stops reading as one event — which is the bug this view had.
    const long = entry({ call: 'LONG', startUnix: MON, endUnix: MON + 20 * DAY })
    const noise = [
      entry({ call: 'N1', startUnix: MON + 2 * DAY, endUnix: MON + 4 * DAY }),
      entry({ call: 'N2', startUnix: MON + 9 * DAY, endUnix: MON + 12 * DAY }),
      entry({ call: 'N3', startUnix: MON + 15 * DAY, endUnix: MON + 18 * DAY }),
    ]
    const weeks = buildWeeks([long, ...noise], MON, 10)
    const rows = layoutCalendar([long, ...noise], weeks, 4)
    const laneOfLong = rows
      .map((r) => r.segments.find((s) => s.call === 'LONG')?.lane)
      .filter((l) => l !== undefined)
    expect(laneOfLong.length).toBeGreaterThanOrEqual(3)
    expect(new Set(laneOfLong).size).toBe(1)
  })

  it('is order-independent — the same set laid out in any order gives the same lanes', () => {
    const es = [
      entry({ call: 'CCC', startUnix: MON + DAY, endUnix: MON + 5 * DAY }),
      entry({ call: 'AAA', startUnix: MON, endUnix: MON + 4 * DAY }),
      entry({ call: 'BBB', startUnix: MON + 2 * DAY, endUnix: MON + 6 * DAY }),
    ]
    const lanes = (list: CalendarEntry[]) =>
      layoutWeek(list, WEEK, 4)
        .segments.map((s) => `${s.call}:${s.lane}`)
        .sort()
    expect(lanes([...es].reverse())).toEqual(lanes(es))
  })
})

describe('layoutWeek — the lane cap', () => {
  /** n operations all running the whole week, so every one needs its own lane. */
  const crowd = (n: number) =>
    Array.from({ length: n }, (_, i) =>
      entry({ call: `OP${i}`, startUnix: MON, endUnix: MON + 7 * DAY }),
    )

  it('draws every lane when the week fits', () => {
    const { segments, hidden, laneRows } = layoutWeek(crowd(4), WEEK, 4)
    expect(segments).toHaveLength(4)
    expect(hidden).toHaveLength(0)
    expect(laneRows).toBe(4)
  })

  it('surrenders the last row to "+N" rather than silently dropping operations', () => {
    const { segments, hidden, overflowByCol, laneRows } = layoutWeek(crowd(7), WEEK, 4)
    expect(segments).toHaveLength(3) // lanes 0..2 drawn
    expect(hidden).toHaveLength(4)
    expect(laneRows).toBe(4) // 3 bar rows + the overflow row
    expect(overflowByCol).toEqual([4, 4, 4, 4, 4, 4, 4])
  })

  it('counts overflow per day, not per week', () => {
    // Four all-week operations overflow the cap; the two short extras land in the
    // hidden lanes on opposite ends of the week. Midweek hides only the all-week
    // straggler, so the chip must read +1 there and +2 at the ends — a per-week
    // count would claim the same number on every day and overstate the middle.
    const es = [
      ...crowd(4),
      entry({ call: 'X1', startUnix: MON, endUnix: MON + 2 * DAY }),
      entry({ call: 'X2', startUnix: MON + 5 * DAY, endUnix: MON + 7 * DAY }),
    ]
    const { overflowByCol } = layoutWeek(es, WEEK, 4)
    expect(overflowByCol).toEqual([2, 2, 1, 1, 1, 2, 2])
  })

  it('expanding a week shows everything', () => {
    const weeks = buildWeeks(crowd(7), MON, 10)
    const [row] = layoutCalendar(crowd(7), weeks, 4, new Set([MON]))
    expect(row.segments).toHaveLength(7)
    expect(row.hidden).toHaveLength(0)
  })
})

describe('buildWeeks', () => {
  it('starts the grid on the Monday of the current week', () => {
    // A Wednesday must still land in a row that begins on Monday.
    const wed = MON + 2 * DAY + 43_200
    const weeks = buildWeeks([entry({ call: 'X' })], wed, 10)
    expect(weeks[0][0]).toBe(MON)
    expect(weeks[0]).toHaveLength(7)
  })

  it('caps the grid so one far-out announcement cannot stretch it', () => {
    const far = entry({ call: 'FAR', startUnix: MON + 300 * DAY, endUnix: MON + 310 * DAY })
    expect(buildWeeks([far], MON, 10)).toHaveLength(10)
  })
})

describe('dxpedColorIndex', () => {
  it('gives a callsign the same colour every time', () => {
    expect(dxpedColorIndex('VK9CM')).toBe(dxpedColorIndex('VK9CM'))
  })

  it('ignores case, so the chase set and the bar cannot disagree', () => {
    expect(dxpedColorIndex('vk9cm')).toBe(dxpedColorIndex('VK9CM'))
  })

  it('stays inside the palette', () => {
    for (const c of ['3Y0J', 'VK9CM', 'T33T', 'FT4GL', 'A', '']) {
      const i = dxpedColorIndex(c)
      expect(i).toBeGreaterThanOrEqual(0)
      expect(i).toBeLessThan(DXPED_COLOR_COUNT)
    }
  })

  it('spreads real callsigns across the palette rather than clumping', () => {
    const calls = ['3Y0J', 'VK9CM', 'T33T', 'FT4GL', 'VP8PJ', 'ZL9HR', 'E44RU', 'TX7G']
    const used = new Set(calls.map(dxpedColorIndex))
    expect(used.size).toBeGreaterThanOrEqual(6)
  })
})

describe('compactBands', () => {
  it('keeps the announcement order, low bands first', () => {
    expect(compactBands(['160m', '80m', '40m'])).toBe('160m·80m·40m')
  })

  it('counts the ones it had to drop', () => {
    expect(compactBands(['160m', '80m', '40m', '20m', '15m'])).toBe('160m·80m·40m+2')
  })

  it('renders nothing when the announcement listed no bands', () => {
    expect(compactBands([])).toBe('')
  })
})
