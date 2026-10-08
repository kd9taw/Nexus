// @vitest-environment jsdom
//
// THE DASHBOARD WINDOW'S BOXES WORK THROUGH THIS WINDOW'S OWN BOARD PATHS (plan piece H8).
//
// A pop-out cannot reach the main window's handlers; it has its own, and the main window follows a
// Work through the engine's workTick. So here "the board's own path" is this window's: the Needed
// pop-out's work (`workNeed`, which the Spots box calls with the spot as a need — exactly how the
// Spots view works a spot in the main window, `handleWorkSpot = handleWorkNeeded(spotNeed(s))`), and
// the POTA/SOTA pop-out's hunt (`huntOta`). THE REAL ConnectView and the real boards are mounted;
// only the backend is stubbed. Each case works the same station from the box and from the pop-out
// board and requires the identical backend traffic, and none of it a transmit command.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor, fireEvent, act } from '@testing-library/react'
import type { AppSnapshot, OtaSpot, SpotRow } from './types'

const snapshot = {
  mycall: 'KD9TAW', mygrid: 'EN52', mode: 'Normal', activePeer: null, hunt: null, logTick: 1,
  radio: { dialMhz: 14.074, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: false, txAllowed: true },
  link: { tier: 'FT8' }, stations: [],
} as unknown as AppSnapshot

const state = vi.hoisted(() => ({ needs: [] as unknown[] }))

const spot = (over: Partial<SpotRow>): SpotRow =>
  ({
    call: 'K1CW', entity: 'United States', zone: 5, state: null, band: '20m', freqMhz: 14.025, mode: 'CW',
    submode: 'CW', spotter: 'W3LPL', corroborators: [], ageSecs: 30, comment: '', licensed: true,
    spotterLocal: true, ...over,
  }) as SpotRow
const SPOTS: SpotRow[] = [
  spot({}),
  spot({ call: 'JA1FT', entity: 'Japan', band: '17m', freqMhz: 18.104, mode: 'Digital', submode: 'FT4' }),
]
const OTA: OtaSpot[] = [
  {
    program: 'POTA', reference: 'US-1000', name: 'Test park', activator: 'K9ABC', freqKhz: 14285,
    mode: 'SSB', spotter: null, comment: null, grid: null, newPark: false, bandOpen: false, huntedToday: false,
  },
]
const TX_VERBS = [
  'callStation', 'startCq', 'callCq', 'setChatCq', 'resumeChatCq', 'sendMessage', 'sendCw', 'setPtt',
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
    getNeedAlerts: vi.fn(async () => state.needs),
    getSettings: vi.fn(async () => null),
    getAllSpots: vi.fn(async () => SPOTS),
    getOtaSpots: vi.fn(async (program: string) => (program === 'POTA' ? OTA : [])),
    getActivation: vi.fn(async () => ({ program: null, reference: null, qsoCount: 0 })),
    parksCount: vi.fn(async () => 0),
    huntedParksCount: vi.fn(async () => 0),
    getBandOutlook: vi.fn(async () => ({ bands: [], asOf: 0 })),
    getGettingOut: vi.fn(async () => null),
    getSpaceWxScales: vi.fn(async () => ({ scales: null, alerts: [] })),
    getKc2gMuf: vi.fn(async () => []),
    getDxpedWindows: vi.fn(async () => []),
    getOtaMapSpots: vi.fn(async () => []),
    getContests: vi.fn(async () => []),
    getWindowBehind: vi.fn(async () => ({ supported: false, on: false })),
    workSpot: vi.fn(async () => snapshot),
    setHuntTarget: vi.fn(async () => snapshot),
  }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

import { DetachedPanel } from './DetachedPanel'
import * as api from './api'
import { spotNeed } from './remote-web/remote-work'
import { pastTheSwitch } from './components/ConnectView.testkit'

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

beforeEach(() => {
  localStorage.clear()
  pastTheSwitch()
  sessionStorage.clear()
  state.needs = []
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { cw: true, phone: true } }))
  localStorage.setItem('nexus.connect.chaseDefault.v1', '1')
  localStorage.setItem(
    'nexus.connect.config',
    JSON.stringify({
      slots: { left1: 'spots', left2: 'bandTiles', right1: 'pota', right2: 'outlook', bottom1: 'openings', bottom2: 'spacewx', bottom3: 'getout' },
      overlays: {},
    }),
  )
  window.history.replaceState(null, '', '/?panel=connect')
  clearMocks()
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
})
afterEach(() => {
  cleanup()
  window.history.replaceState(null, '', '/')
})

