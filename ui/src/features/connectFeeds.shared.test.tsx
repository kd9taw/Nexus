// @vitest-environment jsdom
//
// ONE POLL PER WINDOW, ON THE REAL SURFACES.
//
// connectFeeds.test.ts proves the store's rules with fake feeds. This file proves the surfaces USE
// it, which is the claim the operator gets: two things on screen that show the same feed ask for it
// once a cycle, not once each.
//
//   · Two real ConnectViews in one window. Every feed Connect polls is asked for once on arrival
//     and once per cycle. "Who hears me" answers a DIFFERENT count on every call (1, 2, 3 …), so a
//     second poll cannot hide behind an equal answer: had each surface polled, the two would show
//     two different counts.
//   · The real App on Connect. App polls the X-ray fast lane (its flare watcher) and the DXpedition
//     windows (its chase alerts) for the whole session, and Connect polled both again while it was
//     open — measured on the base tree before this change: two requests each on arrival, four a
//     minute later. One source means one.
// The rail beside a cockpit is the third surface: DashRail.feeds.test.tsx.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { act, render, waitFor } from '@testing-library/react'
import type { GettingOut } from '../types'

vi.mock('../components/MapView', () => ({ MapView: () => <div data-testid="map" /> }))
vi.mock('../components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))

const fx = vi.hoisted(() => {
  const snapshot = {
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    mode: 'Normal',
    radio: {
      dialMhz: 14.074,
      band: '20m',
      catOk: true,
      sideband: 'USB',
      transmitting: false,
      txEnabled: false,
      txAllowed: true,
      rxOffsetHz: 1500,
      txOffsetHz: 1500,
      txLevel: 0.5,
      slot: 0,
    },
    aiCw: { enabled: false, status: '', text: '' },
    link: { tier: 'FT8', periodSecs: 15, snrDb: -8, dtSec: 0.1, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
    stations: [],
    conversations: [],
    activePeer: null,
    qso: null,
    fieldDay: null,
    recentDecodes: [],
    harqRescues: 0,
  }
  return { snapshot }
})

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  return {
    ...auto,
    askLog: vi.fn(async () => {
      throw new Error('no log in this test')
    }),
    getSnapshot: vi.fn(async () => fx.snapshot),
    subscribeSnapshot: vi.fn(() => () => {}),
    getAwards: vi.fn(async () => ({ achievements: [] })),
    getJourney: vi.fn(async () => ({ firsts: [], feats: [], ladders: [] })),
    getSettings: vi.fn(async () => null),
    getBandPlan: vi.fn(async () => []),
    getLicensedBandPlan: vi.fn(async () => []),
    getFdRuleset: vi.fn(async () => null),
    logOperators: vi.fn(async () => []),
    logActivations: vi.fn(async () => []),
    radioLaunchInfo: vi.fn(async () => ({ showPicker: false })),
    uiStateLoad: vi.fn(async () => ({})),
    uiStateSave: vi.fn(async () => ({})),
    getAllSpots: vi.fn(async () => []),
    getNeedAlerts: vi.fn(async () => []),
    getPropagation: vi.fn(async () => null),
    getFeedHealth: vi.fn(async () => null),
    getSatSchedule: vi.fn(async () => []),
    getSatTrackStatus: vi.fn(async () => null),
    getIssPass: vi.fn(async () => null),
    getTleStatus: vi.fn(async () => null),
    setOperatingMode: vi.fn(async () => fx.snapshot),
    setArea: vi.fn(async () => fx.snapshot),
    appVersion: vi.fn(async () => '0.0.0-test'),
    getDxccEntityLocations: vi.fn(async () => []),
    // Connect's feeds — each answer made per call below.
    getGettingOut: vi.fn(),
    getBandOutlook: vi.fn(async () => ({ bands: [], mufNow: 0 })),
    getSpaceWxScales: vi.fn(async () => ({ scales: { r: 0, s: 0, g: 0 }, alerts: [] })),
    getKc2gMuf: vi.fn(async () => []),
    getXrayNow: vi.fn(async () => ({ flux: 1e-7, asOf: 0 })),
    getDxpedWindows: vi.fn(async () => []),
    getKpForecast: vi.fn(async () => ({ points: [] })),
    getSolarIndices: vi.fn(async () => ({ days: [] })),
    getPathOutlook: vi.fn(async () => null),
  }
})
vi.mock('../toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

import * as api from '../api'
import { ConnectView } from '../components/ConnectView'
import App from '../App'

/** "Who hears me" answers 1, 2, 3 … hearing stations, one more on every call. */
function countingGetout(): void {
  let n = 0
  vi.mocked(api.getGettingOut).mockImplementation(async (): Promise<GettingOut> => {
    n += 1
    return { count: n, maxKm: 1000, reports: [{ call: `K${n}AA`, octant: 'NE', km: 1000, band: '20m', snr: -10 } as never] }
  })
}

/** What each Getting Out box says, surface by surface: its count, the summary's emphasised number
 *  (`connect.getout.summary`'s `<b>` tag, which the pane renders as a <strong>). */
const getoutCounts = () =>
  [...document.querySelectorAll('.getout-summary')].map((el) => el.querySelector('strong')?.textContent ?? null)

const CONNECT_FEEDS = ['getGettingOut', 'getBandOutlook', 'getSpaceWxScales', 'getKc2gMuf', 'getXrayNow', 'getDxpedWindows'] as const
const calls = (name: (typeof CONNECT_FEEDS)[number] | 'getXrayNow' | 'getDxpedWindows') =>
  vi.mocked(api[name] as unknown as (...a: unknown[]) => unknown).mock.calls.length

const connectProps = {
  myGrid: 'EN52',
  theme: 'dark' as const,
  stations: [],
  prop: null,
  selectedCall: null,
  onSelectCall: () => {},
  needByCall: new Map(),
}

beforeEach(() => {
  localStorage.clear()
  for (const name of CONNECT_FEEDS) vi.mocked(api[name] as unknown as ReturnType<typeof vi.fn>).mockClear()
  countingGetout()
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) =>
    ({
      matches: false,
      media: q,
      addEventListener() {},
      removeEventListener() {},
      addListener() {},
      removeListener() {},
    }) as unknown as MediaQueryList) as typeof window.matchMedia
})

