// @vitest-environment jsdom
//
// THE STORM TOAST'S KP AT APP'S CALL SITE. capsNow `kpNow` picks NOAA's measured 3-hour Kp while its
// period ended within the last 3 hours, else the 1-minute estimate; its own tests pin that. This mounts
// the REAL App and pins that the propagation poll hands `processStorm` that choice, with the Kp outlook
// App already watches: a recent measured period wins over the snapshot's estimate; with none, the
// estimate (labelled) is what reaches the toast.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor } from '@testing-library/react'
import type { AppSnapshot, DrapGrid, KpForecast, PropagationSnapshot, XrayNow } from './types'

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
// The Kp outlook App watches for its storm toasts.
let forecast: KpForecast
// The snapshot poll answers only when the test says so, after the outlook has been heard.
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
    getKpForecast: vi.fn(async () => forecast),
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
vi.mock('./stormAlert', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  processStorm: vi.fn(),
}))

import App from './App'
import { processStorm } from './stormAlert'
import { getKpForecast } from './api'
import { pastTheSwitch } from './components/ConnectView.testkit'

const BUDGET = 30_000
const H = 3600

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
  lane = { flux: 2e-7, asOf: Date.now() / 1000 - 60, drap: null } as unknown as XrayNow
  forecast = { points: [] } as unknown as KpForecast
  propReply = new Promise((r) => (releaseProp = r))
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

/** A live snapshot whose Kp 6.33 is NOAA's 1-minute estimate, two minutes old. */
const prop = () => {
  const now = Date.now() / 1000
  return {
    asOf: now,
    source: 'live',
    advisory: { bands: [] },
    openings: [],
    dxpeditions: { workableNow: [], active: [], upcoming: [] },
    spaceWx: { sfi: 100, kp: 6.33, aIndex: 5, xrayClass: 'B2.0', flare: false, fetched: true, sfiAt: now - H, kpAt: now - 120, xrayAt: now - 60, aDay: now - 86400 },
  } as unknown as PropagationSnapshot
}

/** Mounts App, waits until the Kp outlook has been asked and answered, then lets the snapshot poll
 *  answer, and returns what the poll handed `processStorm`. */
async function stormHanded(): Promise<unknown> {
  window.location.hash = '#spots'
  render(<App />)
  await waitFor(() => expect(getKpForecast).toHaveBeenCalled(), { timeout: 10_000 })
  await new Promise((r) => setTimeout(r, 50))
  releaseProp(prop())
  const calls = () => vi.mocked(processStorm).mock.calls
  await waitFor(() => expect(calls().length, 'the snapshot poll called processStorm').toBeGreaterThan(0), { timeout: 10_000 })
  return calls()[0][0]
}

describe('App hands the storm toast the Kp that speaks for now, with what it is', () => {
  it('a 3-hour period NOAA measured that ended within 3 hours wins over the estimate', async () => {
    const end = Math.floor(Date.now() / 1000 / (3 * H)) * 3 * H
    forecast = { points: [{ timeUnix: end - 3 * H, kp: 5.67, kind: 'observed', noaaScale: 'G1' }] } as unknown as KpForecast
    expect(await stormHanded()).toEqual({ kp: 5.67, source: 'measured', at: end - 3 * H, until: end })
  }, BUDGET)

  it('CONTROL: with no recent measurement, the usable estimate reaches the toast, labelled as one', async () => {
    const k = (await stormHanded()) as { kp: number; source: string; at: number }
    expect(k).toMatchObject({ kp: 6.33, source: 'estimated' })
    expect(k.at).toBeGreaterThan(0)
  }, BUDGET)
})
