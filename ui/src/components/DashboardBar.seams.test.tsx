// @vitest-environment jsdom
//
// THE DASHBOARD BAR AND THE SPACE WX BOX NEVER DISAGREE. The bar across the dashboard window and
// the TV page repeats two readings the Space Wx box shows: the solar-wind speed (the box's Wind
// gauge) and the sunspot number (the box's 30-day lines, from NOAA's daily solar indices). The two
// surfaces were built apart, so each reading is held here on both at once, over every way the
// data can arrive: the same data may never be a number on one and a dash on the other, or two
// different numbers.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, render } from '@testing-library/react'
import type { DailySolarIndex, DailySolarIndices, PropagationSnapshot, SpaceWxView } from '../types'

vi.mock('../api', () => ({
  getSolarIndices: vi.fn(),
  getWindowBehind: vi.fn(() => new Promise(() => {})),
  setWindowBehind: vi.fn(),
}))

import { getSolarIndices } from '../api'
import { DashboardBar } from './DashboardBar'
import { SpaceWxGauges } from './prop/SpaceWxGauges'
import { SolarTrends } from './prop/SolarTrends'
import { t } from '../i18n'

afterEach(() => {
  cleanup()
  vi.mocked(getSolarIndices).mockReset()
})

const WX: SpaceWxView = { sfi: 97, kp: 2, aIndex: 7, xrayClass: 'B3.1-class', flare: false, solarWind: null }

const live = (spaceWx: SpaceWxView) =>
  ({
    advisory: { headline: '', bands: [], banners: [] },
    openings: [],
    dxpeditions: { workableNow: [], upcoming: [] },
    spaceWx,
    source: 'live',
    asOf: Math.floor(Date.now() / 1000),
  }) as unknown as PropagationSnapshot

/** What the bar draws for an index: its value and its date (null when it has none). */
function onBar(name: string): { value: string | null; day: string | null } {
  const li = [...document.querySelectorAll('.dash-bar .dash-index')].find((x) => x.querySelector('.dash-index-k')?.textContent === name)
  return { value: li?.querySelector('.dash-index-v')?.textContent ?? null, day: li?.querySelector('.dash-index-d')?.textContent ?? null }
}

describe('the solar-wind speed: one no-data rule for the Wind gauge and the bar', () => {
  const sample = (speedKms: number) => ({ bzNt: -6, btNt: 7, speedKms, density: speedKms > 0 ? 4 : 0 })
  const CASES = [
    { what: 'no sample (DSCOVR unavailable)', solarWind: null, shown: null },
    // propagation::solar_wind::assemble keeps Bz from the magnetometer and fills the speed with 0
    // when the plasma file is missing; the solar wind is never 0 km/s.
    { what: 'the magnetometer answered and the plasma feed did not (speed 0)', solarWind: sample(0), shown: null },
    { what: 'an ordinary wind', solarWind: sample(421.6), shown: '422' },
    { what: 'a fast stream', solarWind: sample(650), shown: '650' },
  ]
  it.each(CASES)('$what', ({ solarWind, shown }) => {
    const wx = { ...WX, solarWind }
    render(<SpaceWxGauges wx={wx} />)
    const gauge = [...document.querySelectorAll('.swx-gauge')].find(
      (g) => g.querySelector('.swx-k')?.textContent === t('prop.spaceWx.wind'),
    )
    const onGauge = gauge ? (gauge.querySelector('.swx-v')?.textContent ?? '') : null
    expect(document.querySelector('.swx-gauge'), 'control: the gauges are drawn').not.toBeNull()
    cleanup()
    vi.mocked(getSolarIndices).mockImplementation(() => new Promise(() => {}))
    render(<DashboardBar call="KD9TAW" grid="EN52" prop={live(wx)} />)
    expect(onGauge, 'the Wind gauge').toBe(shown)
    expect(onBar('SW').value, 'the bar').toBe(shown ?? '—')
  })
})

describe('the sunspot number: the bar shows the count the Space Wx box’s lines show, dated the same day', () => {
  const DAY = 86_400
  const TODAY = Math.floor(Date.now() / 1000 / DAY) * DAY
  const file = (n: number, at: (i: number) => Partial<DailySolarIndex> = () => ({}), newestAgo = 1): DailySolarIndices => ({
    days: Array.from({ length: n }, (_, i) => ({ dayUnix: TODAY - (newestAgo + n - 1 - i) * DAY, sfi: 100 + i, ssn: 40 + i, ...at(i) })),
  })
  const CASES = [
    { what: 'yesterday’s count', f: file(30, (i) => (i === 29 ? { ssn: 46 } : {})), shown: '46' },
    { what: 'a newest day with no count', f: file(30, (i) => (i === 29 ? { ssn: null } : i === 28 ? { ssn: 44 } : {})), shown: '44' },
    { what: 'a file that stopped arriving five days ago', f: file(30, () => ({}), 5), shown: '69' },
    { what: 'a file with no count in it', f: file(30, () => ({ ssn: null })), shown: null },
    { what: 'no file at all', f: file(0), shown: null },
  ]
  it.each(CASES)('$what', async ({ f, shown }) => {
    vi.mocked(getSolarIndices).mockResolvedValue(f)
    const box = render(<SolarTrends />)
    await act(async () => {})
    const row = box.container.querySelector('.swx-trend[data-index="SSN"]')
    const inBox = { value: row?.querySelector('.swx-trend-v')?.textContent ?? null, day: row?.querySelector('.swx-trend-d')?.textContent ?? null }
    cleanup()
    render(<DashboardBar call="KD9TAW" grid="EN52" prop={live(WX)} />)
    await act(async () => {})
    const bar = onBar('SSN')
    expect(inBox.value, 'the Space Wx box').toBe(shown)
    expect(bar.value, 'the bar').toBe(shown ?? '—')
    expect(bar.day, 'the same day on both').toBe(inBox.day)
  })
})
