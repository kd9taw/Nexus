// @vitest-environment jsdom
//
// THE FLARE TOAST'S GRID AT APP'S TWO CALL SITES. The toast names NOAA's D-RAP only beside NOAA's own,
// fresh flux: `flareToastGrid` (flareAlert.ts) decides that, and its own tests pin it. This mounts the
// REAL App and pins that both callers go through it with the fast lane's answer:
// - the 60 s fast lane (the X-ray feed's watcher), and
// - the propagation snapshot poll (it reads the lane's last answer).
// With the dev override's simulated flux set, neither may hand `processFlare` NOAA's grid (that is
// what put "(NOAA D-RAP)" on simulated flux); without it, both hand over the fresh lane's grid.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor } from '@testing-library/react'
import type { AppSnapshot, DrapGrid, PropagationSnapshot, XrayNow } from './types'

const snapshot = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  mode: 'Normal',
  radio: {
    dialMhz: 14.074, band: '20m', catOk: true, sideband: 'USB', transmitting: false,
    txEnabled: false, txAllowed: true, rxOffsetHz: 1500, txOffsetHz: 1500, txLevel: 0.5, slot: 0,
  },
  aiCw: { enabled: false, status: '', text: '' },
  link: { tier: 'FT8', periodSecs: 15, snrDb: -8, dtSec: 0.1, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
  stations: [], conversations: [], activePeer: null, qso: null, fieldDay: null,
  recentDecodes: [], harqRescues: 0, logTick: 1, hunt: null,
} as unknown as AppSnapshot

// NOAA's grid, by identity: processFlare is a spy here, so its contents never matter.
const GRID: DrapGrid = { validAt: 0, recoveryAt: null, lats: [0], lons: [0], haf: [10] }
// The fast lane's answer: an M5, sampled a minute ago, with NOAA's grid.
let lane: XrayNow
// The snapshot poll answers only when the test says so, after the lane has been heard.
let releaseProp: (p: PropagationSnapshot) => void
let propReply: Promise<PropagationSnapshot>

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    // A computer running its own station: the backend answers None, and the banner asks once. The
    // catch-all's `{}` would read as driving a remote station, asked every half second.
    clientStatus: vi.fn(async () => null),
    askLog: vi.fn(async () => {
      throw new Error('no log in this test')
    }),
    getSnapshot: vi.fn(async () => snapshot),
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
    getOtaSpots: vi.fn(async () => []),
    getActivation: vi.fn(async () => ({ program: null, reference: null, qsoCount: 0 })),
    parksCount: vi.fn(async () => 0),
    huntedParksCount: vi.fn(async () => 0),
    getPropagation: vi.fn(() => propReply),
    getFeedHealth: vi.fn(async () => null),
    getXrayNow: vi.fn(async () => lane),
    getDxpedWindows: vi.fn(async () => []),
    getBandOutlook: vi.fn(async () => ({ bands: [], asOf: 0 })),
    getGettingOut: vi.fn(async () => null),
    getPathOutlook: vi.fn(async () => null),
    getSpaceWxScales: vi.fn(async () => ({ scales: null, alerts: [] })),
    getKc2gMuf: vi.fn(async () => []),
    getAurora: vi.fn(async () => null),
    getDeclination: vi.fn(async () => null),
    getPca: vi.fn(async () => null),
    getSatellites: vi.fn(async () => null),
    getOtaMapSpots: vi.fn(async () => []),
    getPotaActivator: vi.fn(async () => ({ calls: [], parks: [] })),
    getLogStats: vi.fn(async () => null),
    getContests: vi.fn(async () => []),
    getSatSchedule: vi.fn(async () => []),
    getSatTrackStatus: vi.fn(async () => null),
    getIssPass: vi.fn(async () => null),
    getTleStatus: vi.fn(async () => null),
    setOperatingMode: vi.fn(async () => snapshot),
    setArea: vi.fn(async () => snapshot),
    appVersion: vi.fn(async () => '0.0.0-test'),
    openPanelWindow: vi.fn(async () => {}),
  }
})
vi.mock('./announce', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  announce: vi.fn(),
}))
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))
vi.mock('./flareAlert', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  processFlare: vi.fn(),
}))

