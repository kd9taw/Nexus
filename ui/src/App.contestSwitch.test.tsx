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
//
// OPENING THE SCREEN KEEPS A STATION ALREADY IN THE CONTEST IN ITS ROLE. With the mode on, the
// screen enters the contest's S&P only from another mode. `set_mode` replaces the mode wholesale,
// so re-entering would stop a run's CQs and drop the contact an S&P station is working. The fake
// engine holds the spec `set_mode` last entered and what the sequencer will send next, so the
// tests read the role by value.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor, fireEvent, screen, within, act } from '@testing-library/react'
import type { AppSnapshot } from './types'
import defaultSettings from './components/__fixtures__/defaultSettings.json'

const SFD_START = Date.UTC(2027, 5, 26, 18) / 1000

type Role = { spec: string; queue: string[]; dxcall: string | null }

const { station, engine, setMode, setSettings, startRunning } = vi.hoisted(() => ({
  station: { current: {} as Record<string, unknown> },
  /** The spec `set_mode` last entered, what the sequencer will send next, and the station being
   *  worked. `null` is what a launch restores: the contest's S&P while the master is on, Chat
   *  otherwise (`Engine::restore_field_day_if_enabled`). */
  engine: { current: null as Role | null },
  setMode: vi.fn(),
  setSettings: vi.fn(),
  startRunning: vi.fn(),
}))

/** What `set_mode` leaves (`Engine::set_mode`): the run flag is the spec's own, a run calls CQ, and
 *  the queues and the contact in progress go with the mode it replaced. */
const entered = (spec: string): Role => ({
  spec,
  queue: spec === 'fieldday-run' ? ['CQ FD W9XYZ EN52'] : spec === 'qso-run' ? ['CQ W9XYZ EN52'] : [],
  dxcall: null,
})

/** The master is on and the exchange is set: the contest can run. */
const contestOn = () =>
  station.current.fdActive === true &&
  String(station.current.fdClass ?? '').trim() !== '' &&
  String(station.current.fdSection ?? '').trim() !== ''

/** The engine's half: in the contest while the master is on and the exchange is set. */
const snapshot = (): AppSnapshot => {
  const on = contestOn()
  const spec = engine.current?.spec ?? (on ? 'fieldday-sp' : 'chat')
  const contest = on && spec.startsWith('fieldday')
  const qso = spec.startsWith('qso')
  return {
    mycall: 'W9XYZ',
    mygrid: 'EN52',
    mode: contest ? 'fieldDay' : qso ? 'qso' : 'chat',
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
    qso: qso
      ? {
          state: spec === 'qso-run' ? 'CallingCq' : 'Listening',
          dxcall: null,
          rxReport: null,
          running: spec === 'qso-run',
          cqRunning: spec === 'qso-run',
          txNow: engine.current?.queue[0] ?? null,
        }
      : null,
    fieldDay: contest
      ? {
          running: spec === 'fieldday-run',
          state: 'Idle',
          dxcall: engine.current?.dxcall ?? null,
          qsoCount: 0,
          sections: 0,
          points: 0,
          log: [],
          // The picked contest: a save that moves the pick switches the session at once
          // (`Engine::save_switches_contest`), so the session always runs the pick.
          event: String(station.current.fdEvent || 'arrlfd'),
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
  setMode.mockImplementation(async (spec: string) => {
    engine.current = entered(spec)
    return snapshot()
  })
  // The strip's Call CQ (`Engine::start_running`): the contest's Running while the contest runs.
  startRunning.mockImplementation(async () => {
    engine.current = entered(contestOn() ? 'fieldday-run' : 'qso-run')
    return snapshot()
  })
  return {
    ...auto,
    // A computer running its own station: the backend answers None, and the banner asks once. The
    // catch-all's `{}` would read as driving a remote station, asked every half second.
    clientStatus: vi.fn(async () => null),
    askLog: vi.fn(async () => {
      throw new Error('no log in this test')
    }),
    getSnapshot: vi.fn(async () => snapshot()),
    subscribeSnapshot: vi.fn(() => () => {}),
    getSettings: vi.fn(async () => ({ ...station.current })),
    setSettings,
    setMode,
    startRunning,
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
    setTier: vi.fn(async () => snapshot()),
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
    // The contest setup's reads: the preview of its unsaved answers (the saved pick's, here) and the
    // contests' windows (the snapshot it checks the station against is `getSnapshot` above).
    previewFdRuleset: vi.fn(async () => ({
      event: String(station.current.fdEvent || 'arrlfd'),
      rulesYear: 2026,
      bannedModes: [],
      spottingAllowed: true,
      clusterAllowed: true,
      enforcement: 'warn',
      role: '',
      exchange: [String(station.current.fdClass), String(station.current.fdSection)],
      problem: '',
      sentSettings: ['fd_class', 'fd_section'],
    })),
    contestWindows: vi.fn(async () => []),
  }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))

import App from './App'
import * as api from './api'

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
  engine.current = null
  setMode.mockClear()
  setSettings.mockClear()
  startRunning.mockClear()
})
afterEach(cleanup)