/** Click `row` in `panel` and return the backend traffic the click caused. */
async function clickAndRecord(panel: string, find: () => HTMLElement | null): Promise<Call[]> {
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

const boxRow = (call: string) => () => {
  const box = document.querySelector('.pane-frame[data-pane="spots"]')
  return ([...(box?.querySelectorAll('.sp-row') ?? [])] as HTMLElement[]).find((r) => r.querySelector('.np-call')?.textContent === call) ?? null
}
const neededRow = (call: string) => () =>
  ([...document.querySelectorAll('.np-row:not(.np-header)')] as HTMLElement[]).find((r) => r.querySelector('.np-call')?.textContent?.startsWith(call)) ?? null

describe("the dashboard window's Spots box works a spot through this window's own Needed work", () => {
  for (const [call, expected] of [
    ['K1CW', ['cw', 14.025, '20m', 'K1CW', undefined]],
    ['JA1FT', ['digital', 18.104, '17m', 'JA1FT', 'FT4']],
  ] as const) {
    it(`${call}: the same backend traffic as the Needed pop-out working it, and no transmit command`, async () => {
      const fromBox = await clickAndRecord('connect', boxRow(call))
      expect(writes(fromBox)).toEqual([
        ['selectPeer', [call]],
        ['workSpot', [...expected]],
      ])
      // The Needed pop-out, handed the same station as a need — what the Spots view is to App. A
      // need type, because the Needed board lists needs by type; the work path never reads it.
      state.needs = [{ ...spotNeed(SPOTS.find((s) => s.call === call)!), tags: ['NewEntity'] }]
      const fromBoard = await clickAndRecord('needed', neededRow(call))
      expect(writes(fromBox), "the box worked it exactly as this window's board does").toEqual(writes(fromBoard))
      for (const verb of TX_VERBS) expect(fromBox.some(([n]) => n === verb), `the box called ${verb}`).toBe(false)
    })
  }
})

describe("the dashboard window's Needed box works a need through this window's own Needed work", () => {
  it('the same backend traffic as the Needed pop-out working it, and no transmit command', async () => {
    state.needs = [{ ...spotNeed(SPOTS.find((s) => s.call === 'K1CW')!), tags: ['NewEntity'] }]
    localStorage.setItem(
      'nexus.connect.config',
      JSON.stringify({
        slots: { left1: 'needed', left2: 'bandTiles', right1: 'pota', right2: 'outlook', bottom1: 'openings', bottom2: 'spacewx', bottom3: 'getout' },
        overlays: {},
      }),
    )
    const boxNeed = () =>
      ([...(document.querySelector('.pane-frame[data-pane="needed"]')?.querySelectorAll('.np-row:not(.np-header)') ?? [])] as HTMLElement[]).find(
        (r) => r.querySelector('.np-call')?.textContent?.startsWith('K1CW'),
      ) ?? null
    const fromBox = await clickAndRecord('connect', boxNeed)
    expect(writes(fromBox)).toEqual([
      ['selectPeer', ['K1CW']],
      ['workSpot', ['cw', 14.025, '20m', 'K1CW', undefined]],
    ])
    const fromBoard = await clickAndRecord('needed', neededRow('K1CW'))
    expect(writes(fromBox), "the box worked it exactly as this window's board does").toEqual(writes(fromBoard))
    for (const verb of TX_VERBS) expect(fromBox.some(([n]) => n === verb), `the box called ${verb}`).toBe(false)
  })
})

describe("the dashboard window's POTA/SOTA box hunts through this window's own POTA/SOTA hunt", () => {
  it('the same tag and QSY as the POTA/SOTA pop-out, and no transmit command', async () => {
    const hunt = () => {
      const scope = document.querySelector('.pane-frame[data-pane="pota"]') ?? document.querySelector('.pota-view')
      return (scope?.querySelector('.pota-hunt-btn') as HTMLElement | null) ?? null
    }
    const fromBox = await clickAndRecord('connect', hunt)
    expect(writes(fromBox)).toEqual([
      ['setHuntTarget', ['K9ABC', 'POTA', 'US-1000']],
      ['workSpot', ['phone', 14.285, '20m', 'K9ABC']],
    ])
    const fromBoard = await clickAndRecord('pota', hunt)
    expect(writes(fromBox), "the box hunted exactly as this window's board does").toEqual(writes(fromBoard))
    for (const verb of TX_VERBS) expect(fromBox.some(([n]) => n === verb), `the box called ${verb}`).toBe(false)
  })
})
