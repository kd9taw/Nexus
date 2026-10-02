// @vitest-environment jsdom
//
// THE DASHBOARD BAR — the clock and space-weather line across the top of the Connect pop-out
// (the dashboard window) and the TV page. What must hold:
//   · the station, a UTC clock and a local clock that tick, and the day's indices from the
//     propagation snapshot the surface already polls;
//   · OFFLINE HONESTY: an offline snapshot carries modelled defaults (SFI 120 …) and must never
//     be drawn as numbers, a stale one says how old it is;
//   · the clock is a SEAM — Connect's clock box plugs in through `clock`, and the surface's own
//     controls through `children`;
//   · SSN is NOAA's DAILY count from the daily solar indices (their own fetch, not a snapshot
//     field): the newest day that has one, shown as that day's, and a dash when there is none.
// The stay-behind toggle is the pop-out's, and is tested there (DetachedPanel.dashboard.test.tsx)
// and here for its own half: it shows only where the platform offers it.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import type { DailySolarIndex, DailySolarIndices, PropagationSnapshot } from '../types'

vi.mock('../api', () => ({
  getWindowBehind: vi.fn(),
  setWindowBehind: vi.fn(),
  getSolarIndices: vi.fn(),
}))

import { getSolarIndices, getWindowBehind, setWindowBehind } from '../api'
import { DashboardBar, StayBehindToggle, barIndices } from './DashboardBar'
import { dayLabel } from './prop/SolarTrends'
import { t } from '../i18n'

const WX = {
  sfi: 97.4,
  kp: 2.33,
  aIndex: 7,
  xrayClass: 'B3.1-class',
  flare: false,
  solarWind: { bzNt: -2.1, btNt: 5, speedKms: 421.6, density: 4 },
}

function snapshot(over: Partial<PropagationSnapshot> = {}): PropagationSnapshot {
  return {
    advisory: { headline: '', bands: [], banners: [] },
    openings: [],
    dxpeditions: { workableNow: [], upcoming: [] },
    spaceWx: WX,
    source: 'live',
    asOf: Math.floor(Date.now() / 1000),
    ...over,
  } as unknown as PropagationSnapshot
}

/** The value shown beside an index name, or null when that index is not on the bar. */
function indexValue(name: string): string | null {
  const bar = document.querySelector('.dash-bar') as HTMLElement
  for (const li of bar.querySelectorAll('.dash-index')) {
    if (li.querySelector('.dash-index-k')?.textContent === name) return li.querySelector('.dash-index-v')?.textContent ?? ''
  }
  return null
}

/** The date drawn after an index's value, or null when it carries none. */
function indexDay(name: string): string | null {
  const bar = document.querySelector('.dash-bar') as HTMLElement
  for (const li of bar.querySelectorAll('.dash-index')) {
    if (li.querySelector('.dash-index-k')?.textContent === name) return li.querySelector('.dash-index-d')?.textContent ?? null
  }
  return null
}

const indexItem = (name: string) =>
  [...document.querySelectorAll('.dash-index')].find((li) => li.querySelector('.dash-index-k')?.textContent === name)

const two = (n: number) => String(n).padStart(2, '0')

const DAY = 86_400
/** 00:00 UTC on the day the fake clock below stands on (29 Sep 2026). */
const TODAY = Date.UTC(2026, 8, 29) / 1000

/** NOAA's daily file as the command serves it: `n` days, oldest first, the newest `newestAgo`
 *  days before today (1 = yesterday, SWPC's normal); `at(i)` overrides day i's values. */
function daily(n: number, at: (i: number) => Partial<DailySolarIndex> = () => ({}), newestAgo = 1): DailySolarIndices {
  return {
    days: Array.from({ length: n }, (_, i) => ({
      dayUnix: TODAY - (newestAgo + n - 1 - i) * DAY,
      sfi: 100 + i,
      ssn: 40 + i,
      ...at(i),
    })),
  }
}

beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(new Date(Date.UTC(2026, 8, 29, 17, 32, 10)))
  // Unless a test hands the bar a file, the daily indices are still on their way: nothing settles.
  vi.mocked(getSolarIndices).mockImplementation(() => new Promise<DailySolarIndices>(() => {}))
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
  vi.mocked(getWindowBehind).mockReset()
  vi.mocked(setWindowBehind).mockReset()
  vi.mocked(getSolarIndices).mockReset()
})

describe('the bar', () => {
  it('shows the station, a big UTC clock and the local time, both to the second', () => {
    render(<DashboardBar call="KD9TAW" grid="EN52" prop={snapshot()} />)
    const bar = screen.getByRole('banner', { name: 'Clock and space weather' })
    expect(within(bar).getByText('KD9TAW')).toBeTruthy()
    expect(within(bar).getByText('EN52')).toBeTruthy()
    expect(bar.querySelector('.dash-utc .dash-time-v')?.textContent).toBe('17:32:10')
    expect(bar.querySelector('.dash-utc .dash-time-k')?.textContent).toBe('UTC')
    // Local is this computer's zone, whatever the test runs in — computed, not assumed.
    const now = new Date()
    expect(bar.querySelector('.dash-local .dash-time-v')?.textContent).toBe(
      `${two(now.getHours())}:${two(now.getMinutes())}:${two(now.getSeconds())}`,
    )
    expect(bar.querySelector('.dash-local .dash-time-k')?.textContent).toBe('Local')
  })

  it('the clock ticks', () => {
    render(<DashboardBar call="KD9TAW" grid="EN52" prop={snapshot()} />)
    act(() => {
      vi.advanceTimersByTime(2000)
    })
    expect(document.querySelector('.dash-utc .dash-time-v')?.textContent).toBe('17:32:12')
  })

  it("shows the day's indices, rounded as the Space Wx gauges round them", () => {
    render(<DashboardBar call="KD9TAW" grid="EN52" prop={snapshot()} />)
    expect(indexValue('SFI')).toBe('97')
    expect(indexValue('Kp')).toBe('2')
    expect(indexValue('A')).toBe('7')
    expect(indexValue('X-ray')).toBe('B3.1')
    expect(indexValue('SW')).toBe('422')
    // Live is the normal state: no chip for it.
    expect(document.querySelector('.dash-prov')).toBeNull()
  })

  it('every index says what it means for HF on hover, from the same words the gauges use', () => {
    render(<DashboardBar call="KD9TAW" grid="EN52" prop={snapshot()} />)
    const sfi = [...document.querySelectorAll('.dash-index')].find((li) => li.textContent?.startsWith('SFI'))!
    // SFI 97 is under the gauges' 100 line: their word for it is low flux.
    expect(sfi.getAttribute('title')).toBe('low flux — high bands sluggish')
  })

  it('OFFLINE: the modelled defaults are never drawn as numbers, and the bar says there is no live data', async () => {
    // The offline snapshot is non-null and carries modelled values; SFI 120 is one of them.
    // NOAA's daily file is here with a count in it, and SSN is a dash all the same: offline, the
    // Space Wx box shows neither its gauges nor its lines, and the bar says what the box says.
    vi.mocked(getSolarIndices).mockResolvedValue(daily(30, (i) => (i === 29 ? { ssn: 46 } : {})))
    render(<DashboardBar call="KD9TAW" grid="EN52" prop={snapshot({ source: 'offline', spaceWx: { ...WX, sfi: 120 } })} />)
    await act(async () => {})
    expect(indexValue('SFI'), 'control: the index is on the bar').not.toBeNull()
    for (const k of ['SFI', 'Kp', 'SSN', 'A', 'X-ray', 'SW']) expect(indexValue(k), k).toBe('—')
    expect(indexDay('SSN'), 'no date for a count that is not shown').toBeNull()
    expect(document.querySelector('.dash-prov')?.textContent).toBe('NO LIVE DATA')
    expect(document.querySelector('.dash-bar')?.textContent).not.toContain('120')
  })

  it('a stale snapshot says how old it is; a partial one says so', () => {
    const { rerender } = render(
      <DashboardBar call="KD9TAW" grid="EN52" prop={snapshot({ source: 'cached', asOf: Math.floor(Date.now() / 1000) - 12 * 60 })} />,
    )
    expect(document.querySelector('.dash-prov')?.textContent).toBe('CACHED 12m')
    expect(indexValue('SFI'), 'cached numbers are still the last real ones').toBe('97')
    rerender(<DashboardBar call="KD9TAW" grid="EN52" prop={snapshot({ source: 'partial' })} />)
    expect(document.querySelector('.dash-prov')?.textContent).toBe('PARTIAL')
  })

  it('no solar-wind reading: SW shows a dash, the rest still show', () => {
    render(<DashboardBar call="KD9TAW" grid="EN52" prop={snapshot({ spaceWx: { ...WX, solarWind: null } })} />)
    expect(indexValue('SW')).toBe('—')
    expect(indexValue('SFI')).toBe('97')
  })

  it('DSCOVR’s magnetometer answered but its plasma feed did not: speed 0 is no reading, so SW is a dash, never "0"', () => {
    // propagation::solar_wind::assemble keeps Bz and fills speed and density with 0 when the plasma
    // file is missing (pinned there by assemble_survives_missing_plasma). The solar wind is never
    // 0 km/s, so a 0 is the feed's absence, and the bar must not print it as a reading.
    const noPlasma = { ...WX, solarWind: { bzNt: -6, btNt: 7, speedKms: 0, density: 0 } }
    render(<DashboardBar call="KD9TAW" grid="EN52" prop={snapshot({ spaceWx: noPlasma })} />)
    expect(indexValue('SW')).toBe('—')
    expect(indexValue('SFI'), 'control: the other indices still show').toBe('97')
  })

  it('SSN is NOAA’s daily count: the newest day that has one, shown as that day’s, between Kp and A', async () => {
    const f = daily(30, (i) => (i === 29 ? { ssn: 46 } : {}))
    vi.mocked(getSolarIndices).mockResolvedValue(f)
    render(<DashboardBar call="KD9TAW" grid="EN52" prop={snapshot()} />)
    await act(async () => {})
    expect(indexValue('SSN')).toBe('46')
    expect(indexDay('SSN')).toBe(dayLabel(f.days[29].dayUnix))
    expect([...document.querySelectorAll('.dash-index-k')].map((k) => k.textContent)).toEqual(['SFI', 'Kp', 'SSN', 'A', 'X-ray', 'SW'])
    // The hover names the file it is from, in the Space Wx box's own words for it.
    expect(indexItem('SSN')?.getAttribute('title')).toBe(
      t('connect.solar.caption', { from: dayLabel(f.days[0].dayUnix), to: dayLabel(f.days[29].dayUnix) }),
    )
    // Control: SSN is the one index with a day of its own; the rest are the snapshot's.
    expect(indexDay('SFI')).toBeNull()
  })

  it('a newest day with no count shows the last day that has one, dated as THAT day', async () => {
    const f = daily(30, (i) => (i === 29 ? { ssn: null } : i === 28 ? { ssn: 44 } : {}))
    vi.mocked(getSolarIndices).mockResolvedValue(f)
    render(<DashboardBar call="KD9TAW" grid="EN52" prop={snapshot()} />)
    await act(async () => {})
    expect(indexValue('SSN')).toBe('44')
    expect(indexDay('SSN')).toBe(dayLabel(f.days[28].dayUnix))
  })

  it('a file that has stopped arriving still shows its last count, dated, and says so on hover', async () => {
    const f = daily(30, () => ({}), 5)
    vi.mocked(getSolarIndices).mockResolvedValue(f)
    render(<DashboardBar call="KD9TAW" grid="EN52" prop={snapshot()} />)
    await act(async () => {})
    expect(indexValue('SSN')).toBe('69')
    expect(indexDay('SSN')).toBe(dayLabel(f.days[29].dayUnix))
    expect(indexItem('SSN')?.getAttribute('title')).toBe(t('connect.solar.stale', { date: dayLabel(f.days[29].dayUnix) }))
  })

  it('no file (never arrived), no count in it, or a failed fetch: SSN is a dash with no date, and the rest still show', async () => {
    const served = [
      () => Promise.resolve(daily(0)),
      () => Promise.resolve(daily(30, () => ({ ssn: null }))),
      () => Promise.reject(new Error('no station')),
    ]
    for (const serve of served) {
      vi.mocked(getSolarIndices).mockImplementation(serve)
      render(<DashboardBar call="KD9TAW" grid="EN52" prop={snapshot()} />)
      await act(async () => {})
      expect(indexValue('SSN')).toBe('—')
      expect(indexDay('SSN')).toBeNull()
      expect(indexValue('SFI'), 'control: the other indices still show').toBe('97')
      cleanup()
    }
  })

  it('before the first snapshot, and on one that carries no space weather, nothing is invented and nothing throws', () => {
    const { rerender } = render(<DashboardBar call="" grid="" prop={null} />)
    expect(indexValue('SFI')).toBe('—')
    expect(document.querySelector('.dash-call')?.textContent).toBe('—')
    expect(document.querySelector('.dash-prov'), 'waiting is not a claim about the data').toBeNull()
    rerender(<DashboardBar call="KD9TAW" grid="EN52" prop={snapshot({ spaceWx: undefined })} />)
    expect(indexValue('SFI')).toBe('—')
  })

  it('THE CLOCK SEAM: a clock handed in replaces the built-in one, and the surface’s own controls come last', () => {
    render(
      <DashboardBar call="KD9TAW" grid="EN52" prop={snapshot()} clock={<span data-testid="clock-box">12:00</span>}>
        <button type="button">mine</button>
      </DashboardBar>,
    )
    expect(screen.getByTestId('clock-box')).toBeTruthy()
    expect(document.querySelector('.dash-utc'), 'the built-in clock gives way').toBeNull()
    const bar = document.querySelector('.dash-bar')!
    expect(bar.lastElementChild?.textContent).toBe('mine')
  })
})

