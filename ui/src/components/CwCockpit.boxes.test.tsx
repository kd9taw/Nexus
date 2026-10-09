// @vitest-environment jsdom
//
// CW'S BOXES (any pane in any area, 2026-10-07): Phone's boxes in CW's columns — on the REAL panel
// record and the real ⊞ menu. Nobody's screen changes until a box is added, "+ Add a box" puts one at a
// column's foot, the Rig controls frame keeps the head of column 2, a window that lends no boxes draws
// none, and THE FIBER-IDENTITY SWEEP: no add, pick, hide or move, with tier flips between them, remounts
// the log form. The box's body is stubbed (CockpitBox.test.tsx renders the real one); widths are stubbed
// as in CwCockpit.arrange.test.tsx, whose mocks this file follows.
import type { ReactNode } from 'react'
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen, within } from '@testing-library/react'
import { CwCockpit } from './CwCockpit'
import type { AppSnapshot } from '../types'
import { BOX_IDS, CW_PANELS, boxEntries, panelStorageKey, usePanelLayout, type CwPanelId, type PanelLayoutApi } from '../features/panelState'
import { arrangeIds, isStockPlacement, regionGroups, type PaneMove } from '../features/panelPlace'
import { SHARED_PANES } from '../features/sharedPanes'
import type { BoxSource } from './panes/CockpitBox'
import { t } from '../i18n'

// THE BUDGET (2026-10-09). The slowest case here, "70 random box and pane acts with tier flips between them", takes
// 0.83 s and 0.83 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const decodeState = {
  text: 'CQ CQ DE KD9TAW',
  wpm: 22,
  sent: ['CQ CQ DE KD9TAW K'] as string[],
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

vi.mock('../api', () => ({
  getSettings: vi.fn(async () => ({ macros: { cwProfiles: [], activeCwProfile: 0 }, rigModel: 0 })),
  getCatCwUnprovenRigModels: vi.fn(async () => []),
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

vi.mock('./CockpitHeader', () => ({
  CockpitHeader: ({ actions }: { actions?: ReactNode }) => <header className="cockpit-header">{actions}</header>,
}))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))
vi.mock('./panes/BoxBody', () => ({ BoxBody: ({ pane }: { pane: string }) => <div data-testid={`box-body-${pane}`} /> }))

let fire: (() => void) | null = null
beforeEach(() => {
  localStorage.clear()
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
function makeSnap(): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.05, band: '20m', catOk: true, sideband: 'USB', rigMode: 'CW', transmitting: false, txEnabled: true,
      txAllowed: true, cwWpm: 22, cwKeyer: 'cat', nrLevel: 0.3, agc: 'fast', nb: true, nr: true, notch: null,
      filterWidthHz: 500, splitTxMhz: null, smeterDb: null,
    },
  } as unknown as AppSnapshot
}

const SOURCE: BoxSource = { myGrid: 'EN52', theme: 'dark', stations: [], prop: null, needByCall: new Map() }
const spotsBoard = { bandPlan: [], selectedCall: null, onSelect: () => {}, onWork: () => {} }
const neededBoard = { alerts: [], bandPlan: [], selectedCall: null, onQsy: () => {}, onSelect: () => {} }

let api: PanelLayoutApi<CwPanelId> | null = null
function Live({ lend = true, source = SOURCE }: { lend?: boolean; source?: BoxSource }) {
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
      boxes={lend ? source : undefined}
    />
  )
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
async function mount(lend = true) {
  render(<Live lend={lend} />)
  await act(async () => {
    for (let i = 0; i < 4; i++) await Promise.resolve()
  })
}
const FIRST = SHARED_PANES.map((e) => e.id)
const paneOf = (entry: string) => SHARED_PANES.find((e) => e.id === entry)!.pane
/** CW's own panes on screen in this fixture (the two feeds only once ticked). */
const ownShown = (id: CwPanelId) => id !== 'spots' && id !== 'needed' ? true : api!.stateOf(id) !== 'removed'

/** What the record says the region shows: the Rig controls frame at the head of column 2 (and, below
 *  three tracks, where column 2's panes begin, ahead of the feeds on the stock placement). */
function expected(tracks: number, shownNow: (id: CwPanelId) => boolean) {
  const place = api!.layout.place
  const g3 = regionGroups(CW_PANELS.arrange!, place, 3, shownNow)
  if (tracks === 3) return [g3[0].ids, ['rigctl', ...g3[1].ids], [...g3[2].ids, 'log']]
  const merged = regionGroups(CW_PANELS.arrange!, place, 2, shownNow)[0].ids
  const stock = isStockPlacement(CW_PANELS.arrange!, place)
  const mid = merged.findIndex((id) => !g3[0].ids.includes(id) || (stock && (id === 'spots' || id === 'needed')))
  const at = mid < 0 ? merged.length : mid
  return [[...merged.slice(0, at), 'rigctl', ...merged.slice(at)], [...g3[2].ids, 'log']]
}

