// @vitest-environment jsdom
//
// THE JTTY SHELL, BY STRUCTURE. JTTY takes the REGION shape of the pane-grid contract, CW's and
// JS8's: header chrome, the waterfall and its divider, the TX strip, ONE .cockpit-panes region
// (All Decodes, QSO Frequency and the log strip, each a CockpitPaneFrame) and the pinned
// .cockpit-txdock. Three content panes fill a region's columns, so none is a bare shell child: on
// a wide window WSJT-X's two panes stand side by side with the log column beside them, and the log
// strip keeps a column of its own at every tier that has two. What this file pins: the shell
// census, the tiers, the keyed columns (a tier flip never remounts the log strip mid-entry) and
// the dock's place. The stop wiring is stop-line.test.tsx's JTTY case; the geometry a real layout
// engine gives these columns is the browser check's, since jsdom lays nothing out.
//
// jsdom has no layout, so the region's width is stubbed the way useRegionCols.test.tsx does.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import type { ReactNode, Ref } from 'react'
import { JttyCockpit } from './JttyCockpit'
import type { AppSnapshot, JttyState } from '../types'
import type { JttyPanelId, PanelLayoutApi } from '../features/panelState'
import * as api from '../api'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const mod: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) mod[k] = typeof actual[k] === 'function' ? vi.fn(async () => null) : actual[k]
  return mod
})
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn(async (f: () => Promise<unknown>) => f()) }))
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: (p: { children?: ReactNode }) => <header className="cockpit-header">{p.children}</header>,
}))
// The strip's box reaches its divider through `stripRef`, so the stub forwards it.
vi.mock('./Waterfall', () => ({
  Waterfall: (p: { stripRef?: Ref<HTMLDivElement> }) => <div className="waterfall-wrap" ref={p.stripRef} />,
}))
// The RF scope pane's picture is a renderer canvas jsdom cannot draw; its frame and place are what
// is checked here.
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="rfscope-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))

const snap = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  radio: { dialMhz: 14.09, band: '20m', catOk: true, sideband: 'USB', transmitting: false, tuning: false, txEnabled: false, txAllowed: true },
} as unknown as AppSnapshot

const jtty = (over: Partial<JttyState> = {}): JttyState => ({
  armed: true,
  rxHz: 1500,
  ftolHz: 20,
  lowHz: 200,
  highHz: 3000,
  lines: [],
  heard: [],
  notice: null,
  ...over,
})

