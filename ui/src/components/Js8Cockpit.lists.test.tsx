// @vitest-environment jsdom
//
// THE JS8 LISTS AS JS8Call 3.0.3 HAS THEM, rendered: the RX pane (my offset plus what is for me, a
// line per message, ♢ at its end), Band activity's columns and its heartbeat toggle, the call list's
// marks, @ALLCALL and the groups, every row's menu (jump to offset and speed, add, remove, block,
// clear), the Inbox (reply, delete several, read on pick, filter, sort), the new-message notice and
// the sounds. These read the DOM; jsdom lays nothing out, so how the tables look is the browser
// probe's and a human's.
import { useState, type ReactNode } from 'react'
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, waitFor } from '@testing-library/react'
import { Js8Cockpit, type Js8ListSettings } from './Js8Cockpit'
import { JS8_VIEW_KEY, JS8_LISTS_KEY } from '../features/js8View'
import type { AppSnapshot, Js8ActivityRow, Js8State } from '../types'
import type { PanelLayoutApi, Js8PanelId } from '../features/panelState'

const T0 = Date.now() - 30_000
const P = 15_000

const frame = (o: Partial<Js8ActivityRow> & { atMs: number; freqHz: number; text: string }, first = true, last = true): Js8ActivityRow => ({
  speed: 'normal',
  snrDb: -10,
  dtS: 0.1,
  from: '',
  directedToMe: false,
  mine: false,
  complete: true,
  lowConf: false,
  flags: { first, last, assembled: false },
  ...o,
})

const js8Fixture = (): Js8State => ({
  speed: 'normal',
  rxSpeeds: 15,
  txEnabled: false,
  sending: false,
  hbOn: false,
  hbNextAtMs: null,
  hbIntervalMin: 0,
  cqOn: false,
  cqNextAtMs: null,
  cqIntervalMin: 0,
  autoreply: true,
  relay: true,
  hbAck: false,
  armed: { autoreply: false, cq: false, relay: false, hbAck: false, hb: false },
  idleMinutes: 0,
  idleLimitMin: 60,
  idleTripped: false,
  activity: [
    // My offset is 1500: W0IND's two-frame message there, and a heartbeat far below it.
    frame({ atMs: T0, freqHz: 1503, from: 'W0IND', text: 'W0IND: ' }, true, false),
    frame({ atMs: T0 + P, freqHz: 1503, text: 'QRV ON 40M' }, false, true),
    frame({ atMs: T0, freqHz: 600, from: 'K1HB', text: 'K1HB: @HB HEARTBEAT FN42 ' }),
    // A command to me at 900 Hz: printed in the RX pane wherever it is.
    frame({ atMs: T0 + P, freqHz: 900, from: 'G0ABC', directedToMe: true, text: 'G0ABC: KD9TAW SNR? ' }),
    // The reassembled line of W0IND's message: never a frame of its own.
    { ...frame({ atMs: T0 + P, freqHz: 1503, from: 'W0IND', text: 'QRV ON 40M' }), flags: { first: false, last: false, assembled: true } },
  ],
  stations: [
    { call: 'W0IND', grid: 'EN52', snrDb: -8, freqHz: 1503, speed: 'fast', lastMs: T0 + P, lastHb: false, lastCq: false, storedMsgs: 0, cqMs: T0 },
    { call: 'G0ABC', grid: null, snrDb: -19, freqHz: 900, speed: 'normal', lastMs: T0 + P, lastHb: false, lastCq: false, storedMsgs: 0, ackMs: T0 + P },
    { call: 'K1HB', grid: null, snrDb: -12, freqHz: 600, speed: 'normal', lastMs: T0, lastHb: true, lastCq: false, storedMsgs: 0 },
  ],
  inbox: [
    { id: 1, from: 'W0IND', to: 'KD9TAW', text: 'HELLO', path: ['W0IND'], state: 'unread', atMs: T0, freqHz: 1503, snrDb: -8 },
    { id: 2, from: 'G0ABC', to: 'KD9TAW', text: 'TEST', path: ['N0RLY', 'G0ABC'], state: 'read', atMs: T0 + 1, freqHz: 900, snrDb: -9 },
  ],
  queue: [],
  pendingReply: null,
  lastError: null,
})
const state: { current: Js8State } = { current: js8Fixture() }

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    getJs8State: vi.fn(async () => state.current),
    js8Enter: vi.fn(async () => state.current),
    js8SetSpeed: vi.fn(async () => state.current),
    js8InboxMark: vi.fn(async () => state.current),
    js8InboxDelete: vi.fn(async () => state.current),
    askLog: vi.fn(async () => ({})),
    getLicensedBandPlan: vi.fn(async () => []),
    setRxOffset: vi.fn(async () => ({})),
    setTxOffset: vi.fn(async () => ({})),
    js8SetFilter: vi.fn(async () => state.current),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('../confirm', () => ({ confirmDialog: vi.fn(async () => true) }))
