// @vitest-environment jsdom
//
// APP DOES NOT RE-RENDER ON A CLOCK. App ran a 400 ms interval for its whole life, and the interval's
// only job was to make the two unread memos (`unreadByPeer`, `bandUnread`) run again, on the theory
// that they read a ref cursor a dependency change alone would not catch. Every tick re-rendered the
// whole App, cockpits included: 2.5 renders a second with nothing changed, on top of the 300 ms
// snapshot poll, and a stream of updates every App-mounting test had to flush inside `act`.
//
// The theory does not hold. The read cursor moves in one effect only, after a render that a new
// snapshot or a new active peer caused, and only for the active peer (which the badges leave out) and
// for threads that are gone (which have no badge). So no badge can be stale between snapshots. These
// tests count App's renders through a stub of the TopBar, which App renders on every render of its
// own, and check the badges' numbers through a stub of the station list, snapshot by snapshot.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import type { AppSnapshot, ChatMessage, Conversation } from './types'

const base = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  mode: 'Normal',
  radio: {
    dialMhz: 14.078, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: false,
    txAllowed: true, rxOffsetHz: 1500, txOffsetHz: 1500, txLevel: 0.5, slot: 0,
  },
  aiCw: { enabled: false, status: '', text: '' },
  link: { tier: 'TempoFast', periodSecs: 15, snrDb: -8, dtSec: 0.1, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
  stations: [],
  conversations: [] as Conversation[],
  activePeer: null as string | null,
  qso: null,
  fieldDay: null,
  recentDecodes: [],
  harqRescues: 0,
}
const snapshot = (over: Partial<typeof base> = {}) => ({ ...base, ...over }) as unknown as AppSnapshot

/** `n` messages from `peer` to us, the first `outbound` of them ours. */
function thread(peer: string, n: number, outbound = 0): Conversation {
  const messages: ChatMessage[] = []
  for (let i = 0; i < n; i++) {
    const ours = i < outbound
    messages.push({
      from: ours ? 'KD9TAW' : peer, to: ours ? peer : 'KD9TAW', text: `MSG ${i + 1}`, slot: i, directedToMe: !ours,
      outbound: ours, snr: -8, freqHz: 1500, dtSec: 0.1, tier: 'TempoFast',
    } as ChatMessage)
  }
  return { peer, messages }
}

const seen = vi.hoisted(() => ({
  topBar: 0,
  stations: [] as { unread: Record<string, number>; band: number; active: string | null }[],
  push: null as null | ((s: AppSnapshot) => void),
}))

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
    getSnapshot: vi.fn(async () => snapshot()),
    // The live feed: the test hands App each snapshot itself, as the poll would.
    subscribeSnapshot: vi.fn((fn: (s: AppSnapshot) => void) => {
      seen.push = fn
      return () => {
        seen.push = null
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
    setOperatingMode: vi.fn(async () => snapshot()),
    setArea: vi.fn(async () => snapshot()),
    appVersion: vi.fn(async () => '0.0.0-test'),
    // A callsign as the active peer makes the (always mounted) Operate cockpit's recall card look
    // the station up: answer as the backend does when it knows nothing, never with an object.
    resolveEntity: vi.fn(async () => null),
    qrzLookup: vi.fn(async () => null),
  }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))
// App renders the TopBar on every render of its own; the stub has no state or timer of its own (the
// real one keeps a 1 s UTC clock), so its count IS App's render count.
// The module's other exports stay real (the Operate cockpit's QSO strip calls its `modeMismatch`).
vi.mock('./components/TopBar', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  TopBar: () => {
    seen.topBar++
    return <div data-testid="topbar" />
  },
}))
// What App hands the station list: the badges' numbers, as the list would draw them.
vi.mock('./components/StationList', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  StationList: (p: { unreadByPeer: Record<string, number>; bandUnread?: number; activePeer: string | null }) => {
    seen.stations.push({ unread: p.unreadByPeer, band: p.bandUnread ?? 0, active: p.activePeer })
    return <div data-testid="stations" />
  },
}))

import App from './App'

// THE BUDGET (2026-10-04). The App this file mounts is real work, and it scales with the CPU a test gets: the
// slowest test takes 0.50 s on a quiet box, 2.4–4.2 s with a fifth of a CPU and 5.4 s with a tenth, against
// vitest's 5 s default. 15 s is over twice the tenth; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

