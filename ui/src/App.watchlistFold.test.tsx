// @vitest-environment jsdom
//
// THE RETIRED WANTED LIST FOLDS ON THE DESKTOP, AND ONLY THERE (operator, 2026-09-24: "One list").
//
// `features/watchlistFold` adds the old list's entries to the watch list once; App is what runs
// it, so a fold nobody calls would pass every test of the fold and fold nothing. This mounts the
// REAL App. The desktop (no `remote`) runs it; a Remote browser — which mounts the same App with
// `remote` — must not: the watch list it holds is the browser's own, and the station's old list
// belongs on the station's.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor } from '@testing-library/react'
import type { AppSnapshot, Settings } from './types'
import defaults from './components/__fixtures__/defaultSettings.json'

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
const settings = { ...(defaults as unknown as Settings), wantedCalls: ['VP8*'] } as Settings

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
    getNeedAlerts: vi.fn(async () => []),
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
import { foldRetiredWantedList } from './features/watchlistFold'

beforeEach(() => {
  localStorage.clear()
  vi.mocked(foldRetiredWantedList).mockClear()
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) =>
    ({ matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(cleanup)

describe('App folds the retired wanted list', () => {
  it('on the desktop', async () => {
    render(<App />)
    await waitFor(() => expect(foldRetiredWantedList).toHaveBeenCalled())
  })

  it('not on a Remote browser', async () => {
    const { container } = render(
      <App remote={{ snapshot, settings, bandPlan: [], status: <div data-testid="remote-status">Observer</div> }} />,
    )
    // Positive control: the Remote App is up (the same shell, observing).
    await waitFor(() => expect(container.querySelector('[data-testid="remote-status"]')).not.toBeNull())
    await new Promise((r) => setTimeout(r, 50))
    expect(foldRetiredWantedList).not.toHaveBeenCalled()
  })
})