describe('CW with boxes', () => {
  it('with no box in the record, the screen is exactly what it was, lent the boxes or not', async () => {
    const shapes: string[] = []
    for (const lend of [false, true]) {
      await mount(lend)
      for (const w of [900, 1200, 1800]) {
        await tier(w)
        shapes.push(`${w}:${JSON.stringify(rendered())}`)
      }
      expect(document.querySelector('.pane-frame[data-pane^="box"]')).toBeNull()
      cleanup()
    }
    expect(shapes.slice(3)).toEqual(shapes.slice(0, 3))
  })

  it('“+ Add a box” puts one at the foot of the column; the Rig controls keep the head of column 2', async () => {
    await mount()
    await tier(1800)
    fireEvent.click(document.querySelector('.panels-menu-btn')!)
    fireEvent.click(screen.getByRole('button', { name: t('panels.box.add.b.aria') }))
    expect(rendered()).toEqual([['decode', 'sent'], ['rigctl', 'bandActivity', 'copilot', 'box1'], ['log']])
    expect(bodyOf('box1')).toBe(`box-body-${paneOf(FIRST[0])}`)
    fireEvent.click(screen.getByRole('button', { name: t('panels.box.add.log.aria') }))
    expect(rendered()[2]).toEqual(['box2', 'log'])
  })

  it('picking the Spots board while CW’s own Spots pane shows it moves the board into the box', async () => {
    await mount()
    await tier(1800)
    act(() => api!.setPanelState('spots', 'docked'))
    act(() => api!.addBox!('b'))
    fireEvent.change(boxFrame('box1')!.querySelector('select')!, { target: { value: 'spotsBoard' } })
    expect(document.querySelector('.pane-frame[data-pane="spots"]')).toBeNull()
    expect(bodyOf('box1')).toBe('box-body-spots')
  })

  it('a window that lends no boxes (the hosted Remote page) draws none, whatever the record says', async () => {
    localStorage.setItem(panelStorageKey('cw'), JSON.stringify({ v: 2, state: { box1: 'docked' }, share: {}, boxes: { box1: 'clock' } }))
    await mount(false)
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

describe('the dashboard rail’s boxes, on a window too small for the rail', () => {
  it('stand at the foot of the leading column at every tier, with no ✕, and keep the leading track alone', async () => {
    const pick = vi.fn()
    const source: BoxSource = {
      ...SOURCE,
      rail: { shows: ['clock'], take: vi.fn(), folded: { boxes: [{ slot: 'rail1', pane: 'clock', entry: 'clock' }], pick } },
    }
    render(<Live source={source} />)
    await act(async () => {
      for (let i = 0; i < 4; i++) await Promise.resolve()
    })
    for (const w of [700, 1300, 1800]) {
      await tier(w)
      expect(framesIn(cols()[0]).slice(-1), `at ${w} px`).toEqual(['rail1'])
    }
    expect(within(boxFrame('rail1')!).queryByRole('button', { name: /^Hide/ })).toBeNull()
    fireEvent.change(boxFrame('rail1')!.querySelector('select.pane-pick')!, { target: { value: 'getout' } })
    expect(pick).toHaveBeenCalledWith('rail1', 'getout')
    // A box counts for the column it stands in: with CW's own panes there hidden, it keeps the leading track.
    act(() => api!.setPanelState('decode', 'removed'))
    act(() => api!.setPanelState('sent', 'removed'))
    await tier(1800)
    expect(region().getAttribute('data-cols')).toBe('3')
    expect(framesIn(cols()[0])).toEqual(['rail1'])
  })
})

describe('THE FIBER-IDENTITY SWEEP, with boxes: no add, pick, hide or move remounts the log form', () => {
  it('70 random box and pane acts with tier flips between them', async () => {
    await mount()
    await tier(1800)
    const log0 = document.querySelector('[data-testid="log-stub"]')!
    const ids = arrangeIds(CW_PANELS.arrange!)
    const moves: PaneMove[] = ['up', 'down', 'left', 'right']
    const areas = ['a', 'b', 'log'] as const
    const next = rng(20261007)
    const pick = <T,>(xs: readonly T[]) => xs[Math.floor(next() * xs.length)]
    const seen = { changed: 0, boxesShown: 0, tiers: new Set<number>() }
    for (let step = 0; step < 70; step++) {
      const entries = boxEntries(CW_PANELS, api!.layout)
      const shownBoxes = Object.keys(entries) as CwPanelId[]
      const isShown = (id: CwPanelId) => (isBox(id) ? entries[id] != null : ownShown(id))
      const roll = next()
      const before = JSON.stringify(api!.layout)
      if (roll < 0.3) act(() => api!.addBox!(pick(areas)))
      else if (roll < 0.45 && shownBoxes.length > 0) act(() => api!.setBox!(pick(shownBoxes), pick(FIRST)))
      else if (roll < 0.55 && shownBoxes.length > 0) act(() => api!.setPanelState(pick(shownBoxes), 'removed'))
      else act(() => api!.movePane!(pick(ids), pick(moves), isShown))
      if (JSON.stringify(api!.layout) !== before) seen.changed++
      if (step % 7 === 3) await tier(pick([900, 1200, 1800]))
      const now = boxEntries(CW_PANELS, api!.layout)
      const shownNow = (id: CwPanelId) => (isBox(id) ? now[id] != null : ownShown(id))
      seen.boxesShown += Object.keys(now).length
      const tracks = Number(region().getAttribute('data-cols')) as 1 | 2 | 3
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
