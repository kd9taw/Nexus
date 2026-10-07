// @vitest-environment jsdom
//
// A WORK FROM A CONNECT BOX IS ITS BOARD'S OWN WORK — proven on the real App (plan piece H8).
//
// THIS MOUNTS THE REAL APP, the real ConnectView, the real Spots board and the real POTA/SOTA board,
// and stubs only the backend (api) and the waterfall canvas. Each case works the same station twice —
// from its box on Connect and from the board's own screen — and compares what reached the backend
// and where the operator ended up. They must be identical, because the box is handed the screen's
// own wiring object; and they must contain no transmit command, because that wiring keys nothing.
//
// What the box proves by being identical to the board, the engine proves once for both:
// `work_spot` into a band the licence does not cover QSYs (listening is always legal) and every
// transmit path then refuses — tempo-app's engine test
// `work_spot_outside_the_licence_moves_the_dial_and_every_transmit_path_refuses`, and
// `work_spot_keys_nothing` for the idle case. A UI cannot see that refusal; it can only prove that
// the box asks for exactly what the board asks for, which is what is asserted here.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor, fireEvent, within } from '@testing-library/react'
import type { AppSnapshot, OtaSpot, SpotRow } from './types'
import { t } from './i18n'

const snapshot = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  mode: 'Normal',
  radio: {
    dialMhz: 14.074, band: '20m', catOk: true, sideband: 'USB', transmitting: false,
    txEnabled: false, txAllowed: true, rxOffsetHz: 1500, txOffsetHz: 1500, txLevel: 0.5, slot: 0,
  },
  aiCw: { enabled: false, status: '', text: '' },
  link: { tier: 'FT8', periodSecs: 15, snrDb: -8, dtSec: 0.1, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
  stations: [], conversations: [], activePeer: null, qso: null, fieldDay: null,
  recentDecodes: [], harqRescues: 0, logTick: 1, hunt: null,
} as unknown as AppSnapshot

const spot = (over: Partial<SpotRow>): SpotRow =>
  ({
    call: 'K1CW', entity: 'United States', zone: 5, state: null, band: '20m', freqMhz: 14.025, mode: 'CW',
    submode: 'CW', spotter: 'W3LPL', corroborators: [], ageSecs: 30, comment: '', licensed: true,
    spotterLocal: true, ...over,
  }) as SpotRow
// A CW spot, an FT4 spot (the tier must travel with the Work) and a spot the operator may not
// transmit on (`licensed: false`, computed by the backend from the same tables as the TX lockout).
const SPOTS: SpotRow[] = [
  spot({}),
  spot({ call: 'JA1FT', entity: 'Japan', band: '17m', freqMhz: 18.104, mode: 'Digital', submode: 'FT4' }),
  spot({ call: 'VK9LOCK', entity: 'Norfolk Island', band: '20m', freqMhz: 14.02, licensed: false }),
]
const OTA: OtaSpot[] = [
  {
    program: 'POTA', reference: 'US-1000', name: 'Test park', activator: 'K9ABC', freqKhz: 14285,
    mode: 'SSB', spotter: null, comment: null, grid: null, newPark: false, bandOpen: false, huntedToday: false,
  },
]

/** Every transmit-capable command the api exposes (the voice keyer, CW, PTT, Tune, the FT/Tempo
 *  sequencer, RTTY, PSK, JS8, SSTV, APRS, the beacon and the latches). A box's Work must reach none
 *  of them — nor, being the board's own Work, does the board's. The switches among them key only
 *  when turned ON: a `false` is a release, which the screen a Work opens may send as it mounts. */
