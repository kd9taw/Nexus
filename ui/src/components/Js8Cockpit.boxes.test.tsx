// @vitest-environment jsdom
//
// JS8'S BOXES (any pane in any area, 2026-10-07): Phone's boxes in JS8's columns, on the REAL panel
// record and the real ⊞ menu. Nobody's screen changes until a box is added, "+ Add a box" puts one at
// a column's foot, a window that lends no boxes draws none, a JS8 that is not on screen mounts no box
// (its keep-alive host stays mounted, and a box's body polls while it is), and THE FIBER-IDENTITY
// SWEEP: no add, pick, hide or move, with tier flips between them, remounts the log form (JS8's `log`,
// pinned to its column). The box's body is stubbed; widths are stubbed as in Js8Cockpit.arrange.test.tsx,
// whose fixtures this file follows.
import type { ReactNode, Ref } from 'react'
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen } from '@testing-library/react'
import { Js8Cockpit } from './Js8Cockpit'
import type { AppSnapshot, Js8State } from '../types'
import { BOX_IDS, JS8_PANELS, boxEntries, panelStorageKey, usePanelLayout, type Js8PanelId, type PanelLayoutApi } from '../features/panelState'
import { arrangeIds, regionGroups, type PaneMove } from '../features/panelPlace'
import { SHARED_PANES } from '../features/sharedPanes'
import type { BoxSource } from './panes/CockpitBox'
import { t } from '../i18n'

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
// The header renders what the cockpit gives it to hold — the ⊞ menu, with Arrange in it.
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: (p: { modeIndicator?: ReactNode; actions?: ReactNode }) => (
    <header className="cockpit-header">
      {p.modeIndicator}
      {p.actions}
    </header>
  ),
}))
vi.mock('./panes/BoxBody', () => ({ BoxBody: ({ pane }: { pane: string }) => <div data-testid={`box-body-${pane}`} /> }))
// The strip's box reaches its divider through `stripRef` (layout L2), so the stub forwards it.
vi.mock('./Waterfall', () => ({
  Waterfall: (p: { stripRef?: Ref<HTMLDivElement> }) => <div className="waterfall-wrap" ref={p.stripRef} />,
}))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))

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

