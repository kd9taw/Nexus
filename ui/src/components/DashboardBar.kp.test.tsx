// @vitest-environment jsdom
//
// THE BAR'S KP SAYS WHAT IT IS (capsNow `kpNow`). NOAA's measured 3-hour Kp, while its period ended
// within the last 3 hours, is drawn with that period ("5 21Z–00Z") and wins over the 1-minute estimate;
// otherwise the estimate is drawn with "est."; a stand-in stays a dash with no word, and an old estimate
// keeps the honesty rule's own hover. Each with a control.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { KpForecast, SpaceWxView } from '../types'

vi.mock('../api', () => ({
  getWindowBehind: vi.fn(),
  setWindowBehind: vi.fn(),
  getSolarIndices: vi.fn(),
}))

import { barIndices } from './DashboardBar'
import { kpImpact } from '../propViz'
import { t } from '../i18n'

const NOW_MS = Date.UTC(2026, 9, 31, 1, 30)
const NOW = NOW_MS / 1000
const P21 = Date.UTC(2026, 9, 30, 21) / 1000
beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(NOW_MS)
})
afterEach(() => vi.useRealTimers())

const live = (over: Partial<SpaceWxView> = {}): SpaceWxView => ({
  sfi: 140,
  kp: 6.33,
  aIndex: 20,
  xrayClass: 'B-class',
  flare: false,
  xrayLong: 2e-7,
  fetched: true,
  sfiAt: NOW - 3600,
  kpAt: NOW - 120,
  xrayAt: NOW - 60,
  aDay: NOW - 86400,
  ...over,
})
const fc = (kp: number, start = P21): KpForecast =>
  ({ points: [{ timeUnix: start, kp, kind: 'observed', noaaScale: null }] }) as unknown as KpForecast
const kpOf = (wx: SpaceWxView, f?: KpForecast | null) => barIndices(wx, null, f).find((i) => i.key === 'Kp')!

describe('the bar’s Kp', () => {
  it('a measured period that ended within 3 hours is drawn with that period, and wins over the estimate', () => {
    const k = kpOf(live(), fc(4.67))
    expect(k).toMatchObject({ value: '5', day: '21Z–00Z', warn: true })
    expect(k.title).toContain(t('prop.kpNow.measured', { kp: '4.7', from: '21Z', to: '00Z' }))
    expect(k.title).toContain(kpImpact(4.67).text)
  })

  it('without one, the estimate is drawn with "est." and its time on hover', () => {
    const k = kpOf(live(), fc(4.67, P21 - 6 * 3600))
    expect(k).toMatchObject({ value: '6', day: t('dash.kp.est'), warn: true })
    expect(k.title).toContain(t('prop.kpNow.est', { kp: '6.3', time: '01:28Z' }))
    // and with no forecast at all
    expect(kpOf(live(), null)).toMatchObject({ value: '6', day: t('dash.kp.est') })
  })

  it('a stand-in is a dash with no word; a measured period still speaks beside it', () => {
    expect(kpOf(live({ kpAt: null, kp: 2 }), null)).toMatchObject({ value: '—', dim: true })
    expect(kpOf(live({ kpAt: null, kp: 2 }), null).day).toBeUndefined()
    expect(kpOf(live({ kpAt: null, kp: 2 }), fc(3))).toMatchObject({ value: '3', day: '21Z–00Z' })
  })

  it('an old estimate keeps its honesty-rule hover (its age), dimmed, with no warning', () => {
    const k = kpOf(live({ kpAt: NOW - 3600 }), null)
    expect(k).toMatchObject({ value: '6', dim: true, warn: false, day: t('dash.kp.est') })
    expect(k.title).toBe(t('prop.spaceWx.stale', { ago: t('prop.opening.ago.hours', { hours: 1 }) }))
  })
})
