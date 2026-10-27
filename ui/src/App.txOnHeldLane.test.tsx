// @vitest-environment jsdom
//
// The status lane after a TX On the sign-in hold refused, on a computer driving the station: its
// "or about N min" follows the time the station sends as it counts down, and appears once the
// owner has been told, rather than keeping the words it had when the hold first refused.
//
// This mounts the REAL App and the real status bus: what is proved is the wiring from the snapshot
// to the lane. The words are pinned in features/txOnHeld.test.ts.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, waitFor } from '@testing-library/react'
import { subscribeStatus, type StatusItem } from './status'
import type { AppSnapshot, TxHold } from './types'
import { setRemoteStation } from './features/durableStore'
import { EN } from './i18n/en'

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
    // A computer running its own station: the backend answers None, and the banner asks once. The
    // catch-all's `{}` would read as driving a remote station, asked every half second.
    clientStatus: vi.fn(async () => null),
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

// The same budget as App.parsecStop.test.tsx, for the same reason: the App this mounts is real
// work, and it scales with the CPU a test gets.
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
  setRemoteStation(false)
  cleanup()
  vi.clearAllMocks()
})

async function openApp() {
  window.location.hash = '#spots'
  render(<App />)
  await waitFor(() => expect(state.push).not.toBeNull())
  await waitFor(() => expect(document.querySelector('.app')).not.toBeNull())
}

const held = (txHold: TxHold | null) =>
  ({ ...base, radio: { ...base.radio, txOnHeld: true, txHold } }) as AppSnapshot

/** The lane's item on the status bus. */
function lane(): StatusItem | undefined {
  let items: StatusItem[] = []
  subscribeStatus((next) => {
    items = next
  })()
  return items.find((it) => it.id === 'txOnHeld')
}

const left = (minutes: string) => EN['client.ptt.refusal.onHoldLeft'].replace('{{minutes}}', minutes)

describe('the held lane on a computer driving the station', () => {
  it('follows the time left as it counts down, and appears once the owner is told', async () => {
    setRemoteStation(true)
    await openApp()
    act(() => state.push?.(held({ ownerNotified: false })))
    await waitFor(() => expect(lane()?.detail).toBe(EN['client.ptt.refusal.onHold']))
    act(() => state.push?.(held({ ownerNotified: true, leftSecs: 600 })))
    await waitFor(() => expect(lane()?.detail).toBe(left('10')))
    act(() => state.push?.(held({ ownerNotified: true, leftSecs: 125 })))
    await waitFor(() => expect(lane()?.detail).toBe(left('3')))
    act(() => state.push?.({ ...base } as AppSnapshot))
    await waitFor(() => expect(lane()).toBeUndefined())
  })
})
