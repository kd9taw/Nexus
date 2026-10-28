// @vitest-environment jsdom
//
// THE SETUP WIZARD AND THE LICENCE CLASS, through the REAL App against a backend that keeps the
// settings file (its reads answer what it holds now, its writers change it). The licence class sets
// the Part 97 transmit lockout, so the wizard must never change it behind the operator's back:
// the licence step opens on the class the station holds, and Finish writes a class only when the
// operator picked a different one. Re-run from Settings and finished untouched, a Technician or
// General station keeps its class (it used to be written over with "Outside the US", which takes the
// lockout off). Once the wizard writes a class, a second run in the same session opens on it.
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

/** A station that has used Nexus, with a US licence class. */
const station = (licenseClass: string) => ({
  ...defaultSettings,
  mycall: 'W1AW',
  mygrid: 'FN31pr',
  licenseClass,
  setupWizardSeen: true,
})

const backend = vi.hoisted(() => ({
  stored: {} as Record<string, unknown>,
  /** Every licence class written, in order. */
  licence: [] as unknown[],
}))

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
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
    setSettings: vi.fn(async (s: Record<string, unknown>) => {
      backend.stored = structuredClone(s)
      return snapshot
    }),
    setLicenseClass: vi.fn(async (c: unknown) => {
      backend.licence.push(c)
      backend.stored.licenseClass = c
      return snapshot
    }),
    setExperienceLevel: vi.fn(async (l: unknown) => {
      backend.stored.experienceLevel = l
      return snapshot
    }),
    markSetupWizardSeen: vi.fn(async () => {
      backend.stored.setupWizardSeen = true
      return snapshot
    }),
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
  backend.stored = station('technician')
  backend.licence = []
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

const settle = () => new Promise((r) => setTimeout(r, 100))
const next = () => fireEvent.click(screen.getByRole('button', { name: /^Next/ }))
const back = () => fireEvent.click(screen.getByRole('button', { name: EN['setup.nav.back'] }))
const LABEL: Record<string, string> = {
  technician: EN['setup.finish.license.technician'],
  general: EN['setup.finish.license.general'],
  extra: EN['setup.finish.license.extra'],
  open: EN['setup.finish.license.open'],
}
const classButton = (c: string) => screen.getByRole('button', { name: new RegExp(`^${LABEL[c]}`) })
/** The licence class the licence step shows as picked, by its token. */
function shownClass(): string | null {
  const picked = Object.keys(LABEL).filter((c) => classButton(c).getAttribute('aria-pressed') === 'true')
  return picked.length === 1 ? picked[0] : `${picked.length} picked`
}
async function finish() {
  fireEvent.click(screen.getByRole('button', { name: /^Finish/ }))
  await waitFor(() => expect(screen.queryByRole('heading', { name: EN['setup.finish.title'] })).toBeNull())
  await settle()
}

async function mount(hash: string): Promise<void> {
  window.location.hash = hash
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
  await new Promise((r) => setTimeout(r, 50))
}
/** Open the wizard again from Settings, by the Radio tab's button or the Appearance tab's link. */
async function openRerun(from: 'radio' | 'appearance') {
  // Finish lands on the operating screen; the gear in the mode rail goes back to Settings.
  const gear = screen.queryByRole('button', { name: EN['nav.settings.label'] })
  if (gear && gear.getAttribute('aria-current') !== 'page') fireEvent.click(gear)
  fireEvent.click(await screen.findByRole('tab', { name: EN[`settings.tabs.${from}`] }))
  const name = from === 'radio' ? /Re-run setup wizard/ : EN['settings.features.rerunWizard']
  fireEvent.click(await screen.findByRole('button', { name }))
  await screen.findByRole('heading', { name: EN['setup.station.title'] })
}
/** A station that has used Nexus, in Settings. */
async function mountStation() {
  localStorage.setItem('nexus.features.wizardSeen', '1')
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'everything', enabled: {} }))
  await mount('#settings')
}
/** Station → rig → log → the licence step. */
const toLicenceStep = () => {
  for (let i = 0; i < 3; i++) next()
}

