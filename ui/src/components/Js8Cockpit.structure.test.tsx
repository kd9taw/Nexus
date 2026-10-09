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
import type { ReactNode, Ref } from 'react'
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen } from '@testing-library/react'
import { Js8Cockpit } from './Js8Cockpit'
import * as api from '../api'
import type { AppSnapshot, Js8State } from '../types'
import type { PanelLayoutApi, Js8PanelId } from '../features/panelState'
import { JS8_PANEL_IDS, JS8_PANELS, panelStateIn, panelStorageKey, seamShares, usePanelLayout } from '../features/panelState'

// THE BUDGET (2026-10-09). The slowest case here, "the RF scope pane ships hidden; ticked, it heads the…", takes
// 0.28 s and 0.20 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

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
  armed: { autoreply: false, relay: false, hbAck: false, hb: false, cq: false },
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
// The strip's box reaches its divider through `stripRef` (layout L2), so the stub forwards it.
// The RF scope pane's picture is a renderer canvas jsdom cannot draw; its frame and place are what
// these cases check.
vi.mock('./PhoneScope', () => ({ PhoneScope: (p: { feed?: string }) => <div data-testid="rfscope-stub" data-feed={p.feed} /> }))
vi.mock('./Waterfall', () => ({
  Waterfall: (p: { stripRef?: Ref<HTMLDivElement> }) => <div className="waterfall-wrap" ref={p.stripRef} />,
}))
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
  it('the RF scope pane ships hidden; ticked, it heads the leading column of the region, under the TX strip', async () => {
    // The stock record, read the vocabulary's way (this file's fixture docks every absent id).
    const stock = { ...fakePanels(), stateOf: (id: Js8PanelId) => panelStateIn(JS8_PANELS, { v: 1, state: {}, share: {} }, id) }
    await renderCockpit({ panels: stock })
    expect(document.querySelector('[data-pane="rfScope"]'), 'the RF scope pane shipped visible').toBeNull()
    expect(document.querySelector('[data-pane="activity"]'), 'control: the stock panes are on screen').not.toBeNull()
    cleanup()
    await renderCockpit({ panels: fakePanels() })
    const region = document.querySelector('.cockpit-panes')!
    const pane = region.querySelector('[data-pane="rfScope"]')
    expect(pane, 'the RF scope pane is not in the region').not.toBeNull()
    expect(pane!.parentElement?.classList.contains('cockpit-col'), 'it is not a column pane').toBe(true)
    expect(pane!.parentElement!.firstElementChild, 'it does not head its column').toBe(pane)
    expect(pane!.getAttribute('data-fit')).toBe('fill')
    expect(pane!.querySelector('[data-testid="rfscope-stub"]')?.getAttribute('data-feed')).toBe('rf')
  })

  it('the shell holds no child kinds beyond the census', async () => {
    state.current = { ...state.current, lastError: 'receive-only tier' }
    await renderCockpit()
    const shell = document.querySelector('main.layout.single.js8-cockpit')!
    expect(shell).not.toBeNull()
    // The waterfall's divider (layout L2) is the scope divider's shell-child kind in Phone and CW.
    const ALLOWED = ['.cockpit-header', '.waterfall-wrap', '.pane-splitter', '.cw-keyer-warn', '.cockpit-txstrip', '.cockpit-panes', '.cockpit-txdock']
    for (const el of Array.from(shell.children)) {
      expect(
        ALLOWED.some((s) => el.matches(s)),
        `unexpected shell-level child <${el.tagName.toLowerCase()} class="${el.className}">`,
      ).toBe(true)
    }
    expect(document.querySelector('.cw-keyer-warn'), 'error banner did not render — census untested').not.toBeNull()
    expect(shell.querySelectorAll(':scope > .cockpit-panes').length).toBe(1)
    expect(shell.querySelectorAll(':scope > .cockpit-txdock').length).toBe(1)
    // THE TX STRIP (2026-10-01): exactly one, a shell child directly under the scope (after
    // its divider), holding the stop controls the header used to hold — and the header none.
    const strips = shell.querySelectorAll(':scope > .cockpit-txstrip')
    expect(strips.length, 'no TX strip in the shell').toBe(1)
    expect(strips[0].previousElementSibling?.matches('.pane-splitter'), 'the TX strip is not directly under the scope').toBe(true)
    const named = (root: Element, re: RegExp) => [...root.querySelectorAll('button')].filter((b) => re.test(b.textContent!.trim()))
    expect(named(strips[0], /^stop tx$/i).length, 'Stop TX is not in the TX strip').toBe(1)
    expect(named(strips[0], /^tune$/i).length, 'Tune is not in the TX strip').toBe(1)
    expect(named(shell.querySelector('.cockpit-header')!, /^stop tx$|^tune$|^tuning…$|^atu$|tx (on|off)$/i), 'the header still draws a transmit control').toEqual([])
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
    await renderCockpit({ panels: fakePanels([...JS8_PANEL_IDS]) })
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

// ── THE COLUMN DIVIDERS (layout L2) ──────────────────────────────────────────────────────────
// JS8's wiring of panes/RegionColumnSeams (the divider's own behaviour is tested there), including
// the one state the other two cockpits do not have: two tracks with the log hidden, where the
// second track holds Stations and Inbox and the width divider says so.
describe('Js8Cockpit column dividers', () => {
  let live: Set<() => void>
  const resize = () => act(() => [...live].forEach((cb) => cb()))
  beforeEach(() => {
    live = new Set()
    localStorage.clear()
    globalThis.ResizeObserver = class {
      cb: () => void
      constructor(cb: () => void) {
        this.cb = cb
        live.add(cb)
      }
      observe() {}
      disconnect() {
        live.delete(this.cb)
      }
      unobserve() {}
    } as unknown as typeof ResizeObserver
  })
  function Live() {
    const panels = usePanelLayout(JS8_PANELS)
    return <Js8Cockpit snap={snap} panels={panels} />
  }
  const dividers = (region: Element) =>
    [...region.querySelectorAll(':scope > [role="separator"]')].map((s) => [
      s.getAttribute('aria-label'),
      [...s.classList].filter((c) => c.startsWith('cockpit-colseam-')).join(' '),
    ])
  async function mountLive() {
    render(<Live />)
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })
    return document.querySelector('.cockpit-panes')!
  }
  async function tier(region: Element, width: number) {
    stubWidth(region, width)
    resize()
    await frame()
  }

  it('none in the stacking tier; at two columns the log width; at three the split between activity and stations as well', async () => {
    const region = await mountLive()
    expect(dividers(region)).toEqual([])
    await tier(region, 1200)
    expect(dividers(region)).toEqual([['log column width', 'cockpit-colseam-2']])
    await tier(region, 1800)
    expect(dividers(region)).toEqual([
      ['Activity column / Stations column', 'cockpit-colseam-2'],
      ['log column width', 'cockpit-colseam-3'],
    ])
    await tier(region, 900)
    expect(dividers(region)).toEqual([])
  })

  it('with the log hidden, two tracks are activity | stations and the width divider sizes the stations column', async () => {
    localStorage.setItem(panelStorageKey('js8'), JSON.stringify({ v: 2, state: { log: 'removed' }, share: {} }))
    const region = await mountLive()
    await tier(region, 1800)
    expect(region.getAttribute('data-cols')).toBe('2')
    expect(dividers(region)).toEqual([['Stations column width', 'cockpit-colseam-2']])
    const cols = region.querySelectorAll(':scope > .cockpit-col')
    expect(cols[1].querySelector('[data-pane="stations"]'), 'the width divider is not over the stations column').not.toBeNull()
  })

  it('the widths stored in the JS8 record ride the region', async () => {
    localStorage.setItem(panelStorageKey('js8'), JSON.stringify({ v: 2, state: {}, share: {}, cols: { a: 1.2, b: 0.8, log: 480 } }))
    const region = (await mountLive()) as HTMLElement
    expect(region.style.getPropertyValue('--cockpit-col-a')).toBe('1.2fr')
    expect(region.style.getPropertyValue('--cockpit-col-b')).toBe('0.8fr')
    expect(region.style.getPropertyValue('--cockpit-col-log')).toBe('min(480px, 50%)')
  })
})

