// @vitest-environment jsdom
//
// NIGHT — Off / On / Auto (Settings ▸ Appearance ▸ Workspace). Auto is civil dusk to civil dawn
// at the station's grid square: Night shows while the sun is more than 6° below the horizon there
// (operator, 2026-09-26: "At dusk, sun 6° down"), looked at again about once a minute, and with no
// grid square Auto stays OFF. The instants below are fixed and their solar elevations are asserted
// in place, through the same `solarElevationDeg` the map's terminator draws with, so each case
// says what it is testing rather than trusting a clock.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { act, renderHook } from '@testing-library/react'
import { CIVIL_DUSK_DEG, NIGHT_STORAGE_KEY, NIGHT_TICK_MS, sunDownAt, useNight, useNightActive } from './useNight'
import { PALETTE_EVENT, usePaletteKey } from './usePaletteRoles'
import { solarElevationDeg } from './mapGeo'
import { gridToLatLon } from './grid'

const root = () => document.documentElement
const at = (iso: string) => Date.parse(iso)
const elevation = (grid: string, iso: string) => {
  const ll = gridToLatLon(grid)!
  return solarElevationDeg(ll.lat, ll.lon, at(iso))
}

// EN52 (42.5° N, 89° W), the evening of 2026-03-20 local — 00:34Z the next day is the last
// minute above −6°, 00:35Z the first below it. And the next morning's civil dawn.
const DUSK_BEFORE = '2026-03-21T00:34:00Z'
const DUSK_AFTER = '2026-03-21T00:35:00Z'
const DAWN_BEFORE = '2026-03-21T11:32:00Z'
const DAWN_AFTER = '2026-03-21T11:33:00Z'
const LOCAL_MIDNIGHT = '2026-03-21T06:00:00Z'
const LOCAL_NOON = '2026-03-20T18:00:00Z'

beforeEach(() => {
  localStorage.clear()
  root().removeAttribute('data-night')
})
afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
  root().removeAttribute('data-night')
})

function atTime(iso: string) {
  vi.useFakeTimers()
  vi.setSystemTime(at(iso))
}

describe('the fixtures are what they say they are', () => {
  it('civil dusk and dawn at EN52 fall between these minutes, and noon and midnight are not close', () => {
    expect(CIVIL_DUSK_DEG).toBe(-6)
    expect(elevation('EN52', DUSK_BEFORE)).toBeGreaterThan(-6)
    expect(elevation('EN52', DUSK_AFTER)).toBeLessThan(-6)
    expect(elevation('EN52', DAWN_BEFORE)).toBeLessThan(-6)
    expect(elevation('EN52', DAWN_AFTER)).toBeGreaterThan(-6)
    expect(elevation('EN52', LOCAL_NOON)).toBeGreaterThan(30)
    expect(elevation('EN52', LOCAL_MIDNIGHT)).toBeLessThan(-30)
  })
})

describe('sunDownAt — the verdict', () => {
  it('is the sun more than 6° down at the grid', () => {
    expect(sunDownAt('EN52', at(DUSK_BEFORE))).toBe(false)
    expect(sunDownAt('EN52', at(DUSK_AFTER))).toBe(true)
    expect(sunDownAt('en52', at(LOCAL_MIDNIGHT)), 'a lower-case square is the same square').toBe(true)
    expect(sunDownAt('EN52AB', at(LOCAL_NOON)), 'six characters').toBe(false)
  })

  it('has no verdict without a real grid square', () => {
    for (const g of ['', '   ', 'EN5', 'ZZ99', 'EN52X', 'KD9TAW'] as const) {
      expect(sunDownAt(g, at(LOCAL_MIDNIGHT)), JSON.stringify(g)).toBeNull()
    }
  })

  it('knows polar night and polar day (JQ78, 78.5° N)', () => {
    // The sun never climbs within 6° of the horizon there at the December solstice, and never
    // sets at the June one: Auto is night at noon, and day at midnight.
    expect(elevation('JQ78', '2026-12-21T12:00:00Z')).toBeLessThan(-6)
    expect(sunDownAt('JQ78', at('2026-12-21T12:00:00Z'))).toBe(true)
    expect(elevation('JQ78', '2026-06-21T00:00:00Z')).toBeGreaterThan(0)
    expect(sunDownAt('JQ78', at('2026-06-21T00:00:00Z'))).toBe(false)
  })
})

