// @vitest-environment jsdom
//
// THE DASHBOARD BAR — the clock and space-weather line across the top of the Connect pop-out
// (the dashboard window) and the TV page. What must hold:
//   · the station, a UTC clock and a local clock that tick, and the day's indices from the
//     propagation snapshot the surface already polls;
//   · OFFLINE HONESTY: an offline snapshot carries modelled defaults (SFI 120 …) and must never
//     be drawn as numbers, a stale one says how old it is;
//   · the clock is a SEAM — Connect's clock box plugs in through `clock`, and the surface's own
//     controls through `children`.
// The stay-behind toggle is the pop-out's, and is tested there (DetachedPanel.dashboard.test.tsx)
// and here for its own half: it shows only where the platform offers it.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import type { PropagationSnapshot } from '../types'

vi.mock('../api', () => ({
  getWindowBehind: vi.fn(),
  setWindowBehind: vi.fn(),
}))

import { getWindowBehind, setWindowBehind } from '../api'
import { DashboardBar, StayBehindToggle, barIndices } from './DashboardBar'

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

const two = (n: number) => String(n).padStart(2, '0')

beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(new Date(Date.UTC(2026, 8, 29, 17, 32, 10)))
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
  vi.mocked(getWindowBehind).mockReset()
  vi.mocked(setWindowBehind).mockReset()
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

  it('OFFLINE: the modelled defaults are never drawn as numbers, and the bar says there is no live data', () => {
    // The offline snapshot is non-null and carries modelled values; SFI 120 is one of them.
    render(<DashboardBar call="KD9TAW" grid="EN52" prop={snapshot({ source: 'offline', spaceWx: { ...WX, sfi: 120 } })} />)
    expect(indexValue('SFI'), 'control: the index is on the bar').not.toBeNull()
    for (const k of ['SFI', 'Kp', 'A', 'X-ray', 'SW']) expect(indexValue(k), k).toBe('—')
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

describe('barIndices — the list the bar draws', () => {
  it('is the day’s indices, SFI first and the solar wind last, as a wall-display clock reads them', () => {
    expect(barIndices(WX as never).map((i) => i.key)).toEqual(['SFI', 'Kp', 'A', 'X-ray', 'SW'])
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
