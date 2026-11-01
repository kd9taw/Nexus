// @vitest-environment jsdom
//
// THE STORM TOAST'S KP AT APP'S CALL SITE. capsNow `kpAlert` warns on the HIGHER of NOAA's recent
// measured 3-hour Kp and the usable 1-minute estimate, labelled with which; its own tests pin that. This
// mounts the REAL App and pins that the propagation poll hands `processStorm` that choice, with the Kp
// outlook App already watches.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor } from '@testing-library/react'
import type { AppSnapshot, KpForecast, PropagationSnapshot, XrayNow } from './types'

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

// The fast lane's answer (a quiet sun here; the X-ray toast is not this test's).
let lane: XrayNow
// The Kp outlook App watches for its storm toasts.
let forecast: KpForecast
// The outlook answers at once unless a test holds it back.
let forecastReply: Promise<unknown>
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
    getKpForecast: vi.fn(() => forecastReply.then(() => forecast)),
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
  processStormForecast: vi.fn(),
}))

import App from './App'
import { processStorm, processStormForecast } from './stormAlert'
import { auroraDismissal, setAuroraDismissal } from './capsNow'
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
  forecastReply = Promise.resolve()
  setAuroraDismissal(false)
  propReply = new Promise((r) => (releaseProp = r))
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

/** A live snapshot whose Kp 6.33 is NOAA's 1-minute estimate, two minutes old. */
const prop = (kp = 6.33) => {
  const now = Date.now() / 1000
  return {
    asOf: now,
    source: 'live',
    advisory: { bands: [] },
    openings: [],
    dxpeditions: { workableNow: [], active: [], upcoming: [] },
    spaceWx: { sfi: 100, kp, aIndex: 5, xrayClass: 'B2.0', flare: false, fetched: true, sfiAt: now - H, kpAt: now - 120, xrayAt: now - 60, aDay: now - 86400 },
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
  it('a higher 3-hour period NOAA measured within 3 hours warns, labelled measured', async () => {
    const end = Math.floor(Date.now() / 1000 / (3 * H)) * 3 * H
    forecast = { points: [{ timeUnix: end - 3 * H, kp: 6.67, kind: 'observed', noaaScale: 'G2' }] } as unknown as KpForecast
    expect(await stormHanded()).toEqual({ kp: 6.67, source: 'measured', at: end - 3 * H, until: end })
  }, BUDGET)

  it('a lower measured period (Kp 4) never holds back the higher estimate (6.33): it warns, labelled est.', async () => {
    const end = Math.floor(Date.now() / 1000 / (3 * H)) * 3 * H
    forecast = { points: [{ timeUnix: end - 3 * H, kp: 4, kind: 'observed', noaaScale: null }] } as unknown as KpForecast
    expect(await stormHanded()).toMatchObject({ kp: 6.33, source: 'estimated' })
  }, BUDGET)

  it('CONTROL: with no recent measurement, the usable estimate reaches the toast, labelled as one', async () => {
    const k = (await stormHanded()) as { kp: number; source: string; at: number }
    expect(k).toMatchObject({ kp: 6.33, source: 'estimated' })
    expect(k.at).toBeGreaterThan(0)
  }, BUDGET)
})

describe('round 4: App’s storm housekeeping', () => {
  /** Mounts App, lets the snapshot poll answer with an estimate of `kp`, then the held-back Kp outlook
   *  (a measured 4 and a predicted G1), and returns the Kp App handed the forecast heads-up. */
  async function forecastGate(kp: number): Promise<unknown> {
    let releaseFc!: () => void
    forecastReply = new Promise<void>((r) => (releaseFc = r))
    const end = Math.floor(Date.now() / 1000 / (3 * H)) * 3 * H
    forecast = {
      points: [
        { timeUnix: end - 3 * H, kp: 4, kind: 'observed', noaaScale: null },
        { timeUnix: end + 6 * H, kp: 5.33, kind: 'predicted', noaaScale: 'G1' },
      ],
    } as unknown as KpForecast
    window.location.hash = '#spots'
    render(<App />)
    await waitFor(() => expect(getKpForecast).toHaveBeenCalled(), { timeout: 10_000 })
    releaseProp(prop(kp))
    await waitFor(() => expect(vi.mocked(processStorm).mock.calls.length).toBeGreaterThan(0), { timeout: 10_000 })
    releaseFc()
    await waitFor(() => expect(vi.mocked(processStormForecast).mock.calls.length).toBeGreaterThan(0), { timeout: 10_000 })
    const calls = vi.mocked(processStormForecast).mock.calls
    return calls[calls.length - 1][1]
  }

  it('the forecast heads-up is told it is already stormy by a usable estimate of 6.33 (measured only 4)', async () => {
    expect(await forecastGate(6.33)).toBe(6.33)
  }, BUDGET)

  it('CONTROL — an estimate of 3: the heads-up is told the Kp now is 3, so it may fire', async () => {
    expect(await forecastGate(3)).toBe(3)
  }, BUDGET)

  it('the storm ending clears the aurora untick, even with no map on screen', async () => {
    setAuroraDismissal(true)
    window.location.hash = '#spots'
    render(<App />)
    releaseProp(prop(3))
    await waitFor(() => expect(vi.mocked(processStorm).mock.calls.length).toBeGreaterThan(0), { timeout: 10_000 })
    await waitFor(() => expect(auroraDismissal()).toBe(false), { timeout: 5_000 })
  }, BUDGET)

  it('CONTROL — while the storm goes on the untick holds', async () => {
    setAuroraDismissal(true)
    window.location.hash = '#spots'
    render(<App />)
    releaseProp(prop(6.33))
    await waitFor(() => expect(vi.mocked(processStorm).mock.calls.length).toBeGreaterThan(0), { timeout: 10_000 })
    await new Promise((r) => setTimeout(r, 100))
    expect(auroraDismissal()).toBe(true)
  }, BUDGET)
})