describe('useNight — the owner', () => {
  it('a machine that never chose is Off: no attribute, nothing stored', () => {
    const { result } = renderHook(() => useNight('EN52'))
    expect(result.current.night).toBe('off')
    expect(result.current.active).toBe(false)
    expect(root().getAttribute('data-night')).toBeNull()
    expect(localStorage.getItem(NIGHT_STORAGE_KEY)).toBeNull()
  })

  it('On shows Night whatever the grid or the hour, and Off takes it away', () => {
    atTime(LOCAL_NOON)
    const { result } = renderHook(() => useNight(''))
    act(() => result.current.setNight('on'))
    expect(result.current.active).toBe(true)
    expect(root().getAttribute('data-night')).toBe('1')
    expect(localStorage.getItem(NIGHT_STORAGE_KEY)).toBe('on')
    act(() => result.current.setNight('off'))
    expect(result.current.active).toBe(false)
    expect(root().getAttribute('data-night')).toBeNull()
    expect(localStorage.getItem(NIGHT_STORAGE_KEY)).toBe('off')
  })

  it('applies a saved choice on mount, and the mount writes nothing', () => {
    localStorage.setItem(NIGHT_STORAGE_KEY, 'on')
    const set = vi.spyOn(Storage.prototype, 'setItem')
    const { result } = renderHook(() => useNight(''))
    expect(result.current.night).toBe('on')
    expect(root().getAttribute('data-night')).toBe('1')
    // Opening a window must not rewrite a choice made in another one.
    expect(set).not.toHaveBeenCalled()
  })

  it('a stored value it does not know is Off', () => {
    localStorage.setItem(NIGHT_STORAGE_KEY, 'dim')
    const { result } = renderHook(() => useNight('EN52'))
    expect(result.current.night).toBe('off')
    expect(root().getAttribute('data-night')).toBeNull()
  })

  it('unreadable storage is Off, and a pick still applies for the session', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    const { result } = renderHook(() => useNight(''))
    expect(result.current.night).toBe('off')
    act(() => result.current.setNight('on'))
    expect(root().getAttribute('data-night')).toBe('1')
  })
})