// ── THE DIVIDERS BETWEEN PANES IN A COLUMN (layout L2) ───────────────────────────────────────
describe('Js8Cockpit dividers between panes', () => {
  let live: Set<() => void>
  const resize = () => act(() => [...live].forEach((cb) => cb()))
  beforeEach(() => {
    live = new Set()
    localStorage.clear()
    globalThis.ResizeObserver = class {
      cb: () => void
      constructor(cb: () => void) {
        this.cb = cb
        live.add(cb)
      }
      observe() {}
      disconnect() {
        live.delete(this.cb)
      }
      unobserve() {}
    } as unknown as typeof ResizeObserver
  })
  function Live() {
    const panels = usePanelLayout(JS8_PANELS)
    return <Js8Cockpit snap={snap} panels={panels} />
  }
  async function mountLive() {
    render(<Live />)
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })
    return document.querySelector('.cockpit-panes')!
  }
  async function tier(region: Element, width: number) {
    stubWidth(region, width)
    resize()
    await frame()
  }
  const pane = (id: string) => document.querySelector<HTMLElement>(`[data-pane="${id}"]`)!
  const seam = (name: RegExp) => screen.queryByRole('separator', { name })
  /** The panes around a divider, by their data-pane. */
  const around = (sep: Element) => [
    (sep.previousElementSibling as HTMLElement | null)?.dataset.pane,
    (sep.nextElementSibling as HTMLElement | null)?.dataset.pane,
  ]

  it('exist only in the bounded flow, each between its own pair, at three columns and at two', async () => {
    const region = await mountLive()
    expect(seam(/^Activity \/ Band activity$/), 'a stack cannot be divided').toBeNull()
    await tier(region, 1800)
    expect(around(seam(/^Activity \/ Band activity$/)!)).toEqual(['activity', 'offsets'])
    expect(around(seam(/^Stations \/ Inbox$/)!)).toEqual(['stations', 'inbox'])
    // A column's dividers, marked as such: the class the sheet keys their in-gap margins on
    // (styles.css `.in-column`; cockpit-shells.test.ts computes the net).
    for (const name of [/^Activity \/ Band activity$/, /^Stations \/ Inbox$/]) {
      expect(seam(name)!.parentElement!.classList.contains('cockpit-col')).toBe(true)
      expect(seam(name)!.classList.contains('in-column'), `${name} takes a 12 px gap of its own`).toBe(true)
    }
    // Two columns: all four in the leading column, each divider still between its own pair.
    await tier(region, 1200)
    expect(region.getAttribute('data-cols')).toBe('2')
    expect(around(seam(/^Activity \/ Band activity$/)!)).toEqual(['activity', 'offsets'])
    expect(around(seam(/^Stations \/ Inbox$/)!)).toEqual(['stations', 'inbox'])
    await tier(region, 900)
    expect(seam(/^Activity \/ Band activity$/)).toBeNull()
    expect(seam(/^Stations \/ Inbox$/)).toBeNull()
  })

  it('panes nobody has divided are the stock panes: their weights as the grow, the stock floor', async () => {
    const region = await mountLive()
    await tier(region, 1800)
    expect(pane('activity').style.getPropertyValue('--pane-share')).toBe('')
    expect(pane('activity').getAttribute('style')).toContain('var(--pane-share, 2) 1 0')
    expect(pane('offsets').getAttribute('style')).toContain('var(--pane-share, 1) 1 0')
    expect(pane('activity').style.minHeight).toBe('min(calc(var(--cockpit-fill-min, 0px) * var(--pane-share, 1.5) / 1.5), 100%)')
  })

  it('a key commits the pair’s shares; the panes carry them as grows that keep the pair’s stock total', async () => {
    const region = await mountLive()
    await tier(region, 1800)
    const rect = (top: number, h: number) => () => ({ top, bottom: top + h, height: h, left: 0, right: 600, width: 600, x: 0, y: top, toJSON: () => ({}) }) as DOMRect
    pane('activity').getBoundingClientRect = rect(100, 300)
    pane('offsets').getBoundingClientRect = rect(412, 150)
    fireEvent.keyDown(seam(/^Activity \/ Band activity$/)!, { key: 'ArrowDown' })
    const [a, b] = seamShares(300 / 450 + 0.05)
    expect(JSON.parse(localStorage.getItem(panelStorageKey('js8'))!).share).toEqual({ activity: a, offsets: b })
    expect(Number(pane('activity').style.getPropertyValue('--pane-share'))).toBeCloseTo(a * 1.5, 10)
    expect(Number(pane('offsets').style.getPropertyValue('--pane-share'))).toBeCloseTo(b * 1.5, 10)
    // Backspace: the stock split back, no share of its own left behind.
    fireEvent.keyDown(seam(/^Activity \/ Band activity$/)!, { key: 'Backspace' })
    expect(JSON.parse(localStorage.getItem(panelStorageKey('js8'))!).share).toEqual({})
    expect(pane('activity').style.getPropertyValue('--pane-share')).toBe('')
  })

  it('a pane whose partner is hidden is the stock pane again, whatever split is stored', async () => {
    localStorage.setItem(panelStorageKey('js8'), JSON.stringify({ v: 2, state: { offsets: 'removed' }, share: { activity: 0.3, offsets: 1.7 } }))
    const region = await mountLive()
    await tier(region, 1800)
    expect(seam(/^Activity \/ Band activity$/)).toBeNull()
    expect(pane('activity').style.getPropertyValue('--pane-share'), 'a lone pane took a split share').toBe('')
    expect(pane('activity').style.minHeight, 'a lone pane took a split floor').toBe('var(--cockpit-fill-min, 0)')
  })
})