// ⚠️ THE BUDGETS ARE FOR WORK, NOT A CLOCK: the whole App boots before the rail is there.
const WAIT = { timeout: 30_000 }

const railContest = () => screen.findByRole('button', { name: 'Contest — ARRL Field Day' }, WAIT)
const screenSwitch = () =>
  within(document.querySelector('main.layout.single .fd-event-banner') as HTMLElement).getByRole('switch')
const clubBoard = () => screen.queryByRole('button', { name: /club band board/i })
const roleButton = (name: 'Running' | 'S&P') =>
  within(screen.getByRole('group', { name: 'Running or S&P' })).getByRole('button', { name })
/** Lets a mode change the last click set off land, so "it did not happen" is not just "not yet". */
const settle = () => act(async () => { await new Promise((resolve) => setTimeout(resolve, 50)) })

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
    const section = document.querySelector('#settings-contest-pick') as HTMLElement | null
    expect(section).not.toBeNull()
    // Start contest… with the mode off, Stop contest… with it on: the form saves the mode no more.
    return within(section!).getByRole('button', { name: /^(Start|Stop) contest…$/ })
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

  it('CONTROL: with the mode on, opening it from FT8 enters the contest in S&P as it always has, never calling CQ, and the Club Board shows', async () => {
    station.current = { ...station.current, fdActive: true }
    engine.current = entered('qso-run')
    render(<App />)
    await openContest()
    await waitFor(() => expect(engine.current).toEqual({ spec: 'fieldday-sp', queue: [], dxcall: null }), WAIT)
    expect(setMode.mock.calls.map((c) => c[0])).toEqual(['fieldday-sp'])
    await waitFor(() => expect(roleButton('S&P').getAttribute('aria-pressed')).toBe('true'), WAIT)
    expect(clubBoard()).not.toBeNull()
  })
})