// A STORM ON THE BAR (the operator's pick, 2026-09-30: "Warning colour on the bar" — "The bar's Kp,
// X-ray and wind take the Space Wx gauges' warning colour: amber, never the transmit red"). Each index
// is marked exactly when its own gauge is at its warning level, by the gauge's own rule, so the bar and
// the Space Wx box can never disagree about a storm. The colour itself is the contrast test's.
describe('a storm on the bar', () => {
  /** The storm the Connect box tests use (Kp 5, A 18, a 612 km/s wind), with an M-class flare. */
  const STORM = { ...WX, kp: 5, aIndex: 18, xrayClass: 'M1.2-class', flare: true, solarWind: { bzNt: -6.1, btNt: 9.4, speedKms: 612, density: 7.2 } }
  const marked = () =>
    [...document.querySelectorAll('.dash-index[data-warn]')].map((li) => li.querySelector('.dash-index-k')?.textContent)

  it('Kp, X-ray and the solar wind are marked while their gauges warn, and nothing else is', () => {
    render(<DashboardBar call="KD9TAW" grid="EN52" prop={snapshot({ spaceWx: STORM })} />)
    expect(indexValue('Kp'), 'control: the storm is on the bar').toBe('5')
    expect(marked()).toEqual(['Kp', 'X-ray', 'SW'])
  })

  it('a quiet day marks nothing', () => {
    render(<DashboardBar call="KD9TAW" grid="EN52" prop={snapshot()} />)
    expect(indexValue('Kp'), 'control: the indices are drawn').toBe('2')
    expect(marked()).toEqual([])
  })

  it('each by its own gauge’s rule: Kp 4 is already a warning; a C-class flare and a 599 km/s wind are not', () => {
    const edge = { ...WX, kp: 4, xrayClass: 'C9.9-class', solarWind: { ...WX.solarWind, speedKms: 599 } }
    render(<DashboardBar call="KD9TAW" grid="EN52" prop={snapshot({ spaceWx: edge })} />)
    expect(marked()).toEqual(['Kp'])
    cleanup()
    render(<DashboardBar call="KD9TAW" grid="EN52" prop={snapshot({ spaceWx: { ...edge, kp: 3.9, xrayClass: 'X1.0-class', solarWind: { ...WX.solarWind, speedKms: 600 } } })} />)
    expect(marked()).toEqual(['X-ray', 'SW'])
  })

  it('the A index is not marked: the operator named Kp, X-ray and the wind', () => {
    render(<DashboardBar call="KD9TAW" grid="EN52" prop={snapshot({ spaceWx: { ...WX, aIndex: 40 } })} />)
    expect(indexValue('A'), 'control: a storm-level A is on the bar').toBe('40')
    expect(marked()).toEqual([])
  })

  it('no reading, no mark: a wind the feed did not send, and every index while there is no live data', () => {
    render(<DashboardBar call="KD9TAW" grid="EN52" prop={snapshot({ spaceWx: { ...STORM, solarWind: { ...STORM.solarWind, speedKms: 0 } } })} />)
    expect(indexValue('SW'), 'control: the wind is a dash').toBe('—')
    expect(marked()).toEqual(['Kp', 'X-ray'])
    cleanup()
    render(<DashboardBar call="KD9TAW" grid="EN52" prop={snapshot({ spaceWx: STORM, source: 'offline' })} />)
    expect(indexValue('Kp'), 'control: offline draws dashes').toBe('—')
    expect(marked()).toEqual([])
  })
})

