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
import { render, cleanup, waitFor, act, screen } from '@testing-library/react'
import type { AppSnapshot, NeedAlert } from './types'
import { ASK_AGAIN_AFTER_MS, ASK_AGAIN_TIMES, NOT_ANSWERED } from './features/notAnswered'

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
import { askLog, getNeedAlerts } from './api'

// THE BUDGET (2026-10-04). The App this file mounts is real work, and it scales with the CPU a test gets: the
// slowest test takes 0.52 s on a quiet box, 2.4–2.5 s with a fifth of a CPU and 4.5–4.6 s with a tenth, against
// vitest's 5 s default. 15 s is over twice the tenth; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

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

// On a slow disk the engine refuses the needs until the contact just logged is saved, rather than
// answer without it (features/notAnswered). The board asks again a second later, three times at
// most, so the station just worked leaves it then, not at the next 30 s poll.
describe('on a slow disk, the needs asked after a log ask again until the contact is saved', () => {
  const REFUSED = `${NOT_ANSWERED}: a logbook change is still on its way (0 of 1 saved)`
  /** ZD7AA, needed on 20 m until it is worked. */
  const NEEDED = [{
    call: 'ZD7AA', entity: 'St Helena', band: '20m', zone: 36, tags: ['NewBand'],
    priority: 50, headline: 'New band slot', mode: 'Digital', freqMhz: 14.076, park: null,
  }] as unknown as NeedAlert[]
  const onTheBoard = (call: string) =>
    screen.queryAllByRole('row').some((r) => r.getAttribute('aria-label')?.includes(call))

  /** The Needed board open with ZD7AA on it; then ZD7AA worked and logged by the sequencer, while
   *  the engine refuses the needs `refusals` times before it answers without ZD7AA's need. */
  async function workedOnASlowDisk(refusals: number | 'always') {
    localStorage.removeItem('nexus.features.v1')
    window.location.hash = '#needed'
    // The engine's log questions go unanswered here, as in the board's own tests.
    vi.mocked(askLog).mockRejectedValue(new Error('no log in this test'))
    vi.mocked(getNeedAlerts).mockResolvedValue(NEEDED)
    render(<App />)
    await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
    await waitFor(() => expect(onTheBoard('ZD7AA'), 'premise: ZD7AA is needed').toBe(true))
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
    const needs = vi.mocked(getNeedAlerts).mockReset()
    if (refusals === 'always') needs.mockRejectedValue(REFUSED)
    else {
      for (let i = 0; i < refusals; i++) needs.mockRejectedValueOnce(REFUSED)
      needs.mockResolvedValue([])
    }
    act(() => state.push!({ ...base, loggedTick: 8 }))
    await act(async () => {})
    expect(needReads(), 'asked at once after the log').toBe(1)
    expect(onTheBoard('ZD7AA'), 'a refusal leaves the board as it was').toBe(true)
  }
  afterEach(() => {
    vi.useRealTimers()
    vi.mocked(getNeedAlerts).mockReset()
    vi.mocked(askLog).mockReset()
  })

  it('the station just worked leaves the board a second later, once the engine answers', async () => {
    await workedOnASlowDisk(1)
    await act(() => vi.advanceTimersByTimeAsync(ASK_AGAIN_AFTER_MS))
    expect(onTheBoard('ZD7AA'), 'ZD7AA is still on the board').toBe(false)
    expect(needReads()).toBe(2)
  })

  it(`refused ${1 + ASK_AGAIN_TIMES} times, the board keeps its last answer and asks no more`, async () => {
    await workedOnASlowDisk('always')
    for (let i = 0; i < ASK_AGAIN_TIMES; i++) await act(() => vi.advanceTimersByTimeAsync(ASK_AGAIN_AFTER_MS))
    expect(needReads()).toBe(1 + ASK_AGAIN_TIMES)
    expect(onTheBoard('ZD7AA'), 'left for the 30 s poll, as before').toBe(true)
    await act(() => vi.advanceTimersByTimeAsync(20_000))
    expect(needReads(), 'never past its limit').toBe(1 + ASK_AGAIN_TIMES)
  })
})
