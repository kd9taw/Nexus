// @vitest-environment jsdom
//
// CW, ARRANGED (layout L3): the pane region renders the panel record's placement — the Rig controls
// frame (not a vocabulary pane) holding the head of the middle column — and THE FIBER-IDENTITY SWEEP:
// under any sequence of moves and tier flips the log form is never remounted (a half-typed contact
// would be lost). jsdom lays nothing out: widths are stubbed as in CwCockpit.structure.test.tsx, whose
// fixtures this file shares.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { CwCockpit } from './CwCockpit'
import type { AppSnapshot } from '../types'
import { BOX_IDS, CW_PANELS, panelStorageKey, usePanelLayout, type CwPanelId, type PanelLayoutApi } from '../features/panelState'
import { arrangeIds, isStockPlacement, placedColumns, type PaneMove } from '../features/panelPlace'

const decodeState = {
  text: 'CQ CQ DE KD9TAW',
  wpm: 22,
  sent: [] as string[],
  keyerError: null as string | null,
  candidates: [] as { call: string; best: boolean }[],
  state: 'listening',
  headline: '',
  prompt: '',
  recommended: null as string | null,
  workedCall: null as string | null,
  rst: null as string | null,
  name: null as string | null,
}

/** What the mount-time `getSettings` resolves with. Mutable per test, like `decodeState` —
 *  the cockpit reads `rigModel` from here to decide whether the CAT-keying caution applies. */
const settingsState = {
  macros: { cwProfiles: [] as unknown[], activeCwProfile: 0 },
  rigModel: 0,
}
/** The backend's "CAT CW keying is unproven on this model" list. Empty = rule unread. */
let unprovenModels: number[] = []

vi.mock('../api', () => ({
  getSettings: vi.fn(async () => settingsState),
  getCatCwUnprovenRigModels: vi.fn(async () => unprovenModels),
  setSettings: vi.fn(async () => ({})),
  sendCw: vi.fn(async () => {}),
  setCwKeyer: vi.fn(async () => null),
  setCwWpm: vi.fn(async () => {}),
  stopCw: vi.fn(async () => {}),
  cwDecode: vi.fn(async () => decodeState),
  cwClear: vi.fn(async () => {}),
  setAiCw: vi.fn(async () => {}),
  selectPeer: vi.fn(async () => null),
  previewCw: vi.fn(async (t: string) => t),
  pointRotatorAtCall: vi.fn(async () => 0),
  setRigFunc: vi.fn(async () => ({})),
  setFilterWidth: vi.fn(async () => ({})),
  setNrLevel: vi.fn(async () => {}),
  setAgc: vi.fn(async () => ({})),
  setScopeSpan: vi.fn(async () => ({})),
  setScopeRef: vi.fn(async () => {}),
  setFlexPanSpan: vi.fn(async () => ({})),
  setFlexPanRef: vi.fn(async () => ({})),
  openPanelWindow: vi.fn(async () => {}),
  setTune: vi.fn(async () => ({})),
  setFrequency: vi.fn(async () => ({})),
  haltTx: vi.fn(async () => ({})),
}))

vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
// The stub reports `titled` so this suite can see the ONE prop that is a placement decision
// rather than log behaviour: whether the strip draws its own heading under a frame head that
// already says LOG. The strip's own half is in LogEntry.density.test.tsx.
vi.mock('./LogEntry', () => ({
  LogEntry: (p: { titled?: boolean }) => (
    <div data-testid="log-stub" data-titled={String(p.titled ?? true)} />
  ),
}))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))


let fire: (() => void) | null = null
beforeEach(() => {
  localStorage.clear()
  decodeState.sent = ['CQ CQ DE KD9TAW K']
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

function makeSnap(over: Record<string, unknown> = {}): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.05,
      band: '20m',
      catOk: true,
      sideband: 'USB',
      rigMode: 'CW',
      transmitting: false,
      txEnabled: true,
      txAllowed: true,
      cwWpm: 22,
      cwKeyer: 'cat',
      nrLevel: 0.3,
      agc: 'fast',
      nb: true,
      nr: true,
      notch: null,
      filterWidthHz: 500,
      splitTxMhz: null,
      smeterDb: null,
      ...over,
    },
  } as unknown as AppSnapshot
}


