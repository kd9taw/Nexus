// @vitest-environment jsdom
//
// WHAT THE FIRST-RUN WIZARD WRITES, through the REAL App against a station that keeps a full settings
// file (the App.experience.test.tsx harness, with a backend whose reads answer what it holds now and
// whose writers change it). An untouched wizard writes nothing: re-run from Settings and finished or
// skipped without a change, the stored file is exactly what it was; on a new install the only change
// is the "wizard done" record. A wizard that does change things writes them in order: the station
// and rig draft, then the licence class, then the experience answer. An updating station is shown
// nothing at all.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor, fireEvent, screen } from '@testing-library/react'
import type { AppSnapshot } from './types'
import defaultSettings from './components/__fixtures__/defaultSettings.json'
import { EN } from './i18n'

const snapshot = {
  mycall: 'W1AW',
  mygrid: 'FN31pr',
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

/** A station's whole settings file: the full stored shape, set up the way an operator has it. */
const STATION = {
  ...defaultSettings,
  mycall: 'W1AW',
  mygrid: 'FN31pr',
  licenseClass: 'technician',
  experienceLevel: 'experienced',
  setupWizardSeen: true,
  rigModel: 1042,
  rigModelName: 'Yaesu FTDX10',
  serialPort: 'COM5',
  pttMethod: 'cat',
  audioIn: 'USB Audio CODEC',
  audioOut: 'USB Audio CODEC',
}

const backend = vi.hoisted(() => ({
  stored: {} as Record<string, unknown>,
  writes: [] as string[],
}))

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  const write = (name: string, change: (s: Record<string, unknown>) => void) =>
    vi.fn(async (...args: unknown[]) => {
      backend.writes.push(name)
      change.call(null, backend.stored, ...(args as []))
      return snapshot
    })
  return {
    ...auto,
    clientStatus: vi.fn(async () => null),
    getSnapshot: vi.fn(async () => snapshot),
    subscribeSnapshot: vi.fn(() => () => {}),
    getAwards: vi.fn(async () => ({ achievements: [] })),
    getJourney: vi.fn(async () => ({ firsts: [], feats: [], ladders: [] })),
    getSettings: vi.fn(async () => structuredClone(backend.stored)),
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
    haltTx: vi.fn(async () => snapshot),
    setArea: vi.fn(async () => snapshot),
    appVersion: vi.fn(async () => '0.0.0-test'),
    openPanelWindow: vi.fn(async () => {}),
    // The settings file's writers, each changing what the backend holds.
    setSettings: vi.fn(async (s: Record<string, unknown>) => {
      backend.writes.push('setSettings')
      // As the engine applies a whole-settings save: the "done" record only ever goes from not done
      // to done, whatever a stale copy carries (`apply_settings_inner`).
      const seen = backend.stored.setupWizardSeen === true
      backend.stored = structuredClone(s)
      if (seen) backend.stored.setupWizardSeen = true
      return snapshot
    }),
    setLicenseClass: write('setLicenseClass', (s, c?: unknown) => void (s.licenseClass = c)),
    setExperienceLevel: write('setExperienceLevel', (s, l?: unknown) => void (s.experienceLevel = l)),
    markSetupWizardSeen: write('markSetupWizardSeen', (s) => void (s.setupWizardSeen = true)),
    // What Settings and the rig step read.
    getRigModels: vi.fn(async () => []),
    getAllRigModels: vi.fn(async () => []),
    getSerialPortsDetailed: vi.fn(async () => []),
    getCredentialsStatus: vi.fn(async () => []),
    getConnectionLog: vi.fn(async () => []),
    detectRigs: vi.fn(async () => []),
    discoverFlex: vi.fn(async () => []),
    tciProbe: vi.fn(async () => ({ host: '127.0.0.1', resolved: true, tried: [], found: null })),
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
vi.mock('./components/LogEntry', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  LogEntry: () => null,
}))

import App from './App'

vi.setConfig({ testTimeout: 20_000 })

beforeEach(() => {
  localStorage.clear()
  backend.stored = structuredClone(STATION)
  backend.writes = []
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
  Element.prototype.scrollIntoView = () => {}
  Element.prototype.scrollTo = () => {}
})
afterEach(cleanup)

async function mount(hash = '#operate'): Promise<void> {
  window.location.hash = hash
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
  await new Promise((r) => setTimeout(r, 50))
}
const next = () => fireEvent.click(screen.getByRole('button', { name: /^Next/ }))
const settle = () => new Promise((r) => setTimeout(r, 100))

