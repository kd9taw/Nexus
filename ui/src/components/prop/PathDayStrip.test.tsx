// @vitest-environment jsdom
//
// Path over the day: the selected station's strip and the ten regions' strips. What a cell shows
// (the route the model favours, its modelled figure, the polar underline, the MUF ceiling), the Kp
// ribbon's three sources drawn three ways, the hours that scrub the map, the long-path label
// wherever a long-path figure can show, and the regions in the operator's order.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { act, fireEvent, render, waitFor, within } from '@testing-library/react'
import type { PathDay, PathDayCell, PathPrediction, PathRegions } from '../../types'

const fx = vi.hoisted(() => ({ regions: null as unknown, zones: [] as { zone: number; qsos: number }[] }))
vi.mock('../../api', async (importOriginal) => {
  const real = await importOriginal<typeof import('../../api')>()
  return {
    ...real,
    getPathRegions: vi.fn(async () => fx.regions),
    getLogStats: vi.fn(async () => ({ total: 0, resolved: 0, dx: 0, domestic: 0, byContinent: [], byZone: fx.zones })),
  }
})

import { PathDayPane, PathRegionsPane } from './PathDayStrip'
import { getPathRegions } from '../../api'
import { heatColor } from '../../propViz'

const T0 = 1_778_803_200 // 2026-05-15 00:00 UTC

function cells(f: (h: number) => PathDayCell): PathDayCell[] {
  return Array.from({ length: 24 }, (_, h) => f(h))
}

/** 20m: short path open 00–11Z, long path 12–23Z; 40m: within the margin all day. */
function day(long = true): PathDay {
  return {
    nowHour: 12,
    mufShort: Array(24).fill(16),
    mufLong: long ? Array(24).fill(16) : undefined,
    bands: [
      {
        band: '20m',
        cells: cells((h) =>
          !long
            ? { shortPct: h < 12 ? 80 : 4 }
            : h < 12
              ? { shortPct: 80, longPct: 3, verdict: 'short', confidence: 'likely' }
              : { shortPct: 4, longPct: 70, verdict: 'long', confidence: 'likely' },
        ),
      },
      {
        band: '40m',
        cells: cells(() => (long ? { shortPct: 30, longPct: 35, verdict: 'either' } : { shortPct: 30 })),
      },
    ],
    hours: Array.from({ length: 24 }, (_, h) => ({
      unix: T0 + h * 3600,
      kp: h === 18 ? 5.33 : 2,
      kpSource: h < 12 ? 'measured' : h === 23 ? 'current' : 'forecast',
      polarShort: false,
      polarLong: true,
      greyMe: h === 10,
      greyDx: false,
    })),
  }
}

function pred(d: PathDay | undefined): PathPrediction {
  return { engine: 'p533', bands: [], mufNow: 16, mufHourly: [], day: d }
}

const rows = (c: HTMLElement) => [...c.querySelectorAll('.pd-row:not(.pd-grey):not(.pd-kp)')]
const cellsOf = (row: Element) => [...row.querySelectorAll('.pd-cell')] as HTMLElement[]

