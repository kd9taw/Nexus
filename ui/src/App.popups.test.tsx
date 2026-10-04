// @vitest-environment jsdom
//
// #391: A WAY TO TURN OFF THE POP-UPS IN THE BOTTOM-RIGHT CORNER. InsaneSplash, 2026-09-29: "how do
// I turn off the popup notifications in the bottom right of the application?" Settings ▸ Spots &
// Alerts ▸ Alerts ▸ Pop-up notifications, on by default.
//
// Off, the confirmations and the alerts stay out of the corner, and what must never vanish
// silently still pops up: every error, every notice (how a refusal or a change on the transmit
// path, the rig or the log is said), and anything with a button that is a control. The full rule
// is `popsUpWhenOff` (toast.ts) and its own table is in toast.test.ts; this file proves the WIRING,
// from the settings the app loads to what the real toast column draws.
//
// This mounts the REAL App and the real toast column (App.logStoreNotice.test.tsx's harness).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen, act, waitFor } from '@testing-library/react'
import { dismissToast, pushToast, subscribeToasts, type Toast } from './toast'
import settingsFixture from './components/__fixtures__/defaultSettings.json'
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
  recentDecodes: [], harqRescues: 0, logTick: 1,
} as unknown as AppSnapshot

const state = vi.hoisted(() => ({
  snap: null as unknown,
  settings: null as unknown,
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
    getSettings: vi.fn(async () => structuredClone(state.settings)),
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
import { getSettings } from './api'

// THE BUDGET (2026-10-04). The App this file mounts is real work, and it scales with the CPU a test gets: the
// slowest test takes 0.55 s on a quiet box, 2.3–2.4 s with a fifth of a CPU and 4.7–4.9 s with a tenth, against
// vitest's 5 s default. 15 s is over twice the tenth; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

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
  // The toast bus is module state: clear it so no test inherits another's toast.
  for (const toast of toasts) dismissToast(toast.id)
  unsubscribe()
  vi.clearAllMocks()
})

/** Mount the real App with `settings` as the station's, and let them load. */
async function openApp(settings: Record<string, unknown>) {
  state.settings = settings
  window.location.hash = '#spots'
  render(<App />)
  await waitFor(() => expect(state.push).not.toBeNull())
  await waitFor(() => expect(vi.mocked(getSettings)).toHaveBeenCalled())
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}
/** The corner's toast for `text`, if it popped up (Radix may echo it into a live region too). */
const inCorner = (text: string) => screen.queryAllByText(text).length > 0

/** One toast of each kind the rule decides about, raised the way the app raises them. */
function raiseOneOfEach() {
  act(() => {
    pushToast('Logged W1AW', 'success')
    pushToast('W1AW calling you', 'success', 20000, { alert: true, prominent: true, action: () => {}, actionLabel: 'Answer' })
    pushToast('TX locked — outside your privileges', 'info')
    pushToast('Could not write the log', 'error')
    pushToast('DXpedition alarm', 'success', 0, { prominent: true, action: () => {}, actionLabel: 'Stop alarm' })
  })
}

describe('pop-up notifications off in Settings', () => {
  it('keeps the confirmations and the alerts out of the corner, and still pops up what must be seen', async () => {
    await openApp({ ...settingsFixture, popupNotifications: false })
    raiseOneOfEach()
    expect(inCorner('Could not write the log'), 'an error about the log vanished').toBe(true)
    expect(inCorner('TX locked — outside your privileges'), 'a transmit refusal vanished').toBe(true)
    expect(inCorner('DXpedition alarm'), 'an alarm lost its Stop button').toBe(true)
    // Soft, so a red run reports both halves of the switch.
    expect.soft(inCorner('Logged W1AW'), 'a confirmation popped up with pop-ups off').toBe(false)
    expect.soft(inCorner('W1AW calling you'), 'an alert popped up with pop-ups off').toBe(false)
    // Off hides; it does not drop. The bus still carries every toast raised.
    expect(toasts.map((x) => x.message)).toContain('Logged W1AW')
  })

  // THE CONTROL: a settings file from before the switch has no key, and the corner is as it was.
  it('pops everything up from a settings file that predates the switch', async () => {
    const old: Record<string, unknown> = { ...settingsFixture }
    delete old.popupNotifications
    await openApp(old)
    raiseOneOfEach()
    for (const text of ['Logged W1AW', 'W1AW calling you', 'TX locked — outside your privileges', 'Could not write the log', 'DXpedition alarm']) {
      expect(inCorner(text), `${text} did not pop up with the setting absent`).toBe(true)
    }
  })
})