let fire: (() => void) | null = null
beforeEach(() => {
  localStorage.clear()
  state.current = js8Fixture()
  const live = new Set<() => void>()
  fire = () => [...live].forEach((cb) => cb())
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
afterEach(cleanup)

function stubWidth(el: Element, w: number) {
  Object.defineProperty(el, 'clientWidth', { configurable: true, get: () => w })
}
async function frame() {
  await act(async () => {
    await new Promise((r) => requestAnimationFrame(() => r(null)))
  })
}

const SOURCE: BoxSource = { myGrid: 'EN52', theme: 'dark', stations: [], prop: null, needByCall: new Map() }
let api: PanelLayoutApi<Js8PanelId> | null = null
function Live({ lend = true, active = true }: { lend?: boolean; active?: boolean }) {
  const panels = usePanelLayout(JS8_PANELS)
  api = panels
  return <Js8Cockpit snap={snap} panels={panels} active={active} boxes={lend ? SOURCE : undefined} />
}
async function mount(props: { lend?: boolean; active?: boolean } = {}) {
  render(<Live {...props} />)
  await act(async () => {
    for (let i = 0; i < 4; i++) await Promise.resolve()
  })
}
const isBox = (id: string) => (BOX_IDS as readonly string[]).includes(id)
const region = () => document.querySelector('.cockpit-panes')!
const cols = () => [...region().querySelectorAll(':scope > .cockpit-col')] as HTMLElement[]
const framesIn = (col: Element) => [...col.querySelectorAll(':scope > .pane-frame')].map((f) => f.getAttribute('data-pane'))
const rendered = () => cols().map(framesIn)
const boxFrame = (b: string) => document.querySelector(`.pane-frame[data-pane="${b}"]`) as HTMLElement | null
const bodyOf = (b: string) => boxFrame(b)?.querySelector('[data-testid^="box-body-"]')?.getAttribute('data-testid')
async function tier(width: number) {
  stubWidth(region(), width)
  act(() => fire!())
  await frame()
}
const FIRST = SHARED_PANES.map((e) => e.id)
const paneOf = (entry: string) => SHARED_PANES.find((e) => e.id === entry)!.pane
/** JS8's own panes on screen in this fixture: everything but the RF scope pane, which ships hidden. */
const ownShown = (id: Js8PanelId) => id !== 'scope' && api!.stateOf(id) !== 'removed'
/** What the record says the region shows, as Js8Cockpit renders a placement at each tier. */
function expected(tracks: number, shownNow: (id: Js8PanelId) => boolean) {
  const c = regionGroups(JS8_PANELS.arrange!, api!.layout.place, 3, shownNow).map((g) => g.ids)
  const groups = tracks === 3 ? c : c[2].length === 0 && tracks === 2 ? [c[0], c[1]] : [[...c[0], ...c[1]], c[2]]
  return groups.filter((g) => tracks === 3 || g.length > 0)
}

describe('JS8 with boxes', () => {
  it('with no box in the record, the screen is exactly what it was, lent the boxes or not', async () => {
    const shapes: string[] = []
    for (const lend of [false, true]) {
      await mount({ lend })
      for (const w of [900, 1200, 1800]) {
        await tier(w)
        shapes.push(`${w}:${JSON.stringify(rendered())}`)
      }
      expect(document.querySelector('.pane-frame[data-pane^="box"]')).toBeNull()
      cleanup()
    }
    expect(shapes.slice(3)).toEqual(shapes.slice(0, 3))
  })

  it('“+ Add a box” puts one at the foot of the column, showing the first entry not on screen', async () => {
    await mount()
    await tier(1800)
    fireEvent.click(document.querySelector('.panels-menu-btn')!)
    fireEvent.click(screen.getByRole('button', { name: t('panels.box.add.b.aria') }))
    expect(rendered()).toEqual([['activity', 'offsets'], ['stations', 'inbox', 'box1'], ['log']])
    expect(bodyOf('box1')).toBe(`box-body-${paneOf(FIRST[0])}`)
  })

  it('a JS8 that is not on screen mounts no box — its keep-alive host stays mounted, and a box polls while it is', async () => {
    localStorage.setItem(panelStorageKey('js8'), JSON.stringify({ v: 2, state: { box1: 'docked' }, share: {}, boxes: { box1: 'clock' } }))
    await mount({ active: false })
    await tier(1800)
    expect(boxFrame('box1')).toBeNull()
    cleanup()
    await mount({ active: true })
    await tier(1800)
    expect(bodyOf('box1')).toBe('box-body-clock')
  })

  it('a window that lends no boxes (the hosted Remote page) draws none, whatever the record says', async () => {
    localStorage.setItem(panelStorageKey('js8'), JSON.stringify({ v: 2, state: { box1: 'docked' }, share: {}, boxes: { box1: 'clock' } }))
    await mount({ lend: false })
    await tier(1800)
    expect(boxFrame('box1')).toBeNull()
  })
})

function rng(seed: number) {
  let a = seed >>> 0
  return () => {
    a = (a + 0x6d2b79f5) >>> 0
    let t = Math.imul(a ^ (a >>> 15), 1 | a)
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

describe('THE FIBER-IDENTITY SWEEP, with boxes: no add, pick, hide or move remounts the log form', () => {
  it('70 random box and pane acts with tier flips between them', async () => {
    await mount()
    await tier(1800)
    const log0 = document.querySelector('[data-testid="log-stub"]')!
    const ids = arrangeIds(JS8_PANELS.arrange!)
    const moves: PaneMove[] = ['up', 'down', 'left', 'right']
    const areas = ['a', 'b', 'log'] as const
    const next = rng(20261007)
    const pick = <T,>(xs: readonly T[]) => xs[Math.floor(next() * xs.length)]
    const seen = { changed: 0, boxesShown: 0, tiers: new Set<number>() }
    for (let step = 0; step < 70; step++) {
      const entries = boxEntries(JS8_PANELS, api!.layout)
      const shownBoxes = Object.keys(entries) as Js8PanelId[]
      const isShown = (id: Js8PanelId) => (isBox(id) ? entries[id] != null : ownShown(id))
      const roll = next()
      const before = JSON.stringify(api!.layout)
      if (roll < 0.3) act(() => api!.addBox!(pick(areas)))
      else if (roll < 0.45 && shownBoxes.length > 0) act(() => api!.setBox!(pick(shownBoxes), pick(FIRST)))
      else if (roll < 0.55 && shownBoxes.length > 0) act(() => api!.setPanelState(pick(shownBoxes), 'removed'))
      else act(() => api!.movePane!(pick(ids), pick(moves), isShown))
      if (JSON.stringify(api!.layout) !== before) seen.changed++
      if (step % 7 === 3) await tier(pick([900, 1200, 1800]))
      const now = boxEntries(JS8_PANELS, api!.layout)
      const shownNow = (id: Js8PanelId) => (isBox(id) ? now[id] != null : ownShown(id))
      seen.boxesShown += Object.keys(now).length
      const tracks = Number(region().getAttribute('data-cols'))
      seen.tiers.add(tracks)
      expect(rendered(), `step ${step} — the region is not the arrangement`).toEqual(expected(tracks, shownNow))
      for (const [b, e] of Object.entries(now)) expect(bodyOf(b), `step ${step}: ${b}`).toBe(`box-body-${paneOf(e!)}`)
      expect(document.querySelector('[data-testid="log-stub"]')!.isSameNode(log0), `step ${step}: remounted the log form`).toBe(true)
    }
    expect(seen.changed, 'too few acts changed the record').toBeGreaterThan(30)
    expect(seen.boxesShown, 'the boxes were rarely on screen').toBeGreaterThan(80)
    expect([...seen.tiers].sort()).toEqual([1, 2, 3])
  })
})