describe('useNight — Auto, by the sun at the station', () => {
  it('with no grid square Auto stays Off, even at midnight, and says it has no grid', () => {
    atTime(LOCAL_MIDNIGHT)
    localStorage.setItem(NIGHT_STORAGE_KEY, 'auto')
    for (const g of ['', 'EN5', 'ZZ99']) {
      const { result, unmount } = renderHook(() => useNight(g))
      expect(result.current.night).toBe('auto')
      expect(result.current.gridKnown, JSON.stringify(g)).toBe(false)
      expect(result.current.active, JSON.stringify(g)).toBe(false)
      expect(root().getAttribute('data-night'), JSON.stringify(g)).toBeNull()
      unmount()
    }
  })

  it('comes on at civil dusk, looked at again each minute', () => {
    atTime('2026-03-21T00:30:00Z')
    localStorage.setItem(NIGHT_STORAGE_KEY, 'auto')
    const { result } = renderHook(() => useNight('EN52'))
    expect(result.current.gridKnown).toBe(true)
    expect(result.current.active).toBe(false)
    // 00:31 … 00:34: the sun is still above −6°.
    act(() => void vi.advanceTimersByTime(4 * NIGHT_TICK_MS))
    expect(Date.now()).toBe(at(DUSK_BEFORE))
    expect(result.current.active).toBe(false)
    expect(root().getAttribute('data-night')).toBeNull()
    // 00:35: below it.
    act(() => void vi.advanceTimersByTime(NIGHT_TICK_MS))
    expect(Date.now()).toBe(at(DUSK_AFTER))
    expect(result.current.active).toBe(true)
    expect(root().getAttribute('data-night')).toBe('1')
  })

  it('goes off at civil dawn', () => {
    atTime(DAWN_BEFORE)
    localStorage.setItem(NIGHT_STORAGE_KEY, 'auto')
    const { result } = renderHook(() => useNight('EN52'))
    expect(result.current.active).toBe(true)
    expect(root().getAttribute('data-night')).toBe('1')
    act(() => void vi.advanceTimersByTime(NIGHT_TICK_MS))
    expect(result.current.active).toBe(false)
    expect(root().getAttribute('data-night')).toBeNull()
  })

  it('under polar night it is Night at noon', () => {
    atTime('2026-12-21T12:00:00Z')
    localStorage.setItem(NIGHT_STORAGE_KEY, 'auto')
    const { result } = renderHook(() => useNight('JQ78'))
    expect(result.current.active).toBe(true)
  })

  it('follows a change of grid square at once, without waiting for the minute', () => {
    // 04:00Z on the June solstice: deep night at EN52 (−18.7°), already day at JO01 (+1.8°).
    atTime('2026-06-21T04:00:00Z')
    expect(elevation('EN52', '2026-06-21T04:00:00Z')).toBeLessThan(-6)
    expect(elevation('JO01', '2026-06-21T04:00:00Z')).toBeGreaterThan(-6)
    localStorage.setItem(NIGHT_STORAGE_KEY, 'auto')
    const { result, rerender } = renderHook(({ grid }) => useNight(grid), { initialProps: { grid: 'EN52' } })
    expect(result.current.active).toBe(true)
    rerender({ grid: 'JO01' })
    expect(result.current.active).toBe(false)
    expect(root().getAttribute('data-night')).toBeNull()
    rerender({ grid: '' })
    expect(result.current.gridKnown).toBe(false)
    expect(result.current.active).toBe(false)
  })

  it('a verdict stored on an earlier night cannot swallow tonight’s dusk', () => {
    // The minute tick only wakes the hook when what it sees changes. Had it kept the "down" it
    // stored last night, then at dusk it would see "down" again, change nothing, and Night would
    // stay off until something else happened to re-render — so it re-reads the sun on restart.
    atTime(LOCAL_MIDNIGHT)
    localStorage.setItem(NIGHT_STORAGE_KEY, 'auto')
    const { result } = renderHook(() => useNight('EN52'))
    expect(result.current.active).toBe(true)
    act(() => result.current.setNight('off'))
    vi.setSystemTime(at(DUSK_BEFORE))
    act(() => result.current.setNight('auto'))
    expect(result.current.active).toBe(false)
    act(() => void vi.advanceTimersByTime(NIGHT_TICK_MS))
    expect(Date.now()).toBe(at(DUSK_AFTER))
    expect(result.current.active).toBe(true)
    expect(root().getAttribute('data-night')).toBe('1')
  })

  it('picking Auto at night turns Night on at once', () => {
    atTime(LOCAL_MIDNIGHT)
    const { result } = renderHook(() => useNight('EN52'))
    expect(result.current.active).toBe(false)
    act(() => result.current.setNight('auto'))
    expect(result.current.active).toBe(true)
    expect(localStorage.getItem(NIGHT_STORAGE_KEY)).toBe('auto')
  })
})

describe('the readers: a canvas that caches colours hears about Night', () => {
  it('a flip fires the colour event, moves usePaletteKey and flips useNightActive', () => {
    const heard = vi.fn()
    window.addEventListener(PALETTE_EVENT, heard)
    const owner = renderHook(() => useNight(''))
    const key = renderHook(() => usePaletteKey())
    const active = renderHook(() => useNightActive())
    const before = key.result.current
    expect(active.result.current).toBe(false)
    heard.mockClear()
    act(() => owner.result.current.setNight('on'))
    expect(heard).toHaveBeenCalled()
    expect(key.result.current).not.toBe(before)
    expect(active.result.current).toBe(true)
    act(() => owner.result.current.setNight('off'))
    expect(key.result.current).toBe(before)
    expect(active.result.current).toBe(false)
    window.removeEventListener(PALETTE_EVENT, heard)
  })
})
