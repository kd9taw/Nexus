// @vitest-environment jsdom
//
// #240 — FIRST LAUNCH: THE NEEDED WINDOW WAITS FOR THE SETUP WIZARD.
//
// Operator mail: on a fresh install the Needed board popped out into its own window as soon as
// the first snapshot landed, on top of the setup wizard, and the new operator could not get at
// the wizard. The auto-pop effect in App.tsx knew nothing about the wizard.
//
// Mounts the REAL App (the App.js8workspace.test.tsx pattern) because the defect is an ordering
// between two effects in it — a source grep cannot see an order.
//
// The control matters as much as the fix: a later launch (features persisted, wizard seen) must
// still auto-pop the board, or "never opens" would pass the first case.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor, fireEvent } from '@testing-library/react'
import type { AppSnapshot } from './types'
import defaultSettings from './components/__fixtures__/defaultSettings.json'

const snapshot = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  mode: 'Normal',
  radio: {
    dialMhz: 14.074,
    band: '20m',
    catOk: true,
    sideband: 'USB',
    transmitting: false,
    txEnabled: false,
    txAllowed: true,
    rxOffsetHz: 1500,
    txOffsetHz: 1500,
    txLevel: 0.5,
    slot: 0,
  },
  aiCw: { enabled: false, status: '', text: '' },
  link: { tier: 'FT8', periodSecs: 15, snrDb: -8, dtSec: 0.1, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
  stations: [],
  conversations: [],
  activePeer: null,
  qso: null,
  fieldDay: null,
  recentDecodes: [],
  harqRescues: 0,
} as unknown as AppSnapshot

// Settings are needed only by the dismissal case: App renders the wizard as
// `showWizard && settings`, so without them there is no wizard to dismiss.
let settingsAnswer: unknown = null

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    getSnapshot: vi.fn(async () => snapshot),
    subscribeSnapshot: vi.fn(() => () => {}),
    getAwards: vi.fn(async () => ({ achievements: [] })),
    getJourney: vi.fn(async () => ({ firsts: [], feats: [], ladders: [] })),
    getSettings: vi.fn(async () => settingsAnswer),
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
    setOperatingMode: vi.fn(async () => snapshot),
    // Escape on the wizard also reaches App's window-level Esc (a stop control), which answers
    // with a SNAPSHOT; the auto-stub's `{}` would be fed straight into setSnap.
    haltTx: vi.fn(async () => snapshot),
    setArea: vi.fn(async () => snapshot),
    appVersion: vi.fn(async () => '0.0.0-test'),
    openPanelWindow: vi.fn(async () => {}),
  }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))

import App from './App'
import { openPanelWindow } from './api'

const neededOpens = () =>
  (openPanelWindow as unknown as ReturnType<typeof vi.fn>).mock.calls.filter((c) => c[0] === 'needed').length

beforeEach(() => {
  localStorage.clear()
  settingsAnswer = null
  ;(openPanelWindow as unknown as ReturnType<typeof vi.fn>).mockClear()
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) =>
    ({
      matches: false,
      media: q,
      addEventListener() {},
      removeEventListener() {},
      addListener() {},
      removeListener() {},
    }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(cleanup)

async function mount(): Promise<void> {
  window.location.hash = '#operate'
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
  // Give the post-snapshot effects room to run, so a "not opened" is a real negative.
  await new Promise((r) => setTimeout(r, 50))
}

describe('#240 — the Needed window on first launch', () => {
  it('does NOT pop out while the first-run setup wizard is pending', async () => {
    // Fresh install: no persisted feature state, wizard never seen.
    await mount()
    expect(neededOpens(), 'Needed popped out over the first-run wizard').toBe(0)
  })

  it('pops out once the wizard is dismissed', async () => {
    settingsAnswer = defaultSettings
    await mount()
    const dialog = await waitFor(() => {
      const d = document.querySelector('[role="dialog"]')
      expect(d).not.toBeNull()
      return d as HTMLElement
    })
    expect(neededOpens()).toBe(0)
    // ESC = skip-all (SetupWizard's own contract), which is App's handleWizardSkip.
    fireEvent.keyDown(dialog, { key: 'Escape' })
    await waitFor(() => expect(neededOpens()).toBe(1))
  })

  it('control: a later launch (features persisted, wizard seen) still pops it out at once', async () => {
    localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: {} }))
    localStorage.setItem('nexus.features.wizardSeen', '1')
    await mount()
    await waitFor(() => expect(neededOpens()).toBe(1))
  })
})
