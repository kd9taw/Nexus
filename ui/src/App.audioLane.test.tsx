// @vitest-environment jsdom
//
// The station's audio-error line in the Now-Bar's status lane. One line carries several different
// problems, and the station names the kind of each beside its sentence; the chip's headline says
// which, tiered by what it costs, and the sentence stays whole for the tooltip. It used to read
// "RADIO STOPPED", pulsing critical, over every one of them: an operator whose rig refused PTT was
// told the radio had stopped while the dial and CAT both worked.
//
// This mounts the REAL App and the real status bus: what is proved is the wiring from the snapshot
// to the screen. The words are pinned as text (features/audioError.test.ts holds every kind).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen, act, waitFor } from '@testing-library/react'
import settingsFixture from './components/__fixtures__/defaultSettings.json'
import { subscribeStatus, type StatusItem } from './status'
import type { AppSnapshot, AudioErrorKind, Settings } from './types'

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
  cleanup()
  vi.clearAllMocks()
})

async function openApp() {
  window.location.hash = '#spots'
  render(<App />)
  await waitFor(() => expect(state.push).not.toBeNull())
  await waitFor(() => expect(document.querySelector('.app')).not.toBeNull())
}

const PTT = "The rig didn't accept PTT — check your PTT method and CAT/port."
const MONITOR =
  "Headphone monitor is off: the chosen output is the rig's TX device — monitoring it would transmit the received band. Pick a separate headphone or speaker device."

const withLine = (audioError: string | null, audioErrorKind?: AudioErrorKind) =>
  ({ ...base, radio: { ...base.radio, audioError, audioErrorKind } }) as AppSnapshot

/** The chip on screen whose headline is `words`. */
const chip = (words: string) => screen.getByText(words).closest('.status-chip')

/** The audio item on the status bus: what the chip's tooltip renders. */
function audioItem(): StatusItem | undefined {
  let items: StatusItem[] = []
  subscribeStatus((next) => {
    items = next
  })()
  return items.find((it) => it.id === 'audio')
}

describe('the Now-Bar for the station audio-error line', () => {
  it('names a PTT the rig did not accept, critical, keeps the sentence, and goes when the line clears', async () => {
    await openApp()
    act(() => state.push?.(withLine(PTT, 'ptt')))
    await screen.findByText('PTT NOT ACCEPTED')
    expect(screen.queryByText('RADIO STOPPED')).toBeNull()
    expect(chip('PTT NOT ACCEPTED')?.className).toContain('tier-critical')
    expect(audioItem()?.detail).toBe(PTT)
    // The rig keys again: the station clears its line, and the chip goes with it.
    act(() => state.push?.(withLine(null)))
    await waitFor(() => expect(screen.queryByText('PTT NOT ACCEPTED')).toBeNull())
    expect(audioItem()).toBeUndefined()
  })

  it('shows a notice the radio works through as a warning, not a pulsing critical', async () => {
    await openApp()
    act(() => state.push?.(withLine(MONITOR, 'monitor')))
    await screen.findByText('HEADPHONE MONITOR OFF')
    const monitor = chip('HEADPHONE MONITOR OFF')
    expect(monitor?.className).toContain('tier-warning')
    expect(monitor?.className).not.toContain('tier-critical')
    expect(audioItem()?.detail).toBe(MONITOR)
  })

  it('on the Remote page, names the kind, and heads a line from a station too old to name it plainly', async () => {
    const remote = (snapshot: AppSnapshot) => ({
      snapshot,
      settings: settingsFixture as unknown as Settings,
      bandPlan: [],
      status: <div>Observer</div>,
    })
    state.snap = withLine(PTT, 'ptt')
    render(<App remote={remote(withLine(PTT, 'ptt'))} />)
    await screen.findByText('Observer')
    expect(document.querySelector('.app.remote-workspace'), 'premise: the Remote page mounted').not.toBeNull()
    await screen.findByText('PTT NOT ACCEPTED')
    // A station older than the field sends the sentence alone.
    await waitFor(() => expect(state.push).not.toBeNull())
    act(() => state.push?.(withLine(PTT)))
    await screen.findByText('RADIO ALERT')
    expect(chip('RADIO ALERT')?.className).toContain('tier-critical')
    expect(screen.queryByText('PTT NOT ACCEPTED')).toBeNull()
    expect(audioItem()?.detail).toBe(PTT)
  })
})