/** A panel record with `removed` hidden and every other id on screen (the RF scope pane included). */
function fakePanels(removed: JttyPanelId[]): PanelLayoutApi<JttyPanelId> {
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

const m = <K extends keyof typeof api>(k: K) => vi.mocked(api[k] as (...a: unknown[]) => Promise<unknown>)

/** Every ResizeObserver callback alive, so a resize reaches the region's whichever came last. */
let live: Set<() => void>
beforeEach(() => {
  live = new Set()
  globalThis.ResizeObserver = class {
    cb: () => void
    constructor(cb: () => void) {
      this.cb = cb
      live.add(cb)
    }
    observe() {}
    unobserve() {}
    disconnect() {
      live.delete(this.cb)
    }
  } as unknown as typeof ResizeObserver
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

async function mount(s: JttyState = jtty(), panels?: PanelLayoutApi<JttyPanelId>) {
  for (const k of ['getJttyState', 'jttyAutoArm', 'jttyArm', 'jttySetRx', 'jttySetBand', 'jttyClear'] as const) m(k).mockResolvedValue(s)
  m('getLicensedBandPlan').mockResolvedValue([])
  render(
    <JttyCockpit snap={snap} active onSetTxEnabled={() => {}} panels={panels} />,
  )
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
}

const region = () => {
  const el = document.querySelector('main.jtty-cockpit > .cockpit-panes')
  expect(el, 'the cockpit has no .cockpit-panes region').not.toBeNull()
  return el!
}
/** The region measured `w` CSS px wide, the way the browser reports a resize. */
async function resize(w: number) {
  const el = region()
  Object.defineProperty(el, 'clientWidth', { configurable: true, get: () => w })
  act(() => [...live].forEach((cb) => cb()))
  await act(async () => {
    await new Promise((r) => requestAnimationFrame(() => r(null)))
  })
  return el
}
/** The panes each column holds, in order, by pane id. */
const columns = (el: Element) =>
  [...el.querySelectorAll(':scope > .cockpit-col')].map((c) => [...c.querySelectorAll(':scope > [data-pane]')].map((p) => p.getAttribute('data-pane')))

describe('the JTTY shell is the census: header, scope, TX strip, ONE pane region, the dock', () => {
  it('holds no child kinds beyond the census, the engine’s notice showing', async () => {
    await mount(jtty({ notice: 'JTTY receive audio was lost; decoding restarted.' }))
    const shell = document.querySelector('main.layout.single.jtty-cockpit')!
    expect(shell).not.toBeNull()
    const ALLOWED = ['.cockpit-header', '.waterfall-wrap', '.pane-splitter', '.cw-keyer-warn', '.cockpit-txstrip', '.cockpit-panes', '.cockpit-txdock']
    for (const el of Array.from(shell.children)) {
      expect(
        ALLOWED.some((s) => el.matches(s)),
        `unexpected shell-level child <${el.tagName.toLowerCase()} class="${el.className}" data-pane="${el.getAttribute('data-pane')}">`,
      ).toBe(true)
    }
    expect(shell.querySelector(':scope > .cw-keyer-warn'), 'the notice did not render: census untested').not.toBeNull()
    expect(shell.querySelectorAll(':scope > .cockpit-panes').length).toBe(1)
    expect(shell.querySelectorAll(':scope > .cockpit-txdock').length).toBe(1)
    const dock = shell.querySelector(':scope > .cockpit-txdock')!
    expect(region().compareDocumentPosition(dock) & Node.DOCUMENT_POSITION_FOLLOWING, 'the dock is not after the region').toBeTruthy()
    expect(dock.querySelector('.pane-frame'), 'a pane inside the dock').toBeNull()
    for (const id of ['all', 'rx', 'log']) {
      const pane = document.querySelector(`[data-pane="${id}"]`)
      expect(pane, `pane "${id}" missing`).not.toBeNull()
      expect(pane!.classList.contains('pane-frame'), `"${id}" is not a pane frame`).toBe(true)
      expect(pane!.parentElement?.matches('.cockpit-panes > .cockpit-col'), `"${id}" is not a pane in a region column`).toBe(true)
    }
  })
})

describe('the region’s tiers (useRegionCols stamps them; the columns follow)', () => {
  it('three tracks: All Decodes | QSO Frequency | the log, WSJT-X’s two panes side by side', async () => {
    await mount()
    const el = await resize(1800)
    expect(el.getAttribute('data-cols')).toBe('3')
    expect(columns(el)).toEqual([['all'], ['rx'], ['log']])
  })

  it('two tracks: the two decode panes share the leading column beside the log', async () => {
    await mount()
    const el = await resize(1470)
    expect(el.getAttribute('data-cols')).toBe('2')
    expect(columns(el)).toEqual([['all', 'rx'], ['log']])
  })

  it('one track on a narrow region: the same two groups, stacked, the region scrolling', async () => {
    await mount()
    const el = await resize(900)
    expect(el.getAttribute('data-cols')).toBe('1')
    expect(el.getAttribute('data-flow')).toBe('stack')
    expect(columns(el)).toEqual([['all', 'rx'], ['log']])
  })

  it('a tier flip never remounts the log strip or All Decodes (keyed columns)', async () => {
    await mount()
    await resize(1470)
    const strip = document.querySelector('[data-testid="log-stub"]')
    const all = document.querySelector('[data-pane="all"]')
    expect(strip, 'control: the log strip renders').not.toBeNull()
    for (const w of [1800, 900, 1800, 1470]) {
      await resize(w)
      expect(document.querySelector('[data-testid="log-stub"]'), `the log strip remounted at ${w} px`).toBe(strip)
      expect(document.querySelector('[data-pane="all"]'), `All Decodes remounted at ${w} px`).toBe(all)
    }
  })

  it('the ⊞ menu caps the tracks: one decode pane hidden, two; both hidden, the log alone; the dock stays', async () => {
    await mount(jtty(), fakePanels(['rfScope', 'rx']))
    let el = await resize(1800)
    expect(el.getAttribute('data-cols'), 'a three-track template with an empty track').toBe('2')
    expect(columns(el)).toEqual([['all'], ['log']])
    cleanup()
    await mount(jtty(), fakePanels(['rfScope', 'all', 'rx']))
    el = await resize(1800)
    expect(el.getAttribute('data-cols')).toBe('1')
    expect(el.getAttribute('data-flow'), 'a wide region with one column fills; it does not stack').toBe('fill')
    expect(columns(el)).toEqual([['log']])
    expect(document.querySelector('.cockpit-txdock .cw-send'), 'the dock went with the panes').not.toBeNull()
  })

  it('the RF scope pane ships hidden; ticked, it heads the leading column', async () => {
    await mount()
    expect(document.querySelector('[data-pane="rfScope"]'), 'the RF scope pane shipped visible').toBeNull()
    cleanup()
    await mount(jtty(), fakePanels([]))
    const el = await resize(1800)
    expect(columns(el)).toEqual([['rfScope', 'all'], ['rx'], ['log']])
    expect(el.querySelector('[data-testid="rfscope-stub"]'), 'control: the scope pane drew its picture').not.toBeNull()
  })
})