beforeEach(() => {
  localStorage.clear()
  seen.topBar = 0
  seen.stations = []
  seen.push = null
  // A desktop-width window, as App.tempoRails.test.tsx mounts Tempo: the three-pane layout.
  Object.defineProperty(window, 'innerWidth', { value: 1920, configurable: true, writable: true })
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) =>
    ({ matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

/** Let pending promises (the mocked api's answers) land: real macrotask turns, inside act. */
async function settle(turns = 5) {
  for (let i = 0; i < turns; i++) await act(async () => { await new Promise((r) => setTimeout(r, 5)) })
}

/** Mount App on Tempo ('chat' needs the 'msg' workspace), the view whose station list shows badges. */
async function mountTempo() {
  localStorage.setItem('nexus-ui-scale-mode', '100')
  localStorage.setItem('nexus.workspace', 'msg')
  window.location.hash = '#chat'
  render(<App />)
  const up = () => document.querySelector('.layout[data-three-pane] [data-testid="stations"]') && seen.push
  for (let i = 0; i < 300 && !up(); i++) await settle(1)
  expect(document.querySelector('.layout[data-three-pane]'), 'App is on the Tempo three-pane layout').not.toBeNull()
  expect(document.querySelector('[data-testid="stations"]'), 'the Tempo station list is up').not.toBeNull()
  expect(seen.push, 'App subscribed to snapshots').not.toBeNull()
  await settle()
}

/** Hand App one snapshot, as the 300 ms poll would, and return what the station list was given. */
async function feed(over: Partial<typeof base>) {
  await act(async () => {
    seen.push!(snapshot(over))
  })
  await settle(2)
  return seen.stations[seen.stations.length - 1]
}

describe('App renders when something changes, never on a clock', () => {
  it('does not render again while nothing changes', async () => {
    // Only the intervals are fake, so App's own timers (the old ticker among them) run on test time;
    // the 2 s satellite-pass poll is the shortest of App's other intervals and stays out of the window.
    vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
    await mountTempo()
    const before = seen.topBar
    // One 400 ms step per act, so a render on each step counts as its own.
    for (let step = 0; step < 4; step++) {
      act(() => {
        vi.advanceTimersByTime(400)
      })
    }
    expect(seen.topBar - before, 'App renders in 1.6 s with nothing changed').toBe(0)
  })

  it('renders once for a new snapshot, as before', async () => {
    await mountTempo()
    const before = seen.topBar
    await feed({ conversations: [thread('K1ABC', 1)] })
    expect(seen.topBar - before, 'a new snapshot renders App').toBeGreaterThan(0)
  })
})

describe('the unread badges count live, snapshot by snapshot', () => {
  it('a station you are not talking to gets a badge for each new message, at the snapshot that brings it', async () => {
    await mountTempo()
    expect((await feed({ conversations: [thread('K1ABC', 1)] })).unread).toEqual({ K1ABC: 1 })
    expect((await feed({ conversations: [thread('K1ABC', 3)] })).unread).toEqual({ K1ABC: 3 })
    // Our own messages are never unread.
    expect((await feed({ conversations: [thread('K1ABC', 4, 1)] })).unread).toEqual({ K1ABC: 3 })
  })

  it('the station you are talking to has none, and leaving it starts from what you had read', async () => {
    await mountTempo()
    expect((await feed({ conversations: [thread('K1ABC', 2)], activePeer: 'K1ABC' })).unread).toEqual({})
    // A message arrives while you are in the conversation: you have read it.
    expect((await feed({ conversations: [thread('K1ABC', 3)], activePeer: 'K1ABC' })).unread).toEqual({})
    // You move to another station: K1ABC has nothing unread...
    expect((await feed({ conversations: [thread('K1ABC', 3), thread('W2XYZ', 1)], activePeer: 'W2XYZ' })).unread).toEqual({})
    // ...until it sends again.
    expect((await feed({ conversations: [thread('K1ABC', 4), thread('W2XYZ', 1)], activePeer: 'W2XYZ' })).unread).toEqual({ K1ABC: 1 })
  })

  it('the band row counts the band feed the same way', async () => {
    await mountTempo()
    expect((await feed({ conversations: [thread('*', 2)] })).band).toBe(2)
    expect((await feed({ conversations: [thread('*', 2)], activePeer: '*' })).band).toBe(0)
    expect((await feed({ conversations: [thread('*', 3)], activePeer: 'K1ABC' })).band).toBe(1)
  })
})