const TX_VERBS = [
  'callStation', 'startCq', 'callCq', 'setChatCq', 'resumeChatCq', 'sendMessage', 'sendCw', 'setPtt',
  'playVoiceMessage', 'setTxEnabled', 'setTune', 'atuTune', 'setBeacon', 'aprsSendBeacon', 'aprsSendMessage',
  'rttyAutoArm', 'rttySend', 'rttySetLatched', 'rttyType', 'rttyAutoCq', 'rttyAutoAnswer', 'pskSend',
  'pskSetLatched', 'pskType', 'js8Send', 'js8SendCommand', 'js8CallCq', 'js8Arm', 'js8CqRepeat', 'sstvSend',
] as const
const SWITCHES = new Set(['setPtt', 'setTxEnabled', 'setTune', 'setBeacon', 'rttySetLatched', 'pskSetLatched', 'rttyAutoArm', 'js8Arm'])
/** The calls that could put the rig on the air: any transmit verb, a switch only when set on. */
const keyed = (calls: Call[]) =>
  calls.filter(([n, args]) => (TX_VERBS as readonly string[]).includes(n) && !(SWITCHES.has(n) && args[0] === false))

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    askLog: vi.fn(async () => {
      throw new Error('no log in this test')
    }),
    getSnapshot: vi.fn(async () => snapshot),
    subscribeSnapshot: vi.fn(() => () => {}),
    selectPeer: vi.fn(async () => snapshot),
    getAwards: vi.fn(async () => ({ achievements: [] })),
    getJourney: vi.fn(async () => ({ firsts: [], feats: [], ladders: [] })),
    getSettings: vi.fn(async () => null),
    getBandPlan: vi.fn(async () => []),
    getLicensedBandPlan: vi.fn(async () => []),
    getFdRuleset: vi.fn(async () => null),
    logOperators: vi.fn(async () => []),
    logActivations: vi.fn(async () => []),
    radioLaunchInfo: vi.fn(async () => ({ showPicker: false })),
    uiStateLoad: vi.fn(async () => ({})),
    uiStateSave: vi.fn(async () => ({})),
    getAllSpots: vi.fn(async () => SPOTS),
    getNeedAlerts: vi.fn(async () => []),
    getOtaSpots: vi.fn(async (program: string) => (program === 'POTA' ? OTA : [])),
    getActivation: vi.fn(async () => ({ program: null, reference: null, qsoCount: 0 })),
    parksCount: vi.fn(async () => 0),
    huntedParksCount: vi.fn(async () => 0),
    getPropagation: vi.fn(async () => null),
    getFeedHealth: vi.fn(async () => null),
    getXrayNow: vi.fn(async () => null),
    getDxpedWindows: vi.fn(async () => []),
    getBandOutlook: vi.fn(async () => ({ bands: [], asOf: 0 })),
    getGettingOut: vi.fn(async () => null),
    getPathOutlook: vi.fn(async () => null),
    getSpaceWxScales: vi.fn(async () => ({ scales: null, alerts: [] })),
    getKc2gMuf: vi.fn(async () => []),
    getAurora: vi.fn(async () => null),
    getDeclination: vi.fn(async () => null),
    getPca: vi.fn(async () => null),
    getSatellites: vi.fn(async () => null),
    getOtaMapSpots: vi.fn(async () => []),
    getLogStats: vi.fn(async () => null),
    getContests: vi.fn(async () => []),
    getSatSchedule: vi.fn(async () => []),
    getSatTrackStatus: vi.fn(async () => null),
    getIssPass: vi.fn(async () => null),
    getTleStatus: vi.fn(async () => null),
    setOperatingMode: vi.fn(async () => snapshot),
    setArea: vi.fn(async () => snapshot),
    appVersion: vi.fn(async () => '0.0.0-test'),
    workSpot: vi.fn(async () => snapshot),
    setHuntTarget: vi.fn(async () => snapshot),
    openPanelWindow: vi.fn(async () => {}),
    // What the cockpit a Work opens reads as it mounts, shaped so it mounts cleanly.
    getVoiceMessages: vi.fn(async () => []),
    resolveEntity: vi.fn(async () => null),
    previewCw: vi.fn(async () => ''),
    readRotator: vi.fn(async () => null),
    readRotatorState: vi.fn(async () => null),
    cwDecode: vi.fn(async () => ({ text: '', wpm: 0, sent: [], keyerError: null, candidates: [], rst: null, name: null })),
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
import { pastTheSwitch } from './components/ConnectView.testkit'

type Call = [string, unknown[]]
/** Every call any api mock received, by name, in order — the whole backend traffic. */
function traffic(): Call[] {
  const out: Array<{ order: number; call: Call }> = []
  for (const [name, fn] of Object.entries(api)) {
    const m = (fn as { mock?: { calls: unknown[][]; invocationCallOrder: number[] } }).mock
    if (!m) continue
    m.calls.forEach((args, i) => out.push({ order: m.invocationCallOrder[i], call: [name, args] }))
  }
  return out.sort((a, b) => a.order - b.order).map((x) => x.call)
}
/** The writes: everything but the reads a mounted screen makes. `appVersion` is the top bar's read of
 *  the app's version, made when the bar mounts: Connect draws no top bar (the operator, 2026-10-01:
 *  "remove all radio control from connect, reclaim that space"), so a Work from a Connect box that
 *  opens a cockpit mounts the bar, and its read lands among these calls. */
