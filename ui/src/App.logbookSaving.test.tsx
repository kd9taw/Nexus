// @vitest-environment jsdom
//
// The quit's logbook dialog is MOUNTED by the desktop App, and only there (SPEC-1's C10). The
// component's own behaviour is pinned in components/LogbookSaving.test.tsx; what is proved here
// is the wiring a unit test cannot see — a host nobody mounts is a dialog nobody ever sees, and
// every test of the host itself stays green.
//
// This mounts the REAL App (App.logStoreNotice.test.tsx's mount), with the desktop event bridge
// stubbed the way Tauri provides it, and delivers the station's events through it.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen, act, waitFor } from '@testing-library/react'
import { t } from './i18n'
import settingsFixture from './components/__fixtures__/defaultSettings.json'
import type { AppSnapshot, Settings } from './types'

const base = {
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

const state = vi.hoisted(() => ({
  snap: null as unknown,
  /** The snapshot subscription App made — how a test delivers the next snapshot. */
  push: null as null | ((s: unknown) => void),
}))

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    getSnapshot: vi.fn(async () => state.snap),
    subscribeSnapshot: vi.fn((next: (s: unknown) => void) => {
      state.push = next
      return () => {}
    }),
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
    getPropagation: vi.fn(async () => null),
    getFeedHealth: vi.fn(async () => null),
    getXrayNow: vi.fn(async () => null),
    getDxpedWindows: vi.fn(async () => []),
    getSatSchedule: vi.fn(async () => []),
    getSatTrackStatus: vi.fn(async () => null),
    getIssPass: vi.fn(async () => null),
    getTleStatus: vi.fn(async () => null),
    setOperatingMode: vi.fn(async () => state.snap),
    setArea: vi.fn(async () => state.snap),
    appVersion: vi.fn(async () => '0.0.0-test'),
    openPanelWindow: vi.fn(async () => {}),
  }
})
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))

import App from './App'

type Handler = (e: { payload: unknown }) => void
let handlers: Record<string, Handler> = {}

beforeEach(() => {
  localStorage.clear()
  sessionStorage.clear()
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { spots: true } }))
  localStorage.setItem('nexus.needed.autopop', 'off')
  state.snap = base
  state.push = null
  handlers = {}
  ;(window as unknown as { __TAURI__?: unknown }).__TAURI__ = {
    event: {
      listen: async (name: string, h: Handler) => {
        handlers[name] = h
        return () => {
          delete handlers[name]
        }
      },
    },
  }
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) => ({
    matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
  }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(() => {
  cleanup()
  delete (window as unknown as { __TAURI__?: unknown }).__TAURI__
  vi.clearAllMocks()
})

/** Mount the real App on the Spots view and let the first snapshot land. */
async function openApp() {
  window.location.hash = '#spots'
  render(<App />)
  await waitFor(() => expect(state.push).not.toBeNull())
  await waitFor(() => expect(document.querySelector('.app')).not.toBeNull())
}

const FAILED = { pending: 2, refused: 0, reason: null, radioLive: false }

describe('the quit dialog is mounted by the desktop App (C10)', () => {
  it('a save the station cannot finish reaches the screen, with both choices', async () => {
    await openApp()
    await waitFor(() => expect(handlers['logbook-save-failed'], 'the App listens for the question').toBeDefined())
    act(() => handlers['logbook-save-failed']!({ payload: FAILED }))
    expect(await screen.findByText(t('quit.logbook.slow.title'))).toBeTruthy()
    expect(screen.getByRole('button', { name: t('quit.logbook.keepTrying') })).toBeTruthy()
    expect(screen.getByRole('button', { name: t('quit.logbook.quitWithout', { count: 2 }) })).toBeTruthy()
  })

  it('stays off the Remote page, which has no quit to report', async () => {
    const snapshot = base as AppSnapshot
    const page = render(
      <App remote={{ snapshot, settings: settingsFixture as unknown as Settings, bandPlan: [], status: <div>Observer</div> }} />,
    )
    await screen.findByText('Observer')
    expect(document.querySelector('.app.remote-workspace'), 'premise: the Remote page mounted').not.toBeNull()
    await act(async () => {})
    expect(handlers['logbook-save-failed'], 'a Remote page does not listen').toBeUndefined()
    page.unmount()
    // The control: the station's own window, with the same bridge, does listen.
    await openApp()
    await waitFor(() => expect(handlers['logbook-save-failed']).toBeDefined())
  })
})
