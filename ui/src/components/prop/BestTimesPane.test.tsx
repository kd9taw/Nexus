// @vitest-environment jsdom
//
// The Best times box: asking for a call answers it with its basis; a country-centre answer after an
// explicit ask looks the call up once and asks again; nothing on it transmits, tunes or works; the
// answer stacks above the chart in a narrow box and sits beside it once the box is wide.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, fireEvent, render, waitFor } from '@testing-library/react'
import type { BestTimesView } from '../../types'

const fx = vi.hoisted(() => ({ view: null as unknown, lookups: 0, asks: 0 }))
vi.mock('../../api', async (importOriginal) => {
  const real = await importOriginal<typeof import('../../api')>()
  return {
    ...real,
    getBestTimes: vi.fn(async (call: string) => {
      fx.asks++
      return { ...(fx.view as BestTimesView), call }
    }),
    qrzLookup: vi.fn(async () => {
      fx.lookups++
      return { grid: null }
    }),
  }
})

import { BestTimesPane } from './BestTimesPane'
import { askBestTimes, resetBestTimesAsk } from '../../features/bestTimesAsk'

const NOW = Math.floor(Date.now() / 3_600_000) * 3600

function answerView(over: Partial<BestTimesView> = {}): BestTimesView {
  return {
    state: 'answer',
    call: 'EA8ABC',
    entity: 'Canary Islands',
    basis: { kind: 'grid', from: 'heard', grid: 'IL18' },
    km: 5000,
    bearing: 105,
    engine: 'p533',
    power: { watts: 100, source: 'default', amplifier: false },
    answer: {
      engine: 'p533',
      mode: 'FT8',
      perMode: true,
      now: { kind: 'try', band: '20m', word: 'Good', untilUnix: NOW + 3 * 3600, allDay: false },
      windows: [{ band: '15m', fromUnix: NOW + 5 * 3600, toUnix: NOW + 6 * 3600, word: 'Fair', greyline: false, longShot: false }],
      week: { flags: [] },
      chart: [
        { band: '20m', licensed: true, cells: Array.from({ length: 24 }, () => ({ word: 'Good' as const, route: 'short' as const })) },
        { band: '15m', licensed: false, cells: Array.from({ length: 24 }, () => ({ word: 'Fair' as const })) },
      ],
      hours: [],
      nowHour: new Date().getUTCHours(),
      longUsed: false,
    },
    ...over,
  }
}

function ask(c: ReturnType<typeof render>, call: string) {
  const input = c.getByRole('textbox', { name: 'Callsign or grid square' })
  fireEvent.change(input, { target: { value: call } })
  fireEvent.click(c.getByRole('button', { name: 'Best times' }))
}

beforeEach(() => {
  fx.view = answerView()
  fx.lookups = 0
  fx.asks = 0
  localStorage.clear()
  resetBestTimesAsk()
})
afterEach(() => vi.unstubAllGlobals())

describe('the Best times box', () => {
  it('says what it is for until a call is asked, then answers with the basis', async () => {
    const c = render(<BestTimesPane units="metric" />)
    expect(c.container.textContent).toMatch(/Type a call to see when/)
    ask(c, 'ea8abc')
    await waitFor(() => expect(c.container.textContent).toMatch(/Now: try 20m/))
    expect(c.container.textContent).toMatch(/Worked out from grid IL18, sent on the air\./)
    expect(c.container.textContent).toMatch(/A path forecast, not a promise they are on the air\./)
    expect(c.container.textContent).not.toMatch(/%/)
    // The chart's hovers say words, never figures; the unlicensed band is marked.
    const titles = [...c.container.querySelectorAll('.pd-cell')].map((e) => e.getAttribute('title') ?? '')
    expect(titles.length).toBe(48)
    expect(titles.every((x) => !/\d+\s*%/.test(x))).toBe(true)
    expect(titles.some((x) => /not on your licence/.test(x))).toBe(true)
  })

  it('has no control that transmits, tunes or works the station', async () => {
    const c = render(<BestTimesPane units="metric" />)
    ask(c, 'EA8ABC')
    await waitFor(() => expect(c.container.textContent).toMatch(/Now: try/))
    const words = [...c.container.querySelectorAll('button')].map((b) => (b.textContent ?? '').trim())
    for (const w of words) expect(w).toMatch(/^(Best times|FT8|FT4|CW|SSB|Look up|See full day|Use)$/)
    expect(c.container.querySelector('[data-tx], .tx-dock, .stop-tx')).toBeNull()
  })

  it('looks a country-centre answer up once after an explicit ask, never on its own', async () => {
    fx.view = answerView({ basis: { kind: 'entity' } })
    const c = render(<BestTimesPane units="metric" />)
    // A call remembered from before is answered without a lookup: nobody asked this time.
    c.unmount()
    localStorage.setItem('nexus.bestTimes.call', 'EA8ABC')
    const d = render(<BestTimesPane units="metric" />)
    await waitFor(() => expect(d.container.textContent).toMatch(/Rough\./))
    expect(fx.lookups).toBe(0)
    // The operator presses Best times: one lookup, then the answer is asked again.
    const before = fx.asks
    ask(d, 'EA8ABC')
    await waitFor(() => expect(fx.lookups).toBe(1))
    await waitFor(() => expect(fx.asks).toBeGreaterThan(before))
    await waitFor(() => expect(d.container.textContent).toMatch(/The callbook did not place them/))
    // Pressing again asks the callbook nothing more.
    ask(d, 'EA8ABC')
    await act(async () => {})
    expect(fx.lookups).toBe(1)
  })

  it('takes a call handed over by a call menu', async () => {
    const c = render(<BestTimesPane units="metric" />)
    act(() => askBestTimes('k1abc'))
    await waitFor(() => expect(c.container.querySelector('.bt-call')?.textContent).toBe('K1ABC'))
  })

  it('stacks the answer above the chart when narrow, and sets them side by side when wide', async () => {
    let width = 400
    let fire: () => void = () => {}
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(cb: () => void) {
          fire = cb
        }
        observe() {}
        disconnect() {}
      },
    )
    vi.stubGlobal('requestAnimationFrame', (f: () => void) => {
      f()
      return 0
    })
    const spy = vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockImplementation(() => width)
    const c = render(<BestTimesPane units="metric" />)
    const root = c.container.querySelector('.best-times')!
    expect(root.classList.contains('wide')).toBe(false)
    width = 700
    act(() => fire())
    expect(root.classList.contains('wide')).toBe(true)
    // Hidden (0 wide): keeps what it had, never flips to narrow.
    width = 0
    act(() => fire())
    expect(root.classList.contains('wide')).toBe(true)
    spy.mockRestore()
  })
})
