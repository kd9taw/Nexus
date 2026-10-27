// @vitest-environment jsdom
//
// THE SPOTS VIEW POPS OUT, AS THE NEEDED VIEW DOES: "⧉ Pop out" in the view's head opens the Spots board in
// a window of its own (`openPanelWindow('spots')`, the `?panel=spots` window DetachedPanel draws).
//
// Only the view carries it. Every other host of the board — the Spots box on Conditions and in the rail,
// and the Phone and CW cockpits' Spots panes — is lent App's one `spotsBoard` object, and the Pop out is
// given at the view's own call site, never in that object, as the Needed view's is: Needed's boxes and
// panes show none either. The Conditions box stands for all of them here, because it is lent the same
// object. THE REAL App is mounted; only the backend and the waterfall canvas are stubbed.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor, fireEvent } from '@testing-library/react'
import type { AppSnapshot, SpotRow } from './types'
import { t } from './i18n'

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

const SPOTS = [
  {
    call: 'K1CW', entity: 'United States', zone: 5, state: null, band: '20m', freqMhz: 14.025, mode: 'CW',
    submode: 'CW', spotter: 'W3LPL', corroborators: [], ageSecs: 30, comment: '', licensed: true, spotterLocal: true,
  },
] as unknown as SpotRow[]

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    // A computer running its own station: the backend answers None, and the banner asks once.
    clientStatus: vi.fn(async () => null),
    askLog: vi.fn(async () => {
      throw new Error('no log in this test')
    }),
    getSnapshot: vi.fn(async () => snapshot),
    subscribeSnapshot: vi.fn(() => () => {}),
    selectPeer: vi.fn(async () => snapshot),
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
    getAllSpots: vi.fn(async () => SPOTS),
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
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))

import App from './App'
import * as api from './api'
import { pastTheSwitch } from './components/ConnectView.testkit'

// THE BUDGET: App.connectBoards.test.tsx's reasoning, for a mount of the same real App (a hang still fails).
vi.setConfig({ testTimeout: 30_000 })

beforeEach(() => {
  localStorage.clear()
  pastTheSwitch()
  sessionStorage.clear()
  localStorage.setItem(
    'nexus.features.v1',
    JSON.stringify({ profile: 'custom', enabled: { connect: true, spots: true, cw: true, phone: true } }),
  )
  localStorage.setItem('nexus.connect.autopop', 'off')
  localStorage.setItem('nexus.needed.autopop', 'off')
  // Connect with the Spots box in the left rail.
  localStorage.setItem('nexus.connect.chaseDefault.v1', '1')
  localStorage.setItem(
    'nexus.connect.config',
    JSON.stringify({
      slots: { left1: 'spots', left2: 'bandTiles', right1: 'pota', right2: 'outlook', bottom1: 'openings', bottom2: 'spacewx', bottom3: 'getout' },
      overlays: {},
    }),
  )
  vi.mocked(api.openPanelWindow).mockClear()
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) => ({
    matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
  }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(cleanup)

/** Boot the real App on `view` and wait for the board in `scope` to list the spot. */
async function boardOn(view: 'spots' | 'connect', scope: () => Element | null): Promise<Element> {
  window.location.hash = `#${view}`
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
  await waitFor(() => expect([...(scope()?.querySelectorAll('.sp-row .np-call') ?? [])].map((c) => c.textContent)).toContain('K1CW'))
  return scope()!
}

describe('the Spots view pops out into a window of its own', () => {
  it('shows ⧉ Pop out in its head, and a press opens the Spots window', async () => {
    const view = await boardOn('spots', () => document.querySelector('main.spots-panel'))
    const popOut = view.querySelector('.np-head .np-popout') as HTMLButtonElement | null
    expect(popOut, 'the Spots view shows Pop out').not.toBeNull()
    expect(popOut!.textContent).toBe(t('spots.popOut.label'))
    expect(popOut!.title).toBe(t('spots.popOut.title'))
    fireEvent.click(popOut!)
    expect(api.openPanelWindow).toHaveBeenCalledTimes(1)
    expect(api.openPanelWindow).toHaveBeenCalledWith('spots')
  })

  it('the board lent to a box shows none: the Spots box on Conditions', async () => {
    const spotsBox = await boardOn('connect', () => document.querySelector('.pane-frame[data-pane="spots"]'))
    expect(spotsBox.querySelector('.np-popout'), 'a box of the board offers no Pop out').toBeNull()
  })
})
