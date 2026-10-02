// @vitest-environment jsdom
//
// "(FORMERLY CONNECT)" ONLY WHERE THE OPERATOR PICKED IT (2026-10-02): "Tooltip, window title,
// CHANGELOG and release notes only. … Settings and the website just say "Conditions"."
//
// The window's title and the screen reader's "now on" announcement come from one App effect, over the
// feature's label, so this mounts the REAL App on Conditions and reads both, with the nav button beside
// them: the tooltip and the window's title carry the note; the button, the announcement and the
// feature's label do not (Settings, a crash message and the website's feature list all read that
// label). Spots is the control: its title is its label, as every other screen's is.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor } from '@testing-library/react'
import type { AppSnapshot } from './types'

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

// App.connectBoards.test.tsx's backend: what Conditions and Spots read as they mount, shaped so they
// mount cleanly.
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
    getPropagation: vi.fn(async () => null),
    getFeedHealth: vi.fn(async () => null),
    getXrayNow: vi.fn(async () => null),
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

import App from './App'
import { announce } from './announce'
import { featureById } from './features/registry'
import { pastTheSwitch } from './components/ConnectView.testkit'

// A mount of the whole App takes seconds on a loaded box (App.connectBoards.test.tsx's budget).
const BUDGET = 30_000

beforeEach(() => {
  localStorage.clear()
  pastTheSwitch()
  sessionStorage.clear()
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { connect: true, spots: true } }))
  localStorage.setItem('nexus.connect.autopop', 'off')
  localStorage.setItem('nexus.needed.autopop', 'off')
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) => ({
    matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
  }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

async function boot(view: 'connect' | 'spots') {
  window.location.hash = `#${view}`
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull(), { timeout: 10_000 })
}
/** What the screen reader was told, politely or not, since the mount. */
const said = () => vi.mocked(announce).mock.calls.map(([text]) => String(text))
const navButton = (label: string) =>
  [...document.querySelectorAll<HTMLButtonElement>('.mode-nav button')].find((b) => b.querySelector('.mode-label')?.textContent === label)

describe('Conditions says "(formerly Connect)" on its tooltip and the window title, and nowhere else', () => {
  it("the window's title and the tooltip carry it; the announcement, the button and the feature's label do not", async () => {
    await boot('connect')
    expect(document.title).toBe('Conditions (formerly Connect) — Nexus')
    expect(said(), 'the "now on" announcement').toContain('Conditions')
    expect(said().filter((s) => s.includes('formerly')), 'an announcement with the note').toEqual([])
    const button = navButton('Conditions')
    expect(button, 'the nav button reads Conditions').toBeDefined()
    expect(button!.getAttribute('aria-label')).toMatch(/^Conditions \(formerly Connect\) — /)
    expect(featureById('connect')!.label, 'what Settings, a crash message and the website read').toBe('Conditions')
  }, BUDGET)

  it("control: another screen's title is its label, as before", async () => {
    await boot('spots')
    expect(document.title).toBe(`${featureById('spots')!.label} — Nexus`)
    expect(said()).toContain(featureById('spots')!.label)
  }, BUDGET)
})
