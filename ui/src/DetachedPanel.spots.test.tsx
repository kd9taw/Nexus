// @vitest-environment jsdom
//
// THE SPOTS BOARD IN A WINDOW OF ITS OWN (`?panel=spots`), as the Needed board has had one.
//
// The window is the real board (SpotsPanel, the Spots view's own component) with THIS window's paths, and it
// shares them with the dashboard window's Spots box through one object, as the Needed window and the
// dashboard window's Needed box share theirs. A pop-out cannot reach the main window's handlers, so "the
// board's own path" here is this window's: its Needed work applied to the spot as a need, exactly how the
// Spots view works a spot in the main window (`handleWorkSpot = handleWorkNeeded(spotNeed(s))`). THE REAL
// DetachedPanel is mounted, as the Spots window and, where a case is about the shared board, as the
// dashboard window beside it; only the backend is stubbed.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { StrictMode } from 'react'
import { render, cleanup, waitFor, fireEvent, act } from '@testing-library/react'
import type { AppSnapshot, NeedAlert, SpotRow } from './types'
import { t } from './i18n'

const snapshot = {
  mycall: 'KD9TAW', mygrid: 'EN52', mode: 'Normal', activePeer: null, hunt: null, logTick: 1,
  radio: { dialMhz: 14.025, band: '20m', catOk: true, sideband: 'CW', operatingMode: 'cw', transmitting: false, txEnabled: false, txAllowed: true },
  link: { tier: 'FT8' }, stations: [],
} as unknown as AppSnapshot

const spot = (over: Partial<SpotRow>): SpotRow =>
  ({
    call: 'K1CW', entity: 'United States', zone: 5, state: null, band: '20m', freqMhz: 14.025, mode: 'CW',
    submode: 'CW', spotter: 'W3LPL', corroborators: [], ageSecs: 30, comment: '', licensed: true,
    spotterLocal: true, ...over,
  }) as SpotRow
const SPOTS: SpotRow[] = [
  spot({}),
  spot({ call: 'JA1FT', entity: 'Japan', band: '17m', freqMhz: 18.104, mode: 'Digital', submode: 'FT4', ageSecs: 45 }),
]
/** Two stations worked today, each still wanted by a need the engine sent on 20 m CW. */
const WORKED: SpotRow[] = [
  spot({ call: 'W7B', freqMhz: 14.03, workedAgoSecs: 600, workedTodayUtc: true }),
  spot({ call: 'W6A', freqMhz: 14.035, workedAgoSecs: 600, workedTodayUtc: true }),
]
const need = (call: string, tags: NeedAlert['tags'], freqMhz: number): NeedAlert =>
  ({ call, entity: 'United States', band: '20m', zone: 5, tags, priority: 50, headline: '', mode: 'CW', freqMhz }) as NeedAlert

const backend = vi.hoisted(() => ({ spots: [] as unknown[], needs: [] as unknown[] }))

const TX_VERBS = [
  'callStation', 'startCq', 'startRunning', 'callCq', 'setChatCq', 'resumeChatCq', 'sendMessage', 'sendCw', 'setPtt',
  'playVoiceMessage', 'setTxEnabled', 'setTune', 'atuTune', 'setBeacon', 'aprsSendBeacon', 'aprsSendMessage',
  'rttyAutoArm', 'rttySend', 'rttySetLatched', 'rttyType', 'rttyAutoCq', 'rttyAutoAnswer', 'pskSend',
  'pskSetLatched', 'pskType', 'js8Send', 'js8SendCommand', 'js8CallCq', 'js8Arm', 'js8CqRepeat', 'sstvSend',
]

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => null) : actual[k]
  }
  return {
    ...auto,
    subscribeSnapshot: vi.fn((cb: (s: AppSnapshot) => void) => {
      cb(snapshot)
      return () => {}
    }),
    selectPeer: vi.fn(async () => snapshot),
    getBandPlan: vi.fn(async () => []),
    getPropagation: vi.fn(async () => null),
    getNeedAlerts: vi.fn(async () => backend.needs),
    getSettings: vi.fn(async () => null),
    getAllSpots: vi.fn(async () => backend.spots),
    getOtaSpots: vi.fn(async () => []),
    getActivation: vi.fn(async () => ({ program: null, reference: null, qsoCount: 0 })),
    parksCount: vi.fn(async () => 0),
    huntedParksCount: vi.fn(async () => 0),
    getBandOutlook: vi.fn(async () => ({ bands: [], asOf: 0 })),
    getGettingOut: vi.fn(async () => null),
    getSpaceWxScales: vi.fn(async () => ({ scales: null, alerts: [] })),
    getKc2gMuf: vi.fn(async () => []),
    getDxpedWindows: vi.fn(async () => []),
    getOtaMapSpots: vi.fn(async () => []),
    getPotaActivator: vi.fn(async () => ({ calls: [], parks: [] })),
    getContests: vi.fn(async () => []),
    getWindowBehind: vi.fn(async () => ({ supported: false, on: false })),
    workSpot: vi.fn(async () => snapshot),
    setHuntTarget: vi.fn(async () => snapshot),
    clusterSelfSpot: vi.fn(async (comment: string) => ({ freqKhz: 14025, call: 'KD9TAW', comment, cooldownSecs: 60 })),
  }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

