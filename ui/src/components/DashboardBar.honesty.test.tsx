// @vitest-environment jsdom
//
// The dashboard bar (the Connect pop-out's top line and the TV page) draws only what NOAA measured.
// A "partial" snapshot — SWPC failed, the spots answered — carries the model's stand-ins (SFI 120,
// Kp 2, A 8, B1.0) with no sample time; the bar used to draw them as the day's numbers. Each is now
// a dash with no warning mark, a reading past its cadence keeps its number but loses the warning
// mark and says how old it is on hover, and the A is NOAA's daily planetary A, dated.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { SpaceWxView } from '../types'

vi.mock('../api', () => ({
  getWindowBehind: vi.fn(),
  setWindowBehind: vi.fn(),
  getSolarIndices: vi.fn(),
}))

import { barIndices } from './DashboardBar'
import { dayLabel } from './prop/SolarTrends'
import { WX_STALE_SECS } from '../propViz'
import { t } from '../i18n'

const NOW_MS = Date.UTC(2026, 9, 30, 2, 46, 0)
const NOW = NOW_MS / 1000
const DAY = Date.UTC(2026, 9, 29) / 1000
beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(NOW_MS)
})
afterEach(() => vi.useRealTimers())

const live = (over: Partial<SpaceWxView> = {}): SpaceWxView => ({
  sfi: 89,
  kp: 0.67,
  aIndex: 4,
  xrayClass: 'B-class',
  flare: false,
  xrayLong: 2.13e-7,
  fetched: true,
  sfiAt: DAY + 20 * 3600,
  kpAt: NOW - 6 * 60,
  xrayAt: NOW - 4 * 60,
  aDay: DAY,
  apNow: 2.68,
  ...over,
})
const of = (wx: SpaceWxView, key: string) => barIndices(wx).find((i) => i.key === key)!

describe('the bar draws no stand-in', () => {
  it('a partial snapshot is a dash on every snapshot index, with no warning mark', () => {
    const wx = live({ sfi: 120, kp: 2, aIndex: 8, fetched: false, sfiAt: null, kpAt: null, xrayAt: null, aDay: null })
    for (const k of ['SFI', 'Kp', 'A', 'X-ray']) {
      expect(of(wx, k), k).toMatchObject({ value: '—', title: t('prop.spaceWx.none') })
      expect(of(wx, k).warn, k).toBeFalsy()
      expect(of(wx, k).day, k).toBeUndefined()
    }
  })

  it('a storm Kp past its cadence keeps its number but loses the warning, and says how old', () => {
    const kpAt = NOW - WX_STALE_SECS.kp - 45 * 60
    const k = of(live({ kp: 6, kpAt }), 'Kp')
    expect(k.value).toBe('6')
    expect(k.warn).toBe(false)
    expect(k.title).toBe(t('prop.spaceWx.stale', { ago: t('prop.opening.ago.hours', { hours: 1 }) }))
    // Positive control: the same storm, fresh, is marked.
    expect(of(live({ kp: 6 }), 'Kp').warn).toBe(true)
  })

  it('a stale or missing number is dimmed, as its gauge is; a fresh one is not', () => {
    const kpAt = NOW - WX_STALE_SECS.kp - 45 * 60
    expect(of(live({ kp: 6, kpAt }), 'Kp')).toMatchObject({ value: '6', dim: true })
    expect(of(live({ xrayAt: NOW - WX_STALE_SECS.xray - 60 }), 'X-ray').dim).toBe(true)
    expect(of(live({ kpAt: null }), 'Kp')).toMatchObject({ value: '—', dim: true })
    // Negative control: every fresh reading is drawn in full ink.
    for (const k of ['SFI', 'Kp', 'A', 'X-ray']) expect(of(live({ kp: 6 }), k).dim, k).toBeFalsy()
  })

  it('the A is the daily planetary A, dated, never the ap estimate', () => {
    expect(of(live(), 'A')).toMatchObject({ value: '4', day: dayLabel(DAY) })
    expect(of(live({ aDay: null }), 'A').value).toBe('—')
  })
})