describe('opening the screen keeps a station already in the contest in its role', () => {
  it('a station running CQ keeps its run: still Running, its CQ still queued, and no set_mode', async () => {
    station.current = { ...station.current, fdActive: true }
    engine.current = entered('fieldday-run')
    render(<App />)
    await openContest()
    await settle()
    expect(engine.current).toEqual({ spec: 'fieldday-run', queue: ['CQ FD W9XYZ EN52'], dxcall: null })
    expect(roleButton('Running').getAttribute('aria-pressed')).toBe('true')
    expect(setMode).not.toHaveBeenCalled()
  })

  it('a station in S&P keeps the contact it is working, and no set_mode', async () => {
    station.current = { ...station.current, fdActive: true }
    engine.current = { spec: 'fieldday-sp', queue: ['K1ABC W9XYZ 1D WI'], dxcall: 'K1ABC' }
    render(<App />)
    await openContest()
    await settle()
    expect(engine.current).toEqual({ spec: 'fieldday-sp', queue: ['K1ABC W9XYZ 1D WI'], dxcall: 'K1ABC' })
    expect(roleButton('S&P').getAttribute('aria-pressed')).toBe('true')
    expect(setMode).not.toHaveBeenCalled()
  })

  it('a run started at the screen survives a trip to the FT cockpit and back', async () => {
    station.current = { ...station.current, fdActive: true }
    engine.current = entered('qso-monitor')
    render(<App />)
    await openContest()
    await waitFor(() => expect(engine.current?.spec).toBe('fieldday-sp'), WAIT)
    fireEvent.click(roleButton('Running'))
    await waitFor(() => expect(roleButton('Running').getAttribute('aria-pressed')).toBe('true'), WAIT)
    fireEvent.click(screen.getByRole('button', { name: /^FT weak-signal cockpit/ }))
    await waitFor(() => expect(document.querySelector('main.layout.single .fd-event-banner')).toBeNull(), WAIT)
    await openContest()
    await settle()
    expect(engine.current).toEqual({ spec: 'fieldday-run', queue: ['CQ FD W9XYZ EN52'], dxcall: null })
    expect(roleButton('Running').getAttribute('aria-pressed')).toBe('true')
    expect(setMode.mock.calls.map((c) => c[0])).toEqual(['fieldday-sp', 'fieldday-run'])
  })
})

describe("the FT cockpit's Call CQ while the contest runs", () => {
  const stripCallCq = () => {
    const group = screen.getByRole('group', { name: 'Sequencer role' })
    const found = [...group.querySelectorAll('button')].find((b) => b.textContent === 'Call CQ')
    if (!found) throw new Error('no Call CQ in the strip')
    return found
  }

  it("starts the contest's run, lights Call CQ, and opening the screen keeps the run", async () => {
    station.current = { ...station.current, fdActive: true }
    render(<App />)
    fireEvent.click(await waitFor(stripCallCq, WAIT))
    await waitFor(() => expect(engine.current?.spec).toBe('fieldday-run'), WAIT)
    await waitFor(() => expect(stripCallCq().getAttribute('aria-pressed')).toBe('true'), WAIT)
    await openContest()
    await settle()
    expect(engine.current).toEqual({ spec: 'fieldday-run', queue: ['CQ FD W9XYZ EN52'], dxcall: null })
    expect(roleButton('Running').getAttribute('aria-pressed')).toBe('true')
    expect(setMode).not.toHaveBeenCalled()
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
    expect(inSettings.textContent).toBe('Stop contest…')
    // A restart: a new App reads the backend's settings, the file a launch reads.
    cleanup()
    window.location.hash = '#operate'
    render(<App />)
    await openContest()
    await waitFor(() => expect(screenSwitch().getAttribute('aria-checked')).toBe('true'), WAIT)
  })

  it('Start contest… in Settings: the setup starts it, and the screen shows it on', async () => {
    render(<App />)
    const inSettings = await openSettingsContesting()
    expect(inSettings.textContent).toBe('Start contest…')
    fireEvent.click(inSettings)
    await screen.findByRole('dialog', { name: 'Set up a contest' }, WAIT)
    for (let i = 0; i < 4; i++) {
      const next = screen.getByRole('button', { name: 'Next' })
      await waitFor(() => expect(next).toHaveProperty('disabled', false), WAIT)
      fireEvent.click(next)
    }
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Start ARRL Field Day' }))
    })
    await screen.findByText('ARRL Field Day is on · S&P', undefined, WAIT)
    expect(station.current.fdActive).toBe(true)
    fireEvent.click(screen.getByRole('button', { name: 'Close' }))
    // Settings follows at once, without its Save.
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull(), WAIT)
    expect(within(document.querySelector('#settings-contest-pick') as HTMLElement).getByRole('button', { name: 'Stop contest…' })).toBeTruthy()
    await openContest()
    await waitFor(() => expect(screenSwitch().getAttribute('aria-checked')).toBe('true'), WAIT)
    // …and off again at the screen is off in Settings.
    await act(async () => {
      fireEvent.click(screenSwitch())
    })
    await waitFor(() => expect(station.current.fdActive).toBe(false), WAIT)
    const again = await openSettingsContesting()
    expect(again.textContent).toBe('Start contest…')
  })
})

