// @vitest-environment jsdom
//
// THE JS8Call PRESENTATION PARITY ROUND (2026-09) — three gaps, one cockpit:
//
//   1. the call-activity roster's DX columns: ★ pin · Distance · Azimuth · ✓ worked-before ·
//      Name · Comment (JS8Call's tableWidgetCalls, mainwindow.ui:1265-1348). Distance and
//      azimuth are a grid join, worked-before/name/comment a LOGBOOK join — no new engine
//      state, and a column with nothing behind it renders NOTHING rather than a blank.
//   2. `offsets`, the band-activity-by-offset pane with the DT column (tableWidgetRXAll,
//      mainwindow.ui:989), an ordinary ⊞-hideable pane in the JS8 vocabulary.
//   3. the two "differs from JS8Call" tooltips: the fused transcript, and the arm chips that
//      look dead until the session TX latch is up.
//
// These RENDER the cockpit and read the DOM. jsdom does not lay out, so nothing here says the
// columns are readable at any window size — only that they exist, carry the right values, and
// sit inside the pane region. The layout half is a human sweep (tasks/js8-activity-parity.md).
import type { ReactNode } from 'react'
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent } from '@testing-library/react'
import { Js8Cockpit } from './Js8Cockpit'
import { JS8_PINS_KEY } from '../features/js8Pins'
import type { AppSnapshot, Js8State, LoggedQso } from '../types'
import type { PanelLayoutApi, Js8PanelId } from '../features/panelState'

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
    // Two decodes at one offset (the newer wins its bucket), one 8 Hz away (inside the ±10 Hz
    // tolerance — it JOINS and re-keys), and one far below.
    { atMs: 1_757_000_000_000, speed: 'normal', freqHz: 1500, snrDb: -14, dtS: 0.9, from: 'W0IND', text: 'OLD', directedToMe: false, mine: false, complete: true, lowConf: false },
    { atMs: 1_757_000_015_000, speed: 'normal', freqHz: 1508, snrDb: -8, dtS: 0.2, from: 'W0IND', text: 'W0IND: @ALLCALL CQ CQ CQ EN52', directedToMe: false, mine: false, complete: true, lowConf: false },
    { atMs: 1_757_000_010_000, speed: 'fast', freqHz: 700, snrDb: -3, dtS: -0.4, from: 'G0ABC', text: 'G0ABC: KD9TAW HI', directedToMe: true, mine: false, complete: true, lowConf: false },
  ],
  stations: [
    { call: 'W0IND', grid: 'EN52', snrDb: -8, freqHz: 1508, speed: 'normal', lastMs: 1_757_000_015_000, lastHb: false, lastCq: true, storedMsgs: 0 },
    // No grid, and nothing in the log: every joined column must be ABSENT for this one.
    { call: 'N0GRD', grid: null, snrDb: -19, freqHz: 900, speed: 'normal', lastMs: 1_757_000_010_000, lastHb: true, lastCq: false, storedMsgs: 0 },
  ],
  inbox: [],
  queue: [],
  pendingReply: null,
  lastError: null,
})
const state: { current: Js8State } = { current: js8Fixture() }

/** One prior QSO with W0IND — the source of ✓, Name and Comment (and, when a station has sent
 *  no grid, of the grid the distance/azimuth are taken from). */
const logFixture = (): LoggedQso[] => [
  {
    call: 'W0IND',
    grid: 'EN52',
    band: '40m',
    freqMhz: 7.078,
    mode: 'MFSK',
    rstSent: '-08',
    rstRcvd: '-11',
    name: 'Dave',
    comment: 'JS8 ragchew',
    whenUnix: 1_750_000_000,
    confirmed: false,
    awardConfirmed: false,
  } as LoggedQso,
]
const log: { current: LoggedQso[] } = { current: logFixture() }

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
    js8Arm: vi.fn(async () => state.current),
    getLog: vi.fn(async () => log.current),
    getLicensedBandPlan: vi.fn(async () => []),
    setRxOffset: vi.fn(async () => ({})),
    haltTx: vi.fn(async () => ({})),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: (p: { modeIndicator?: ReactNode }) => <header className="cockpit-header">{p.modeIndicator}</header>,
}))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))

