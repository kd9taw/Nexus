// @vitest-environment jsdom
//
// THE DESKTOP SENDS THE STATION ITS WATCH LIST (operator, 2026-09-24: "watched counts as needed").
// The station puts the list's stations first on the Needed board — this window's and every Remote
// browser's — so the main window sends the list on launch and after every edit, identity only, and
// then reads the board again: taking an entry off the list takes its row off at once, not at the
// next 30-second poll. A Remote browser mounts the same App and must send nothing: its watch list is
// its own, and the station's board follows the station's.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor, act } from '@testing-library/react'
import type { AppSnapshot, Settings } from './types'
import defaults from './components/__fixtures__/defaultSettings.json'
import { newWatchFilter, saveWatchlist, WATCHLIST_CHANGED } from './watchlist'

const snapshot = {
  mycall: 'W9XYZ',
  mygrid: 'EN52',
  mode: 'Normal',
  radio: {
    dialMhz: 14.074, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: false,
    txAllowed: true, rxOffsetHz: 1500, txOffsetHz: 1500, txLevel: 0.5, slot: 0,
  },
  aiCw: { enabled: false, status: '', text: '' },
  link: { tier: 'Ft8', periodSecs: 15, snrDb: 0, dtSec: 0, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
  stations: [], conversations: [], activePeer: null, qso: null, fieldDay: null, recentDecodes: [],
  harqRescues: 0, hunt: null, b4MatchMode: false,
} as unknown as AppSnapshot
const settings = defaults as unknown as Settings

/** What reached the station, in order. */
const station = vi.hoisted(() => ({ calls: [] as string[] }))

vi.mock('./features/watchlistFold', () => ({ foldRetiredWantedList: vi.fn(async () => {}) }))
vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    askLog: vi.fn(async () => {
      throw new Error('no log in this test')
    }),
    getSnapshot: vi.fn(async () => snapshot),
    subscribeSnapshot: vi.fn(() => () => {}),
    getAwards: vi.fn(async () => ({ achievements: [] })),
    getJourney: vi.fn(async () => ({ firsts: [], feats: [], ladders: [] })),
    getSettings: vi.fn(async () => settings),
    getBandPlan: vi.fn(async () => []),
    getLicensedBandPlan: vi.fn(async () => []),
    getFdRuleset: vi.fn(async () => null),
    logOperators: vi.fn(async () => []),
    logActivations: vi.fn(async () => []),
    radioLaunchInfo: vi.fn(async () => ({ showPicker: false })),
    uiStateLoad: vi.fn(async () => ({})),
    uiStateSave: vi.fn(async () => ({})),
    getAllSpots: vi.fn(async () => []),
    getNeedAlerts: vi.fn(async () => {
      station.calls.push('board')
      return []
    }),
    setWatchList: vi.fn(async (entries: unknown) => {
      station.calls.push(`list ${JSON.stringify(entries)}`)
      return true
    }),
    getPropagation: vi.fn(async () => null),
    getFeedHealth: vi.fn(async () => null),
    getXrayNow: vi.fn(async () => null),
    getDxpedWindows: vi.fn(async () => []),
    getSatSchedule: vi.fn(async () => []),
    getSatTrackStatus: vi.fn(async () => null),
    getIssPass: vi.fn(async () => null),
    getTleStatus: vi.fn(async () => null),
    setOperatingMode: vi.fn(async () => snapshot),
    setArea: vi.fn(async () => snapshot),
    appVersion: vi.fn(async () => '0.0.0-test'),
  }
})
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))

import App from './App'
import { setWatchList } from './api'

beforeEach(() => {
  localStorage.clear()
  station.calls = []
  vi.mocked(setWatchList).mockClear()
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) =>
    ({ matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(cleanup)

const LAUNCH = [newWatchFilter('call', 'VP8*', { cqOnly: true, minSnr: -10, label: 'Falklands' }), newWatchFilter('grid', 'EM7*')]

describe('the desktop sends the station its watch list', () => {
  it('on launch, identity only, and reads the board after it', async () => {
    saveWatchlist(LAUNCH)
    render(<App />)
    const sent = `list ${JSON.stringify([{ kind: 'call', value: 'VP8*' }, { kind: 'grid', value: 'EM7*' }])}`
    await waitFor(() => expect(station.calls).toContain(sent))
    await waitFor(() => expect(station.calls.slice(station.calls.indexOf(sent))).toContain('board'))
  })

  it('FIX: after an edit — so a removed entry takes its row off at once', async () => {
    saveWatchlist(LAUNCH)
    render(<App />)
    await waitFor(() => expect(setWatchList).toHaveBeenCalled())
    await waitFor(() => expect(station.calls[station.calls.length - 1]).toBe('board'))
    station.calls = []
    act(() => {
      saveWatchlist([LAUNCH[1]])
      window.dispatchEvent(new Event(WATCHLIST_CHANGED))
    })
    const sent = `list ${JSON.stringify([{ kind: 'grid', value: 'EM7*' }])}`
    await waitFor(() => expect(station.calls).toEqual([sent, 'board']))
  })

  it('not from a Remote browser', async () => {
    saveWatchlist(LAUNCH)
    const { container } = render(
      <App remote={{ snapshot, settings, bandPlan: [], status: <div data-testid="remote-status">Observer</div> }} />,
    )
    // Positive control: the Remote App is up.
    await waitFor(() => expect(container.querySelector('[data-testid="remote-status"]')).not.toBeNull())
    await new Promise((r) => setTimeout(r, 50))
    expect(setWatchList).not.toHaveBeenCalled()
  })
})
