// @vitest-environment jsdom
//
// #353 — HELP ▸ GETTING STARTED ▸ "COMING FROM WSJT-X?" TAKES YOU TO THE SWITCH.
//
// The note promised that JTAlert and GridTracker keep working, and they do only once the WSJT-X
// UDP API switch is on, which it is not by default. The note now names the switch and links to
// its section. The component test (GettingStartedGuide.test.tsx) proves the guide calls back with
// the right section id when it is GIVEN a way into Settings; this one mounts the REAL App (the
// App.neededWizard.test.tsx pattern), because the two halves that can silently drop out live
// here: App has to hand the guide that way in, and Settings has to land where the id points.
// A guide that is never given the callback shows no link at all, and its own test cannot see it.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor, fireEvent, screen, within } from '@testing-library/react'
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

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    // The engine's log questions go unanswered here, as the whole-log read (answered `{}`) did.
    askLog: vi.fn(async () => {
      throw new Error('no log in this test')
    }),
    getSnapshot: vi.fn(async () => snapshot),
    subscribeSnapshot: vi.fn(() => () => {}),
    getAwards: vi.fn(async () => ({ achievements: [] })),
    getJourney: vi.fn(async () => ({ firsts: [], feats: [], ladders: [] })),
    getSettings: vi.fn(async () => ({ ...defaultSettings, mycall: 'KD9TAW' })),
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
    // What Settings itself reads on mount, answered with real shapes (the
    // SettingsPanel.datafolder.test.tsx list) rather than the auto-stub's `{}`.
    getRigModels: vi.fn(async () => []),
    getAllRigModels: vi.fn(async () => []),
    getSerialPortsDetailed: vi.fn(async () => []),
    getCredentialsStatus: vi.fn(async () => []),
    getConnectionLog: vi.fn(async () => []),
    detectRigs: vi.fn(async () => []),
    getAudioDevices: vi.fn(async () => ({ input: [], output: [] })),
    // `null` is the panel's own "no country-file status yet"; the auto-stub's `{}` would be
    // read as a status with no count and crash the page this test has to reach.
    getCtyStatus: vi.fn(async () => null),
    // The section under test prints these three paths; they are strings, and '' is the
    // panel's own starting value.
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

import App from './App'

/** The id of every element asked to scroll into view. Settings scrolls the section a deep link
 *  targets to the top, so this tells the TARGETED section apart from its neighbours on the same
 *  tab, all of which render. */
const scrolledTo: string[] = []

beforeEach(() => {
  scrolledTo.length = 0
  localStorage.clear()
  // A station that has been through the first-run wizard, so nothing covers the top bar.
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: {} }))
  localStorage.setItem('nexus.features.wizardSeen', '1')
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
  // Radix's menu captures pointers and the guide scrolls its dialog; jsdom has neither.
  Element.prototype.hasPointerCapture = () => false
  Element.prototype.setPointerCapture = () => {}
  Element.prototype.releasePointerCapture = () => {}
  Element.prototype.scrollIntoView = function (this: Element) {
    scrolledTo.push(this.id)
  }
  Element.prototype.scrollTo = () => {}
})
afterEach(cleanup)

/** Help ▸ Getting started, then the link in its WSJT-X note. */
async function followTheNoteLink(): Promise<void> {
  fireEvent.pointerDown(screen.getByRole('button', { name: EN['topbar.help.label'] }), {
    button: 0,
    ctrlKey: false,
    pointerType: 'mouse',
  })
  fireEvent.click(await screen.findByText(EN['gettingStarted.title']))
  const note = await waitFor(() => {
    const n = document.querySelector<HTMLElement>('.gsg-wsjtx')
    expect(n, 'the guide opened').not.toBeNull()
    return n as HTMLElement
  })
  fireEvent.click(within(note).getByRole('button', { name: EN['settings.integrations.legend'] }))
}

describe('#353 — the WSJT-X note in Getting started opens the switch it names', () => {
  it('Help ▸ Getting started ▸ the link lands on the section holding WSJT-X UDP API', async () => {
    window.location.hash = '#operate'
    render(<App />)
    await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())

    await followTheNoteLink()

    // The guide closed — it is a modal, and would otherwise cover what it just opened.
    await waitFor(() => expect(document.querySelector('.gsg')).toBeNull())
    // Settings is open on the tab the section lives in…
    const tab = await screen.findByRole('tab', { name: EN['settings.tabs.logging'] })
    expect(tab.getAttribute('aria-selected')).toBe('true')
    // …and that section is the one holding the switch the note names.
    const section = await waitFor(() => {
      const s = document.getElementById('settings-integrations-feeds')
      expect(s, 'the Integrations & Feeds section rendered').not.toBeNull()
      return s as HTMLElement
    })
    expect(within(section).getByRole('switch', { name: 'WSJT-X UDP API' })).toBeTruthy()
    // …and it is the section the link sent Settings to, not merely one on the same tab.
    const sectionScrolls = () => scrolledTo.filter((id) => id.startsWith('settings-'))
    await waitFor(() => expect(sectionScrolls().length).toBeGreaterThan(0))
    expect([...new Set(sectionScrolls())]).toEqual(['settings-integrations-feeds'])
  })

  it('following it again still lands there, from the same tab or another one', async () => {
    // The guide is a modal over ANY view, Settings included, so it is the first caller of the
    // deep link that can be followed while Settings already holds that very target. Asking for
    // the same place twice must still move the panel there.
    window.location.hash = '#operate'
    render(<App />)
    await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
    const toSection = () => scrolledTo.filter((id) => id === 'settings-integrations-feeds').length
    await followTheNoteLink()
    const logging = await screen.findByRole('tab', { name: EN['settings.tabs.logging'] })
    await waitFor(() => expect(logging.getAttribute('aria-selected')).toBe('true'))
    await waitFor(() => expect(toSection()).toBeGreaterThan(0))

    // Same tab, scrolled elsewhere since: the section comes back to the top.
    const before = toSection()
    await followTheNoteLink()
    await waitFor(() => expect(toSection(), 'a repeat on the same tab scrolls again').toBeGreaterThan(before))

    // Another tab: the panel moves back to Logging & Connectors.
    fireEvent.click(screen.getByRole('tab', { name: EN['settings.tabs.radio'] }))
    expect(logging.getAttribute('aria-selected')).toBe('false')
    await followTheNoteLink()
    await waitFor(() => expect(document.querySelector('.gsg')).toBeNull())
    await waitFor(() =>
      expect(
        screen.getByRole('tab', { name: EN['settings.tabs.logging'] }).getAttribute('aria-selected'),
        'a repeat from another tab moves the panel back',
      ).toBe('true'),
    )
    expect(document.getElementById('settings-integrations-feeds')).not.toBeNull()
    // It mounts the whole App and follows the link three times: about 1 s alone, and past the
    // default 5 s on a loaded box, where it timed out without a single assertion failing.
  }, 15_000)
})