// ── THE WATERFALL'S DIVIDER (layout L2) ───────────────────────────────────────────────────────
// JS8 had no way to size its waterfall. Now a strip divider under it, the Phone/CW scope divider's
// kind: focusable, arrows/Home/End/Backspace, its height announced in CSS px, stored per surface
// and clamped on load. jsdom lays nothing out, so the shell's box is stubbed; the clamps are
// the sheet's (WATERFALL_SPLIT_MIN/MAX), at jsdom's 16 px font and 768 px window.
describe('the JS8 waterfall divider', () => {
  function layOut(boxes: Record<string, { height: number }>) {
    const real = HTMLElement.prototype.getBoundingClientRect
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
      for (const [sel, b] of Object.entries(boxes)) {
        if (!this.matches(sel)) continue
        return { top: 0, left: 0, width: 800, height: b.height, right: 800, bottom: b.height, x: 0, y: 0, toJSON: () => ({}) } as DOMRect
      }
      return real.call(this)
    })
  }
  const shell = () => document.querySelector<HTMLElement>('main.js8-cockpit')!
  const aria = (el: HTMLElement) => ['aria-valuenow', 'aria-valuemin', 'aria-valuemax'].map((a) => el.getAttribute(a))
  beforeEach(() => {
    localStorage.clear()
    document.documentElement.style.removeProperty('--vh-eff')
    document.documentElement.style.removeProperty('--ui-zoom')
  })
  afterEach(() => vi.restoreAllMocks())

  it('sits under the waterfall, focusable, announcing its height, and steps, jumps and resets', async () => {
    layOut({ 'main.js8-cockpit': { height: 1000 } })
    await renderCockpit()
    const sep = screen.getByRole('separator', { name: 'waterfall height' })
    expect(sep.previousElementSibling?.classList.contains('waterfall-wrap'), 'the divider is not under the strip').toBe(true)
    expect(sep.tabIndex, 'a divider only a mouse can reach').toBe(0)
    // 25 % of the shell; 8em at 16 px (under 28 % of the window); 45 % of the 768 px window.
    expect(aria(sep)).toEqual(['250', '128', '346'])
    fireEvent.keyDown(sep, { key: 'ArrowDown' })
    expect(shell().style.getPropertyValue('--js8-wf-h')).toBe(`${(266 / 1000) * 100}%`)
    expect(localStorage.getItem('nexus.split.js8.waterfall')).toBe(String((266 / 1000) * 100))
    fireEvent.keyDown(sep, { key: 'End' })
    expect(sep.getAttribute('aria-valuenow')).toBe('346')
    fireEvent.keyDown(sep, { key: 'Home' })
    expect(sep.getAttribute('aria-valuenow')).toBe('128')
    fireEvent.keyDown(sep, { key: 'Backspace' })
    expect(shell().style.getPropertyValue('--js8-wf-h')).toBe('25%')
    expect(localStorage.getItem('nexus.split.js8.waterfall')).toBe('25')
  })

  it('a stored height is restored, clamped against this window, and kept for a bigger one', async () => {
    localStorage.setItem('nexus.split.js8.waterfall', '30')
    layOut({ 'main.js8-cockpit': { height: 1000 } })
    const first = await renderCockpit()
    expect(shell().style.getPropertyValue('--js8-wf-h')).toBe('30%')
    first.unmount()
    localStorage.setItem('nexus.split.js8.waterfall', '75')
    await renderCockpit()
    expect(screen.getByRole('separator', { name: 'waterfall height' }).getAttribute('aria-valuenow')).toBe('346')
    expect(localStorage.getItem('nexus.split.js8.waterfall'), 'the clamp is apply-side only').toBe('75')
  })

  it('goes with the waterfall when the operator hides it, and the stored height stays', async () => {
    localStorage.setItem('nexus.split.js8.waterfall', '30')
    await renderCockpit({ panels: fakePanels(['scope']) })
    expect(screen.queryByRole('separator', { name: 'waterfall height' })).toBeNull()
    expect(localStorage.getItem('nexus.split.js8.waterfall')).toBe('30')
  })
})
