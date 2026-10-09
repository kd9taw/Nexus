// @vitest-environment jsdom
//
// CONNECT AND THE DASHBOARD RAIL ON SCREEN TOGETHER: ONE POLL.
//
// Two consumers of every feed Connect polls — Connect's grid and the rail's column — and each feed is
// asked for once on arrival and once a cycle, never once per surface. "Who hears me" answers a
// DIFFERENT count on every call (1, 2, 3 …): had the rail polled on its own, its Getting Out box and
// Connect's would show two different counts. Both show the one answer.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { act, render } from '@testing-library/react'
import type { GettingOut, PropagationSnapshot } from '../types'

vi.mock('./MapView', () => ({ MapView: () => <div data-testid="map" /> }))
vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  return {
    ...auto,
    getGettingOut: vi.fn(),
    getBandOutlook: vi.fn(async () => ({ bands: [], mufNow: 0 })),
    getSpaceWxScales: vi.fn(async () => ({ scales: { r: 0, s: 0, g: 0 }, alerts: [] })),
    getKc2gMuf: vi.fn(async () => []),
    getXrayNow: vi.fn(async () => ({ flux: 1e-7, asOf: 0 })),
    getDxpedWindows: vi.fn(async () => []),
    getKpForecast: vi.fn(async () => ({ points: [] })),
    getSolarIndices: vi.fn(async () => ({ days: [] })),
    getPathOutlook: vi.fn(async () => null),
    getDxccEntityLocations: vi.fn(async () => []),
  }
})

import * as api from '../api'
import { ConnectView } from './ConnectView'
import { OwnedDashRail } from './DashRail.testkit'
import { pastTheSwitch } from './ConnectView.testkit'

// THE BUDGET (2026-10-09). The slowest case here, "ask for each feed once on arrival and once a cycle, and…", takes
// 0.29 s and 0.31 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const FEEDS = ['getGettingOut', 'getBandOutlook', 'getSpaceWxScales', 'getKc2gMuf', 'getXrayNow', 'getDxpedWindows'] as const
const calls = (name: (typeof FEEDS)[number]) => vi.mocked(api[name] as unknown as ReturnType<typeof vi.fn>).mock.calls.length

/** Each Getting Out box's count, by surface: [Connect's, the rail's]. */
const counts = () =>
  ['.connect', '.dash-rail'].map(
    (sel) => document.querySelector(`${sel} .getout-summary`)?.querySelector('strong')?.textContent ?? null,
  )

beforeEach(() => {
  localStorage.clear()
  pastTheSwitch()
  for (const name of FEEDS) vi.mocked(api[name] as unknown as ReturnType<typeof vi.fn>).mockClear()
  let n = 0
  vi.mocked(api.getGettingOut).mockImplementation(async (): Promise<GettingOut> => {
    n += 1
    return { count: n, maxKm: 1000, reports: [{ call: `K${n}AA`, octant: 'NE', km: 1000, band: '20m', snr: -10 } as never] }
  })
  Object.defineProperty(window, 'innerWidth', { value: 1920, configurable: true })
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})

/** A live snapshot, so both Space Wx boxes draw — and each asks for NOAA's daily file for its lines. */
const LIVE = {
  advisory: { headline: 'Bands are fair', bands: [], banners: [] },
  openings: [],
  dxpeditions: { workableNow: [], upcoming: [] },
  spaceWx: { sfi: 97, kp: 2, aIndex: 7, xrayClass: 'B3.1-class', flare: false, solarWind: null },
  source: 'live',
  asOf: Math.floor(Date.now() / 1000),
} as unknown as PropagationSnapshot

describe('Connect and the dashboard rail on screen together', () => {
  it('ask for each feed once on arrival and once a cycle, and show the same answer', async () => {
    vi.useFakeTimers()
    try {
      render(
        <>
          <ConnectView
            myGrid="EN52"
            theme="dark"
            stations={[]}
            prop={null}
            selectedCall={null}
            onSelectCall={() => {}}
            needByCall={new Map()}
          />
          <OwnedDashRail section="operate" myGrid="EN52" theme="dark" stations={[]} prop={null} needByCall={new Map()} onHide={() => {}} />
        </>,
      )
      await act(async () => {})
      // Both surfaces really show the box: a rail that drew no Getting Out would prove nothing.
      expect(counts(), 'the two Getting Out boxes were told different things').toEqual(['1', '1'])
      for (const name of FEEDS) expect(calls(name), `${name}: asked once per surface`).toBe(1)
      await act(async () => {
        vi.advanceTimersByTime(30_000)
      })
      expect(calls('getGettingOut'), 'the rail polled who hears me on a cycle of its own').toBe(2)
      expect(counts()).toEqual(['2', '2'])
    } finally {
      vi.useRealTimers()
    }
  })

  it('the two Space Wx boxes ask for NOAA’s daily file once between them', async () => {
    vi.mocked(api.getSolarIndices).mockClear()
    render(
      <>
        <ConnectView
          myGrid="EN52"
          theme="dark"
          stations={[]}
          prop={LIVE}
          selectedCall={null}
          onSelectCall={() => {}}
          needByCall={new Map()}
        />
        <OwnedDashRail section="operate" myGrid="EN52" theme="dark" stations={[]} prop={LIVE} needByCall={new Map()} onHide={() => {}} />
      </>,
    )
    await act(async () => {})
    // Both boxes are really there, each drawing its lines' block (the "unavailable" line, from an
    // empty file): two consumers of the one file.
    expect(document.querySelectorAll('.connect [data-pane="spacewx"] .swx-trend-none').length).toBe(1)
    expect(document.querySelectorAll('.dash-rail [data-pane="spacewx"] .swx-trend-none').length).toBe(1)
    expect(vi.mocked(api.getSolarIndices).mock.calls.length, 'each Space Wx box asked for the file').toBe(1)
  })
})