describe('a pick saved in Settings while the mode is on switches, and the screen says so', () => {
  const SWITCHED =
    "Now running Illinois QSO Party. Saving the new pick ended ARRL Field Day, and its contacts stay apart from this contest's."
  const modeLines = () =>
    [...document.querySelectorAll('main.layout.single .fd-mode-status .fd-mode-line')].map((l) => l.textContent)

  it('Settings ▸ Contesting, Illinois QSO Party, Save: the Contest screen names both contests; off and on says nothing', async () => {
    station.current = { ...station.current, fdActive: true }
    render(<App />)
    await openSettingsContesting()
    fireEvent.click(
      within(screen.getByRole('group', { name: 'Which contest you are entering' })).getByRole('button', {
        name: 'Illinois QSO Party',
      }),
    )
    await act(async () => {
      fireEvent.click(screen.getAllByRole('button', { name: 'Save' })[0])
    })
    await waitFor(() => expect(station.current.fdEvent).toBe('ilqp'), WAIT)
    fireEvent.click(await screen.findByRole('button', { name: 'Contest — Illinois QSO Party' }, WAIT))
    await waitFor(() => expect(modeLines()[0]).toBe(SWITCHED), WAIT)
    // CONTROL: off and on at the screen is no switch, and the line goes.
    await act(async () => {
      fireEvent.click(screenSwitch())
    })
    await waitFor(() => expect(station.current.fdActive).toBe(false), WAIT)
    await act(async () => {
      fireEvent.click(screenSwitch())
    })
    await waitFor(() => expect(screenSwitch().getAttribute('aria-checked')).toBe('true'), WAIT)
    await settle()
    expect(modeLines()).not.toContain(SWITCHED)
  })
})

// THE CW AND PHONE SCREENS ENTER THE CONTEST AS THE CONTEST SCREEN DOES. With the mode on and the
// session dropped (a trip to Chat, a tier switch), the CW and Phone log lines fell back to the
// everyday form: no exchange boxes, no dupe check, the casual macros. Opening either screen now
// enters the contest's S&P from another mode, the contest screen's own rule (`handleView`), and
// keeps a station already in the contest in its role.
describe('opening CW or Phone with the mode on enters the contest, as the contest screen does', () => {
  const railButton = (name: RegExp) => screen.findByRole('button', { name }, WAIT)
  const CW = /^CW operating/
  const PHONE = /^Phone \(SSB\) operating/
  beforeEach(() => {
    localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { cw: true, phone: true } }))
  })

  it.each([
    ['CW', CW],
    ['Phone', PHONE],
  ] as const)('%s, after the session dropped to Chat: S&P, never calling CQ', async (_name, rail) => {
    station.current = { ...station.current, fdActive: true }
    engine.current = entered('chat')
    render(<App />)
    fireEvent.click(await railButton(rail))
    await waitFor(() => expect(engine.current).toEqual({ spec: 'fieldday-sp', queue: [], dxcall: null }), WAIT)
    expect(setMode.mock.calls.map((c) => c[0])).toEqual(['fieldday-sp'])
  })

  it('CONTROL: with the mode off, opening CW does not enter the contest', async () => {
    engine.current = entered('chat')
    render(<App />)
    fireEvent.click(await railButton(CW))
    await settle()
    expect(engine.current).toEqual({ spec: 'chat', queue: [], dxcall: null })
    expect(setMode.mock.calls.some((c) => c[0] === 'fieldday-sp')).toBe(false)
  })

  it.each([
    ['CW', CW],
    ['Phone', PHONE],
  ] as const)('%s keeps a running station running: its CQ still queued, and no set_mode', async (_name, rail) => {
    station.current = { ...station.current, fdActive: true }
    engine.current = entered('fieldday-run')
    render(<App />)
    fireEvent.click(await railButton(rail))
    await settle()
    expect(engine.current).toEqual({ spec: 'fieldday-run', queue: ['CQ FD W9XYZ EN52'], dxcall: null })
    expect(setMode).not.toHaveBeenCalled()
  })
})