import { DetachedPanel } from './DetachedPanel'
import { SpotsPanel } from './components/SpotsPanel'
import * as api from './api'
import { spotNeed } from './features/needs'
import { pastTheSwitch } from './components/ConnectView.testkit'

// THE BUDGET: DetachedPanel.connectBoards.test.tsx's, for the same mounts (15 s; a hang still fails).
vi.setConfig({ testTimeout: 15_000 })

type Call = [string, unknown[]]
function traffic(): Call[] {
  const out: Array<{ order: number; call: Call }> = []
  for (const [name, fn] of Object.entries(api)) {
    const m = (fn as { mock?: { calls: unknown[][]; invocationCallOrder: number[] } }).mock
    if (!m) continue
    m.calls.forEach((args, i) => out.push({ order: m.invocationCallOrder[i], call: [name, args] }))
  }
  return out.sort((a, b) => a.order - b.order).map((x) => x.call)
}
const writes = (calls: Call[]) => calls.filter(([n]) => !/^(get|ask|read|resolve|preview)/.test(n))
function clearMocks() {
  for (const fn of Object.values(api)) (fn as { mockClear?: () => void }).mockClear?.()
}

/** The Spots window's board, and the dashboard window's Spots box. */
const windowBoard = () => document.querySelector('main.spots-panel') as HTMLElement | null
const box = () => document.querySelector('.pane-frame[data-pane="spots"]') as HTMLElement | null
const callsIn = (root: HTMLElement | null) => [...(root?.querySelectorAll('.sp-row .np-call') ?? [])].map((c) => c.textContent)
const rowIn = (root: () => HTMLElement | null, call: string) => () =>
  ([...(root()?.querySelectorAll('.sp-row') ?? [])] as HTMLElement[]).find((r) => r.querySelector('.np-call')?.textContent === call) ?? null
const neededRow = (call: string) => () =>
  ([...document.querySelectorAll('.np-row:not(.np-header)')] as HTMLElement[]).find((r) => r.querySelector('.np-call')?.textContent?.startsWith(call)) ?? null

beforeEach(() => {
  localStorage.clear()
  pastTheSwitch()
  sessionStorage.clear()
  backend.spots = SPOTS
  backend.needs = []
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { cw: true, phone: true } }))
  localStorage.setItem('nexus.connect.chaseDefault.v1', '1')
  localStorage.setItem(
    'nexus.connect.config',
    JSON.stringify({
      slots: { left1: 'spots', left2: 'bandTiles', right1: 'pota', right2: 'outlook', bottom1: 'openings', bottom2: 'spacewx', bottom3: 'getout' },
      overlays: {},
    }),
  )
  window.history.replaceState(null, '', '/?panel=spots')
  clearMocks()
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
  window.history.replaceState(null, '', '/')
})

/** Mount `panel`'s window, click the row `find` returns once it is there, and return the traffic the click caused. */
async function clickAndRecord(panel: string, find: () => HTMLElement | null): Promise<Call[]> {
  window.history.replaceState(null, '', `/?panel=${panel}`)
  const r = render(<DetachedPanel panel={panel} />)
  await waitFor(() => expect(find(), `the row is on the ${panel} window`).not.toBeNull())
  clearMocks()
  await act(async () => {
    fireEvent.click(find()!)
  })
  await waitFor(() => expect(api.workSpot).toHaveBeenCalled())
  const calls = traffic()
  r.unmount()
  return calls
}