const writes = (calls: Call[]) => calls.filter(([n]) => !/^(get|ask|read|resolve|preview|cwDecode|cwSkim|uiStateLoad|appVersion)/.test(n))
/** The Work itself: the writes up to and including the QSY. */
const theWork = (calls: Call[]) => {
  const w = writes(calls)
  return w.slice(0, w.findIndex(([n]) => n === 'workSpot') + 1)
}
/** A Work's window: the traffic from the click to the entry write of the cockpit it opens. App re-asserts the rig
 *  mode of every cockpit it enters (`setOperatingMode(mode, false)`, from its view effect), in the commit that mounts
 *  the cockpit, after the cockpit's own mount effects. What the cockpit then does on its own clock is not the Work's:
 *  the CW and Phone log strips look the worked call up in the callbook 700 ms after it lands in them, and on a slow
 *  box that lookup landed inside a fixed 50 ms wait in one of a test's two runs and not the other (2026-09-30). No
 *  entry write, and the window is everything seen, for the assertions to name. */
function toEntry(calls: Call[]): Call[] {
  const work = calls.findIndex(([n]) => n === 'workSpot')
  const entry = calls.findIndex(([n], i) => i > work && n === 'setOperatingMode')
  return work < 0 || entry < 0 ? calls : calls.slice(0, entry + 1)
}

// THE BUDGET (2026-09-30). Every test here mounts the real App and, with each Work, the cockpit the Work opens:
// real work, and it scales with the CPU a test gets. The longest, the file's first (it also warms up), takes
// 0.6 s on a quiet box, 4.1 s with a fifth of a CPU, 8.5 s with a tenth and 12.9 s with a sixteenth, where
// vitest's 5 s default runs out; in full-suite runs on a loaded box a test of two Works took 6.3 s. So each
// test has 30 s. What is checked here is the traffic, not the speed.
const BUDGET = 30_000

/** Wait for a Work's window to close: the entry write (`toEntry`). A condition, and outside act. Not `act(async)`
 *  around a sleep, as this file had: App re-renders itself on a 400 ms clock, act flushes every render queued while
 *  it waits, and once one render of the whole App takes longer than 400 ms (a loaded box) the next is due before act
 *  can finish, and it went on flushing: 6 s inside one click at a sixteenth of a CPU, and two tests past a 30 s
 *  budget in one of three runs at a tenth. A Work that opens no cockpit writes no entry: then the window is what
 *  arrived by the wait's end. */
async function settled() {
  await waitFor(() => expect(api.setOperatingMode).toHaveBeenCalled()).catch(() => {})
}

function clearMocks() {
  for (const fn of Object.values(api)) (fn as { mockClear?: () => void }).mockClear?.()
}