vi.mock('../alerts', () => ({ beep: vi.fn(), doubleBeep: vi.fn() }))
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: (p: { modeIndicator?: ReactNode }) => <header className="cockpit-header">{p.modeIndicator}</header>,
}))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
// A Remote observer's receiver: allowed to move the receive offset, and nothing else.
const receiver = vi.hoisted(() => ({ rxAllowed: true, depthAllowed: false, tuneRx: vi.fn(), setDepth: vi.fn() }))
vi.mock('../remote-web/useReceiverSettings', () => ({ useReceiverSettings: () => receiver }))

const snap = {
  mycall: 'KD9TAW',
  mygrid: 'EN61',
  radio: {
    dialMhz: 7.078,
    band: '40m',
    catOk: true,
    sideband: 'USB',
    transmitting: false,
    txEnabled: false,
    txAllowed: true,
    rxOffsetHz: 1500,
    txOffsetHz: 1500,
    txLevel: 0.5,
  },
} as unknown as AppSnapshot

const panelCalls: [Js8PanelId, string][] = []
function fakePanels(removed: Js8PanelId[] = []): PanelLayoutApi<Js8PanelId> {
  return {
    layout: { v: 1, state: {}, share: {} },
    stateOf: (id) => (removed.includes(id) ? 'removed' : 'docked'),
    setPanelState: (id, s) => {
      panelCalls.push([id, s])
    },
    shareOf: () => 1,
    setShare: () => {},
    setShares: () => {},
    undo: () => {},
    canUndo: false,
    undoRemoves: [],
    reset: () => {},
  }
}

const lists = (o: Partial<Js8ListSettings> = {}): Js8ListSettings => ({
  rxBlocklist: [],
  highlightPrimary: [],
  highlightSecondary: [],
  eot: '♢',
  mfi: '……',
  sounds: [],
  groups: ['@FUN'],
  avoidAllcall: false,
  setRxBlocklist: vi.fn(async () => {}),
  setGroups: vi.fn(async () => {}),
  setAvoidAllcall: vi.fn(async () => {}),
  ...o,
})