/** Settings ▸ Appearance ▸ Run the setup again, from a station that has used Nexus. */
async function rerun() {
  localStorage.setItem('nexus.features.wizardSeen', '1')
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'everything', enabled: {} }))
  await mount('#settings')
  fireEvent.click(await screen.findByRole('tab', { name: EN['settings.tabs.appearance'] }))
  fireEvent.click(await screen.findByRole('button', { name: EN['settings.features.rerunWizard'] }))
  await screen.findByRole('heading', { name: EN['setup.station.title'] })
}

describe('an untouched wizard writes nothing', () => {
  it('re-run from Settings and finished without a change: the stored file is exactly as it was', async () => {
    const before = JSON.stringify(backend.stored)
    await rerun()
    for (let i = 0; i < 3; i++) next()
    fireEvent.click(screen.getByRole('button', { name: /^Finish/ }))
    await waitFor(() => expect(screen.queryByRole('heading', { name: EN['setup.finish.title'] })).toBeNull())
    await settle()
    expect(backend.writes).toEqual([])
    expect(JSON.stringify(backend.stored)).toBe(before)
  })

  it('re-run and skipped: nothing written', async () => {
    const before = JSON.stringify(backend.stored)
    await rerun()
    fireEvent.click(screen.getByRole('button', { name: EN['setup.nav.skip'] }))
    await settle()
    expect(backend.writes).toEqual([])
    expect(JSON.stringify(backend.stored)).toBe(before)
  })

  it('a new install finished without a change: the "done" record is the one change', async () => {
    backend.stored = { ...structuredClone(defaultSettings), setupWizardSeen: false }
    const before = { ...structuredClone(backend.stored), setupWizardSeen: true }
    await mount()
    await screen.findByRole('heading', { name: EN['setup.about.title'] })
    for (let i = 0; i < 4; i++) next()
    fireEvent.click(screen.getByRole('button', { name: /^Finish/ }))
    await waitFor(() => expect(backend.writes).toEqual(['markSetupWizardSeen']))
    await settle()
    expect(backend.writes).toEqual(['markSetupWizardSeen'])
    expect(JSON.stringify(backend.stored)).toBe(JSON.stringify(before))
  })
})

describe('a wizard that changes things writes them in order', () => {
  it('the station draft first, then the licence class, then the experience answer', async () => {
    backend.stored = { ...structuredClone(defaultSettings), setupWizardSeen: false }
    await mount()
    await screen.findByRole('heading', { name: EN['setup.about.title'] })
    fireEvent.click(screen.getByRole('button', { name: new RegExp(`^${EN['setup.about.newToHobby']}`) }))
    next()
    fireEvent.change(screen.getByRole('textbox', { name: EN['setup.station.callsign.label'] }), { target: { value: 'k1ab' } })
    for (let i = 0; i < 3; i++) next()
    fireEvent.click(screen.getByRole('button', { name: new RegExp(`^${EN['setup.finish.license.technician']}`) }))
    fireEvent.click(screen.getByRole('button', { name: /^Finish/ }))
    await waitFor(() => expect(backend.writes.filter((w) => w !== 'markSetupWizardSeen')).toEqual(['setSettings', 'setLicenseClass', 'setExperienceLevel']))
    expect(backend.stored.mycall).toBe('K1AB')
    expect(backend.stored.licenseClass).toBe('technician')
    expect(backend.stored.experienceLevel).toBe('newToHobby')
    expect(backend.stored.setupWizardSeen).toBe(true)
  })
})

describe('an updating station', () => {
  it('sees no wizard and no journey, and nothing is written', async () => {
    // An existing file (it predates the "done" record) and the browser copy of the features.
    const { setupWizardSeen: _omit, ...older } = structuredClone(STATION) as Record<string, unknown>
    void _omit
    backend.stored = older
    const before = JSON.stringify(backend.stored)
    localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { phone: false } }))
    localStorage.setItem('nexus.features.wizardSeen', '1')
    await mount()
    await settle()
    expect(screen.queryByRole('dialog')).toBeNull()
    expect(screen.queryByText(EN['setup.about.title'])).toBeNull()
    expect(screen.queryByText(EN['setup.about.radios.title'])).toBeNull()
    expect(backend.writes).toEqual([])
    expect(JSON.stringify(backend.stored)).toBe(before)
  })
})