beforeEach(() => {
  localStorage.clear()
  pastTheSwitch()
  sessionStorage.clear()
  localStorage.setItem(
    'nexus.features.v1',
    JSON.stringify({ profile: 'custom', enabled: { connect: true, spots: true, pota: true, cw: true, phone: true } }),
  )
  localStorage.setItem('nexus.connect.autopop', 'off')
  localStorage.setItem('nexus.needed.autopop', 'off')
  // Connect with the Spots box in the left rail and the POTA/SOTA box in the right.
  localStorage.setItem('nexus.connect.chaseDefault.v1', '1')
  localStorage.setItem(
    'nexus.connect.config',
    JSON.stringify({
      slots: { left1: 'spots', left2: 'bandTiles', right1: 'pota', right2: 'outlook', bottom1: 'openings', bottom2: 'spacewx', bottom3: 'getout' },
      overlays: {},
    }),
  )
  clearMocks()
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) => ({
    matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
  }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(cleanup)

async function boot(view: 'connect' | 'spots' | 'pota' | 'needed') {
  window.location.hash = `#${view}`
  const r = render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
  return r
}

/** The Spots row for `call` — in the Connect box when `box`, else on the Spots screen. */
async function spotsRow(call: string, box: boolean): Promise<HTMLElement> {
  const scope = () => (box ? (document.querySelector('.pane-frame[data-pane="spots"]') as HTMLElement | null) : document.body)
  await waitFor(() => {
    const root = scope()
    expect(root, 'the Spots box is on Connect').not.toBeNull()
    expect([...root!.querySelectorAll('.sp-row .np-call')].map((c) => c.textContent)).toContain(call)
  })
  return [...scope()!.querySelectorAll('.sp-row')].find((r) => r.querySelector('.np-call')?.textContent === call) as HTMLElement
}

/** Work `call` from the box or the screen; return what reached the backend and the view it left. */
async function workSpotRow(call: string, box: boolean) {
  const r = await boot(box ? 'connect' : 'spots')
  const row = await spotsRow(call, box)
  clearMocks()
  fireEvent.click(row)
  await waitFor(() => expect(api.workSpot).toHaveBeenCalled())
  await settled()
  const out = { calls: toEntry(traffic()), view: localStorage.getItem('nexus.view') }
  r.unmount()
  cleanup()
  return out
}

describe('the Spots box works a spot exactly as the Spots screen does', () => {
  it("reaches the board's own path: the same backend call, the same cockpit", async () => {
    const fromBox = await workSpotRow('K1CW', true)
    const fromBoard = await workSpotRow('K1CW', false)
    expect(theWork(fromBox.calls)).toEqual([
      ['selectPeer', ['K1CW']],
      ['workSpot', ['cw', 14.025, '20m', 'K1CW', undefined]],
    ])
    // The whole window, the cockpit it opens included (App re-asserts that cockpit's rig mode on
    // entry, as on every entry — `setOperatingMode(mode, false)`, the dial kept).
    expect(writes(fromBox.calls), 'the box asked for exactly what the board asks for').toEqual(writes(fromBoard.calls))
    expect(fromBox.view, 'the box opens the cockpit the board opens').toBe('cw')
    expect(fromBoard.view).toBe('cw')
  }, BUDGET)

  it("carries an FT4 spot's tier in the same one call, as the board does", async () => {
    const fromBox = await workSpotRow('JA1FT', true)
    const fromBoard = await workSpotRow('JA1FT', false)
    expect(theWork(fromBox.calls)).toEqual([
      ['selectPeer', ['JA1FT']],
      ['workSpot', ['digital', 18.104, '17m', 'JA1FT', 'FT4']],
    ])
    expect(writes(fromBox.calls)).toEqual(writes(fromBoard.calls))
    expect(fromBox.view).toBe('operate')
  }, BUDGET)

  it("treats a spot outside the operator's privileges exactly as the board does: the same QSY, which listening always allows; the engine refuses the transmit", async () => {
    const fromBox = await workSpotRow('VK9LOCK', true)
    const fromBoard = await workSpotRow('VK9LOCK', false)
    expect(writes(fromBox.calls)).toEqual(writes(fromBoard.calls))
    expect(theWork(fromBox.calls)).toEqual([
      ['selectPeer', ['VK9LOCK']],
      ['workSpot', ['cw', 14.02, '20m', 'VK9LOCK', undefined]],
    ])
  }, BUDGET)

  it("hides an out-of-privilege spot with the board's own privileges chip, in the box as on the screen", async () => {
    await boot('connect')
    await spotsRow('VK9LOCK', true)
    const box = document.querySelector('.pane-frame[data-pane="spots"]') as HTMLElement
    fireEvent.click(within(box).getByRole('button', { name: new RegExp(t('spots.filter.toggle.active')) }))
    fireEvent.click(within(box).getByRole('button', { name: t('spots.filter.privileges.label') }))
    const calls = [...box.querySelectorAll('.sp-row .np-call')].map((c) => c.textContent)
    expect(calls).toContain('K1CW')
    expect(calls).not.toContain('VK9LOCK')
  }, BUDGET)

  // One test per spot, from the box and from the board: the six Works in one test were three times the work
  // of any other test here.
  for (const call of ['K1CW', 'JA1FT', 'VK9LOCK'])
    it(`transmits nothing: no transmit command, from the box or the board, working ${call}`, async () => {
      for (const box of [true, false]) {
        const { calls } = await workSpotRow(call, box)
        expect(keyed(calls), `${box ? 'the box' : 'the board'} working ${call}`).toEqual([])
      }
    }, BUDGET)
})

describe('the Needed box works a need exactly as the Needed screen does', () => {
  const K1CW_NEED = {
    call: 'K1CW', entity: 'United States', band: '20m', zone: 5, tags: ['NewBand'], priority: 50,
    headline: 'New band — United States 20m', mode: 'CW', freqMhz: 14.025,
  }
  async function workNeedRow(box: boolean) {
    // Connect with the Needed box in the bottom row, in place of the Openings box.
    localStorage.setItem(
      'nexus.connect.config',
      JSON.stringify({
        slots: { left1: 'spots', left2: 'bandTiles', right1: 'pota', right2: 'outlook', bottom1: 'needed', bottom2: 'spacewx', bottom3: 'getout' },
        overlays: {},
      }),
    )
    const r = await boot(box ? 'connect' : 'needed')
    const root = () => (box ? (document.querySelector('.pane-frame[data-pane="needed"]') as HTMLElement | null) : document.body)
    const row = await waitFor(() => {
      const x = [...(root()?.querySelectorAll<HTMLElement>('.np-row:not(.np-header)') ?? [])].find((el) =>
        el.querySelector('.np-call')?.textContent?.startsWith('K1CW'),
      )
      if (!x) throw new Error(box ? 'the Needed box does not list the need' : 'the Needed screen does not list the need')
      return x
    })
    clearMocks()
    fireEvent.click(row)
    await waitFor(() => expect(api.workSpot).toHaveBeenCalled())
    await settled()
    const out = { calls: toEntry(traffic()), view: localStorage.getItem('nexus.view') }
    r.unmount()
    cleanup()
    return out
  }

  it("reaches the board's own path: the same backend call, the same cockpit, and transmits nothing", async () => {
    vi.mocked(api.getNeedAlerts).mockResolvedValue([K1CW_NEED] as unknown as Awaited<ReturnType<typeof api.getNeedAlerts>>)
    try {
      const fromBox = await workNeedRow(true)
      const fromBoard = await workNeedRow(false)
      expect(theWork(fromBox.calls)).toEqual([
        ['selectPeer', ['K1CW']],
        ['workSpot', ['cw', 14.025, '20m', 'K1CW', undefined]],
      ])
      expect(writes(fromBox.calls), 'the box asked for exactly what the board asks for').toEqual(writes(fromBoard.calls))
      expect(fromBox.view, 'the box opens the cockpit the board opens').toBe('cw')
      expect(fromBoard.view).toBe('cw')
      expect(keyed(fromBox.calls), 'a Work from the box').toEqual([])
      expect(keyed(fromBoard.calls), 'a Work from the board').toEqual([])
    } finally {
      vi.mocked(api.getNeedAlerts).mockImplementation(async () => [])
    }
  }, BUDGET)
})

describe('the POTA/SOTA box hunts exactly as the POTA/SOTA screen does', () => {
  async function hunt(box: boolean) {
    const r = await boot(box ? 'connect' : 'pota')
    const root = () => (box ? (document.querySelector('.pane-frame[data-pane="pota"]') as HTMLElement | null) : document.body)
    await waitFor(() => expect(root()?.querySelector('.pota-hunt-btn')).toBeTruthy())
    clearMocks()
    fireEvent.click(within(root()!).getByRole('button', { name: t('ota.hunt.button.aria', { call: 'K9ABC' }) }))
    await waitFor(() => expect(api.workSpot).toHaveBeenCalled())
    await settled()
    const out = { calls: toEntry(traffic()), view: localStorage.getItem('nexus.view') }
    r.unmount()
    cleanup()
    return out
  }

  it("tags the hunt and QSYs through the board's own path, and transmits nothing", async () => {
    const fromBox = await hunt(true)
    const fromBoard = await hunt(false)
    expect(theWork(fromBox.calls)).toEqual([
      ['setHuntTarget', ['K9ABC', 'POTA', 'US-1000']],
      ['workSpot', ['phone', 14.285, '20m', 'K9ABC', undefined]],
    ])
    expect(writes(fromBox.calls), 'the box hunted exactly as the board hunts').toEqual(writes(fromBoard.calls))
    expect(fromBox.view).toBe('phone')
    expect(fromBoard.view).toBe('phone')
    expect(keyed(fromBox.calls), 'a hunt from the box').toEqual([])
    expect(keyed(fromBoard.calls), 'a hunt from the board').toEqual([])
  }, BUDGET)
})
