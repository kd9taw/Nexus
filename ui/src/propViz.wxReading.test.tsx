// @vitest-environment jsdom
//
// The one "is this real, and how old is it" rule every space-weather surface reads (propViz
// `wxReading` / `wxUsable` / `snapXray` / `xrayFastFresh`), and the surfaces built straight on it
// that have no gauge of their own: the Connect Basic line, the scales chips, the flare toast.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, render } from '@testing-library/react'
import type { NoaaScalesView, SpaceWxView } from './types'
import type { PaneContext } from './components/connect/paneContext'
import { kpImpact, snapXray, WX_STALE_SECS, wxReading, wxUsable, xrayFastFresh } from './propViz'
import { flareHafMhz, flareRecoveryMin } from './mapGeo'
import { spaceWxLine } from './components/connect/paneFormat'
import { ScalesAnnunciator } from './components/prop/ScalesAnnunciator'
import { t } from './i18n'

vi.mock('./toast', () => ({ pushToast: vi.fn() }))
vi.mock('./alerts', () => ({ doubleBeep: vi.fn() }))
import { pushToast } from './toast'
import { processFlare, resetFlareAlerts } from './flareAlert'

const NOW_MS = Date.UTC(2026, 9, 30, 2, 46, 0)
const NOW = NOW_MS / 1000
beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(NOW_MS)
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

const live = (over: Partial<SpaceWxView> = {}): SpaceWxView => ({
  sfi: 89,
  kp: 0.67,
  aIndex: 4,
  xrayClass: 'M2-class',
  flare: true,
  xrayLong: 2e-5,
  fetched: true,
  sfiAt: NOW - 7 * 3600,
  kpAt: NOW - 6 * 60,
  xrayAt: NOW - 4 * 60,
  aDay: NOW - 27 * 3600,
  ...over,
})

describe('wxReading', () => {
  it('a null stamp is a stand-in; a stamp is a reading with its age', () => {
    expect(wxReading(live({ kpAt: null }), 'kp', NOW_MS)).toEqual({ real: false, at: null, ageSecs: null, stale: false })
    expect(wxReading(live(), 'kp', NOW_MS)).toEqual({ real: true, at: NOW - 360, ageSecs: 360, stale: false })
  })
  it('stale exactly at the cadence, not one second before', () => {
    expect(wxReading(live({ xrayAt: NOW - WX_STALE_SECS.xray }), 'xray', NOW_MS).stale).toBe(true)
    expect(wxReading(live({ xrayAt: NOW - WX_STALE_SECS.xray + 1 }), 'xray', NOW_MS).stale).toBe(false)
  })
  it("an older station's unstamped numbers read as they always did", () => {
    const old = { sfi: 97, kp: 2, aIndex: 7, xrayClass: 'B5', flare: false } as SpaceWxView
    expect(wxUsable(old, 'kp', NOW_MS)).toBe(true)
  })
})

describe('the X-ray inputs to the flare watcher and the map', () => {
  it('a stand-in or old snapshot flux is no flux', () => {
    expect(snapXray(live(), NOW_MS)).toBe(2e-5)
    expect(snapXray(live({ xrayAt: null }), NOW_MS)).toBeNull()
    expect(snapXray(live({ xrayAt: NOW - 3600 }), NOW_MS)).toBeNull()
    expect(snapXray(null, NOW_MS)).toBeNull()
  })
  it("the fast lane's GOES sample time gates it: a gap is not a flare now", () => {
    expect(xrayFastFresh({ flux: 2e-5, asOf: NOW - 120 }, NOW_MS)).toBe(2e-5)
    expect(xrayFastFresh({ flux: 2e-5, asOf: NOW - 2 * 3600 }, NOW_MS)).toBeNull()
    expect(xrayFastFresh(null, NOW_MS)).toBeNull()
  })
})

describe('the Connect Basic line', () => {
  const ctx = (wx: SpaceWxView) => ({ prop: { source: 'partial', spaceWx: wx }, scales: null }) as unknown as PaneContext
  it('states no stand-in: a partial snapshot without a measured SFI or Kp is "unavailable"', () => {
    const stand = live({ sfi: 120, kp: 2, sfiAt: null, kpAt: null, xrayAt: null, fetched: false })
    expect(spaceWxLine(ctx(stand))).toBe(t('connect.basic.spaceWx.unavailable'))
    expect(spaceWxLine(ctx(live({ kpAt: NOW - 3 * 3600 })))).toBe(t('connect.basic.spaceWx.unavailable'))
  })
  it('a measured line still reads, and its flare clause needs a measured flux', () => {
    expect(spaceWxLine(ctx(live()))).toContain('89')
    expect(spaceWxLine(ctx(live()))).toBe(
      t('connect.basic.spaceWx.flare', { sfi: '89', kp: '1', impact: kpImpact(0.67).text, xray: 'M2-class', scale: 0 }),
    )
    expect(spaceWxLine(ctx(live({ xrayAt: null })))).not.toContain('M2')
  })
})

describe('the scales chips', () => {
  const scales = (asOf: number): NoaaScalesView => ({ r: 0, s: 0, g: 0, gTomorrow: 0, asOf }) as NoaaScalesView
  it('an old copy says how old it is; a fresh one does not', () => {
    const old = render(<ScalesAnnunciator scales={scales(NOW - 3 * 3600)} alerts={[]} />)
    expect(old.container.textContent).toContain(t('prop.scales.asOf', { ago: t('prop.opening.ago.hours', { hours: 3 }) }))
    cleanup()
    const fresh = render(<ScalesAnnunciator scales={scales(NOW - 300)} alerts={[]} />)
    expect(fresh.container.textContent).not.toContain(t('prop.scales.asOf', { ago: '' }).trim())
  })

  it('an old copy keeps its letters but not their alarm colour; a fresh one is coloured', () => {
    const storm = (asOf: number): NoaaScalesView => ({ r: 3, s: 1, g: 4, gTomorrow: 0, asOf }) as NoaaScalesView
    const classes = (asOf: number) => {
      const { container } = render(<ScalesAnnunciator scales={storm(asOf)} alerts={[]} />)
      const out = [...container.querySelectorAll('.swsc-chip')].map((c) => [c.textContent, c.className])
      cleanup()
      return out
    }
    expect(classes(NOW - 3 * 3600)).toEqual([
      ['R3', 'swsc-chip swsc-quiet'],
      ['S1', 'swsc-chip swsc-quiet'],
      ['G4', 'swsc-chip swsc-quiet'],
    ])
    expect(classes(NOW - 300)).toEqual([
      ['R3', 'swsc-chip swsc-major'],
      ['S1', 'swsc-chip swsc-minor'],
      ['G4', 'swsc-chip swsc-major'],
    ])
  })
})

describe('the flare toast', () => {
  it('comes from the catalog, with the fade estimate', () => {
    resetFlareAlerts()
    vi.mocked(pushToast).mockClear()
    processFlare(2e-5)
    expect(vi.mocked(pushToast).mock.calls[0][0]).toBe(
      t('prop.flareAlert.fade', {
        cls: 'M2.0',
        r: 1,
        mhz: Math.round(flareHafMhz(2e-5)),
        mins: Math.round(flareRecoveryMin(2e-5)!),
      }),
    )
  })
})
