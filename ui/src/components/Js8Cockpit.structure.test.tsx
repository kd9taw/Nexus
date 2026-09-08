// @vitest-environment jsdom
//
// JS8 COCKPIT SHELL STRUCTURE (the JS8 programme, 2026-09 — RX in B5/B6, TX in B7).
//
// JS8 is the eighth cockpit and takes CW's REGION shape of the pane-grid contract: header
// chrome, the waterfall, ONE .cockpit-panes region (activity · stations · inbox · log, every one
// a CockpitPaneFrame) and the pinned .cockpit-txdock. THE STOP LINE is held the Operate way
// (a slotted mode): Stop TX + Tune in the header, Esc keyboard-only — none with a ⊞ id — and the
// WIRING sweep for that is stop-line.test.tsx's JS8 case. What THIS file pins is the shell
// census, the region tiers, dock placement (transmit controls never inside a pane), and the
// view-entry wiring (`js8_enter`, once per activation edge, RX only).
//
// jsdom has no layout, so region widths are stubbed the way useRegionCols.test.tsx does.
import type { ReactNode } from 'react'
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent } from '@testing-library/react'
import { Js8Cockpit } from './Js8Cockpit'
import * as api from '../api'
import type { AppSnapshot, Js8State } from '../types'
import type { PanelLayoutApi, Js8PanelId } from '../features/panelState'

const js8Fixture = (): Js8State => ({
  speed: 'normal',
  rxSpeeds: 15,
  txEnabled: false,
  sending: false,
  hbOn: false,
  hbNextAtMs: null,
  hbIntervalMin: 0,
  autoreply: true,
  relay: true,
  hbAck: false,
  armed: { autoreply: false, relay: false, hbAck: false, hb: false },
  idleMinutes: 0,
  idleLimitMin: 60,
  idleTripped: false,
  activity: [
    { atMs: 1_757_000_000_000, speed: 'normal', freqHz: 1210, snrDb: -8, dtS: 0.1, from: 'W0IND', text: 'W0IND: @ALLCALL CQ CQ CQ EN52 ', directedToMe: false, mine: false, complete: true, lowConf: false },
  ],
  stations: [
    { call: 'W0IND', grid: 'EN52', snrDb: -8, freqHz: 1210, speed: 'normal', lastMs: 1_757_000_000_000, lastHb: false, lastCq: true, storedMsgs: 0 },
  ],
  inbox: [
    { id: 1, from: 'W0IND', to: 'KD9TAW', text: 'HELLO', path: ['W0IND'], state: 'unread', atMs: 1_757_000_000_000, freqHz: 1210, snrDb: -8 },
  ],
  queue: [],
  pendingReply: null,
  lastError: null,
})
const state: { current: Js8State } = { current: js8Fixture() }

vi.mock('../api', async (importOriginal) => {
  // Derived from the real module (the stop-line.test.tsx pattern): every export is auto-stubbed
  // so an api call added to the cockpit later cannot make this suite throw on mount.
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
    js8Send: vi.fn(async () => state.current),
    js8SendCommand: vi.fn(async () => state.current),
    js8CallCq: vi.fn(async () => state.current),
    js8Arm: vi.fn(async () => state.current),
    js8InboxMark: vi.fn(async () => state.current),
    js8InboxDelete: vi.fn(async () => state.current),
    // The roster's ✓/Name/Comment columns join against the logbook (features/callHistory),
    // so the auto-stub's `{}` is not a usable log — this suite runs against an empty one.
    getLog: vi.fn(async () => []),
    getLicensedBandPlan: vi.fn(async () => []),
    haltTx: vi.fn(async () => ({})),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
// The header stub RENDERS its modeIndicator: the speed chips live there and are pinned below.
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: (p: { modeIndicator?: ReactNode }) => (
    <header className="cockpit-header">{p.modeIndicator}</header>
  ),
}))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))

const js8Enter = api.js8Enter as ReturnType<typeof vi.fn>
const js8SetSpeed = api.js8SetSpeed as ReturnType<typeof vi.fn>
const haltTx = api.haltTx as ReturnType<typeof vi.fn>

const snap = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
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