describe('re-running setup keeps the licence class', () => {
  it.each([
    ['technician', 'radio'],
    ['general', 'appearance'],
  ] as const)('a %s station re-run from Settings ▸ %s and finished untouched keeps its class', async (cls, from) => {
    backend.stored = station(cls)
    await mountStation()
    await openRerun(from)
    toLicenceStep()
    expect(shownClass()).toBe(cls)
    await finish()
    expect(backend.licence).toEqual([])
    expect(backend.stored.licenseClass).toBe(cls)
  })

  it('a re-run skipped writes no class', async () => {
    await mountStation()
    await openRerun('radio')
    fireEvent.click(screen.getByRole('button', { name: EN['setup.nav.skip'] }))
    await settle()
    expect(backend.licence).toEqual([])
    expect(backend.stored.licenseClass).toBe('technician')
  })

  it('Back and Next keep the class on screen; a pick put back to the held class writes nothing', async () => {
    await mountStation()
    await openRerun('radio')
    toLicenceStep()
    back()
    next()
    expect(shownClass()).toBe('technician')
    fireEvent.click(classButton('general'))
    back()
    next()
    expect(shownClass()).toBe('general')
    fireEvent.click(classButton('technician'))
    await finish()
    expect(backend.licence).toEqual([])
    expect(backend.stored.licenseClass).toBe('technician')
  })

  it('a different class picked on a re-run is written', async () => {
    await mountStation()
    await openRerun('radio')
    toLicenceStep()
    fireEvent.click(classButton('general'))
    await finish()
    expect(backend.licence).toEqual(['general'])
    expect(backend.stored.licenseClass).toBe('general')
  })
})

describe('two runs in one session', () => {
  it('the second run opens on the class the first one wrote, and writes only a change', async () => {
    await mountStation()
    await openRerun('radio')
    toLicenceStep()
    expect(shownClass()).toBe('technician')
    fireEvent.click(classButton('extra'))
    await finish()
    expect(backend.licence).toEqual(['extra'])

    // Second run, same session: the screen shows what the station now holds.
    await openRerun('radio')
    toLicenceStep()
    expect(shownClass()).toBe('extra')
    // Untouched, it writes nothing more.
    await finish()
    expect(backend.licence).toEqual(['extra'])
    expect(backend.stored.licenseClass).toBe('extra')

    // Third run: pressing Technician writes it.
    await openRerun('radio')
    toLicenceStep()
    expect(shownClass()).toBe('extra')
    fireEvent.click(classButton('technician'))
    await finish()
    expect(backend.licence).toEqual(['extra', 'technician'])
    expect(backend.stored.licenseClass).toBe('technician')
  })
})

describe('the first run on a new install', () => {
  // A new install holds the engine's default, Outside the US. It ends there when the step is left
  // alone, as it always did, and on the class picked when one is.
  const fresh = () => {
    backend.stored = { ...structuredClone(defaultSettings), setupWizardSeen: false }
    expect(backend.stored.licenseClass).toBe('open')
  }
  async function firstRunToLicenceStep() {
    await mount('#operate')
    await screen.findByRole('heading', { name: EN['setup.about.title'] })
    for (let i = 0; i < 4; i++) next()
  }

  it('left alone, the station holds Outside the US', async () => {
    fresh()
    await firstRunToLicenceStep()
    expect(shownClass()).toBe('open')
    await finish()
    expect(backend.stored.licenseClass).toBe('open')
    expect(backend.stored.setupWizardSeen).toBe(true)
  })

  it('a class picked is written', async () => {
    fresh()
    await firstRunToLicenceStep()
    fireEvent.click(classButton('technician'))
    await finish()
    expect(backend.licence).toEqual(['technician'])
    expect(backend.stored.licenseClass).toBe('technician')
  })
})