let api: PanelLayoutApi<CwPanelId> | null = null
// The two feeds (plan H8) render only with their boards lent, as App lends them; they ship hidden.
const spotsBoard = { bandPlan: [], selectedCall: null, onSelect: () => {}, onWork: () => {} }
const neededBoard = { alerts: [], bandPlan: [], selectedCall: null, onQsy: () => {}, onSelect: () => {} }
function Live() {
  const panels = usePanelLayout(CW_PANELS)
  api = panels
  return (
    <CwCockpit
      snap={makeSnap()}
      theme="dark"
      onWorkSpot={() => {}}
      spots={[]}
      panels={panels}
      spotsBoard={spotsBoard}
      neededBoard={neededBoard}
    />
  )
}

const region = () => document.querySelector('.cockpit-panes')!
const cols = () => [...region().querySelectorAll(':scope > .cockpit-col')] as HTMLElement[]
const framesIn = (col: Element) => [...col.querySelectorAll(':scope > .pane-frame')].map((f) => f.getAttribute('data-pane'))
const rendered = () => cols().map(framesIn)
async function tier(width: number) {
  stubWidth(region(), width)
  act(() => fire!())
  await frame()
}
async function mount() {
  render(<Live />)
  await act(async () => {
    for (let i = 0; i < 4; i++) await Promise.resolve()
  })
}
/** What the placement says the region shows (every CW pane renders in this fixture once the two
 *  feeds are ticked, as the sweep ticks them), with the Rig controls frame at the head of the
 *  middle column — and below three tracks, on the stock placement, the two feeds after every strip
 *  (Phone's rule, `stockMerged`). */
function expected(tracks: number) {
  const place = api!.layout.place
  // The boxes (2026-10-07) ship hidden and none is added here: the columns without them.
  const all = placedColumns(CW_PANELS.arrange!, place)
  const own = (ids: readonly CwPanelId[]) => ids.filter((id) => !(BOX_IDS as readonly string[]).includes(id))
  const c = { a: own(all.a), b: own(all.b), log: own(all.log) }
  const mid = ['rigctl', ...c.b]
  if (tracks === 3) return [[...c.a], mid, [...c.log, 'log']]
  if (isStockPlacement(CW_PANELS.arrange!, place)) {
    return [['decode', 'sent', 'rigctl', 'bandActivity', 'copilot', 'spots', 'needed'], ['log']]
  }
  return [[...c.a, ...mid], [...c.log, 'log']]
}

describe('CW renders the placement', () => {
  it('stock, the grouping is exactly today’s at each tier', async () => {
    await mount()
    await tier(1200)
    expect(rendered()).toEqual([['decode', 'sent', 'rigctl', 'bandActivity', 'copilot'], ['log']])
    await tier(900)
    expect(rendered(), 'one track stacks the same two groups').toEqual([['decode', 'sent', 'rigctl', 'bandActivity', 'copilot'], ['log']])
    await tier(1800)
    expect(rendered()).toEqual([['decode', 'sent'], ['rigctl', 'bandActivity', 'copilot'], ['log']])
  })

  it('a stored placement opens where the operator left it; the Rig controls frame keeps the head of column 2', async () => {
    localStorage.setItem(
      panelStorageKey('cw'),
      JSON.stringify({ v: 2, state: {}, share: {}, place: { copilot: { col: 'a', order: 0 }, sent: { col: 'log', order: 0 } } }),
    )
    await mount()
    await tier(1800)
    expect(rendered()).toEqual([['copilot', 'decode'], ['rigctl', 'bandActivity'], ['sent', 'log']])
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
    // The feeds ticked, so every id the moves draw is on screen.
    localStorage.setItem(panelStorageKey('cw'), JSON.stringify({ v: 2, state: { spots: 'docked', needed: 'docked' }, share: {} }))
    await mount()
    await tier(1800)
    const log0 = document.querySelector('[data-testid="log-stub"]')!
    const ids = arrangeIds(CW_PANELS.arrange!)
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