import App from './App'
import { processFlare } from './flareAlert'
import { pastTheSwitch } from './components/ConnectView.testkit'

// A mount of the whole App takes seconds on a loaded box (App.connectBoards.test.tsx's budget).
const BUDGET = 30_000

beforeEach(() => {
  localStorage.clear()
  pastTheSwitch()
  sessionStorage.clear()
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { spots: true } }))
  localStorage.setItem('nexus.connect.autopop', 'off')
  localStorage.setItem('nexus.needed.autopop', 'off')
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) => ({
    matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
  }) as unknown as MediaQueryList) as typeof window.matchMedia
  lane = { flux: 5e-5, asOf: Date.now() / 1000 - 60, drap: GRID }
  propReply = new Promise((r) => (releaseProp = r))
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

/** A snapshot with no X-ray of its own: the poll's flux is the fast lane's (or the override). */
const PROP = {
  asOf: 0,
  source: 'model',
  advisory: { bands: [] },
  openings: [],
  dxpeditions: { workableNow: [], active: [], upcoming: [] },
  spaceWx: { sfi: 100, kp: 1, aIndex: 5, xrayClass: 'M5.0', flare: true, fetched: true, xrayAt: null },
} as unknown as PropagationSnapshot

/** Mounts App, waits for the fast lane's call, then lets the snapshot poll answer and waits for its
 *  call. Returns the grid each call handed `processFlare`: [fast lane, snapshot poll]. */
async function gridsHanded(): Promise<[unknown, unknown]> {
  window.location.hash = '#spots'
  render(<App />)
  const calls = () => vi.mocked(processFlare).mock.calls
  await waitFor(() => expect(calls().length, 'the fast lane called processFlare').toBe(1), { timeout: 10_000 })
  releaseProp(PROP)
  await waitFor(() => expect(calls().length, 'the snapshot poll called processFlare').toBe(2), { timeout: 10_000 })
  const [lanes, poll] = calls()
  expect(lanes[0], "the fast lane's flux").toBeGreaterThan(0)
  expect(poll[0], "the snapshot poll's flux").toBeGreaterThan(0)
  return [lanes[1], poll[1]]
}

describe("App hands the flare toast NOAA's grid only beside NOAA's own fresh flux", () => {
  it("with the dev override's simulated flux, neither caller hands over NOAA's grid", async () => {
    localStorage.setItem('nexus.dev.xray', '5e-5')
    const [fast, poll] = await gridsHanded()
    expect(fast, 'fast lane').toBeNull()
    expect(poll, 'snapshot poll').toBeNull()
  }, BUDGET)

  it("POSITIVE CONTROL: with the real, fresh flux both callers hand over the lane's grid", async () => {
    const [fast, poll] = await gridsHanded()
    expect(fast, 'fast lane').toBe(GRID)
    expect(poll, 'snapshot poll').toBe(GRID)
  }, BUDGET)

  it("a stale lane sample hands over no grid from either caller", async () => {
    lane = { ...lane, asOf: Date.now() / 1000 - 3600 }
    // No override: staleness alone must withhold the grid. The stale flux is no flux, so this does
    // not go through gridsHanded (it asserts a positive one); the spy still records each call.
    window.location.hash = '#spots'
    render(<App />)
    const calls = () => vi.mocked(processFlare).mock.calls
    await waitFor(() => expect(calls().length, 'the fast lane called processFlare').toBe(1), { timeout: 10_000 })
    releaseProp(PROP)
    await waitFor(() => expect(calls().length, 'the snapshot poll called processFlare').toBe(2), { timeout: 10_000 })
    const [fast, poll] = [calls()[0][1], calls()[1][1]]
    expect(fast, 'fast lane').toBeNull()
    expect(poll, 'snapshot poll').toBeNull()
  }, BUDGET)
})