describe('the selected station strip', () => {
  it('marks each hour with the route the model favours and colours it by that route', () => {
    const { container } = render(<PathDayPane pred={pred(day())} scrub={null} />)
    const [b20, b40] = rows(container)
    const c20 = cellsOf(b20)
    expect(c20).toHaveLength(24)
    expect(c20[3].textContent).toBe('▸')
    expect(c20[3].style.background).toBe(heatColor(0.8))
    expect(c20[20].textContent).toBe('◂')
    expect(c20[20].style.background).toBe(heatColor(0.7))
    // Either way: the better figure, no route mark.
    expect(cellsOf(b40)[5].textContent).toBe('')
    expect(cellsOf(b40)[5].style.background).toBe(heatColor(0.35))
    // The long path is over the cap every hour: underlined where the long path is shown.
    expect(c20[20].classList.contains('polar')).toBe(true)
    expect(c20[3].classList.contains('polar')).toBe(false)
    // MUF 16 MHz: 20m is the highest band under it, so the ceiling sits on the 20m row.
    expect(c20[0].classList.contains('ceil')).toBe(true)
    expect(cellsOf(b40)[0].classList.contains('ceil')).toBe(false)
    expect(c20[12].classList.contains('now')).toBe(true)
    expect(container.textContent).toContain('20m 00Z–12Z by the short path')
    expect(container.textContent).toContain('20m 12Z–00Z by the long path')
    expect(container.textContent).toContain('Best: 20m 00Z–12Z by the short path; 20m 12Z–00Z by the long path.')
    expect(container.querySelector('.route-lp-badge')?.textContent).toBe('modelled, unvalidated for long path')
    expect(c20[20].title).toContain('long path 70%')
    expect(c20[20].title).toContain('the model favours the long path (likely)')
    expect(c20[20].title).toContain('modelled')
  })

  it('names a crossing as the why line does: polar cap from 70°, else auroral zone, "if" only for a forecast storm', () => {
    const at = (d: PathDay, h: number) => {
      const { container, unmount } = render(<PathDayPane pred={pred(d)} scrub={null} />)
      const t = cellsOf(rows(container)[0])[h].title
      unmount()
      return t
    }
    // Deep: the long path reaches 75° geomagnetic.
    const deep = { ...day(), maxGmlatShort: 40, maxGmlatLong: 75 }
    expect(at(deep, 20)).toContain('crosses the polar cap')
    // Shallow and already in the zone now (12Z): the auroral zone, even at a forecast hour.
    const zone = { ...day(), maxGmlatShort: 40, maxGmlatLong: 65 }
    expect(at(zone, 18)).toContain('crosses the auroral zone')
    expect(at(zone, 18)).not.toContain('Auroral zone if')
    // Shallow, outside the zone now; only the 18Z forecast storm brings the long path in.
    const stormIf = day()
    stormIf.maxGmlatShort = 40
    stormIf.maxGmlatLong = 65
    stormIf.hours = stormIf.hours.map((h, i) => ({ ...h, polarLong: i === 18 }))
    expect(at(stormIf, 18)).toContain('Auroral zone if Kp 5.3 (forecast from 1800Z)')
    expect(at(stormIf, 20)).not.toMatch(/auroral zone|polar cap/i)
  })

  it('SP shows the short path alone and drops the long-path label; LP the long path', () => {
    const { container, getByRole } = render(<PathDayPane pred={pred(day())} scrub={null} />)
    fireEvent.click(getByRole('button', { name: 'SP' }))
    const c20 = cellsOf(rows(container)[0])
    expect(c20[20].style.background).toBe(heatColor(0.04))
    expect(c20[20].textContent).toBe('▸')
    expect(container.querySelector('.route-lp-badge')).toBeNull()
    fireEvent.click(getByRole('button', { name: 'LP' }))
    expect(cellsOf(rows(container)[0])[3].style.background).toBe(heatColor(0.03))
    expect(container.querySelector('.route-lp-badge')).not.toBeNull()
  })

  it('draws the Kp ribbon measured solid, forecast hollow, carried-forward dashed, each said in the hover', () => {
    const { container } = render(<PathDayPane pred={pred(day())} scrub={null} />)
    const kp = [...container.querySelectorAll('.pd-kpcell')] as HTMLElement[]
    expect(kp).toHaveLength(24)
    expect(kp[3].className).toContain('measured')
    expect(kp[18].className).toContain('forecast')
    expect(kp[18].className).toContain('bad')
    expect(kp[18].title).toBe('18Z: Kp 5.3, forecast')
    expect(kp[23].className).toContain('current')
    expect(kp[23].title).toContain('latest reading, no forecast for this hour')
    expect(kp[3].title).toContain('measured')
    // The grey line at the operator's end at 10Z.
    const grey = [...container.querySelectorAll('.pd-greycell')]
    expect(grey[10].textContent).toBe('◐')
    expect(grey[11].textContent).toBe('')
  })

  it('with no space-weather data a Kp hour is empty and says unknown, never a quiet number', () => {
    const d = day()
    d.hours = d.hours.map((h) => ({ ...h, kp: null, kpSource: 'unknown' as const }))
    const { container } = render(<PathDayPane pred={pred(d)} scrub={null} />)
    const kp = [...container.querySelectorAll('.pd-kpcell')] as HTMLElement[]
    expect(kp[5].className).toContain('unknown')
    expect(kp[5].className).not.toMatch(/good|warn|bad/)
    expect(kp[5].title).toBe('05Z: unknown (no Kp reading)')
  })

  it('an hour scrubs the map to the middle of that hour, and the same hour goes back to now', () => {
    const onScrub = vi.fn()
    const { getAllByRole, rerender } = render(<PathDayPane pred={pred(day())} scrub={null} onScrub={onScrub} />)
    const hours = getAllByRole('button', { name: /on the map/ })
    expect(hours).toHaveLength(24)
    fireEvent.click(hours[14])
    expect(onScrub).toHaveBeenLastCalledWith({ hour: 14, ms: (T0 + 14 * 3600 + 1800) * 1000 })
    rerender(<PathDayPane pred={pred(day())} scrub={14} onScrub={onScrub} />)
    fireEvent.click(getAllByRole('button', { name: 'Now', pressed: true })[0])
    expect(onScrub).toHaveBeenLastCalledWith(null)
  })

  it('with no map beside it the hours are not buttons that do anything', () => {
    const { getAllByRole } = render(<PathDayPane pred={pred(day())} scrub={null} />)
    expect(getAllByRole('button', { name: /on the map/ }).every((b) => (b as HTMLButtonElement).disabled)).toBe(true)
  })

  it('an engine with no long path shows the short path only and claims nothing long', () => {
    const { container, getByRole } = render(<PathDayPane pred={pred(day(false))} scrub={null} />)
    expect((getByRole('button', { name: 'LP' }) as HTMLButtonElement).disabled).toBe(true)
    expect(container.querySelector('.route-lp-badge')).toBeNull()
    expect(container.textContent).toContain('Best: 20m 00Z–12Z.')
    expect(container.textContent).not.toMatch(/long path/)
  })

  it('without the day it draws its one line', () => {
    const { container } = render(<PathDayPane pred={pred(undefined)} scrub={null} />)
    expect(container.querySelector('.pane-basic')).not.toBeNull()
    expect(container.querySelector('.pd-grid')).toBeNull()
  })
})

