// @vitest-environment jsdom
//
// THE CONTEST SCREEN'S SWITCH AND SETTINGS' SWITCH ARE ONE SETTING, THROUGH APP.
//
// The component suite (ContestView.modeSwitch.test.tsx) proves what the switch saves. This one
// proves the seams around it line up in the shell that ships it: the rail always carries the
// Contest item, so the switch is one click away with the mode off; opening the screen then does
// not enter the contest; a turn-on there is what Settings shows, a turn-on in Settings is what
// the screen shows, and either survives a restart — a fresh App reading the backend's settings,
// which is what a launch reads from settings.json.
//
// The backend is a live fake: settings are read and replaced whole, as `set_settings` does, and
// the snapshot follows the engine's rule (a session while the master is on and the class and
// section are set — `Engine::restore_opens_session`).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor, fireEvent, screen, within, act } from '@testing-library/react'
import type { AppSnapshot } from './types'
import defaultSettings from './components/__fixtures__/defaultSettings.json'

const SFD_START = Date.UTC(2027, 5, 26, 18) / 1000

const { station, setMode, setSettings } = vi.hoisted(() => ({
  station: { current: {} as Record<string, unknown> },
  setMode: vi.fn(),
  setSettings: vi.fn(),
}))

/** The engine's half: in the contest while the master is on and the exchange is set. */
const snapshot = (): AppSnapshot => {
  const on =
    station.current.fdActive === true &&
    String(station.current.fdClass ?? '').trim() !== '' &&
    String(station.current.fdSection ?? '').trim() !== ''
  return {
    mycall: 'W9XYZ',
    mygrid: 'EN52',
    mode: on ? 'fieldDay' : 'chat',
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
    fieldDay: on
      ? {
          running: false,
          state: 'Idle',
          qsoCount: 0,
          sections: 0,
          points: 0,
          log: [],
          event: 'arrlfd',
          composing: [
            { key: 'CLASS', raw: String(station.current.fdClass) },
            { key: 'SECTION', raw: String(station.current.fdSection), domain: 'fd_sections' },
          ],
          eventStartUnix: SFD_START,
          eventEndUnix: SFD_START + 27 * 3600,
          rulesYear: 2026,
        }
      : null,
    recentDecodes: [],
    harqRescues: 0,
  } as unknown as AppSnapshot
}

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  setSettings.mockImplementation(async (s: Record<string, unknown>) => {
    station.current = { ...s }
    return snapshot()
  })
  setMode.mockImplementation(async () => snapshot())
  return {
    ...auto,
    askLog: vi.fn(async () => {
      throw new Error('no log in this test')
    }),
    getSnapshot: vi.fn(async () => snapshot()),
    subscribeSnapshot: vi.fn(() => () => {}),
    getSettings: vi.fn(async () => ({ ...station.current })),
    setSettings,
    setMode,
    getFdRuleset: vi.fn(async () => ({
      event: 'arrlfd',
      rulesYear: 2026,
      bannedModes: [],
      spottingAllowed: true,
      clusterAllowed: true,
      enforcement: 'warn',
      role: '',
      exchange: [],
      problem: '',
      eventStartUnix: SFD_START,
      eventEndUnix: SFD_START + 27 * 3600,
    })),
    getAwards: vi.fn(async () => ({ achievements: [] })),
    getJourney: vi.fn(async () => ({ firsts: [], feats: [], ladders: [] })),
    getBandPlan: vi.fn(async () => []),
    getLicensedBandPlan: vi.fn(async () => []),
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
    haltTx: vi.fn(async () => snapshot()),
    setArea: vi.fn(async () => snapshot()),
    appVersion: vi.fn(async () => '0.0.0-test'),
    openPanelWindow: vi.fn(async () => {}),
    // What Settings reads on mount, in real shapes (App.guideUdpLink.test.tsx's list).
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
    // …and what its Contesting tab reads (the assistance record is a list).
    getAssistanceJournal: vi.fn(async () => []),
    getFdRulesStatus: vi.fn(async () => null),
  }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))

import App from './App'

