// @vitest-environment jsdom
//
// The sun disc in the unfolded flare chip: NOAA's regions where NOAA observed them (east on the left),
// each named with its day; nothing drawn, and the reason given, for a list that is stale, ahead,
// unknown, or not held.
import { describe, it, expect, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { SunRegionsDisc } from './SunRegionsDisc'
import { t } from '../../i18n'
import { regionsDayLabel } from '../../sunRegions'
import type { SolarRegionsView } from '../../types'

afterEach(cleanup)

const DAY29 = Date.UTC(2026, 9, 29) / 1000
const NOW = Date.UTC(2026, 9, 30, 2, 46)
const region = (n: number, lat: number, lon: number) => ({
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
})
const HELD: SolarRegionsView = {
  held: true,
  why: 'held',
  observedDay: DAY29,
  until: DAY29 + 3 * 86_400,
  regions: [region(4563, -18, -81), region(4564, 5, 68)],
}
const day = regionsDayLabel(DAY29)

describe('the sun disc', () => {
  it('draws each held region, east on the left, and names the day', () => {
    const r = render(<SunRegionsDisc view={HELD} nowMs={NOW} />)
    const dots = r.container.querySelectorAll('[data-region]')
    expect(dots.length).toBe(2)
    const x = (n: number) => Number(r.container.querySelector(`[data-region="${n}"] circle`)!.getAttribute('cx'))
    expect(x(4564), 'the east-limb region is on the left').toBeLessThan(0)
    expect(x(4563), 'the west-limb region is on the right').toBeGreaterThan(0)
    expect(r.container.textContent).toContain(t('map.sun.regions.head', { day }))
    expect(r.container.textContent).toContain(`4564 N05E68 (${t('map.sun.regions.eastLimb')})`)
    expect(r.container.querySelector('.sun-regions')!.getAttribute('title')).toBe(t('map.sun.regions.title', { day }))
  })

  it('says so when the held list has no region on the visible disc', () => {
    const r = render(<SunRegionsDisc view={{ ...HELD, regions: [] }} nowMs={NOW} />)
    expect(r.container.querySelectorAll('[data-region]').length).toBe(0)
    expect(r.container.textContent).toContain(t('map.sun.regions.empty', { day }))
  })

  it('draws nothing, and says why, for a list it may not draw', () => {
    const cases: Array<[SolarRegionsView | null, string]> = [
      [{ ...HELD, held: false, why: 'stale', until: null }, t('map.sun.regions.stale', { day })],
      [{ ...HELD, held: false, why: 'ahead', until: null }, t('map.sun.regions.ahead', { day })],
      [{ ...HELD, until: null }, t('map.sun.regions.unknown')],
      [null, t('map.sun.regions.none')],
    ]
    for (const [view, words] of cases) {
      const r = render(<SunRegionsDisc view={view} nowMs={NOW} />)
      expect(r.container.querySelector('svg'), `drew a disc for ${words}`).toBeNull()
      expect(r.container.textContent).toBe(words)
      cleanup()
    }
    // Past its until on this clock: stale, whatever the shack last said.
    const r = render(<SunRegionsDisc view={HELD} nowMs={HELD.until! * 1000} />)
    expect(r.container.querySelector('svg')).toBeNull()
    expect(r.container.textContent).toBe(t('map.sun.regions.stale', { day }))
  })
})