describe('the Spots window', () => {
  it('is the Spots board, with no Pop out of its own', () => {
    render(<DetachedPanel panel="spots" />)
    expect(windowBoard(), 'the Spots board is the window').not.toBeNull()
    expect(windowBoard()!.querySelector('h2')?.textContent).toBe(t('spots.title'))
    expect(document.body.textContent).not.toContain(t('detached.unavailable', { panel: 'spots' }))
    // Already a window of its own: a second Pop out would only focus this one.
    expect(document.querySelector('.np-popout')).toBeNull()
  })

  it("lists the station's spots, asked as it opens and again every 15 s", async () => {
    vi.useFakeTimers()
    render(<DetachedPanel panel="spots" />)
    await act(() => vi.advanceTimersByTimeAsync(100))
    expect(api.getAllSpots, 'asked as the window opens').toHaveBeenCalledTimes(1)
    expect(callsIn(windowBoard())).toEqual(['K1CW', 'JA1FT'])
    await act(() => vi.advanceTimersByTimeAsync(15_000))
    expect(api.getAllSpots, 'asked again 15 s later, the main window’s cadence').toHaveBeenCalledTimes(2)
  })

  for (const [call, expected] of [
    ['K1CW', ['cw', 14.025, '20m', 'K1CW', undefined]],
    ['JA1FT', ['digital', 18.104, '17m', 'JA1FT', 'FT4']],
  ] as const) {
    it(`${call}: a row works through this window's Needed work — the box's traffic and the Needed window's, no transmit command`, async () => {
      const fromWindow = await clickAndRecord('spots', rowIn(windowBoard, call))
      expect(writes(fromWindow)).toEqual([
        ['selectPeer', [call]],
        ['workSpot', [...expected]],
      ])
      const fromBox = await clickAndRecord('connect', rowIn(box, call))
      expect(writes(fromWindow), 'the window worked it exactly as the dashboard window’s Spots box does').toEqual(writes(fromBox))
      // The Needed window, handed the same station as a need — what the Spots view is to App.
      backend.needs = [{ ...spotNeed(SPOTS.find((s) => s.call === call)!), tags: ['NewEntity'] }]
      const fromNeeded = await clickAndRecord('needed', neededRow(call))
      expect(writes(fromWindow), 'the window worked it exactly as the Needed window does').toEqual(writes(fromNeeded))
      for (const verb of TX_VERBS) expect(fromWindow.some(([n]) => n === verb), `the window called ${verb}`).toBe(false)
    })
  }
})

describe("the Spots window and the dashboard window's Spots box are one board", () => {
  it("Hide worked keeps a station by the band-scoped board needs, in both: mode-feature neutral, band scopes honoured", async () => {
    // The CW cockpit switched off. W7B's new entity on 20 m CW still rescues it (the board's needs are
    // mode-feature neutral); W6A's new grid on 20 m does not (New grid is VHF+ unless Settings says otherwise).
    localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { cw: false, phone: true } }))
    backend.spots = [...SPOTS, ...WORKED]
    backend.needs = [need('W7B', ['NewEntity'], 14.03), need('W6A', ['NewGrid'], 14.035)]
    for (const [panel, root] of [['spots', windowBoard], ['connect', box]] as const) {
      window.history.replaceState(null, '', `/?panel=${panel}`)
      const r = render(<DetachedPanel panel={panel} />)
      await waitFor(() => expect(callsIn(root()), `${panel}: the needed station is rescued`).toContain('W7B'))
      expect(callsIn(root()), `${panel}: a need the band scope leaves off the board rescues nothing`).not.toContain('W6A')
      r.unmount()
    }
  })

  it('both offer Spot me', async () => {
    for (const [panel, root] of [['spots', windowBoard], ['connect', box]] as const) {
      window.history.replaceState(null, '', `/?panel=${panel}`)
      const r = render(<DetachedPanel panel={panel} />)
      await waitFor(() => expect(callsIn(root())).toContain('K1CW'))
      expect(root()!.querySelector('.np-spot-me .np-spot-btn'), `${panel}: Spot me`).not.toBeNull()
      r.unmount()
    }
  })

  it('Spot me posts one spot per press, and never on its own', async () => {
    vi.useFakeTimers()
    render(<DetachedPanel panel="spots" />)
    // Two spot polls, two of the needs and Settings, three of propagation.
    await act(() => vi.advanceTimersByTimeAsync(31_000))
    const press = windowBoard()?.querySelector('.np-spot-me .np-spot-btn') as HTMLButtonElement | null
    expect(press, 'Spot me is in the window').not.toBeNull()
    expect(api.clusterSelfSpot, 'nothing posted before a press').not.toHaveBeenCalled()
    fireEvent.click(press!)
    await act(() => vi.advanceTimersByTimeAsync(31_000))
    expect(api.clusterSelfSpot, 'one press, one spot').toHaveBeenCalledTimes(1)
  })
})