vi.setConfig({ testTimeout: 60_000 })

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: {} }))
  localStorage.setItem('nexus.features.wizardSeen', '1')
  localStorage.setItem('tempo-onboarded', '1')
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
  Element.prototype.hasPointerCapture = () => false
  Element.prototype.setPointerCapture = () => {}
  Element.prototype.releasePointerCapture = () => {}
  Element.prototype.scrollIntoView = () => {}
  Element.prototype.scrollTo = () => {}
  window.location.hash = '#operate'
  station.current = {
    ...defaultSettings,
    mycall: 'W9XYZ',
    mygrid: 'EN52',
    fdEvent: '',
    fdClass: '1D',
    fdSection: 'WI',
    fdActive: false,
  }
  setMode.mockClear()
  setSettings.mockClear()
})
afterEach(cleanup)

// ⚠️ THE BUDGETS ARE FOR WORK, NOT A CLOCK: the whole App boots before the rail is there.
const WAIT = { timeout: 30_000 }

const railContest = () => screen.findByRole('button', { name: 'Contest — ARRL Field Day' }, WAIT)
const screenSwitch = () =>
  within(document.querySelector('main.layout.single .fd-event-banner') as HTMLElement).getByRole('switch')
const clubBoard = () => screen.queryByRole('button', { name: /club band board/i })

async function openContest() {
  fireEvent.click(await railContest())
  await waitFor(() => expect(document.querySelector('main.layout.single .fd-event-banner')).not.toBeNull(), WAIT)
}

async function openSettingsContesting() {
  const nav = await waitFor(() => {
    const n = document.querySelector('nav.mode-nav')
    expect(n).not.toBeNull()
    return n as HTMLElement
  }, WAIT)
  fireEvent.click(within(nav).getByRole('button', { name: 'Settings' }))
  fireEvent.click(await screen.findByRole('tab', { name: 'Contesting' }, WAIT))
  return waitFor(() => {
    const section = document.querySelector('#settings-field-day') as HTMLElement | null
    expect(section).not.toBeNull()
    return within(section!).getByRole('switch', { name: /Field Day mode/ })
  }, WAIT)
}

describe('the switch is one click away, with the mode off', () => {
  it('the rail carries Contest with the mode off, and opening it does not enter the contest', async () => {
    render(<App />)
    await openContest()
    expect(screenSwitch().getAttribute('aria-checked')).toBe('false')
    // Entering would start a session the master switch does not know about.
    expect(setMode.mock.calls.some((c) => c[0] === 'fieldday-sp')).toBe(false)
    // What the mode reveals still follows the mode.
    expect(clubBoard()).toBeNull()
  })

  it('CONTROL: with the mode on, opening it enters the contest as it always has, and the Club Board shows', async () => {
    station.current = { ...station.current, fdActive: true }
    render(<App />)
    await openContest()
    await waitFor(() => expect(setMode.mock.calls.some((c) => c[0] === 'fieldday-sp')).toBe(true), WAIT)
    expect(clubBoard()).not.toBeNull()
  })
})

describe('Settings and the screen agree, both ways, and across a restart', () => {
  it('on at the screen: saved, shown in Settings, the Club Board appears, and a fresh launch still has it', async () => {
    render(<App />)
    await openContest()
    await act(async () => {
      fireEvent.click(screenSwitch())
    })
    await waitFor(() => expect(station.current.fdActive).toBe(true), WAIT)
    // App re-read the settings: the rail now shows what the mode reveals.
    await waitFor(() => expect(clubBoard()).not.toBeNull(), WAIT)
    const inSettings = await openSettingsContesting()
    expect(inSettings.getAttribute('aria-checked')).toBe('true')
    // A restart: a new App reads the backend's settings, the file a launch reads.
    cleanup()
    window.location.hash = '#operate'
    render(<App />)
    await openContest()
    await waitFor(() => expect(screenSwitch().getAttribute('aria-checked')).toBe('true'), WAIT)
  })

  it('on in Settings (and Save): the screen shows it on', async () => {
    render(<App />)
    const inSettings = await openSettingsContesting()
    expect(inSettings.getAttribute('aria-checked')).toBe('false')
    fireEvent.click(inSettings)
    const save = screen.getAllByRole('button', { name: 'Save' })[0]
    await act(async () => {
      fireEvent.click(save)
    })
    await waitFor(() => expect(station.current.fdActive).toBe(true), WAIT)
    await openContest()
    await waitFor(() => expect(screenSwitch().getAttribute('aria-checked')).toBe('true'), WAIT)
    // …and off again at the screen is off in Settings.
    await act(async () => {
      fireEvent.click(screenSwitch())
    })
    await waitFor(() => expect(station.current.fdActive).toBe(false), WAIT)
    const again = await openSettingsContesting()
    expect(again.getAttribute('aria-checked')).toBe('false')
  })
})
