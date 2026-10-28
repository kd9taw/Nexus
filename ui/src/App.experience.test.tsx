// @vitest-environment jsdom
//
// The first-run experience question, through the REAL App (the App.neededWizard.test.tsx harness):
// a fresh install is asked, and its answer is written through its own writer and picks the
// starting features; the guide and the first-contact walk follow Finish one after the other. An
// updating station (feature state already stored) is shown no wizard, no question, and nothing is
// written or changed. "Fresh" is the settings file's word (`setupWizardSeen: false`, only where
// there was no settings file), so an existing station whose WebView storage was wiped is never shown
// the wizard either. Re-run from Settings, the wizard keeps the screens on now.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor, fireEvent, screen } from '@testing-library/react'
import type { AppSnapshot } from './types'
import defaultSettings from './components/__fixtures__/defaultSettings.json'
import { EN } from './i18n'

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
    // A computer running its own station: the backend answers None, and the banner asks once. The
    // catch-all's `{}` would read as driving a remote station, asked every half second.
    clientStatus: vi.fn(async () => null),
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
    setLicenseClass: vi.fn(async () => snapshot),
    setExperienceLevel: vi.fn(async () => snapshot),
    markSetupWizardSeen: vi.fn(async () => snapshot),
    // What Settings reads as it opens (the re-run case opens it from there).
    getRigModels: vi.fn(async () => []),
    getAllRigModels: vi.fn(async () => []),
    getSerialPortsDetailed: vi.fn(async () => []),
    getCredentialsStatus: vi.fn(async () => []),
    getConnectionLog: vi.fn(async () => []),
    detectRigs: vi.fn(async () => []),
    getAudioDevices: vi.fn(async () => ({ input: [], output: [] })),
    getCtyStatus: vi.fn(async () => null),
    allTxtLocation: vi.fn(async () => ''),
    diagLogLocation: vi.fn(async () => ''),
    recordingsLocation: vi.fn(async () => ''),
  }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))
// The log form reads the log through a query this harness does not answer; nothing here logs.
vi.mock('./components/LogEntry', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  LogEntry: () => null,
}))

import App from './App'
import { markSetupWizardSeen, setExperienceLevel, setLicenseClass, setSettings } from './api'

vi.setConfig({ testTimeout: 15_000 })

const calls = (f: unknown) => (f as ReturnType<typeof vi.fn>).mock.calls