describe('two Connect surfaces in one window ask for each feed once', () => {
  it('one request per feed on arrival and per cycle, and both surfaces show the same answer', async () => {
    vi.useFakeTimers()
    try {
      render(
        <>
          <ConnectView {...connectProps} />
          <ConnectView {...connectProps} />
        </>,
      )
      await act(async () => {})
      for (const name of CONNECT_FEEDS) expect(calls(name), `${name}: each surface polled it on arrival`).toBe(1)
      expect(getoutCounts(), 'the two surfaces were told different things').toEqual(['1', '1'])
      await act(async () => {
        vi.advanceTimersByTime(30_000)
      })
      expect(calls('getGettingOut'), 'each surface polled who hears me on its own cycle').toBe(2)
      expect(getoutCounts()).toEqual(['2', '2'])
    } finally {
      vi.useRealTimers()
    }
  })
})

describe('App’s alert watchers and Connect share the X-ray lane and the DXpedition windows', () => {
  it('opening Connect asks for neither a second time', async () => {
    localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { connect: true } }))
    localStorage.setItem('nexus.workspace', 'dx')
    window.location.hash = '#connect'
    render(<App />)
    await waitFor(() => expect(document.querySelector('.connect-shell')).not.toBeNull())
    await act(async () => {})
    expect(calls('getXrayNow'), 'App’s flare watcher and Connect’s map each asked for the X-ray lane').toBe(1)
    expect(calls('getDxpedWindows'), 'App’s chase alerts and Connect each asked for the windows').toBe(1)
  })
})