beforeEach(() => {
  state.current = js8Fixture()
  panelCalls.length = 0
  window.localStorage.clear()
  vi.clearAllMocks()
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

async function renderCockpit(props: Partial<Parameters<typeof Js8Cockpit>[0]> = {}) {
  const r = render(<Js8Cockpit snap={snap} panels={fakePanels()} lists={lists()} {...props} />)
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}
/** One more poll of the engine (the cockpit polls every 500 ms). */
async function poll() {
  await act(async () => {
    await new Promise((r) => setTimeout(r, 650))
  })
}

const rxLines = () => Array.from(document.querySelectorAll('[data-pane="activity"] .js8-rx-line')).map((l) => [
  l.querySelector('.js8-freq')!.textContent,
  l.querySelector('.js8-text')!.textContent,
])
const bandOffsets = () => Array.from(document.querySelectorAll('[data-pane="offsets"] .js8-offset-row .js8-freq')).map((c) => c.textContent)
const stationRow = (call: string) => {
  const hit = Array.from(document.querySelectorAll('[data-pane="stations"] .js8-station')).find(
    (r) => r.querySelector('.js8-station-call')?.textContent === call,
  )
  expect(hit, `no call list row for ${call}`).toBeTruthy()
  return hit as HTMLElement
}

/** Open a menu by its trigger and pick an entry (through a submenu when `sub` names one). */
async function pick(trigger: Element, entry: string, sub?: string) {
  await act(async () => {
    fireEvent.pointerDown(trigger, { button: 0, pointerType: 'mouse' })
  })
  const find = (name: string) =>
    waitFor(() => {
      // An item's text without its tick or dot, and a submenu's without its ▸.
      const hit = Array.from(document.querySelectorAll('[role^="menuitem"]')).find((el) => el.textContent?.replace(/^[✓●]|▸$/g, '') === name)
      expect(hit, `no "${name}" in the menu`).toBeTruthy()
      return hit as HTMLElement
    })
  if (sub) {
    const s = await find(sub)
    await act(async () => {
      fireEvent.keyDown(s, { key: 'ArrowRight' })
    })
  }
  const item = await find(entry)
  await act(async () => {
    fireEvent.click(item)
  })
  return item
}
const paneMenu = (pane: string) => document.querySelector(`[data-pane="${pane}"] .pane-menu`)!
/** The menu entries as they stand, with whether each is disabled. */
async function menuEntries(trigger: Element): Promise<[string, boolean][]> {
  await act(async () => {
    fireEvent.pointerDown(trigger, { button: 0, pointerType: 'mouse' })
  })
  await waitFor(() => expect(document.querySelector('[role="menu"]')).not.toBeNull())
  return Array.from(document.querySelectorAll('[role="menu"] [role^="menuitem"]')).map((el) => [
    el.textContent ?? '',
    el.hasAttribute('data-disabled'),
  ])
}

describe('the RX pane is JS8Call’s', () => {
  it('prints my offset’s message on one line with the end mark, and what is for me wherever it is, nothing else', async () => {
    await renderCockpit()
    expect(rxLines()).toEqual([
      ['1503', 'W0IND: QRV ON 40M ♢ '],
      ['900', 'G0ABC: KD9TAW SNR? ♢ '],
    ])
  })

  it('takes its marks from Settings ▸ JS8, and hides a blocked call', async () => {
    await renderCockpit({ lists: lists({ eot: '#', rxBlocklist: ['W0IND'] }) })
    expect(rxLines()).toEqual([['900', 'G0ABC: KD9TAW SNR? # ']])
  })

  it('clears from its ⋯ menu, and prints what is heard after', async () => {
    await renderCockpit()
    await pick(paneMenu('activity'), 'Clear')
    expect(rxLines()).toEqual([])
    expect(JSON.parse(localStorage.getItem(JS8_LISTS_KEY)!).cleared.rx).toBeGreaterThan(0)
    state.current = {
      ...js8Fixture(),
      activity: [...js8Fixture().activity, frame({ atMs: Date.now() + 1000, freqHz: 1500, from: 'N0NEW', text: 'N0NEW: @ALLCALL CQ CQ CQ EM10 ' })],
    }
    await poll()
    expect(rxLines()).toEqual([['1500', 'N0NEW: @ALLCALL CQ CQ CQ EM10 ♢ ']])
  })
})

describe('Band activity is JS8Call’s', () => {
  it('shows JS8Call’s default columns and every frame of an offset, joined', async () => {
    await renderCockpit()
    expect(bandOffsets()).toEqual(['600 Hz', '900 Hz', '1503 Hz'])
    const heads = Array.from(document.querySelectorAll('[data-pane="offsets"] th')).map((th) => th.textContent)
    expect(heads).toEqual(['Offset', 'Age', 'SNR', 'Message(s)', ''])
    const row1503 = document.querySelectorAll('[data-pane="offsets"] .js8-offset-row')[2]
    expect(row1503.querySelector('.js8-text')!.textContent).toBe('W0IND: QRV ON 40M ♢')
    expect(document.querySelector('[data-pane="offsets"] .js8-dt'), 'Time delta is off by default').toBeNull()
  })

  it('turns a column on, the labels off and the heartbeats off from its ⋯, and remembers it', async () => {
    await renderCockpit()
    await pick(paneMenu('offsets'), 'Time delta', 'Show columns')
    await waitFor(() => expect(document.querySelectorAll('[data-pane="offsets"] .js8-dt')).toHaveLength(3))
    cleanup()
    await renderCockpit()
    expect(document.querySelector('[data-pane="offsets"] thead'), 'labels on by default').not.toBeNull()
    await pick(paneMenu('offsets'), 'Show column labels', 'Show columns')
    await waitFor(() => expect(document.querySelector('[data-pane="offsets"] thead')).toBeNull())
    expect(document.querySelector('[data-pane="stations"] thead'), 'one switch for every list, as JS8Call’s').toBeNull()
    cleanup()
    await renderCockpit()
    await pick(paneMenu('offsets'), 'Show band heartbeats and ACKs')
    expect(bandOffsets(), 'the heartbeat at 600 Hz is hidden').toEqual(['900 Hz', '1503 Hz'])
    const stored = JSON.parse(localStorage.getItem(JS8_VIEW_KEY)!)
    expect([stored.band.columns.tdrift, stored.labels, stored.showHb]).toEqual([true, false, false])
  })

  it('sorts by any of JS8Call’s orders', async () => {
    await renderCockpit()
    await pick(paneMenu('offsets'), 'SNR (strongest first)', 'Sort by')
    // SNRs: 600 −10, 900 −10, 1503 −10 for the heartbeat and the command; make one stronger.
    state.current = {
      ...js8Fixture(),
      activity: js8Fixture().activity.map((r) => (r.freqHz === 900 ? { ...r, snrDb: 5 } : r)),
    }
    await poll()
    expect(bandOffsets()[0]).toBe('900 Hz')
  })

  it('a row’s menu jumps both offsets there, and is refused while a message is queued', async () => {
    const api = await import('../api')
    await renderCockpit()
    const row = document.querySelectorAll('[data-pane="offsets"] .js8-offset-row')[1]
    await pick(row.querySelector('.js8-row-menu')!, 'Jump to 900 Hz')
    await waitFor(() => expect(api.setTxOffset).toHaveBeenCalledWith(900))
    expect(api.setRxOffset).toHaveBeenCalledWith(900)
    cleanup()
    vi.mocked(api.setTxOffset).mockClear()
    vi.mocked(api.setRxOffset).mockClear()
    state.current = { ...js8Fixture(), queue: [{ origin: 'operator', display: 'X', first: true, last: true }] }
    await renderCockpit({ txOffsetWhileSending: false })
    const menu = () => document.querySelectorAll('[data-pane="offsets"] .js8-offset-row')[1].querySelector('.js8-row-menu')!
    const entries = await menuEntries(menu())
    expect(entries.find(([name]) => name === 'Jump to 900 Hz')).toEqual(['Jump to 900 Hz', true])
    cleanup()
    await renderCockpit({ txOffsetWhileSending: false })
    await pick(menu(), 'Jump to 900 Hz')
    await poll()
    expect(api.setRxOffset, 'refused whole: RX stays too').not.toHaveBeenCalled()
    expect(api.setTxOffset).not.toHaveBeenCalled()
  })

  it('with “Allow TX offset changes while sending” on, a jump is taken while a message is queued, as JS8Call’s changeFreq takes it', async () => {
    // JS8Call refuses only `isMessageQueuedForTransmit() && !tx_qsy_allowed()` (mainwindow.cpp:
    // 5381-5392). The frame on the air moving with it is the engine's (js8_design_tests).
    const api = await import('../api')
    state.current = { ...js8Fixture(), sending: true, queue: [{ origin: 'operator', display: 'X', first: true, last: true }] }
    await renderCockpit({ txOffsetWhileSending: true })
    const menu = () => document.querySelectorAll('[data-pane="offsets"] .js8-offset-row')[1].querySelector('.js8-row-menu')!
    expect((await menuEntries(menu())).find(([name]) => name === 'Jump to 900 Hz')).toEqual(['Jump to 900 Hz', false])
    cleanup()
    await renderCockpit({ txOffsetWhileSending: true })
    await pick(menu(), 'Jump to 900 Hz')
    await waitFor(() => expect(api.setTxOffset).toHaveBeenCalledWith(900))
    expect(api.setRxOffset).toHaveBeenCalledWith(900)
  })

  it('offers “Centre filter at … Hz” after the jump while the decode filter is on, and centres it there, as JS8Call’s row menus do', async () => {
    // UI_Constructor.cpp:981-988 (Band activity) and :1251-1258 (the call list): only while the
    // filter is on, between "Jump to …Hz" and the speed.
    const api = await import('../api')
    const filter = { enabled: true, centerHz: 1500, widthHz: 500 }
    state.current = { ...js8Fixture(), controls: { filter, qsyCenterHz: 1500, hop: null } }
    await renderCockpit()
    const names = (await menuEntries(stationRow('W0IND').querySelector('.js8-row-menu')!)).map(([n, off]) => (off ? `(${n})` : n))
    expect(names.slice(0, 3)).toEqual(['Jump to 1503 Hz', 'Centre filter at 1503 Hz', 'Jump to Fast speed'])
    cleanup()
    await renderCockpit()
    await pick(document.querySelectorAll('[data-pane="offsets"] .js8-offset-row')[1].querySelector('.js8-row-menu')!, 'Centre filter at 900 Hz')
    await waitFor(() => expect(api.js8SetFilter).toHaveBeenCalledWith({ enabled: true, centerHz: 900, widthHz: 500 }))
    cleanup()
    state.current = { ...js8Fixture(), controls: { filter: { ...filter, enabled: false }, qsyCenterHz: 1500, hop: null } }
    await renderCockpit()
    const off = (await menuEntries(stationRow('W0IND').querySelector('.js8-row-menu')!)).map(([n]) => n)
    expect(off.slice(0, 2), 'with the filter off it is not offered').toEqual(['Jump to 1503 Hz', 'Jump to Fast speed'])
  })

  it('removes one offset’s activity until it is heard again', async () => {
    await renderCockpit()
    const row = document.querySelectorAll('[data-pane="offsets"] .js8-offset-row')[0]
    await pick(row.querySelector('.js8-row-menu')!, 'Remove activity')
    expect(bandOffsets()).toEqual(['900 Hz', '1503 Hz'])
  })
})

// JS8Call's "Minimal Column Labels" (Show Columns, one switch for both lists, off: mainwindow.cpp:
// 4657-4712): the short headings of its label map and the cells' units dropped (:4890-4898,
// displayBandActivity.cpp:232-251, displayCallActivity.cpp:280-291).
describe('Minimal column labels, as JS8Call’s', () => {
  const heads = (pane: string) => Array.from(document.querySelectorAll(`[data-pane="${pane}"] thead th`)).map((th) => th.textContent ?? '')
  /** A row's cell under the heading `head` (the header and the row hold their cells in one order). */
  const cell = (pane: string, row: Element, head: string) => {
    const i = heads(pane).indexOf(head)
    expect(i, `no "${head}" heading in ${pane}`).toBeGreaterThanOrEqual(0)
    return row.querySelectorAll('td')[i].textContent
  }
  const band1503 = () => document.querySelectorAll('[data-pane="offsets"] .js8-offset-row')[2]

  it('shortens both lists’ headings and drops the cells’ units, from either list’s ⋯, and is remembered', async () => {
    await renderCockpit()
    await pick(paneMenu('offsets'), 'Time delta', 'Show columns')
    await pick(paneMenu('stations'), 'Time delta', 'Show columns')
    expect(heads('offsets').filter(Boolean), 'control: the full headings').toEqual(['Offset', 'Age', 'SNR', 'Time delta', 'Message(s)'])
    expect(heads('stations').filter(Boolean)).toEqual(['Callsigns (3)', 'Age', 'SNR', 'Offset', 'Time delta', '✓', 'Name', 'Comment'])
    expect([cell('offsets', band1503(), 'Offset'), cell('offsets', band1503(), 'Time delta')], 'control: with units').toEqual(['1503 Hz', '100 ms'])
    expect([cell('stations', stationRow('W0IND'), 'Offset'), cell('stations', stationRow('W0IND'), 'Time delta')]).toEqual(['1503 Hz', '100 ms'])

    await pick(paneMenu('offsets'), 'Minimal column labels', 'Show columns')
    expect(heads('offsets').filter(Boolean)).toEqual(['Off', 'Age', 'SN', 'TD', 'Message(s)'])
    expect(heads('stations').filter(Boolean), 'one switch for both lists').toEqual(['Call(3)', 'Age', 'SN', 'Off', 'TD', '✓', 'Name', 'Comment'])
    expect([cell('offsets', band1503(), 'Off'), cell('offsets', band1503(), 'TD')]).toEqual(['1503', '100'])
    expect([cell('stations', stationRow('W0IND'), 'Off'), cell('stations', stationRow('W0IND'), 'TD')]).toEqual(['1503', '100'])
    expect(JSON.parse(localStorage.getItem(JS8_VIEW_KEY)!).minimal).toBe(true)

    cleanup()
    await renderCockpit()
    expect(heads('offsets').filter(Boolean), 'remembered').toEqual(['Off', 'Age', 'SN', 'TD', 'Message(s)'])
    await pick(paneMenu('stations'), 'Minimal column labels', 'Show columns')
    expect(heads('offsets').filter(Boolean), 'and off again from the call list’s ⋯').toEqual(['Offset', 'Age', 'SNR', 'Time delta', 'Message(s)'])
    expect(cell('offsets', band1503(), 'Offset')).toBe('1503 Hz')
  })
})

describe('the call list is JS8Call’s', () => {
  const marks = () =>
    Array.from(document.querySelectorAll('[data-pane="stations"] .js8-station:not(.js8-group)')).map((r) => [
      r.querySelector('.js8-station-call')!.textContent,
      r.querySelector('.js8-mark')?.textContent ?? '',
    ])

  it('puts @ALLCALL and my groups on top, then the stations by callsign, each with its one mark', async () => {
    await renderCockpit()
    const groups = Array.from(document.querySelectorAll('[data-pane="stations"] .js8-group .js8-station-call')).map((b) => b.textContent)
    expect(groups).toEqual(['@ALLCALL', '@FUN'])
    expect(marks()).toEqual([
      // W0IND has a message for me (⚑ beats its ☎), G0ABC is hearing me (★), K1HB none.
      ['W0IND', '⚑'],
      ['G0ABC', '★'],
      ['K1HB', ''],
    ])
    expect(stationRow('G0ABC').querySelector('.js8-mark .js8-chip')!.getAttribute('title')).toMatch(/^Hearing your station \(\d+s ago\)$/)
  })

  it('marks a CQ in the last five minutes ☎ and a blocked station ✕', async () => {
    state.current = { ...js8Fixture(), inbox: [] }
    await renderCockpit({ lists: lists({ rxBlocklist: ['K1HB'] }) })
    expect(marks()).toEqual([
      ['G0ABC', '★'],
      ['K1HB', '✕'],
      ['W0IND', '☎'],
    ])
  })

  it('blocks a station and leaves a group through the row’s menu, each one write', async () => {
    const l = lists()
    await renderCockpit({ lists: l })
    await pick(stationRow('K1HB').querySelector('.js8-row-menu')!, 'Block this station')
    expect(l.setRxBlocklist).toHaveBeenCalledWith(['K1HB'])
    const fun = Array.from(document.querySelectorAll('[data-pane="stations"] .js8-group')).find((r) => r.textContent?.includes('@FUN'))!
    await pick(fun.querySelector('.js8-row-menu')!, 'Remove this group')
    expect(l.setGroups).toHaveBeenCalledWith([])
  })

  it('adds a station or a group from the dialog, and refuses a name that is neither', async () => {
    const l = lists()
    await renderCockpit({ lists: l })
    await pick(paneMenu('stations'), 'Add new station or group…')
    const input = await waitFor(() => document.querySelector('.js8-add-form input') as HTMLInputElement)
    await act(async () => {
      fireEvent.change(input, { target: { value: 'xyz' } })
      fireEvent.submit(input.form!)
    })
    expect(document.querySelector('.js8-add-form [role="alert"]')!.textContent).toBe('XYZ is not a valid callsign or group')
    await act(async () => {
      fireEvent.change(input, { target: { value: 'k1new' } })
      fireEvent.submit(input.form!)
    })
    await waitFor(() => expect(stationRow('K1NEW')).toBeTruthy())
    await pick(paneMenu('stations'), 'Add new station or group…')
    const again = await waitFor(() => document.querySelector('.js8-add-form input') as HTMLInputElement)
    await act(async () => {
      fireEvent.change(again, { target: { value: '@net' } })
      fireEvent.submit(again.form!)
    })
    expect(l.setGroups).toHaveBeenCalledWith(['@FUN', '@NET'])
  })

  // JS8Call's "Remove Station" on @ALLCALL is `set_avoid_allcall(true)` and adding "@ALLCALL" is
  // `set_avoid_allcall(false)` (UI_Constructor.cpp:1108, :1075): Settings' "Do not participate in the
  // @ALLCALL group", which takes the row away while it is on.
  it('takes @ALLCALL off the list when I stop taking part in it, and back when I add it, the setting saved each time', async () => {
    // App's part in miniature: the setting it saves, handed back down as the list's.
    const saved: boolean[] = []
    function WithSetting() {
      const [avoid, setAvoid] = useState(false)
      const setAvoidAllcall = async (on: boolean) => {
        saved.push(on)
        setAvoid(on)
      }
      return <Js8Cockpit snap={snap} panels={fakePanels()} lists={lists({ avoidAllcall: avoid, setAvoidAllcall })} />
    }
    render(<WithSetting />)
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })
    const groups = () => Array.from(document.querySelectorAll('[data-pane="stations"] .js8-group .js8-station-call')).map((b) => b.textContent)
    expect(groups(), 'control: taking part').toEqual(['@ALLCALL', '@FUN'])
    const allcall = Array.from(document.querySelectorAll('[data-pane="stations"] .js8-group')).find((r) => r.textContent?.includes('@ALLCALL'))!
    await pick(allcall.querySelector('.js8-row-menu')!, 'Remove this group')
    expect(saved, 'not taking part, saved').toEqual([true])
    await waitFor(() => expect(groups()).toEqual(['@FUN']))
    await pick(paneMenu('stations'), 'Add new station or group…')
    const input = await waitFor(() => document.querySelector('.js8-add-form input') as HTMLInputElement)
    await act(async () => {
      fireEvent.change(input, { target: { value: '@allcall' } })
      fireEvent.submit(input.form!)
    })
    expect(saved, 'taking part again, saved').toEqual([true, false])
    await waitFor(() => expect(groups()).toEqual(['@ALLCALL', '@FUN']))
  })

  it('offers no Remove on @ALLCALL where the setting cannot be written (the Remote page)', async () => {
    await renderCockpit({ lists: lists({ setAvoidAllcall: undefined }) })
    const allcall = Array.from(document.querySelectorAll('[data-pane="stations"] .js8-group')).find((r) => r.textContent?.includes('@ALLCALL'))!
    const entries = await menuEntries(allcall.querySelector('.js8-row-menu')!)
    expect(entries.find(([n]) => n === 'Remove this group')).toEqual(['Remove this group', true])
  })

  it('removes a station until it is heard again, and clears every list after asking', async () => {
    const { confirmDialog } = await import('../confirm')
    await renderCockpit()
    await pick(stationRow('K1HB').querySelector('.js8-row-menu')!, 'Remove this station')
    expect(marks().map(([c]) => c)).toEqual(['W0IND', 'G0ABC'])
    await pick(paneMenu('stations'), 'Clear all lists…')
    expect(confirmDialog).toHaveBeenCalledTimes(1)
    await waitFor(() => expect(rxLines()).toEqual([]))
    expect(bandOffsets()).toEqual([])
    // W0IND keeps its row: it has a message for me, as JS8Call's inbox count brings it back.
    expect(marks().map(([c]) => c)).toEqual(['W0IND'])
  })

  it('a station row’s menu offers its offset and speed, and the inbox only where there are messages', async () => {
    await renderCockpit()
    const names = (await menuEntries(stationRow('W0IND').querySelector('.js8-row-menu')!)).map(([n, off]) => (off ? `(${n})` : n))
    expect(names).toEqual([
      'Jump to 1503 Hz',
      'Jump to Fast speed',
      'Show message inbox',
      'Pin to the top',
      'Add new station or group…',
      'Remove this station',
      'Block this station',
      'Clear',
      'Clear all lists…',
    ])
    cleanup()
    await renderCockpit()
    const k1hb = (await menuEntries(stationRow('K1HB').querySelector('.js8-row-menu')!)).map(([n, off]) => (off ? `(${n})` : n))
    expect(k1hb.slice(0, 3), 'K1HB is at my speed and sent me nothing').toEqual(['Jump to 600 Hz', '(Show message inbox)', 'Pin to the top'])
  })
})

