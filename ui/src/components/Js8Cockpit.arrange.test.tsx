// @vitest-environment jsdom
//
// JS8, ARRANGED (layout L3): the pane region renders the panel record's placement, the two pair
// dividers follow their pairs only where the pair is adjacent, and THE FIBER-IDENTITY SWEEP: under any
// sequence of moves and tier flips the log form (JS8's `log`, pinned to its column) is never remounted.
// jsdom lays nothing out: widths are stubbed as in Js8Cockpit.structure.test.tsx, whose fixtures this
// file shares.
import type { ReactNode, Ref } from 'react'
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { Js8Cockpit } from './Js8Cockpit'
import type { AppSnapshot, Js8State } from '../types'
import { JS8_PANELS, usePanelLayout, type Js8PanelId, type PanelLayoutApi } from '../features/panelState'
import { arrangeIds, placedColumns, type PaneMove } from '../features/panelPlace'

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

/** The observed element's callback, so a test can fire a resize the way the browser does. */

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

let api: PanelLayoutApi<Js8PanelId> | null = null
function Live() {
  const panels = usePanelLayout(JS8_PANELS)
  api = panels
  return <Js8Cockpit snap={snap} panels={panels} />
}
async function mount() {
  render(<Live />)
  await act(async () => {
    for (let i = 0; i < 4; i++) await Promise.resolve()
  })
}
const region = () => document.querySelector('.cockpit-panes')!
const cols = () => [...region().querySelectorAll(':scope > .cockpit-col')] as HTMLElement[]
const framesIn = (col: Element) => [...col.querySelectorAll(':scope > .pane-frame')].map((f) => f.getAttribute('data-pane'))
const rendered = () => cols().map(framesIn)
const seamsIn = (col: Element) => [...col.querySelectorAll(':scope > [role="separator"]')].map((s) => s.getAttribute('aria-label'))
async function tier(width: number) {
  stubWidth(region(), width)
  act(() => fire!())
  await frame()
}
function expected(tracks: number) {
  const c = placedColumns(JS8_PANELS.arrange!, api!.layout.place)
  const groups = tracks === 3 ? [c.a, c.b, c.log] : c.log.length === 0 && tracks === 2 ? [c.a, c.b] : [[...c.a, ...c.b], c.log]
  return groups.filter((g, i) => tracks === 3 || g.length > 0 || i === -1).map((g) => [...g])
}

describe('JS8 renders the placement', () => {
  it('stock, the grouping is exactly today’s at each tier', async () => {
    await mount()
    await tier(1200)
    expect(rendered()).toEqual([['activity', 'offsets', 'stations', 'inbox'], ['log']])
    await tier(900)
    expect(rendered(), 'one track stacks the same two groups').toEqual([['activity', 'offsets', 'stations', 'inbox'], ['log']])
    await tier(1800)
    expect(rendered()).toEqual([['activity', 'offsets'], ['stations', 'inbox'], ['log']])
  })

  it('a pair keeps its divider only while the two are adjacent in one column', async () => {
    await mount()
    await tier(1800)
    expect(seamsIn(cols()[0]).length, 'Activity | Band activity, stock').toBe(1)
    // The operator's split between them, carried on both frames while they are a pair.
    act(() => api!.setShares({ activity: 1.3, offsets: 0.7 }))
    const shareOf = (id: string) => document.querySelector<HTMLElement>(`[data-pane="${id}"]`)!.style.getPropertyValue('--pane-share')
    expect(shareOf('activity')).not.toBe('')
    act(() => api!.movePane!('offsets', 'right', () => true))
    // Band activity moved under the inbox: the pair is apart, so no divider between them anywhere,
    // and neither frame carries the pair's share (its floor would follow a divider that is not there).
    expect(rendered()).toEqual([['activity'], ['stations', 'inbox', 'offsets'], ['log']])
    expect(cols().flatMap(seamsIn).filter((l) => /activity/i.test(l ?? '')), 'a divider for a pair that is not one').toEqual([])
    expect(shareOf('activity'), 'Activity still sized as half of a pair').toBe('')
    expect(shareOf('offsets'), 'Band activity still sized as half of a pair').toBe('')
    // Stations | Inbox are still adjacent and keep theirs.
    expect(seamsIn(cols()[1]).length).toBe(1)
  })

  it('the log is pinned: it cannot leave its column', () => {
    expect(JS8_PANELS.arrange!.pinned).toEqual(['log'])
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

describe('THE FIBER-IDENTITY SWEEP: no arrangement remounts the log form', () => {
  it('50 random moves with tier flips between them: the region is the placement and the log form the same node throughout', async () => {
    await mount()
    // The RF scope pane ships hidden; ticked, it is one more pane every move may carry, which is
    // the point — none of them may remount the log form either.
    act(() => api!.setPanelState('rfScope', 'docked'))
    await tier(1800)
    const log0 = document.querySelector('[data-testid="log-stub"]')!
    const ids = arrangeIds(JS8_PANELS.arrange!)
    const moves: PaneMove[] = ['up', 'down', 'left', 'right']
    const next = rng(20260929)
    const all = () => true
    let applied = 0
    const seen = new Set<number>()
    for (let step = 0; step < 50; step++) {
      const id = ids[Math.floor(next() * ids.length)]
      const move = moves[Math.floor(next() * moves.length)]
      const before = JSON.stringify(api!.layout.place ?? null)
      act(() => api!.movePane!(id, move, all))
      if (JSON.stringify(api!.layout.place ?? null) !== before) applied++
      // One, two or three tracks (classifyRegionCols: under 1080, under 1700, wider).
      if (step % 7 === 3) await tier([900, 1200, 1800][Math.floor(next() * 3)])
      const tracks = Number(region().getAttribute('data-cols'))
      seen.add(tracks)
      expect(rendered(), `step ${step}: ${id} ${move} — the region is not the placement`).toEqual(expected(tracks))
      expect(document.querySelector('[data-testid="log-stub"]')!.isSameNode(log0), `step ${step}: ${id} ${move} remounted the log form`).toBe(true)
    }
    expect([...seen].sort(), 'the flips never reached every tier').toEqual([1, 2, 3])
    expect(applied, 'too few of the random moves changed the placement').toBeGreaterThan(15)
  })
})
