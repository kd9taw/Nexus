// @vitest-environment jsdom
//
// THE OPERATOR'S RTTY F-KEY SETS REACH THE COCKPIT THROUGH APP.
//
// RttyCockpit reads its macro sets from a `macros` prop — App's settings mirror — instead of
// fetching settings of its own, so a cockpit test can only prove the prop is honoured. This
// mounts the REAL App on #rtty: if App stopped passing the mirror down, the cockpit would show
// the built-in sets and the operator's saved keys would look lost, with every cockpit test green.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor } from '@testing-library/react'
import type { AppSnapshot, Settings } from './types'
import defaults from './components/__fixtures__/defaultSettings.json'

const snapshot = {
  mycall: 'W9XYZ',
  mygrid: 'EN52',
  mode: 'Normal',
  radio: {
    dialMhz: 14.083,
    band: '20m',
    catOk: true,
    sideband: 'LSB',
    transmitting: false,
    txEnabled: false,
    txAllowed: true,
    rxOffsetHz: 1500,
    txOffsetHz: 1500,
    txLevel: 0.5,
    slot: 0,
  },
  aiCw: { enabled: false, status: '', text: '' },
  link: { tier: 'Ft8', periodSecs: 15, snrDb: 0, dtSec: 0, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
  stations: [],
  conversations: [],
  activePeer: null,
  qso: null,
  fieldDay: null,
  recentDecodes: [],
  harqRescues: 0,
  hunt: null,
  b4MatchMode: false,
} as unknown as AppSnapshot

const rttyState = {
  armed: true, afcHz: 0, afcLocked: false, text: '', charConf: [], baud: 45.45, shiftHz: 170,
  markHz: 2125, spaceHz: 2295, sending: false, latched: false, backend: 'afsk', keyerError: null,
  auto: false, seqState: 'idle', peer: null, peerExchange: [], heardCq: null,
}

const settings: { current: Settings } = { current: defaults as unknown as Settings }

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    // The engine's log questions go unanswered here, as the whole-log read (answered `{}`) did.
    askLog: vi.fn(async () => {
      throw new Error('no log in this test')
    }),
    getSnapshot: vi.fn(async () => snapshot),
    subscribeSnapshot: vi.fn(() => () => {}),
    getAwards: vi.fn(async () => ({ achievements: [] })),
    getJourney: vi.fn(async () => ({ firsts: [], feats: [], ladders: [] })),
    getSettings: vi.fn(async () => settings.current),
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
    getRttyState: vi.fn(async () => rttyState),
    rttyAutoArm: vi.fn(async () => rttyState),
    getLog: vi.fn(async () => []),
    qrzLookup: vi.fn(async () => null),
    resolveEntity: vi.fn(async () => null),
    lookupPark: vi.fn(async () => null),
    searchParks: vi.fn(async () => []),
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
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { rtty: true } }))
  localStorage.setItem('nexus.workspace', 'dx')
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
  window.location.hash = '#rtty'
})
afterEach(cleanup)

const label = (key: string) =>
  document.querySelector(`.rtty-host [data-rtty-macro="${key}"] .cw-macro-label`)?.textContent ?? null

describe('App hands the RTTY cockpit the saved macro sets', () => {
  it('shows the operator’s saved Contest set, not the built-ins', async () => {
    settings.current = {
      ...(defaults as unknown as Settings),
      macros: {
        ...(defaults as unknown as Settings).macros,
        activeRttyProfile: 'contest',
        rttyProfiles: [{ name: 'contest', macros: [{ key: 'F2', label: 'Run exch', text: '{CALL} 599 05 05' }] }],
      },
    }
    render(<App />)
    await waitFor(() => expect(label('F2')).toBe('Run exch'))
    // The rest of the set is the Contest built-in — the active set came through too.
    expect(label('F3')).toBe('TU')
  })

  it('control: with nothing saved the cockpit shows the Everyday built-ins', async () => {
    settings.current = defaults as unknown as Settings
    render(<App />)
    await waitFor(() => expect(label('F2')).toBe('Answer'))
    expect(label('F3')).toBe('Exchange')
  })
})
