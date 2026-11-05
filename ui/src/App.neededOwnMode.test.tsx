// @vitest-environment jsdom
//
// A NEEDED ROW OPENS THE SCREEN OF ITS OWN MODE (operator ruling 2026-10-12: "JS8 rows open the JS8
// screen, PSK rows the PSK screen, FT4 the FT screen on FT4").
//
// Before, every row that was not CW, Phone or RTTY opened the FT screen. A JS8 or PSK31 "New mode" row
// went to FT8 at the band's FT8 channel and read as an FT8 need, and a POTA row heard in FT4 opened on
// whatever tier the FT screen was on. A row names its own mode in `exactMode`; its `mode` is a label, the
// class ("Digital") wherever the source did not say FT8, FT4 or RTTY.
//
// THIS MOUNTS THE REAL APP and stubs only the backend (api) and the waterfall canvas, so a click on a row
// reaches App's own work handler and the screen it opens really mounts. `workSpot` answers as the engine
// does (`work_spot`): the work tick moves on and the section it was told comes back as the hint every
// window follows (`workView`), so App's hint-following effect runs here exactly as it does after a real
// click. Asserted: what reached the backend (the Work's section, dial and tier), the screen the operator is
// left on (`nexus.view`), and that nothing on the transmit side was asked for.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor, fireEvent, screen, act } from '@testing-library/react'
import type { AppSnapshot } from './types'

const fx = vi.hoisted(() => {
  const need = (call: string, over: Record<string, unknown>) => ({
    call, entity: 'United States', band: '20m', zone: 5, tags: ['NewMode'], priority: 30, headline: 'New mode',
    mode: 'Digital', exactMode: null, freqMhz: null, park: null, ...over,
  })
  const ch = (band: string, dialMhz: number) => ({ band, group: 'HF', dialMhz, mode: 'USB', label: band, note: '' })
  return {
    rows: [
      // PSK Reporter's rows: the mode beside the class label, and no frequency of their own.
      need('K1JSE', { exactMode: 'JS8' }),
      need('W1PSK', { exactMode: 'PSK31' }),
      need('W2QPK', { exactMode: 'QPSK31' }),
      need('W3PSX', { exactMode: 'PSK63' }),
      need('W4PSY', { exactMode: 'PSK125' }),
      // A POTA activation in FT4: the class as its label, FT4 beside it, the spot's own frequency.
      need('K4POT', {
        exactMode: 'FT4', freqMhz: 14.0815, tags: ['NewPark', 'Pota'], park: { program: 'POTA', reference: 'US-0004' },
      }),
      // CONTROLS, rows whose landing must not move: a scored FT4 row (its label says FT4), an FT8 row, and a
      // row that names only a class.
      need('K5FTF', { mode: 'FT4', exactMode: 'FT4', freqMhz: 14.0812 }),
      need('K6FTE', { mode: 'FT8', exactMode: 'FT8' }),
      need('K7DIG', { tags: ['NewBand'] }),
    ],
    // The tier's own channels (`get_band_plan`, on FT8 here) and the JS8 and PSK screens' (`get_licensed_band_plan`).
    plans: {
      tier: [ch('20m', 14.074)],
      js8: [ch('20m', 14.078)],
      psk: [ch('40m', 7.07), ch('20m', 14.07)],
    } as Record<string, unknown[]>,
    // The PSK screen's state: on PSK31, with Rev on.
    psk: {
      armed: true, afcHz: 0, signal: false, centerHz: 1000, text: '', charConf: [], sending: false, latched: false,
      keyerError: null, mode: 'psk31', reverse: true,
    },
    // The engine's current snapshot (null = the testkit's at boot). Every command that answers with a
    // snapshot answers with THIS one, as the engine does: its work tick only moves on. A stub that
    // answered a later command with the boot snapshot moved the tick back to 0, and App's hint effect
    // read that as another Work naming no screen and opened the FT screen.
    engine: { snap: null as Record<string, unknown> | null },
  }
})

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  const { appApiAnswers, APP_SNAPSHOT } = await import('./appCockpits.testkit')
  const current = () => (fx.engine.snap ?? APP_SNAPSHOT) as typeof APP_SNAPSHOT & Record<string, unknown>
  const answers = appApiAnswers()
  // The testkit answers a cockpit's read, its entry and its arm with ONE mock, so each call would be
  // recorded under all three names. Apart here, because the transmit check and "the screen was entered"
  // read them by name.
  const js8State = await (answers.getJs8State as () => Promise<unknown>)()
  const rttyState = await (answers.getRttyState as () => Promise<unknown>)()
  return {
    ...auto,
    ...answers,
    getJs8State: vi.fn(async () => js8State),
    js8Enter: vi.fn(async () => js8State),
    js8Arm: vi.fn(async () => js8State),
    getRttyState: vi.fn(async () => rttyState),
    rttyArm: vi.fn(async () => rttyState),
    rttyAutoArm: vi.fn(async () => rttyState),
    getSnapshot: vi.fn(async () => current()),
    setOperatingMode: vi.fn(async () => current()),
    setArea: vi.fn(async () => current()),
    selectPeer: vi.fn(async () => current()),
    getNeedAlerts: vi.fn(async () => fx.rows),
    getBandPlan: vi.fn(async () => fx.plans.tier),
    getLicensedBandPlan: vi.fn(async (mode: string) => fx.plans[mode] ?? []),
    getPskState: vi.fn(async () => fx.psk),
    pskArm: vi.fn(async () => fx.psk),
    pskAutoArm: vi.fn(async () => fx.psk),
    pskSetMode: vi.fn(async (mode: string, reverse: boolean) => ({ ...fx.psk, mode, reverse })),
    // As the engine answers: the tick moves on, the section comes back as the hint, the tier and dial land.
    workSpot: vi.fn(async (mode: string, freqMhz: number, band: string, call?: string, tier?: string) => {
      const s = current()
      fx.engine.snap = {
        ...s,
        radio: { ...s.radio, dialMhz: freqMhz, band },
        link: { ...s.link, tier: tier ?? s.link.tier },
        workTick: ((s.workTick as number | undefined) ?? 0) + 1,
        workView: mode,
        workCall: call ?? null,
      }
      return fx.engine.snap
    }),
    setHuntTarget: vi.fn(async () => current()),
    openPanelWindow: vi.fn(async () => {}),
  }
})
const toasted = vi.hoisted(() => vi.fn())
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: toasted,
  // Faithful to the real helper: a rejection becomes an error toast and a null result.
  withErrorToast: vi.fn(async (action: () => Promise<unknown>, fallback: string) => {
    try {
      return await action()
    } catch {
      toasted(fallback, 'error')
      return null
    }
  }),
}))
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))