beforeEach(() => {
  localStorage.clear()
  // A new install: there was no settings file, so the backend says the wizard was never done.
  settingsAnswer = { ...defaultSettings, setupWizardSeen: false }
  vi.clearAllMocks()
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

async function mount(hash = '#operate'): Promise<void> {
  window.location.hash = hash
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
  await new Promise((r) => setTimeout(r, 50))
}

describe('the first-run experience question in the app', () => {
  it('a fresh install is asked; New to the hobby saves the answer, starts the starter set, then guide and walk', async () => {
    await mount()
    await screen.findByRole('heading', { name: EN['setup.about.title'] })
    fireEvent.click(screen.getByRole('button', { name: new RegExp(`^${EN['setup.about.newToHobby']}`) }))
    for (let i = 0; i < 4; i++) fireEvent.click(screen.getByRole('button', { name: /Next/ }))
    fireEvent.click(screen.getByRole('button', { name: /^Finish/ }))
    await waitFor(() => expect(calls(setExperienceLevel)).toEqual([['newToHobby']]))
    // Sequenced after the licence write, as that one is after the draft.
    expect(calls(setLicenseClass).length).toBe(1)
    const stored = JSON.parse(localStorage.getItem('nexus.features.v1') ?? '{}')
    expect(stored.profile).toBe('starter')
    expect(stored.enabled.phone).toBe(false)
    // The guide first; the walk once it closes, never two dialogs at once.
    await screen.findByRole('dialog', { name: EN['gettingStarted.title'] })
    expect(screen.queryByRole('dialog', { name: EN['firstContact.title'] })).toBeNull()
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' })
    await screen.findByRole('dialog', { name: EN['firstContact.title'] })
    fireEvent.click(screen.getByRole('button', { name: EN['firstContact.close'] }))
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    // Done is recorded in the settings file, its one writer.
    expect(calls(markSetupWizardSeen).length).toBe(1)
  })

  it('skipping on a new install records the wizard as done, and answers nothing', async () => {
    await mount()
    await screen.findByRole('heading', { name: EN['setup.about.title'] })
    fireEvent.click(screen.getByRole('button', { name: /set it up myself/ }))
    await waitFor(() => expect(calls(markSetupWizardSeen).length).toBe(1))
    expect(calls(setExperienceLevel)).toEqual([])
    expect(screen.queryByText(EN['setup.about.title'])).toBeNull()
  })

  it('an existing station whose WebView storage was wiped is shown no wizard and asked nothing', async () => {
    // A data reset or reinstall: no browser copy of the features or of "wizard seen". The settings
    // file is there and predates the key, so it is not a new install.
    settingsAnswer = defaultSettings
    expect('setupWizardSeen' in defaultSettings).toBe(false)
    await mount()
    expect(screen.queryByRole('dialog')).toBeNull()
    expect(screen.queryByText(EN['setup.about.title'])).toBeNull()
    expect(calls(setExperienceLevel)).toEqual([])
    expect(calls(setSettings)).toEqual([])
    expect(localStorage.getItem('nexus.features.v1')).toBeNull()
  })

  it('re-run from Settings, Finish keeps the screens the operator has on now', async () => {
    settingsAnswer = { ...defaultSettings, setupWizardSeen: true, experienceLevel: 'newToHobby' }
    // Everything on, by hand: the stored answer's preset (the starter set) would turn screens off.
    const tuned = JSON.stringify({ profile: 'custom', enabled: {} })
    localStorage.setItem('nexus.features.v1', tuned)
    localStorage.setItem('nexus.features.wizardSeen', '1')
    Element.prototype.scrollIntoView = () => {}
    Element.prototype.scrollTo = () => {}
    await mount('#settings')
    fireEvent.click(await screen.findByRole('tab', { name: EN['settings.tabs.appearance'] }))
    fireEvent.click(await screen.findByRole('button', { name: EN['settings.features.rerunWizard'] }))
    await screen.findByRole('heading', { name: EN['setup.station.title'] })
    expect(screen.queryByText(EN['setup.about.title'])).toBeNull()
    for (let i = 0; i < 3; i++) fireEvent.click(screen.getByRole('button', { name: /Next/ }))
    fireEvent.click(screen.getByRole('button', { name: /^Finish/ }))
    await waitFor(() => expect(calls(setLicenseClass).length).toBe(1))
    expect(localStorage.getItem('nexus.features.v1')).toBe(tuned)
    expect(calls(setExperienceLevel)).toEqual([])
  })

  it('Help ▸ Your first FT8 contact says what a double-click does with this station\'s setting', async () => {
    const helpWalk = async () => {
      fireEvent.pointerDown(screen.getByRole('button', { name: EN['topbar.help.label'] }), {
        button: 0,
        ctrlKey: false,
        pointerType: 'mouse',
      })
      fireEvent.click(await screen.findByText(EN['firstContact.title']))
      return screen.findByRole('dialog', { name: EN['firstContact.title'] })
    }
    localStorage.setItem('nexus.features.wizardSeen', '1')
    // "Double-click arms TX" off: the walk must not promise that a double-click turns TX on.
    settingsAnswer = { ...defaultSettings, setupWizardSeen: true, doubleClickSetsTx: false }
    await mount()
    let walk = await helpWalk()
    expect(walk.textContent).toContain('Double-click arms TX')
    expect(walk.textContent).not.toContain('That turns TX on')
    cleanup()
    // Control: on (the default), it says a double-click turns TX on, maybe at once.
    settingsAnswer = { ...defaultSettings, setupWizardSeen: true }
    await mount()
    walk = await helpWalk()
    expect(walk.textContent).toContain('That turns TX on')
    expect(walk.textContent).toContain('right away')
  })

  it('an updating station sees no wizard and no question, and nothing is written or changed', async () => {
    const before = JSON.stringify({ profile: 'custom', enabled: { phone: false, cw: true } })
    localStorage.setItem('nexus.features.v1', before)
    localStorage.setItem('nexus.features.wizardSeen', '1')
    await mount()
    expect(screen.queryByRole('dialog')).toBeNull()
    expect(screen.queryByText(EN['setup.about.title'])).toBeNull()
    expect(calls(setExperienceLevel)).toEqual([])
    expect(calls(setSettings)).toEqual([])
    expect(localStorage.getItem('nexus.features.v1')).toBe(before)
  })
})