describe('the Inbox is JS8Call’s message panel', () => {
  const inboxRows = () => Array.from(document.querySelectorAll('[data-pane="inbox"] .js8-inbox-row')).map((r) => r.querySelector('.js8-text')!.textContent)

  it('lists newest first, reads a message the moment it is picked, and replies by its path into the compose box', async () => {
    const api = await import('../api')
    await renderCockpit()
    expect(inboxRows()).toEqual(['TEST', 'HELLO'])
    const hello = document.querySelectorAll('[data-pane="inbox"] .js8-inbox-row')[1] as HTMLElement
    await act(async () => {
      fireEvent.click(hello)
    })
    expect(api.js8InboxMark).toHaveBeenCalledWith(1, 'read')
    const reply = document.querySelector('.js8-inbox-reply') as HTMLButtonElement
    expect(reply.disabled).toBe(false)
    await act(async () => {
      fireEvent.click(reply)
    })
    expect((document.querySelector('.js8-compose') as HTMLInputElement).value).toBe('W0IND MSG [MESSAGE]')
    expect((document.querySelector('.js8-to') as HTMLInputElement).value).toBe('')
    expect(api.js8Send, 'Reply sends nothing').not.toHaveBeenCalled()
    // A relayed message: the reply goes back by its path.
    await act(async () => {
      fireEvent.click(document.querySelectorAll('[data-pane="inbox"] .js8-inbox-row')[0])
    })
    await act(async () => {
      fireEvent.click(reply)
    })
    expect((document.querySelector('.js8-compose') as HTMLInputElement).value).toBe('N0RLY>G0ABC MSG [MESSAGE]')
  })

  it('deletes the ticked messages together, filters by station and sorts by a column', async () => {
    const api = await import('../api')
    await renderCockpit()
    for (const box of Array.from(document.querySelectorAll('[data-pane="inbox"] input[type="checkbox"]'))) {
      await act(async () => {
        fireEvent.click(box)
      })
    }
    const del = document.querySelector('.js8-inbox-delete') as HTMLButtonElement
    expect(del.textContent).toBe('Delete 2')
    await act(async () => {
      fireEvent.click(del)
      await Promise.resolve()
    })
    await waitFor(() => expect(api.js8InboxDelete).toHaveBeenCalledTimes(2))
    expect(vi.mocked(api.js8InboxDelete).mock.calls.map(([id]) => id).sort()).toEqual([1, 2])
    await act(async () => {
      fireEvent.change(document.querySelector('.js8-inbox-filter select')!, { target: { value: 'G0ABC' } })
    })
    expect(inboxRows()).toEqual(['TEST'])
    await act(async () => {
      fireEvent.change(document.querySelector('.js8-inbox-filter select')!, { target: { value: '' } })
    })
    const fromHead = Array.from(document.querySelectorAll('[data-pane="inbox"] th button')).find((b) => b.textContent?.startsWith('From'))!
    await act(async () => {
      fireEvent.click(fromHead)
    })
    expect(inboxRows(), 'From A→Z: "G0ABC via N0RLY" before "W0IND"').toEqual(['TEST', 'HELLO'])
    await act(async () => {
      fireEvent.click(fromHead)
    })
    expect(inboxRows()).toEqual(['HELLO', 'TEST'])
  })

  it('the call list’s "Show message inbox" picks the station, and brings a hidden Inbox back', async () => {
    await renderCockpit({ panels: fakePanels(['inbox']) })
    await pick(stationRow('W0IND').querySelector('.js8-row-menu')!, 'Show message inbox')
    expect(panelCalls).toContainEqual(['inbox', 'docked'])
  })
})