const snap = {
  mycall: 'KD9TAW',
  mygrid: 'EN61',
  radio: {
    dialMhz: 14.078,
    band: '20m',
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

function fakePanels(removed: Js8PanelId[] = []): PanelLayoutApi<Js8PanelId> {
  return {
    layout: { v: 1, state: {}, share: {} },
    stateOf: (id) => (removed.includes(id) ? 'removed' : 'docked'),
    setPanelState: () => {},
    shareOf: () => 1,
    setShare: () => {},
    setShares: () => {},
    undo: () => {},
    canUndo: false,
    undoRemoves: [],
    reset: () => {},
  }
}

beforeEach(() => {
  state.current = js8Fixture()
  log.current = logFixture()
  window.localStorage.clear()
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

async function renderCockpit(props: Partial<Parameters<typeof Js8Cockpit>[0]> = {}) {
  const r = render(<Js8Cockpit snap={snap} panels={fakePanels()} {...props} />)
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}

/** The roster row for a call, by its call button's text. */
function stationRow(call: string): HTMLElement {
  const rows = Array.from(document.querySelectorAll('[data-pane="stations"] .js8-station'))
  const hit = rows.find((r) => r.querySelector('.js8-station-call')?.textContent === call)
  expect(hit, `no roster row for ${call}`).toBeTruthy()
  return hit as HTMLElement
}

describe('the call-activity roster carries JS8Call’s DX columns', () => {
  it('renders distance, azimuth, ✓ worked-before, name and comment for a heard station', async () => {
    await renderCockpit()
    const row = stationRow('W0IND')
    // EN61 → EN52 is a real path; the exact face is units-dependent, so assert it EXISTS and
    // is a measurement rather than pinning a string the units setting can change.
    const dist = row.querySelector('.js8-dist')
    expect(dist, 'no distance cell').not.toBeNull()
    expect(dist!.textContent).toMatch(/\d/)
    const az = row.querySelector('.js8-az')
    expect(az, 'no azimuth cell').not.toBeNull()
    expect(az!.textContent).toMatch(/^\d+°$/)
    expect(az!.getAttribute('title')).toMatch(/short path/)
    expect(row.querySelector('.js8-b4')!.textContent).toBe('✓')
    expect(row.querySelector('.js8-b4')!.getAttribute('title')).toMatch(/2025-06-15|Worked before/)
    expect(row.querySelector('.js8-opname')!.textContent).toBe('Dave')
    expect(row.querySelector('.js8-opcomment')!.textContent).toBe('JS8 ragchew')
  })

  it('leaves every joined column OFF a station with no grid and nothing in the log', async () => {
    await renderCockpit()
    const row = stationRow('N0GRD')
    for (const sel of ['.js8-dist', '.js8-az', '.js8-b4', '.js8-opname', '.js8-opcomment']) {
      expect(row.querySelector(sel), `${sel} rendered with no source behind it`).toBeNull()
    }
    // The columns it CAN answer are still there — the row is not blank, it is honest.
    expect(row.querySelector('.js8-snr')!.textContent).toBe('-19')
  })

  it('takes the grid from the LOG when the station has not sent one (JS8Call’s fallback)', async () => {
    log.current = [{ ...logFixture()[0], call: 'N0GRD', grid: 'FN31' }]
    await renderCockpit()
    const row = stationRow('N0GRD')
    expect(row.querySelector('.js8-dist'), 'no distance from the logged grid').not.toBeNull()
    expect(row.querySelector('.js8-az'), 'no azimuth from the logged grid').not.toBeNull()
  })

  it('★ pins a call to the top of the roster and remembers it', async () => {
    await renderCockpit()
    const before = Array.from(document.querySelectorAll('[data-pane="stations"] .js8-station-call')).map((b) => b.textContent)
    expect(before).toEqual(['W0IND', 'N0GRD'])
    const pin = stationRow('N0GRD').querySelector('.js8-pin') as HTMLButtonElement
    expect(pin.getAttribute('aria-pressed')).toBe('false')
    await act(async () => {
      fireEvent.click(pin)
    })
    const after = Array.from(document.querySelectorAll('[data-pane="stations"] .js8-station-call')).map((b) => b.textContent)
    expect(after, 'the pinned call did not move to the top').toEqual(['N0GRD', 'W0IND'])
    expect(stationRow('N0GRD').querySelector('.js8-pin')!.getAttribute('aria-pressed')).toBe('true')
    expect(window.localStorage.getItem(JS8_PINS_KEY)).toBe('N0GRD')
  })

  it('★ un-pins, and the roster goes back to the engine’s order', async () => {
    window.localStorage.setItem(JS8_PINS_KEY, 'N0GRD')
    await renderCockpit()
    expect(Array.from(document.querySelectorAll('[data-pane="stations"] .js8-station-call')).map((b) => b.textContent)).toEqual(['N0GRD', 'W0IND'])
    await act(async () => {
      fireEvent.click(stationRow('N0GRD').querySelector('.js8-pin') as HTMLButtonElement)
    })
    expect(Array.from(document.querySelectorAll('[data-pane="stations"] .js8-station-call')).map((b) => b.textContent)).toEqual(['W0IND', 'N0GRD'])
    expect(window.localStorage.getItem(JS8_PINS_KEY)).toBe('')
  })
})

describe('the band-activity-by-offset pane (JS8Call’s tableWidgetRXAll)', () => {
  it('is a CockpitPaneFrame inside the region, one row per offset, ordered by offset', async () => {
    await renderCockpit()
    const pane = document.querySelector('[data-pane="offsets"]')
    expect(pane, 'no offsets pane').not.toBeNull()
    expect(pane!.classList.contains('pane-frame')).toBe(true)
    expect(pane!.closest('.cockpit-panes'), 'the offsets pane renders outside the region').not.toBeNull()
    expect(pane!.closest('.cockpit-txdock'), 'the offsets pane is in the TX dock').toBeNull()
    const rows = Array.from(pane!.querySelectorAll('.js8-offset-row'))
    // Three decodes, two of them within 10 Hz of each other → two buckets, offset-ordered.
    expect(rows.map((r) => r.querySelector('.js8-freq')!.textContent)).toEqual(['700 Hz', '1508 Hz'])
    expect(rows[1].querySelector('.js8-text')!.textContent).toBe('W0IND: @ALLCALL CQ CQ CQ EN52')
  })

  it('carries the DT column the transcript drops', async () => {
    await renderCockpit()
    const rows = Array.from(document.querySelectorAll('[data-pane="offsets"] .js8-offset-row'))
    expect(rows.map((r) => r.querySelector('.js8-dt')!.textContent)).toEqual(['-400 ms', '200 ms'])
    expect(rows[0].querySelector('.js8-dt')!.getAttribute('title')).toMatch(/time delta/i)
    // The transcript itself still has no DT cell — the two panes are not the same table.
    expect(document.querySelector('[data-pane="activity"] .js8-dt')).toBeNull()
  })

  it('a double-click moves the RX cursor and nothing else', async () => {
    const api = await import('../api')
    const setRxOffset = api.setRxOffset as ReturnType<typeof vi.fn>
    setRxOffset.mockClear()
    await renderCockpit()
    const row = document.querySelectorAll('[data-pane="offsets"] .js8-offset-row')[0]
    await act(async () => {
      fireEvent.doubleClick(row)
    })
    expect(setRxOffset).toHaveBeenCalledWith(700)
    expect((api.setTxOffset as ReturnType<typeof vi.fn>)).not.toHaveBeenCalled()
  })

  it('is ⊞-hideable like its siblings, and hiding it leaves the transcript', async () => {
    await renderCockpit({ panels: fakePanels(['offsets']) })
    expect(document.querySelector('[data-pane="offsets"]')).toBeNull()
    expect(document.querySelector('[data-pane="activity"]')).not.toBeNull()
    cleanup()
    // The other direction: hiding only the transcript must not strand the offset table.
    await renderCockpit({ panels: fakePanels(['activity']) })
    const pane = document.querySelector('[data-pane="offsets"]')
    expect(pane, 'hiding `activity` took the offsets pane with it').not.toBeNull()
    expect(pane!.closest('.cockpit-panes')).not.toBeNull()
  })
})

describe('the two "differs from JS8Call" notes are in the UI, not only the manual', () => {
  it('the fused transcript says how it relates to the offset table', async () => {
    await renderCockpit()
    const title = document.querySelector('.js8-activity')!.getAttribute('title') ?? ''
    expect(title).toMatch(/JS8Call/)
    expect(title).toMatch(/Band activity/)
    expect(title).toMatch(/DT/)
  })

  it('every arm chip says the TX latch is the other half, on every face', async () => {
    // OFF · on-but-not-armed · ARMED — the note belongs on all three, since the chip an
    // operator reads as broken is whichever one he is looking at.
    for (const armed of [
      { autoreply: false, cq: false, relay: false, hbAck: false, hb: false },
      { autoreply: true, cq: true, relay: true, hbAck: true, hb: true },
    ]) {
      state.current = { ...js8Fixture(), autoreply: true, relay: true, hbAck: true, armed }
      await renderCockpit()
      for (const sel of ['.js8-autoreply', '.js8-relay', '.js8-hback']) {
        const chip = document.querySelector(sel)
        expect(chip, `${sel} missing`).not.toBeNull()
        expect(chip!.getAttribute('title'), `${sel} carries no latch note`).toMatch(/TX pill is the first/)
      }
      cleanup()
    }
  })
})
