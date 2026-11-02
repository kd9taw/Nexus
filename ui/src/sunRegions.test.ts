import { describe, expect, it } from 'vitest'
import { limbSide, regionDiscXY, regionsShown, regionsUntil, regionsWhy } from './sunRegions'
import type { SolarRegion, SolarRegionsView } from './types'

const DAY29 = Date.UTC(2026, 9, 29) / 1000
const NOW = Date.UTC(2026, 9, 30, 2, 46)

function region(n: number, lat: number, lon: number): SolarRegion {
  return {
    region: n,
    latDeg: lat,
    lonDeg: lon,
    location: null,
    magClass: null,
    spotClass: null,
    area: null,
    numberSpots: null,
    mFlareProbability: null,
    xFlareProbability: null,
  }
}

const HELD: SolarRegionsView = {
  held: true,
  why: 'held',
  observedDay: DAY29,
  until: DAY29 + 3 * 86_400,
  regions: [region(4563, -18, -81), region(4564, 5, 68)],
}

describe('regionsShown: only a list the shack holds, and only until its own time', () => {
  it('draws a held list before its until', () => {
    expect(regionsShown(HELD, NOW)?.map((r) => r.region)).toEqual([4563, 4564])
    expect(regionsShown(HELD, (HELD.until! - 1) * 1000)).not.toBeNull()
  })

  it('draws nothing at or after its until, even if the shack still says held', () => {
    expect(regionsShown(HELD, HELD.until! * 1000)).toBeNull()
    expect(regionsShown(HELD, (HELD.until! + 7200) * 1000)).toBeNull()
  })

  it('draws nothing for a stale, ahead, unheld, or undated view', () => {
    expect(regionsShown({ ...HELD, held: false, why: 'stale', until: null }, NOW)).toBeNull()
    expect(regionsShown({ ...HELD, held: false, why: 'ahead', until: null }, NOW)).toBeNull()
    // A held flag with no until (an older or broken station) is not a held list: fail closed.
    expect(regionsShown({ ...HELD, until: null }, NOW)).toBeNull()
    expect(regionsShown({ ...HELD, observedDay: null }, NOW)).toBeNull()
    expect(regionsShown(null, NOW)).toBeNull()
    expect(regionsShown(undefined, NOW)).toBeNull()
  })

  it('draws nothing for a list whose day starts ahead of this clock', () => {
    // The shack judged it on its clock; a screen whose clock is behind still refuses tomorrow.
    const ahead = { ...HELD, observedDay: NOW / 1000 + 121 }
    expect(regionsShown(ahead, NOW)).toBeNull()
    expect(regionsShown({ ...HELD, observedDay: NOW / 1000 + 120 }, NOW)).not.toBeNull()
  })

  it('wakes at the until only while held', () => {
    expect(regionsUntil(HELD)).toBe(HELD.until)
    expect(regionsUntil({ ...HELD, held: false })).toBeNull()
    expect(regionsUntil(null)).toBeNull()
  })

  it('says why nothing is drawn', () => {
    expect(regionsWhy(HELD, NOW)).toBe('held')
    expect(regionsWhy(HELD, HELD.until! * 1000)).toBe('stale')
    expect(regionsWhy({ ...HELD, held: false, why: 'stale', until: null }, NOW)).toBe('stale')
    expect(regionsWhy({ ...HELD, held: false, why: 'ahead', until: null }, NOW)).toBe('ahead')
    expect(regionsWhy({ ...HELD, held: false, why: 'none', observedDay: null, until: null }, NOW)).toBe('none')
    expect(regionsWhy(null, NOW)).toBe('none')
    expect(regionsWhy({ ...HELD, until: null }, NOW)).toBe('unknown')
  })
})

describe('the disc: solar north up, east on the left, as NOAA images it', () => {
  it('puts east on the left and west on the right', () => {
    expect(regionDiscXY(region(1, 0, 90)).x).toBeCloseTo(-1, 9) // E90: the east limb, left
    expect(regionDiscXY(region(1, 0, -90)).x).toBeCloseTo(1, 9) // W90: the west limb, right
    expect(regionDiscXY(region(1, 0, 0))).toEqual({ x: -0, y: 0 })
    expect(regionDiscXY(region(1, 90, 0)).y).toBeCloseTo(1, 9) // north up
    expect(regionDiscXY(region(1, -30, 0)).y).toBeCloseTo(-0.5, 9)
    // Inside the disc wherever it is.
    const p = regionDiscXY(region(1, 40, -70))
    expect(Math.hypot(p.x, p.y)).toBeLessThanOrEqual(1)
  })

  it('names the limb a region is near', () => {
    expect(limbSide(region(1, 5, 68))).toBe('east')
    expect(limbSide(region(1, -18, -81))).toBe('west')
    expect(limbSide(region(1, -17, 13))).toBeNull()
    expect(limbSide(region(1, 0, 60))).toBe('east')
    expect(limbSide(region(1, 0, 59.9))).toBeNull()
  })
})