/** The observed element's callback, so a test can fire a resize the way the browser does. */
let fire: (() => void) | null = null
beforeEach(() => {
  fire = null
  state.current = js8Fixture()
  js8Enter.mockClear()
  js8SetSpeed.mockClear()
  haltTx.mockClear()
  globalThis.ResizeObserver = class {
    constructor(cb: () => void) {
      fire = cb
    }
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

function stubWidth(el: Element, w: number) {
  Object.defineProperty(el, 'clientWidth', { configurable: true, get: () => w })
}
async function frame() {
  await act(async () => {
    await new Promise((r) => requestAnimationFrame(() => r(null)))
  })
}
async function renderCockpit(props: Partial<Parameters<typeof Js8Cockpit>[0]> = {}) {
  const r = render(<Js8Cockpit snap={snap} panels={fakePanels()} {...props} />)
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}

describe('Js8Cockpit pane shell', () => {
  it('the shell holds no child kinds beyond the census', async () => {
    state.current = { ...state.current, lastError: 'receive-only tier' }
    await renderCockpit()
    const shell = document.querySelector('main.layout.single.js8-cockpit')!
    expect(shell).not.toBeNull()
    const ALLOWED = ['.cockpit-header', '.waterfall-wrap', '.cw-keyer-warn', '.cockpit-panes', '.cockpit-txdock']
    for (const el of Array.from(shell.children)) {
      expect(
        ALLOWED.some((s) => el.matches(s)),
        `unexpected shell-level child <${el.tagName.toLowerCase()} class="${el.className}">`,
      ).toBe(true)
    }
    expect(document.querySelector('.cw-keyer-warn'), 'error banner did not render — census untested').not.toBeNull()
    expect(shell.querySelectorAll(':scope > .cockpit-panes').length).toBe(1)
    expect(shell.querySelectorAll(':scope > .cockpit-txdock').length).toBe(1)
  })

  it('renders exactly one .cockpit-panes region, tier-stamped by useRegionCols', async () => {
    await renderCockpit()
    const regions = document.querySelectorAll('.cockpit-panes')
    expect(regions.length).toBe(1)
    // jsdom width 0 → the hook keeps its initial tier 1, stamped before first paint.
    expect(regions[0].getAttribute('data-cols')).toBe('1')
    // Tier 1 renders the two-column grouping (main + log) — it stacks; the region scrolls.
    expect(regions[0].querySelectorAll(':scope > .cockpit-col').length).toBe(2)
  })

  it('every operator-content block renders through a CockpitPaneFrame inside the region', async () => {
    await renderCockpit()
    for (const id of ['activity', 'offsets', 'stations', 'inbox', 'log']) {
      const pane = document.querySelector(`[data-pane="${id}"]`)
      expect(pane, `pane "${id}" missing`).not.toBeNull()
      expect(pane!.classList.contains('pane-frame'), `"${id}" is not a .pane-frame`).toBe(true)
      expect(pane!.closest('.cockpit-panes'), `"${id}" renders outside the region`).not.toBeNull()
    }
    expect(document.querySelector('[data-pane="activity"] .js8-row')).not.toBeNull()
    expect(document.querySelector('[data-pane="stations"] .js8-station-call')!.textContent).toBe('W0IND')
    expect(document.querySelector('[data-pane="inbox"] .js8-inbox-row')).not.toBeNull()
    expect(document.querySelector('[data-pane="log"] [data-testid="log-stub"]')).not.toBeNull()
  })

  it('three columns group activity | stations + inbox | log', async () => {
    await renderCockpit()
    const region = document.querySelector('.cockpit-panes')!
    stubWidth(region, 1800)
    act(() => fire!())
    await frame()
    expect(region.getAttribute('data-cols')).toBe('3')
    const cols = region.querySelectorAll(':scope > .cockpit-col')
    expect(cols.length).toBe(3)
    expect(cols[0].querySelector('[data-pane="activity"]')).not.toBeNull()
    expect(cols[1].querySelector('[data-pane="stations"]')).not.toBeNull()
    expect(cols[1].querySelector('[data-pane="inbox"]')).not.toBeNull()
    expect(cols[2].querySelector('[data-pane="log"]')).not.toBeNull()
  })

  it('hiding the log caps the template at two tracks; hiding everything leaves the region and the dock', async () => {
    await renderCockpit({ panels: fakePanels(['log']) })
    const region = document.querySelector('.cockpit-panes')!
    stubWidth(region, 1800)
    act(() => fire!())
    await frame()
    expect(region.getAttribute('data-cols'), 'a 3-track template with an empty log track').toBe('2')
    expect(document.querySelector('[data-pane="log"]')).toBeNull()
    cleanup()
    await renderCockpit({ panels: fakePanels(['scope', 'activity', 'offsets', 'stations', 'inbox', 'log']) })
    expect(document.querySelector('.waterfall-wrap')).toBeNull()
    expect(document.querySelector('.cockpit-panes'), 'the region is a shell child, hidden panes or not').not.toBeNull()
    expect(document.querySelectorAll('.pane-frame').length).toBe(0)
    expect(document.querySelector('.cockpit-txdock .js8-send')).not.toBeNull()
  })

  it('every transmit control lives in the pinned TX dock — never inside a pane', async () => {
    await renderCockpit()
    const dock = document.querySelector('.cockpit-txdock')
    expect(dock, 'no .cockpit-txdock').not.toBeNull()
    for (const sel of ['.js8-compose-row', '.js8-to', '.js8-compose', '.js8-send', '.js8-beacon-row', '.js8-cq', '.js8-hb']) {
      const el = document.querySelector(sel)
      expect(el, `${sel} missing`).not.toBeNull()
      expect(el!.closest('.cockpit-txdock'), `${sel} is not in the TX dock`).not.toBeNull()
      expect(el!.closest('.pane-frame'), `${sel} is inside a pane frame`).toBeNull()
    }
    expect(dock!.querySelector('.pane-frame')).toBeNull()
    const region = document.querySelector('.cockpit-panes')!
    expect(region.compareDocumentPosition(dock!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  })

  it('the speed chips render in the header and ask the ENGINE for a speed change', async () => {
    await renderCockpit()
    const chips = Array.from(document.querySelectorAll('.cockpit-header .js8-speed-chip'))
    expect(chips.map((c) => c.textContent)).toEqual(['Slow', 'Normal', 'Fast', 'Turbo'])
    expect(chips[1].getAttribute('aria-pressed')).toBe('true')
    await act(async () => {
      fireEvent.click(chips[2])
    })
    expect(js8SetSpeed).toHaveBeenCalledWith(2)
    expect(document.querySelector('.cockpit-txdock .js8-speed-chip')).toBeNull()
  })

  it('survives the keep-alive hide/show round trip with its structure intact', async () => {
    const { rerender } = await renderCockpit({ active: true })
    await act(async () => {
      rerender(<Js8Cockpit snap={snap} panels={fakePanels()} active={false} />)
    })
    await act(async () => {
      rerender(<Js8Cockpit snap={snap} panels={fakePanels()} active={true} />)
    })
    expect(document.querySelector('[data-pane="activity"]')).not.toBeNull()
    expect(document.querySelector('.cockpit-txdock')).not.toBeNull()
  })
})

describe('JS8 view entry (js8_enter — set_tier + the watering hole, RX only)', () => {
  it('calls the ENGINE once per activation edge', async () => {
    const { rerender } = await renderCockpit({ active: true })
    expect(js8Enter).toHaveBeenCalledTimes(1)
    await act(async () => {
      rerender(<Js8Cockpit snap={snap} panels={fakePanels()} active={true} theme="dark" />)
    })
    expect(js8Enter).toHaveBeenCalledTimes(1)
    await act(async () => {
      rerender(<Js8Cockpit snap={snap} panels={fakePanels()} active={false} />)
    })
    await act(async () => {
      rerender(<Js8Cockpit snap={snap} panels={fakePanels()} active={true} />)
    })
    expect(js8Enter).toHaveBeenCalledTimes(2)
  })

  it('never enters while hidden (the keep-alive host renders it inactive)', async () => {
    await renderCockpit({ active: false })
    expect(js8Enter).not.toHaveBeenCalled()
  })
})

describe('Esc is the keyboard stop — bound only while JS8 is the visible view', () => {
  it('Esc → haltTx while active', async () => {
    await renderCockpit({ active: true })
    await act(async () => {
      fireEvent.keyDown(window, { key: 'Escape' })
    })
    expect(haltTx).toHaveBeenCalledTimes(1)
  })
  it('Esc does nothing while the cockpit is hidden', async () => {
    await renderCockpit({ active: false })
    await act(async () => {
      fireEvent.keyDown(window, { key: 'Escape' })
    })
    expect(haltTx).not.toHaveBeenCalled()
  })
})
