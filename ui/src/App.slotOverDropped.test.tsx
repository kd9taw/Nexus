// @vitest-environment jsdom
//
// An FT8 or FT4 over the radio keyed only after its period had ended, in the Now-Bar's status lane.
// The station dropped that over and left TX on, however many in a row, so the chip is a warning of its
// own, not the refused key's "PTT REFUSED — TX STOPPED". It goes with the field. Not on the Remote page.
//
// This mounts the REAL App and the real status bus: what is proved is the wiring from the snapshot
// to the screen. The words are pinned in features/slotOverDropped.test.ts.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen, act, waitFor } from '@testing-library/react'
import { subscribeStatus, type StatusItem } from './status'
import type { AppSnapshot, SlotOverDropped } from './types'
import { EN } from './i18n/en'

const base = {
  mycall: 'KD9TAW', mygrid: 'EN52', mode: 'Normal',
  radio: {
    dialMhz: 14.074, band: '20m', catOk: true, sideband: 'USB', transmitting: false,
    txEnabled: true, txAllowed: true, rxOffsetHz: 1500, txOffsetHz: 700, txLevel: 0.5, slot: 0,
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
    getKpForecast: vi.fn(async () => ({ points: [] })),
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

// The same budget as App.audioLane.test.tsx, for the same reason: the App this mounts is real work.
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

// 2026-10-15 14:32:05 UTC
const AT = Date.UTC(2026, 9, 15, 14, 32, 5) / 1000

const withDrop = (slotOverDropped: SlotOverDropped | null) =>
  ({ ...base, radio: { ...base.radio, slotOverDropped } }) as AppSnapshot

/** The chip on screen whose headline is `words`. */
const chip = (words: string) => screen.getByText(words).closest('.status-chip')

/** The dropped over's item on the status bus: what the chip's tooltip renders. */
function droppedItem(): StatusItem | undefined {
  let items: StatusItem[] = []
  subscribeStatus((next) => {
    items = next
  })()
  return items.find((it) => it.id === 'slotOverDropped')
}

const DROPPED = EN['shell.lane.slotOverDropped.message']

describe('the Now-Bar for an FT8/FT4 over the radio keyed after its period had ended', () => {
  it('says the over was dropped as a warning of its own, with how late, and goes with the field', async () => {
    await openApp()
    act(() => state.push?.(withDrop({ at: AT, late: '1.2 s', count: 1 })))
    await screen.findByText(DROPPED)
    expect(chip(DROPPED)?.className).toContain('tier-warning')
    expect(chip(DROPPED)?.className).not.toContain('tier-critical')
    expect(screen.queryByText(EN['shell.lane.slotKeyRefused.message'])).toBeNull()
    expect(droppedItem()?.detail).toContain('14:32:05 UTC, 1.2 s after your period had ended')
    // The next over keys in its period: the station clears the field, and the chip goes with it.
    act(() => state.push?.(withDrop(null)))
    await waitFor(() => expect(screen.queryByText(DROPPED)).toBeNull())
    expect(droppedItem()).toBeUndefined()
  })

  it('stays a warning however many in a row: TX is on', async () => {
    await openApp()
    act(() => state.push?.(withDrop({ at: AT, late: '0.3 s', count: 12 })))
    await screen.findByText(DROPPED)
    expect(chip(DROPPED)?.className).toContain('tier-warning')
    expect(screen.queryByText(EN['shell.lane.slotKeyRefused.message'])).toBeNull()
  })

})