// THE WINDOW OPENS ON THE VIEW'S FILTERS (the operator, 2026-10-20: copy the main view's filters). Pop out in the
// main window's Spots view hands the view's filters, as they are at the press, to the window it opens; from there
// each window keeps its own. The view is the real board as App mounts it in the main window (its own copy of the
// filters, and Pop out); the window is the real DetachedPanel as main.tsx mounts it, under StrictMode.
//
// The board keeps its filters in sessionStorage, which the platform keeps one of per window; jsdom has one. So each
// step taken in a window brings that window's copy in and puts it back after (`inWindow`), while localStorage, the
// one store the two windows share, is left alone, as it is.
describe("the Spots window opens on the view's filters, then keeps its own", () => {
  type Win = 'main' | 'spots'
  let sessions: Record<Win, Record<string, string>>
  function inWindow<T>(win: Win, step: () => T): T {
    sessionStorage.clear()
    for (const [k, v] of Object.entries(sessions[win])) sessionStorage.setItem(k, v)
    const out = step()
    const copy: Record<string, string> = {}
    for (let i = 0; i < sessionStorage.length; i++) {
      const k = sessionStorage.key(i)!
      copy[k] = sessionStorage.getItem(k)!
    }
    sessions[win] = copy
    return out
  }
  /** What a hand-off record held each time the view asked for the window: in place BEFORE the ask, or the window
   *  could open first. */
  let askedWith: (string | null)[]
  beforeEach(() => {
    sessions = { main: {}, spots: {} }
    askedWith = []
  })

  const view = () =>
    render(
      <SpotsPanel
        spots={SPOTS}
        bandPlan={[]}
        selectedCall={null}
        onSelect={() => {}}
        onWork={() => {}}
        onPopOut={() => askedWith.push(localStorage.getItem('nexus.spotsPopOut.filters'))}
      />,
    )
  const popOut = () =>
    render(
      <StrictMode>
        <DetachedPanel panel="spots" />
      </StrictMode>,
    )
  const press = (main: HTMLElement) => inWindow('main', () => fireEvent.click(main.querySelector('.np-popout')!))

  const buttons = (root: Element | null | undefined) => [...(root?.querySelectorAll('button') ?? [])] as HTMLButtonElement[]
  const modeGroup = (root: HTMLElement) =>
    [...root.querySelectorAll('[role="group"]')].find((g) => g.getAttribute('aria-label') === t('spots.filters.modes.aria'))
  const bandChip = (root: HTMLElement, band: string) => buttons(root.querySelector('.np-filter-bands')).find((b) => b.textContent === band)!
  const modeChip = (root: HTMLElement, mode: string) => buttons(modeGroup(root)).find((b) => b.textContent === mode)!
  const hideWorkedChip = (root: HTMLElement) => buttons(root).find((b) => b.title === t('spots.filter.worked.title'))!
  /** The filters as the board shows them: the lit band chips, each mode chip and whether it is on, Hide worked,
   *  the sorted column and the search. */
  const filtersOf = (root: HTMLElement) => ({
    bands: [...root.querySelectorAll('.np-filter-bands .np-chip.active')].map((b) => b.textContent),
    modes: buttons(modeGroup(root)).map((b) => `${b.textContent} ${b.getAttribute('aria-pressed')}`),
    hideWorked: hideWorkedChip(root)?.getAttribute('aria-pressed'),
    sort: root.querySelector('.np-th.active')?.textContent,
    search: root.querySelector<HTMLInputElement>('input[type="search"]')?.value,
  })
  /** The view's picks, every one of them off its default. */
  const PICKED = { bands: ['20m'], modes: ['CW true', 'FT4 false'], hideWorked: 'false', sort: `${t('spots.column.call')} ▲`, search: 'K1' }
  const DEFAULTS = { bands: [], modes: ['CW true', 'FT4 true'], hideWorked: 'true', sort: `${t('spots.column.age')} ▲`, search: '' }
  function pick(main: HTMLElement) {
    inWindow('main', () => {
      fireEvent.click(bandChip(main, '20m'))
      fireEvent.click(modeChip(main, 'FT4'))
      fireEvent.click(hideWorkedChip(main))
      fireEvent.click(main.querySelector('.np-th[data-col="call"]')!)
      fireEvent.change(main.querySelector('input[type="search"]')!, { target: { value: 'K1' } })
    })
    expect(filtersOf(main), 'the view shows the picks').toEqual(PICKED)
  }
  /** The window, once its feed is in (the mode chips are the feed's). */
  async function fed(win: HTMLElement) {
    await waitFor(() => expect(modeGroup(win)).toBeDefined())
    return win
  }

  it("opens on the view's filters as Pop out was pressed: bands, modes, Hide worked, sort and search", async () => {
    const main = inWindow('main', view).container
    pick(main)
    press(main)
    const win = inWindow('spots', popOut).container
    // The first paint, before the feed: everything but the mode chips, which the feed brings.
    const { modes: _modes, ...beforeTheFeed } = PICKED
    const { modes: _none, ...firstPaint } = filtersOf(win)
    expect(firstPaint, "the window's first paint is the view's filters").toEqual(beforeTheFeed)
    await fed(win)
    expect(filtersOf(win), "the window opens on the view's filters").toEqual(PICKED)
    expect(callsIn(win.querySelector<HTMLElement>('main.spots-panel')), 'and lists what they let through').toEqual(['K1CW'])
    expect(askedWith, 'the filters were handed over before the window was asked for').toEqual([expect.any(String)])
  })

  it("leaves the view's own filters as they were: on screen, in its own copy, and when the view is opened again", () => {
    let main = inWindow('main', view)
    pick(main.container)
    const before = { ...sessions.main }
    press(main.container)
    expect(askedWith).toHaveLength(1)
    expect(filtersOf(main.container), 'the press moves nothing in the view').toEqual(PICKED)
    expect(sessions.main, '…nor in its own copy').toEqual(before)
    main.unmount()
    main = inWindow('main', view) // leave the Spots view and come back
    expect(filtersOf(main.container), 'the view comes back on its own filters').toEqual(PICKED)
  })

  it('a hand-off from an earlier press or an earlier run of the app is ignored; a slow window still gets its own', async () => {
    const main = inWindow('main', view).container
    pick(main)
    for (const [age, expected, why] of [
      [10_000, PICKED, "a window ten seconds slow to open still opens on the view's filters"],
      [61_000, DEFAULTS, "an earlier press's, over a minute old, is not this window's"],
      [86_400_000, DEFAULTS, "an earlier run's, a day old, is not this window's"],
      [-3_600_000, DEFAULTS, 'one stamped an hour ahead, the clock set back since, is not this window’s'],
    ] as const) {
      const at = Date.now()
      const clock = vi.spyOn(Date, 'now').mockReturnValue(at)
      sessions.spots = {} // a new window
      let win: ReturnType<typeof popOut>
      try {
        press(main)
        clock.mockReturnValue(at + age)
        win = inWindow('spots', popOut)
      } finally {
        clock.mockRestore()
      }
      await fed(win.container)
      expect(filtersOf(win.container), why).toEqual(expected)
      win.unmount()
    }
  })

  it('the hand-off is taken once: closed and opened again with no press, the window opens on the defaults', async () => {
    const main = inWindow('main', view).container
    pick(main)
    press(main)
    const first = inWindow('spots', popOut)
    expect(filtersOf(await fed(first.container)), "the window opens on the view's filters").toEqual(PICKED)
    first.unmount()
    sessions.spots = {} // closing the window forgets its copy
    const second = inWindow('spots', popOut)
    expect(filtersOf(await fed(second.container)), 'a window no press opened opens on the defaults').toEqual(DEFAULTS)
  })

  it('a chip in either window never moves the other, and a press while the window is open moves nothing in it, even as it reloads', async () => {
    const main = inWindow('main', view).container
    pick(main)
    press(main)
    let win = inWindow('spots', popOut)
    expect(filtersOf(await fed(win.container)), "the window opens on the view's filters").toEqual(PICKED)

    // A chip in the window: the view moves neither on screen nor in its own copy.
    const mainCopy = { ...sessions.main }
    inWindow('spots', () => fireEvent.click(bandChip(win.container, '40m')))
    expect(filtersOf(win.container).bands, 'the chip lit in the window').toEqual(['40m', '20m'])
    expect(filtersOf(main), 'a chip in the window moves nothing in the view').toEqual(PICKED)
    expect(sessions.main, '…nor in its own copy').toEqual(mainCopy)

    // A chip in the view, and Pop out pressed again while the window is open (Rust only brings it forward).
    const winCopy = { ...sessions.spots }
    inWindow('main', () => fireEvent.click(bandChip(main, '15m')))
    press(main)
    expect(askedWith, 'the second press asked for the window, with the view as it is now').toHaveLength(2)
    expect(filtersOf(main).bands).toEqual(['20m', '15m'])
    expect(filtersOf(win.container).bands, 'a chip in the view and a second press move nothing in the window').toEqual(['40m', '20m'])
    expect(sessions.spots, '…nor in its own copy').toEqual(winCopy)

    // The window reloads (its session outlives a reload, as the platform's does): still its own, not that press's.
    win.unmount()
    win = inWindow('spots', popOut)
    expect(filtersOf(await fed(win.container)).bands, 'the window keeps its own as it reloads').toEqual(['40m', '20m'])
  })
})
