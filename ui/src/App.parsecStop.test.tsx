// @vitest-environment jsdom
//
// Parsec presence mode stopped a transmission while the operator was away (operator sign-off,
// 2026-09-27). The station reports it in `snap.parsecPresence` and keeps the report until the
// operator transmits again; the shell's job is to put it in the Now-Bar's status lane — not a
// toast, which nobody was there to read — and to take it down when the report goes. Never on the
// Remote page, whose surface is frozen.
//
// This mounts the REAL App and the real status bus: what is proved is the wiring from the snapshot
// to the screen. The words are the catalog's, and are read from it here.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen, act, waitFor } from '@testing-library/react'
import { t } from './i18n'
import settingsFixture from './components/__fixtures__/defaultSettings.json'
import type { AppSnapshot, ParsecPresence, Settings } from './types'

const base = {
  mycall: 'KD9TAW', mygrid: 'EN52', mode: 'Normal',
  radio: {
    dialMhz: 14.2, band: '20m', catOk: true, sideband: 'USB', transmitting: false,
    txEnabled: false, txAllowed: true, rxOffsetHz: 1500, txOffsetHz: 1500, txLevel: 0.5, slot: 0,
  },
  aiCw: { enabled: false, status: '', text: '' },
  link: { tier: 'FT8', periodSecs: 15, snrDb: -8, dtSec: 0.1, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
  stations: [], conversations: [], activePeer: null, qso: null, fieldDay: null,
  recentDecodes: [], harqRescues: 0, logTick: 1,
} as unknown as AppSnapshot

const state = vi.hoisted(() => ({
  snap: null as unknown,
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

// THE BUDGET (2026-10-04). The App this file mounts is real work, and it scales with the CPU a test gets: the
// slowest test takes 0.54 s on a quiet box, 2.3–2.4 s with a fifth of a CPU and 4.6–4.8 s with a tenth, against
// vitest's 5 s default. 15 s is over twice the tenth; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

beforeEach(() => {
  localStorage.clear()
  sessionStorage.clear()
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { spots: true } }))
  localStorage.setItem('nexus.needed.autopop', 'off')
  state.snap = base
  state.push = null
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) => ({
    matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
  }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

async function openApp() {
  window.location.hash = '#spots'
  render(<App />)
  await waitFor(() => expect(state.push).not.toBeNull())
  await waitFor(() => expect(document.querySelector('.app')).not.toBeNull())
}

const AT = Date.UTC(2026, 8, 27, 14, 32, 5) / 1000
const withPresence = (p: ParsecPresence | null) => ({ ...base, parsecPresence: p }) as AppSnapshot
const stoppedPtt = withPresence({ status: 'notConnected', stoppedAt: AT, stopped: ['ptt'] })
const chip = () => t('shell.lane.parsecStop.message')

describe('the Now-Bar after Parsec presence mode stopped a transmission', () => {
  it('shows the stop in the status lane, and takes it down when the station drops the report', async () => {
    await openApp()
    act(() => state.push?.(stoppedPtt))
    await screen.findByText(chip())
    expect(document.querySelector('.status-lane')?.textContent).toContain(chip())
    // The operator transmitted again: the station stops reporting it.
    act(() => state.push?.(withPresence({ status: 'connected', stoppedAt: null, stopped: [] })))
    await waitFor(() => expect(screen.queryByText(chip())).toBeNull())
  })

  it('says nothing while the mode is off or nothing was stopped', async () => {
    await openApp()
    act(() => state.push?.(withPresence(null)))
    act(() => state.push?.(withPresence({ status: 'connected', stoppedAt: null, stopped: [] })))
    act(() => state.push?.({ ...base }))
    expect(screen.queryByText(chip())).toBeNull()
    // The control: the same mounted App, handed a stop, does say it.
    act(() => state.push?.(stoppedPtt))
    await screen.findByText(chip())
  })

  it('stays off the Remote page', async () => {
    state.snap = stoppedPtt
    const page = render(
      <App remote={{ snapshot: stoppedPtt, settings: settingsFixture as unknown as Settings, bandPlan: [], status: <div>Observer</div> }} />,
    )
    await screen.findByText('Observer')
    expect(document.querySelector('.app.remote-workspace'), 'premise: the Remote page mounted').not.toBeNull()
    expect(screen.queryByText(chip())).toBeNull()
    page.unmount()
    // The control: the same snapshot on the station's own screen does raise it.
    await openApp()
    act(() => state.push?.(stoppedPtt))
    await screen.findByText(chip())
  })
})
