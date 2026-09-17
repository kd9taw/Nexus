// @vitest-environment jsdom
//
// THE RESCUE IS ONLY REAL IF THE PANEL IS ACTUALLY GIVEN THE NEEDS.
//
// SpotsPanel decides what Hide worked hides, and "a station you still need on this band and mode
// always shows" depends on App handing it the Needed board's alerts. The panel's own tests pass
// that prop directly, which proves the rule and not the wiring — a panel mounted without the prop
// would pass every one of them and hide a needed station in the shipped app. So this mounts the
// REAL App and reads the rows on the screen.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor, screen } from '@testing-library/react'
import type { AppSnapshot, NeedAlert, SpotRow } from './types'

const snapshot = {
  mycall: 'KD9TAW', mygrid: 'EN52', mode: 'Normal',
  radio: {
    dialMhz: 14.074, band: '20m', catOk: true, sideband: 'USB', transmitting: false,
    txEnabled: false, txAllowed: true, rxOffsetHz: 1500, txOffsetHz: 1500, txLevel: 0.5, slot: 0,
  },
  aiCw: { enabled: false, status: '', text: '' },
  link: { tier: 'FT8', periodSecs: 15, snrDb: -8, dtSec: 0.1, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
  stations: [], conversations: [], activePeer: null, qso: null, fieldDay: null,
  recentDecodes: [], harqRescues: 0, logTick: 1,
} as unknown as AppSnapshot

const state = vi.hoisted(() => ({ needs: [] as unknown[] }))

const worked = {
  call: 'W6A', entity: 'United States', zone: 5, state: null, band: '20m', freqMhz: 14.025,
  mode: 'CW', submode: 'CW', spotter: 'W3LPL', corroborators: [], ageSecs: 30, comment: '',
  licensed: true, spotterLocal: true, workedAgoSecs: 600, workedTodayUtc: true,
} as unknown as SpotRow
const fresh = { ...worked, call: 'K1ABC', workedAgoSecs: undefined, workedTodayUtc: false } as SpotRow
const needOn = (band: string): NeedAlert =>
  ({
    call: 'W6A', entity: 'United States', band, zone: 5, tags: ['NewBand'], priority: 50,
    headline: 'New band slot', mode: 'CW', freqMhz: 14.025,
  }) as NeedAlert

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
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
    getAllSpots: vi.fn(async () => [worked, fresh]),
    getNeedAlerts: vi.fn(async () => state.needs),
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

beforeEach(() => {
  localStorage.clear()
  sessionStorage.clear()
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { spots: true } }))
  localStorage.setItem('nexus.needed.autopop', 'off')
  state.needs = []
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) => ({
    matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
  }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(cleanup)

async function spots(): Promise<void> {
  window.location.hash = '#spots'
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
  await waitFor(() => expect(screen.queryByText('K1ABC')).toBeTruthy())
}

describe('Hide worked in the shipped app', () => {
  it('hides a station worked today', async () => {
    await spots()
    expect(screen.queryByText('W6A')).toBeNull()
  })

  it('keeps it when the Needed board still wants it on the band it is spotted on', async () => {
    state.needs = [needOn('20m')]
    await spots()
    expect(screen.queryByText('W6A')).toBeTruthy()
  })

  it('and a need on ANOTHER band does not rescue it', async () => {
    state.needs = [needOn('40m')]
    await spots()
    expect(screen.queryByText('W6A')).toBeNull()
  })
})
