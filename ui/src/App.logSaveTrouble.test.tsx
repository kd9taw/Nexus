// @vitest-environment jsdom
//
// The logbook database refused a change (C10b). The station keeps the change in memory: it sends
// it again from memory while the refusal can pass (another program holding the database, a full
// or failing disk) and holds it for the quit when it cannot (a change the database refuses for
// what it is). Operator's call, 2026-09-23: "Never silently lose a contact" — so the shell says
// so while it lasts, the way it says the database could not open (App.logStoreNotice.test.tsx):
// one sticky toast when the trouble starts, one more if a change is refused for good, and a
// word when every change is saved again. Never repeated by the next snapshot, never on the
// Remote page.
//
// This mounts the REAL App and the real toast bus: what is proved is the wiring from the
// snapshot to the screen. The words are the catalog's, and are read from it here.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen, act, waitFor } from '@testing-library/react'
import { dismissToast, subscribeToasts, type Toast } from './toast'
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

const REASON = 'logbook database: database or disk is full'

let toasts: Toast[] = []
let unsubscribe: () => void = () => {}

beforeEach(() => {
  localStorage.clear()
  sessionStorage.clear()
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { spots: true } }))
  localStorage.setItem('nexus.needed.autopop', 'off')
  state.snap = base
  state.push = null
  unsubscribe = subscribeToasts((now) => {
    toasts = now
  })
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) => ({
    matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
  }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(() => {
  cleanup()
  for (const toast of toasts) dismissToast(toast.id)
  unsubscribe()
  vi.clearAllMocks()
})

/** Mount the real App on the Spots view and let the first snapshot land. */
async function openApp() {
  window.location.hash = '#spots'
  render(<App />)
  await waitFor(() => expect(state.push).not.toBeNull())
  await waitFor(() => expect(document.querySelector('.app')).not.toBeNull())
}

const retrying = () => t('shell.logSave.retrying', { reason: REASON })
const held = () => t('shell.logSave.held', { reason: REASON })
const saved = () => t('shell.logSave.saved')
/** The toasts on the bus saying `message`. */
const saying = (message: string) => toasts.filter((x) => x.message === message)
const trouble = (retryingN: number, heldN: number) =>
  ({ ...base, logSaveTrouble: { retrying: retryingN, held: heldN, reason: REASON } }) as AppSnapshot

describe('the logbook database refused a change (C10b)', () => {
  it('says so while Nexus sends it again: sticky, with the reason, and once however many snapshots', async () => {
    await openApp()
    act(() => state.push?.(trouble(1, 0)))
    await screen.findByText(retrying())
    expect(retrying(), 'the reason the station gave').toContain(REASON)
    for (let i = 0; i < 3; i++) act(() => state.push?.(trouble(2, 0)))
    expect(saying(retrying()), 'one notice, however many snapshots carry the trouble').toHaveLength(1)
    expect(saying(retrying())[0].kind).toBe('error')
  })

  it('a change refused for good gets its own notice, once', async () => {
    await openApp()
    act(() => state.push?.(trouble(1, 0)))
    await screen.findByText(retrying())
    act(() => state.push?.(trouble(1, 1)))
    await screen.findByText(held())
    act(() => state.push?.(trouble(1, 1)))
    expect(saying(held())).toHaveLength(1)
    expect(saying(retrying()), 'and the first one stays').toHaveLength(1)
  })

  it('when every change is saved again the notice goes, and says so', async () => {
    await openApp()
    act(() => state.push?.(trouble(1, 0)))
    await screen.findByText(retrying())
    act(() => state.push?.({ ...base, logSaveTrouble: null } as AppSnapshot))
    await screen.findByText(saved())
    expect(saying(retrying()), 'the trouble notice is gone').toHaveLength(0)
    expect(saying(saved())[0].kind).toBe('success')
    // More quiet snapshots say nothing more.
    act(() => state.push?.({ ...base, logSaveTrouble: null } as AppSnapshot))
    expect(saying(saved())).toHaveLength(1)
    // …and trouble that starts again is said again.
    act(() => state.push?.(trouble(1, 0)))
    await waitFor(() => expect(saying(retrying())).toHaveLength(1))
  })

  it('says nothing while there is no trouble, or from a station older than the re-send', async () => {
    await openApp()
    act(() => state.push?.({ ...base, logSaveTrouble: null } as AppSnapshot))
    act(() => state.push?.({ ...base }))
    expect(saying(retrying())).toHaveLength(0)
    expect(saying(saved()), 'no "saved again" without trouble first').toHaveLength(0)
    // The control: the same mounted App, handed trouble, does say it.
    act(() => state.push?.(trouble(1, 0)))
    await screen.findByText(retrying())
  })

  it('stays off the Remote page, like the notice that the database could not open', async () => {
    const snapshot = trouble(1, 1)
    state.snap = snapshot
    const page = render(
      <App remote={{ snapshot, settings: settingsFixture as unknown as Settings, bandPlan: [], status: <div>Observer</div> }} />,
    )
    await screen.findByText('Observer')
    expect(document.querySelector('.app.remote-workspace'), 'premise: the Remote page mounted').not.toBeNull()
    expect(saying(retrying())).toHaveLength(0)
    expect(saying(held())).toHaveLength(0)
    page.unmount()
    // The control: the same snapshot on the station's own screen does raise both.
    await openApp()
    await screen.findByText(retrying())
    await screen.findByText(held())
  })
})