describe('a new message announces itself, and the sounds', () => {
  const newMail = () => ({
    ...js8Fixture(),
    inbox: [...js8Fixture().inbox, { id: 3, from: 'K1HB', to: 'KD9TAW', text: 'HI', path: ['K1HB'], state: 'unread' as const, atMs: T0, freqHz: 600, snrDb: -12 }],
  })

  it('stays up as JS8Call’s box does, 300 s, with a way to the message', async () => {
    const toast = await import('../toast')
    await renderCockpit()
    await poll()
    state.current = newMail()
    await poll()
    const call = vi.mocked(toast.pushToast).mock.calls.find(([msg]) => String(msg).startsWith('New message from K1HB'))
    expect(call, 'no notice').toBeTruthy()
    expect(call![1]).toBe('info')
    expect(call![2]).toBe(300_000)
    expect(call![3]?.actionLabel).toBe('Show')
    await act(async () => {
      call![3]!.action!()
    })
    expect((document.querySelector('.js8-inbox-filter select') as HTMLSelectElement).value).toBe('K1HB')
  })

  it('plays only the sounds switched on, for what is new since the last poll', async () => {
    const alerts = await import('../alerts')
    await renderCockpit({ lists: lists({ sounds: ['cq', 'inbox'] }) })
    await poll()
    expect(alerts.beep, 'nothing heard yet is new').not.toHaveBeenCalled()
    state.current = {
      ...newMail(),
      activity: [
        ...js8Fixture().activity,
        frame({ atMs: Date.now(), freqHz: 1800, from: 'N0CQ', text: 'N0CQ: @ALLCALL CQ CQ CQ EM10 ' }),
        frame({ atMs: Date.now(), freqHz: 700, from: 'W9HB', text: 'W9HB: @HB HEARTBEAT EM10 ' }),
      ],
    }
    await poll()
    expect(alerts.beep).toHaveBeenCalledTimes(1)
    expect(alerts.doubleBeep, 'the inbox rings twice').toHaveBeenCalledTimes(1)
    cleanup()
    vi.mocked(alerts.beep).mockClear()
    state.current = js8Fixture()
    await renderCockpit()
    await poll()
    state.current = newMail()
    await poll()
    expect(alerts.beep, 'every sound off by default').not.toHaveBeenCalled()
  })
})
