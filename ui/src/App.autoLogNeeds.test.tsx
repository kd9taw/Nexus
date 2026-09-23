// @vitest-environment jsdom
//
// #350: a contact the SEQUENCER logs on its own (after RR73/73) must refresh the needs at once.
//
// Only the Log button and the prompt-to-log popup asked for fresh needs after a write, so an
// auto-logged contact kept its pre-QSO need tags — a new grid, a new state — until the 30 s poll
// came round, and "Needed only" / "Hide worked" went on showing the station just worked. The
// engine's `loggedTick` moves on EVERY log path, including the auto-log the frontend never
// initiated (#210); that is the signal this rides.
//
// Mounts the REAL App (the App.pendingLogQueue.test.tsx pattern), because what is under test is
// App's wiring between the snapshot and the needs fetch.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor, act } from '@testing-library/react'
import type { AppSnapshot } from './types'

const base = {
  mycall: 'KD9TAW', mygrid: 'EN52', mode: 'Normal',
  radio: {
    dialMhz: 14.074, band: '20m', catOk: true, sideband: 'USB', transmitting: false,
    txEnabled: false, txAllowed: true, rxOffsetHz: 1500, txOffsetHz: 1500, txLevel: 0.5, slot: 0,
  },
  aiCw: { enabled: false, status: '', text: '' },
  link: { tier: 'FT8', periodSecs: 15, snrDb: -8, dtSec: 0.1, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
  stations: [], conversations: [], activePeer: null, qso: null, fieldDay: null,
  recentDecodes: [], harqRescues: 0, logTick: 1, loggedTick: 7,
} as unknown as AppSnapshot

const state = vi.hoisted(() => ({
  /** The live snapshot subscription App opened — how the engine's 300 ms poll reaches it. */
  push: null as null | ((snap: unknown) => void),
}))

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    getSnapshot: vi.fn(async () => base),
    subscribeSnapshot: vi.fn((fn: (snap: unknown) => void) => {
      state.push = fn
      return () => {
        state.push = null
      }
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
    setOperatingMode: vi.fn(async () => base),
    setArea: vi.fn(async () => base),
    appVersion: vi.fn(async () => '0.0.0-test'),
    openPanelWindow: vi.fn(async () => {}),
  }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
}))
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))

import App from './App'
import { getNeedAlerts } from './api'

const needReads = () => vi.mocked(getNeedAlerts).mock.calls.length

beforeEach(() => {
  localStorage.clear()
  sessionStorage.clear()
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { spots: true } }))
  localStorage.setItem('nexus.needed.autopop', 'off')
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

/** Mounted, the first snapshot adopted, and the mount's own needs read already made. */
async function openApp() {
  window.location.hash = '#spots'
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
  await waitFor(() => expect(needReads()).toBeGreaterThan(0))
  expect(state.push, 'App never subscribed to snapshots').not.toBeNull()
}

describe('a contact logged by the engine itself refreshes the needs (#350)', () => {
  it('reads the needs again as soon as loggedTick moves, without waiting for the poll', async () => {
    await openApp()
    const before = needReads()
    // The sequencer logged a contact after RR73: the next snapshot carries the bumped tick,
    // and nothing in the frontend asked for the write.
    act(() => state.push!({ ...base, loggedTick: 8 }))
    await waitFor(() => expect(needReads()).toBeGreaterThan(before))
  })

  // CONTROL — refetching on every snapshot (every 300 ms) would pass the test above too.
  it('a snapshot that logged nothing does not read them again', async () => {
    await openApp()
    const before = needReads()
    act(() => state.push!({ ...base, loggedTick: 7, logTick: 2 }))
    expect(needReads()).toBe(before)
  })
})