describe('the region strips', () => {
  const region = (id: string, zones: number[]) => ({ id, lat: 0, lon: 0, zones, day: day() })
  beforeEach(() => {
    fx.regions = { engine: 'p533', computedUnix: T0, regions: [region('eu', [14]), region('ja', [25]), region('vk', [30])] } satisfies PathRegions
    fx.zones = []
  })

  const names = (c: HTMLElement) => [...c.querySelectorAll('.pd-region-name')].map((n) => n.textContent)

  it('reads in the fixed order on an empty log, one band a region, and opens to every band', async () => {
    const { container } = render(<PathRegionsPane needs={[]} scrub={null} />)
    await waitFor(() => expect(names(container)).toEqual(['Europe', 'Japan & East Asia', 'Australia & New Zealand']))
    expect(rows(container)).toHaveLength(3)
    // The best band: 20m is open 24 hours one way or the other.
    expect(within(rows(container)[0] as HTMLElement).getByText('20m')).toBeTruthy()
    fireEvent.click(container.querySelector('.pd-region')!)
    expect(rows(container)).toHaveLength(4)
    expect(container.querySelector('.pd-region')!.getAttribute('aria-expanded')).toBe('true')
  })

  it('the now column follows the clock within a minute, with no new sweep asked for', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    try {
      vi.setSystemTime(new Date((T0 + 12 * 3600 + 3540) * 1000)) // 12:59Z; the sweep says 12Z
      const { container } = render(<PathRegionsPane needs={[]} scrub={null} myGrid="FN42" engine="p533" />)
      await waitFor(() => expect(names(container).length).toBe(3))
      const nowCol = () => [...container.querySelectorAll('.pd-hour')].findIndex((b) => b.classList.contains('now'))
      expect(nowCol()).toBe(12)
      const asked = vi.mocked(getPathRegions).mock.calls.length
      await act(async () => {
        vi.advanceTimersByTime(61_000) // 13:00Z
      })
      expect(nowCol()).toBe(13)
      expect(vi.mocked(getPathRegions).mock.calls.length, 'no new sweep for the hour').toBe(asked)
    } finally {
      vi.useRealTimers()
    }
  })

  it('asks the shack again when the grid or the engine changes', async () => {
    const { container, rerender } = render(<PathRegionsPane needs={[]} scrub={null} myGrid="FN42" engine="p533" />)
    await waitFor(() => expect(names(container).length).toBe(3))
    const asked = () => vi.mocked(getPathRegions).mock.calls.length
    const before = asked()
    rerender(<PathRegionsPane needs={[]} scrub={null} myGrid="FN42" engine="p533" />)
    expect(asked(), 'control: the same settings ask nothing').toBe(before)
    rerender(<PathRegionsPane needs={[]} scrub={null} myGrid="JN58" engine="p533" />)
    await waitFor(() => expect(asked()).toBe(before + 1))
    rerender(<PathRegionsPane needs={[]} scrub={null} myGrid="JN58" engine="heuristic" />)
    await waitFor(() => expect(asked()).toBe(before + 2))
  })

  it('leaves out a region it has no name for', async () => {
    fx.regions = { engine: 'p533', computedUnix: T0, regions: [region('atlantis', [1]), region('eu', [14])] }
    const { container } = render(<PathRegionsPane needs={[]} scrub={null} />)
    await waitFor(() => expect(names(container)).toEqual(['Europe']))
    expect(rows(container)).toHaveLength(1)
  })

  it('puts the needed stations first, then the logged contacts', async () => {
    fx.zones = [{ zone: 30, qsos: 400 }]
    const { container, rerender } = render(<PathRegionsPane needs={[]} scrub={null} />)
    await waitFor(() => expect(names(container)[0]).toBe('Australia & New Zealand'))
    const need = { zone: 25 } as Parameters<typeof PathRegionsPane>[0]['needs'][number]
    rerender(<PathRegionsPane needs={[need]} scrub={null} />)
    expect(names(container)).toEqual(['Japan & East Asia', 'Australia & New Zealand', 'Europe'])
  })

  it('marks only the operator’s end on the grey row (each region’s far end differs)', async () => {
    const r = day()
    r.hours = r.hours.map((h, i) => ({ ...h, greyMe: false, greyDx: i === 4 }))
    fx.regions = { engine: 'p533', computedUnix: T0, regions: [{ id: 'eu', lat: 0, lon: 0, zones: [14], day: r }] }
    const { container } = render(<PathRegionsPane needs={[]} scrub={null} />)
    await waitFor(() => expect(names(container)).toHaveLength(1))
    expect([...container.querySelectorAll('.pd-greycell')].map((g) => g.textContent).join('')).toBe('')
  })
})