import App from './App'
import * as api from './api'
import { APP_SNAPSHOT } from './appCockpits.testkit'

// Each test mounts the real App and the screen its Work opens: real work, which scales with the CPU a test
// gets (App.connectBoards.test.tsx measured it). What is checked is the traffic, not the speed.
const BUDGET = 30_000

/** Every transmit-capable command the api exposes (App.connectBoards.test.tsx's list). A switch keys only
 *  when turned ON: a `false` is a release, which a screen may send as it mounts. */
const TX_VERBS = [
  'callStation', 'startCq', 'startRunning', 'callCq', 'setChatCq', 'resumeChatCq', 'sendMessage', 'sendCw', 'setPtt',
  'playVoiceMessage', 'setTxEnabled', 'setTune', 'atuTune', 'setBeacon', 'aprsSendBeacon', 'aprsSendMessage',
  'rttyAutoArm', 'rttySend', 'rttySetLatched', 'rttyType', 'rttyAutoCq', 'rttyAutoAnswer', 'pskSend',
  'pskSetLatched', 'pskType', 'js8Send', 'js8SendCommand', 'js8CallCq', 'js8Arm', 'js8CqRepeat', 'sstvSend',
] as const
const SWITCHES = new Set(['setPtt', 'setTxEnabled', 'setTune', 'setBeacon', 'rttySetLatched', 'pskSetLatched', 'rttyAutoArm', 'js8Arm'])
type Call = [string, unknown[]]
const keyed = (calls: Call[]) =>
  calls.filter(([n, args]) => (TX_VERBS as readonly string[]).includes(n) && !(SWITCHES.has(n) && args[0] === false))
/** Every call any api mock received since the last clear, by name, in order. */
function traffic(): Call[] {
  const out: Array<{ order: number; call: Call }> = []
  for (const [name, fn] of Object.entries(api)) {
    const m = (fn as { mock?: { calls: unknown[][]; invocationCallOrder: number[] } }).mock
    if (!m) continue
    m.calls.forEach((args, i) => out.push({ order: m.invocationCallOrder[i], call: [name, args] }))
  }
  return out.sort((a, b) => a.order - b.order).map((x) => x.call)
}
function clearMocks() {
  for (const fn of Object.values(api)) (fn as { mockClear?: () => void }).mockClear?.()
  toasted.mockClear()
}

