// @vitest-environment jsdom
//
// The Clock box: big UTC and local time, the date, the grid, and today's sunrise and sunset.
//
// ⚠️ THE ZONE IS PINNED HERE, AND CHECKED. CI runs in UTC, where "local" and "UTC" are the same
// digits and a clock that printed UTC twice would pass every assertion below. So the file runs
// in America/Chicago, and its first test proves the zone took before any other test can mean
// anything.
import { describe, it, expect, vi, beforeAll, afterAll, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { ClockPane } from './ClockPane'
import { sunDay } from '../../mapGeo'
import { gridToLatLon } from '../../grid'
import { t } from '../../i18n'

const PRIOR_TZ = process.env.TZ
beforeAll(() => {
  process.env.TZ = 'America/Chicago'
})
afterAll(() => {
  if (PRIOR_TZ === undefined) delete process.env.TZ
  else process.env.TZ = PRIOR_TZ
})

/** 14:32:07.250 UTC on Tue 29 Sep 2026 — 09:32:07 in Chicago (CDT, UTC−5). */
const NOW = Date.UTC(2026, 8, 29, 14, 32, 7, 250)

beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(NOW)
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

const digits = (c: HTMLElement, which: 'utc' | 'local') =>
  c.querySelector(`.clock-${which} .clock-digits text`)?.textContent
/** HH:MMZ from the ISO string — an independent formatter, so the test does not share the box's. */
const z = (ms: number) => `${new Date(ms).toISOString().slice(11, 16)}Z`

describe('the Clock box', () => {
  it('precondition: this file runs in a zone that is not UTC', () => {
    expect(new Date(Date.UTC(2026, 8, 29, 14, 0, 0)).getHours()).toBe(9)
  })

  it("shows UTC and this computer's local time, each to the second", () => {
    const { container } = render(<ClockPane myGrid="EN52" />)
    expect(digits(container, 'utc')).toBe('14:32:07')
    expect(digits(container, 'local')).toBe('09:32:07')
  })

  it('ticks on the second', () => {
    const { container } = render(<ClockPane myGrid="EN52" />)
    act(() => {
      vi.advanceTimersByTime(1000)
    })
    expect(digits(container, 'utc')).toBe('14:32:08')
    expect(digits(container, 'local')).toBe('09:32:08')
  })

  it('carries the UTC date and the local date', () => {
    const { container } = render(<ClockPane myGrid="EN52" />)
    expect(container.querySelector('.clock-utc .clock-date')?.textContent).toMatch(/29/)
    expect(container.querySelector('.clock-utc .clock-date')?.textContent).toMatch(/2026/)
    expect(container.querySelector('.clock-local .clock-date')?.textContent).toMatch(/29/)
  })

  it("gives the grid and today's sunrise and sunset there, in UTC", () => {
    const { container } = render(<ClockPane myGrid="EN52" />)
    const sun = container.querySelector('.clock-sun')?.textContent ?? ''
    const here = gridToLatLon('EN52')!
    const today = sunDay(here.lat, here.lon, NOW)
    expect(sun).toContain('EN52')
    expect(sun).toContain(z(today.riseMs!))
    expect(sun).toContain(z(today.setMs!))
  })

  it('asks for a grid rather than guess a place', () => {
    const { container } = render(<ClockPane myGrid="" />)
    expect(container.querySelector('.clock-sun')?.textContent).toBe(t('connect.clock.noGrid'))
    // The clocks themselves need no grid.
    expect(digits(container, 'utc')).toBe('14:32:07')
  })

  it('says the sun stays up, or down, rather than printing a time that does not exist', () => {
    // JP99 is the square around Tromsø (69.5 N): midnight sun in June, polar night in December.
    vi.setSystemTime(Date.UTC(2026, 5, 21, 12, 0, 0))
    const june = render(<ClockPane myGrid="JP99" />)
    expect(june.container.querySelector('.clock-sun')?.textContent).toContain(t('connect.clock.sun.up'))
    cleanup()
    vi.setSystemTime(Date.UTC(2026, 11, 21, 12, 0, 0))
    const dec = render(<ClockPane myGrid="JP99" />)
    expect(dec.container.querySelector('.clock-sun')?.textContent).toContain(t('connect.clock.sun.down'))
  })
})
