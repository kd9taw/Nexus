// @vitest-environment jsdom
//
// A COMPUTER THAT ONLY DRIVES A STATION NEVER OPENS THE FIRST-RUN SETUP WIZARD. The wizard sets up
// a station: its call, grid, radio, roster and log. On a computer driving another station (client
// mode) every one of those is the station's, and the wizard's "fresh install" test reads this
// computer's own storage, so a never-used computer opened the wizard over the station's settings and
// failed on the licence step. It is still offered from Settings; it just never opens by itself there.
// CONTROL: the same fresh computer running its own station opens it.
//
// Mounts the REAL App (the App.wizardProveTx.test.tsx pattern), since App decides when it opens.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen, act } from '@testing-library/react'
import type { AppSnapshot } from './types'
import defaultSettings from './components/__fixtures__/defaultSettings.json'

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
} as unknown as AppSnapshot

const driving = vi.hoisted(() => ({ on: false }))
vi.mock('./features/durableStore', async (actual) => ({
  ...(await actual<typeof import('./features/durableStore')>()),
  drivesAnotherStation: () => driving.on,
}))

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
    getSnapshot: vi.fn(async () => snapshot),
    subscribeSnapshot: vi.fn(() => () => {}),
    getAwards: vi.fn(async () => ({ achievements: [] })),
    getJourney: vi.fn(async () => ({ firsts: [], feats: [], ladders: [] })),
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
    haltTx: vi.fn(async () => snapshot),
    setArea: vi.fn(async () => snapshot),
    appVersion: vi.fn(async () => '0.0.0-test'),
    openPanelWindow: vi.fn(async () => {}),
    getSettings: vi.fn(async () => ({ ...(defaultSettings as Record<string, unknown>) })),
  }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))

import App from './App'

vi.setConfig({ testTimeout: 15_000 })

beforeEach(() => {
  localStorage.clear()
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
afterEach(() => {
  cleanup()
  driving.on = false
})

describe('the first-run setup wizard on a fresh computer', () => {
  it('never opens by itself on a computer driving a station', async () => {
    driving.on = true
    window.location.hash = '#operate'
    render(<App />)
    await act(async () => { await new Promise((r) => setTimeout(r, 50)) })
    expect(screen.queryByRole('dialog', { name: 'Set up Nexus' })).toBeNull()
  })

  it('CONTROL: opens on the same fresh computer running its own station', async () => {
    window.location.hash = '#operate'
    render(<App />)
    expect(await screen.findByRole('dialog', { name: 'Set up Nexus' })).not.toBeNull()
  })
})