const row = (call: string) =>
  screen.getAllByRole('row').find((r) => r.getAttribute('aria-label')?.includes(call)) as HTMLElement
/** The names of the calls, for "the screen was entered": each keyboard cockpit makes its receive-only entry
 *  call (`js8Enter`, `pskAutoArm`) on the rising edge of being the visible screen, never while hidden. */
const names = (calls: Call[]) => calls.map(([n]) => n)

beforeEach(() => {
  localStorage.clear()
  // The Needed board is a core section; the auto-pop would move it into its own window.
  localStorage.setItem('nexus.needed.autopop', 'off')
  fx.engine.snap = null
  clearMocks()
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) => ({
    matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
  }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(cleanup)

/** Switch screens off the way Settings ▸ Features stores it. */
function featuresOff(...ids: string[]) {
  localStorage.setItem(
    'nexus.features.v1',
    JSON.stringify({ profile: 'custom', enabled: Object.fromEntries(ids.map((id) => [id, false])) }),
  )
}

async function boot() {
  window.location.hash = '#needed'
  const r = render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
  return r
}

/** Click `call`'s row on the docked board; return the Work's arguments, the screen it left, and the traffic. */
async function work(call: string) {
  const r = await boot()
  await waitFor(() => expect(row(call)).toBeTruthy())
  clearMocks()
  fireEvent.click(row(call))
  // The Work's last act is its toast, after it has opened its screen; App re-asserts that screen's rig mode as
  // it enters it (`setOperatingMode`, from its view effect).
  await waitFor(() => expect(toasted).toHaveBeenCalledWith(expect.stringContaining(call), 'success', 4000))
  await waitFor(() => expect(api.setOperatingMode).toHaveBeenCalled())
  const out = {
    works: vi.mocked(api.workSpot).mock.calls.map((c) => [...c]),
    view: localStorage.getItem('nexus.view'),
    calls: traffic(),
  }
  r.unmount()
  cleanup()
  return out
}

describe('a Needed row opens the screen of its own mode', () => {
  it('a JS8 row opens the JS8 screen, at the band’s JS8 channel, on the JS8 tier', async () => {
    const out = await work('K1JSE')
    expect(out.works).toEqual([['js8', 14.078, '20m', 'K1JSE', 'JS8']])
    expect(out.view).toBe('js8')
    expect(names(out.calls)).toContain('js8Enter')
    expect(keyed(out.calls)).toEqual([])
  }, BUDGET)

  it('a PSK31 row opens the PSK screen at the band’s PSK31 channel, already on PSK31', async () => {
    const out = await work('W1PSK')
    expect(out.works).toEqual([['keyboard', 14.07, '20m', 'W1PSK', undefined]])
    expect(out.view).toBe('psk')
    expect(names(out.calls)).toContain('pskAutoArm')
    expect(api.pskSetMode).not.toHaveBeenCalled()
    expect(keyed(out.calls)).toEqual([])
  }, BUDGET)

  it('a QPSK31 row opens the PSK screen on QPSK31, keeping its Rev setting', async () => {
    const out = await work('W2QPK')
    expect(out.works).toEqual([['keyboard', 14.07, '20m', 'W2QPK', undefined]])
    expect(out.view).toBe('psk')
    expect(vi.mocked(api.pskSetMode).mock.calls).toEqual([['qpsk31', true]])
    expect(keyed(out.calls)).toEqual([])
  }, BUDGET)

  it('a PSK63 row opens the PSK screen on PSK63', async () => {
    const out = await work('W3PSX')
    expect(out.works).toEqual([['keyboard', 14.07, '20m', 'W3PSX', undefined]])
    expect(out.view).toBe('psk')
    expect(vi.mocked(api.pskSetMode).mock.calls).toEqual([['psk63', true]])
    expect(keyed(out.calls)).toEqual([])
  }, BUDGET)

  it('a PSK125 row opens the PSK screen on the sub-mode it is on: the screen has no PSK125', async () => {
    const out = await work('W4PSY')
    expect(out.works).toEqual([['keyboard', 14.07, '20m', 'W4PSY', undefined]])
    expect(out.view).toBe('psk')
    expect(api.pskSetMode).not.toHaveBeenCalled()
    expect(keyed(out.calls)).toEqual([])
  }, BUDGET)

  it('a POTA row heard in FT4 opens the FT screen on FT4, at the spot’s own frequency', async () => {
    const out = await work('K4POT')
    expect(out.works).toEqual([['digital', 14.0815, '20m', 'K4POT', 'FT4']])
    expect(out.view).toBe('operate')
    expect(keyed(out.calls)).toEqual([])
  }, BUDGET)

  describe('controls: rows whose landing does not move', () => {
    it('a scored FT4 row: the FT screen on FT4', async () => {
      const out = await work('K5FTF')
      expect(out.works).toEqual([['digital', 14.0812, '20m', 'K5FTF', 'FT4']])
      expect(out.view).toBe('operate')
      expect(keyed(out.calls)).toEqual([])
    }, BUDGET)

    it('an FT8 row: the FT screen at the band’s FT8 channel, on the tier it is already on', async () => {
      const out = await work('K6FTE')
      expect(out.works).toEqual([['digital', 14.074, '20m', 'K6FTE', undefined]])
      expect(out.view).toBe('operate')
      expect(keyed(out.calls)).toEqual([])
    }, BUDGET)

    it('a row that names only a class: the FT screen at the band’s channel', async () => {
      const out = await work('K7DIG')
      expect(out.works).toEqual([['digital', 14.074, '20m', 'K7DIG', undefined]])
      expect(out.view).toBe('operate')
      expect(keyed(out.calls)).toEqual([])
    }, BUDGET)
  })

  describe('a screen switched off in Settings ▸ Features is not opened: the row lands where it did before', () => {
    it('JS8 off: a JS8 row opens the FT screen at the band’s channel', async () => {
      featuresOff('js8')
      const out = await work('K1JSE')
      expect(out.works).toEqual([['digital', 14.074, '20m', 'K1JSE', undefined]])
      expect(out.view).toBe('operate')
    }, BUDGET)

    it('PSK off: a PSK31 row opens the FT screen at the band’s channel', async () => {
      featuresOff('psk')
      const out = await work('W1PSK')
      expect(out.works).toEqual([['digital', 14.074, '20m', 'W1PSK', undefined]])
      expect(out.view).toBe('operate')
      expect(api.pskSetMode).not.toHaveBeenCalled()
    }, BUDGET)
  })
})

// A Work from a torn-off board or the dashboard window cannot navigate this window: the engine hands the
// section it was told back as the hint (`workView`), and this window opens the screen it names.
describe('the main window follows a Work from another window to the screen it names', () => {
  it('js8 opens JS8, keyboard opens PSK, digital opens FT; with JS8 off it stays put', async () => {
    let push: ((s: AppSnapshot) => void) | null = null
    vi.mocked(api.subscribeSnapshot).mockImplementation((cb: (s: AppSnapshot) => void) => {
      push = cb
      return () => {}
    })
    // The engine pushes a snapshot; from then on it is the one every command answers with.
    const send = (s: Record<string, unknown>) => {
      fx.engine.snap = { ...APP_SNAPSHOT, ...s }
      act(() => push!(fx.engine.snap as unknown as AppSnapshot))
    }
    const hint = async (workTick: number, workView: string) => {
      vi.mocked(api.setOperatingMode).mockClear()
      send({ workTick, workView, workCall: 'K1JSE' })
      await waitFor(() => expect(api.setOperatingMode).toHaveBeenCalled())
      return localStorage.getItem('nexus.view')
    }
    try {
      const r = await boot()
      await waitFor(() => expect(push).not.toBeNull())
      // The first snapshot is the baseline: a webview reload never replays the engine's last Work.
      send({ workTick: 0 })
      expect(await hint(1, 'js8')).toBe('js8')
      expect(await hint(2, 'keyboard')).toBe('psk')
      expect(await hint(3, 'digital')).toBe('operate')
      r.unmount()
      cleanup()

      featuresOff('js8')
      fx.engine.snap = null
      push = null
      await boot()
      await waitFor(() => expect(push).not.toBeNull())
      send({ workTick: 0 })
      vi.mocked(api.setOperatingMode).mockClear()
      send({ workTick: 1, workView: 'js8', workCall: 'K1JSE' })
      // JS8 is off: the window stays where it was and enters no screen.
      expect(localStorage.getItem('nexus.view')).toBe('needed')
      expect(api.setOperatingMode).not.toHaveBeenCalled()
      // CONTROL, same window: the hint is read, and a screen that is on still opens.
      expect(await hint(2, 'keyboard')).toBe('psk')
    } finally {
      vi.mocked(api.subscribeSnapshot).mockImplementation(() => () => {})
    }
  }, BUDGET)
})