describe('barIndices — the list the bar draws', () => {
  it('is the day’s indices, SFI first and the solar wind last, as a wall-display clock reads them', () => {
    expect(barIndices(WX as never).map((i) => i.key)).toEqual(['SFI', 'Kp', 'SSN', 'A', 'X-ray', 'SW'])
  })
})

describe('the stay-behind toggle', () => {
  it('is not drawn where the platform does not offer it', async () => {
    vi.mocked(getWindowBehind).mockResolvedValue({ supported: false, on: false })
    render(<StayBehindToggle />)
    await act(async () => {})
    expect(vi.mocked(getWindowBehind)).toHaveBeenCalled()
    expect(screen.queryByRole('button', { name: 'Stay behind' })).toBeNull()
  })

  it('where it is offered, shows the window’s state and changes it', async () => {
    vi.mocked(getWindowBehind).mockResolvedValue({ supported: true, on: false })
    vi.mocked(setWindowBehind).mockResolvedValue({ supported: true, on: true })
    render(<StayBehindToggle />)
    await act(async () => {})
    const btn = screen.getByRole('button', { name: 'Stay behind' })
    expect(btn.getAttribute('aria-pressed')).toBe('false')
    await act(async () => {
      fireEvent.click(btn)
    })
    expect(vi.mocked(setWindowBehind)).toHaveBeenCalledWith(true)
    expect(btn.getAttribute('aria-pressed')).toBe('true')
  })

  it('a refused change leaves the toggle as the window really is', async () => {
    vi.mocked(getWindowBehind).mockResolvedValue({ supported: true, on: false })
    vi.mocked(setWindowBehind).mockRejectedValue(new Error('no'))
    render(<StayBehindToggle />)
    await act(async () => {})
    const btn = screen.getByRole('button', { name: 'Stay behind' })
    await act(async () => {
      fireEvent.click(btn)
    })
    expect(btn.getAttribute('aria-pressed')).toBe('false')
  })
})