// "WORK THIS CONTEST IN CW / IN PHONE" (a tester at the Illinois QSO Party, 2026-10-18, looked for Enter
// Sends Message in the Contest window). From another window a press can only ask: it writes the request
// the main window hears as a `storage` event (features/contestWorkIn). The main window then makes the
// rail's own switch, so the contest's S&P is entered where the session dropped and never a CQ, and a
// station already in the contest keeps its role, and it brings itself to the front (`focus_window`).
describe('"Work this contest in CW / in Phone"', () => {
  const ask = (view: string, key = 'nexus.contest.workIn') =>
    act(() => {
      window.dispatchEvent(new StorageEvent('storage', { key, newValue: JSON.stringify({ view, at: Date.now() }) }))
    })
  // The screen the main window shows, as it keeps it (the cockpits themselves need more of the station than
  // this fake engine has to draw).
  const shown = () => localStorage.getItem('nexus.view')
  beforeEach(() => {
    localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { cw: true, phone: true } }))
    vi.mocked(api.focusWindow).mockClear()
  })

  it.each(['cw', 'phone'] as const)(
    'from a contest window: the main window goes to %s, enters the contest in S&P (never CQ) and comes to the front',
    async (view) => {
      station.current = { ...station.current, fdActive: true }
      engine.current = entered('chat')
      render(<App />)
      await railContest()
      await ask(view)
      await waitFor(() => expect(shown()).toBe(view), WAIT)
      await waitFor(() => expect(engine.current).toEqual({ spec: 'fieldday-sp', queue: [], dxcall: null }), WAIT)
      expect(setMode.mock.calls.map((c) => c[0])).toEqual(['fieldday-sp'])
      expect(api.focusWindow).toHaveBeenCalledTimes(1)
    },
  )

  it('a running station keeps its run: its CQ still queued, and no set_mode', async () => {
    station.current = { ...station.current, fdActive: true }
    engine.current = entered('fieldday-run')
    render(<App />)
    await railContest()
    await ask('cw')
    await waitFor(() => expect(shown()).toBe('cw'), WAIT)
    await settle()
    expect(engine.current).toEqual({ spec: 'fieldday-run', queue: ['CQ FD W9XYZ EN52'], dxcall: null })
    expect(setMode).not.toHaveBeenCalled()
  })

  it('CONTROL: another key, or a request for another screen, moves nothing', async () => {
    station.current = { ...station.current, fdActive: true }
    render(<App />)
    await railContest()
    await ask('cw', 'nexus.contest.entryCall')
    await ask('operate')
    await settle()
    expect(shown()).toBe('operate')
    expect(api.focusWindow).not.toHaveBeenCalled()
  })

  it("on the main window's own Contest screen the button is the switch itself", async () => {
    station.current = { ...station.current, fdActive: true }
    engine.current = entered('chat')
    render(<App />)
    await openContest()
    fireEvent.click(await screen.findByRole('button', { name: 'Work this contest in Phone' }, WAIT))
    await waitFor(() => expect(shown()).toBe('phone'), WAIT)
    expect(localStorage.getItem('nexus.contest.workIn')).toBeNull()
  })
})
