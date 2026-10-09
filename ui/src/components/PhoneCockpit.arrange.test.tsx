// @vitest-environment jsdom
//
// PHONE, ARRANGED (layout L3): the pane region renders the panel record's placement — each pane in its
// column, in its order — at every tier, and THE FIBER-IDENTITY SWEEP: under any sequence of moves and
// tier flips the log form and the voice keyer are never remounted (a remount of the keyer stops its
// over and discards its recording; of the log form, loses a half-typed contact). jsdom lays nothing
// out: widths are stubbed as in PhoneCockpit.structure.test.tsx, whose mocks this file shares.
//
// THE LEFT SIDE (2026-10-03): the record's `leftSide` renders beside the scope on a window about 1280 px
// wide or wider, falls back into the columns below that with the record untouched, and comes back; the
// sweep then runs the side's moves and the window crossing that width as well.
import { useRef } from 'react'
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import type { AppSnapshot } from '../types'
import { PHONE_PANELS, panelStorageKey, usePanelLayout, type PanelLayoutApi, type PhonePanelId } from '../features/panelState'
import { arrangeIds, placedColumns, regionGroups, type PaneMove } from '../features/panelPlace'

// THE BUDGET (2026-10-09). The slowest case here, "60 random moves, the side included, with window and…", takes
// 0.58 s and 0.57 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', () => ({
  // The Phone cockpit reads the FM repeater shift from Settings — it is the only surface
  // that carries it, and the transmit contract will not state a frequency without it.
  getSettings: vi.fn(async () => ({})),
  setPtt: vi.fn(async () => {}),
  setRfPower: vi.fn(async () => {}),
  setMicGain: vi.fn(async () => {}),
  setNrLevel: vi.fn(async () => {}),
  setAgc: vi.fn(async () => ({})),
  setScopeSpan: vi.fn(async () => ({})),
  setScopeRef: vi.fn(async () => {}),
  setFlexPanSpan: vi.fn(async () => ({})),
  setFlexPanRef: vi.fn(async () => ({})),
  startQsoRecording: vi.fn(async () => ({})),
  stopQsoRecording: vi.fn(async () => ({})),
  setTune: vi.fn(async () => ({})),
  haltTx: vi.fn(async () => ({})),
  setFrequency: vi.fn(async () => ({})),
  setSplit: vi.fn(async () => ({})),
  setRigFunc: vi.fn(async () => ({})),
  setSidebandOverride: vi.fn(async () => ({})),
  setFilterWidth: vi.fn(async () => ({})),
  openPanelWindow: vi.fn(async () => {}),
}))

// Structure-irrelevant heavy children → stubs. The stubs keep a testid so "the pane's
// content is inside its frame" stays assertable.
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="vk-stub" /> }))
// The stub reports `titled` so this suite can see the ONE prop that is a placement decision
// rather than log behaviour: whether the strip draws its own heading under a frame head that
// already says LOG. The strip's own half is in LogEntry.density.test.tsx.
vi.mock('./LogEntry', () => ({
  LogEntry: (p: { titled?: boolean }) => (
    <div data-testid="log-stub" data-titled={String(p.titled ?? true)} />
  ),
}))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))
// The two boards Phone hosts as feeds (#345). Stubbed like every other pane's content: this
// suite asserts WHERE they sit and that tier flips keep them, PhoneCockpit.boards.test.tsx what
// they show.
vi.mock('./SpotsPanel', () => ({ SpotsPanel: () => <div data-testid="spots-stub" /> }))
vi.mock('./NeededPanel', () => ({ NeededPanel: () => <div data-testid="needed-stub" /> }))

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

function makeSnap(over: Record<string, unknown> = {}): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.2,
      band: '20m',
      catOk: true,
      sideband: 'USB',
      sidebandOverride: null,
      rigMode: 'USB',
      transmitting: false,
      txEnabled: true,
      txAllowed: true,
      qsoRecording: false,
      rfPower: null,
      micGain: null,
      nrLevel: 0.3,
      agc: 'fast',
      nb: true,
      nr: true,
      notch: null,
      comp: null,
      vox: null,
      filterWidthHz: null,
      splitTxMhz: null,
      smeterDb: null,
      rxLevel: 0,
      phoneSegLo: null,
      phoneSegHi: null,
      ...over,
    },
  } as unknown as AppSnapshot
}


/** The cockpit on the REAL panel record; the test drives moves through `api`, as ⊞ Arrange does. */
let api: PanelLayoutApi<PhonePanelId> | null = null
function Live() {
  const panels = usePanelLayout(PHONE_PANELS)
  const ref = useRef(panels)
  ref.current = panels
  api = panels
  return <PhoneCockpit snap={makeSnap()} theme="dark" onWorkSpot={() => {}} spots={[]} panels={panels} />
}

const region = () => document.querySelector('.cockpit-panes')!
const cols = () => [...region().querySelectorAll(':scope > .cockpit-col')] as HTMLElement[]
const framesIn = (col: Element) => [...col.querySelectorAll(':scope > .pane-frame')].map((f) => f.getAttribute('data-pane'))
async function tier(width: number) {
  stubWidth(region(), width)
  act(() => fire!())
  await frame()
}
/** What this Phone renders (no spots/needed boards, no native scope): */
const SHOWN = new Set<PhonePanelId>(['bandActivity', 'voiceKeyer', 'receiver', 'transmitter'])
const shown = (id: PhonePanelId) => SHOWN.has(id)
/** The columns the placement says, as the region must render them. */
function expected(tracks: 1 | 2 | 3) {
  return regionGroups(PHONE_PANELS.arrange!, api!.layout.place, tracks, shown)
    .filter((g) => g.col === 'log' || tracks === 3 || g.ids.length > 0)
    .map((g) => (g.col === 'log' ? [...g.ids, 'log'] : g.ids))
}
const rendered = () => cols().map(framesIn)

describe('Phone renders the placement', () => {
  it('a stored placement opens where the operator left it, at every tier', async () => {
    localStorage.setItem(
      panelStorageKey('phone'),
      JSON.stringify({ v: 2, state: {}, share: {}, place: { receiver: { col: 'a', order: 0 }, transmitter: { col: 'log', order: 0 } } }),
    )
    render(<Live />)
    // Column 2 has nothing on screen left in it, so even a wide region is two tracks: a | log.
    await tier(1800)
    expect(region().getAttribute('data-cols')).toBe('2')
    expect(rendered()).toEqual([['receiver', 'bandActivity', 'voiceKeyer'], ['transmitter', 'log']])
    await tier(1200)
    expect(rendered()).toEqual([['receiver', 'bandActivity', 'voiceKeyer'], ['transmitter', 'log']])
  })

  it('a placement that keeps something in each column is three tracks wide, each pane where it was put', async () => {
    localStorage.setItem(
      panelStorageKey('phone'),
      JSON.stringify({ v: 2, state: {}, share: {}, place: { receiver: { col: 'log', order: 0 }, voiceKeyer: { col: 'a', order: 0 } } }),
    )
    render(<Live />)
    await tier(1800)
    expect(region().getAttribute('data-cols')).toBe('3')
    expect(rendered()).toEqual([['voiceKeyer', 'bandActivity'], ['transmitter'], ['receiver', 'log']])
    // Two tracks: column 2 follows column 1.
    await tier(1200)
    expect(rendered()).toEqual([['voiceKeyer', 'bandActivity', 'transmitter'], ['receiver', 'log']])
  })

  it('column 2 left empty by the operator gives its track back: two columns, not an empty third', async () => {
    render(<Live />)
    for (const id of ['receiver', 'transmitter'] as const) act(() => api!.movePane!(id, 'left', shown))
    await tier(1800)
    expect(region().getAttribute('data-cols'), 'a bounded tier never holds a track with nothing in it').toBe('2')
    expect(rendered()).toEqual([['bandActivity', 'voiceKeyer', 'receiver', 'transmitter'], ['log']])
  })

  it('stock, the grouping is exactly today’s at each tier', async () => {
    render(<Live />)
    await tier(1200)
    expect(rendered()).toEqual([['bandActivity', 'voiceKeyer', 'receiver', 'transmitter'], ['log']])
    await tier(900)
    expect(rendered(), 'one track stacks the same two groups').toEqual([['bandActivity', 'voiceKeyer', 'receiver', 'transmitter'], ['log']])
    await tier(1800)
    expect(rendered()).toEqual([['bandActivity', 'voiceKeyer'], ['receiver', 'transmitter'], ['log']])
  })
})

/** A small seeded generator, so a red names a reproducible sequence. */
function rng(seed: number) {
  let a = seed >>> 0
  return () => {
    a = (a + 0x6d2b79f5) >>> 0
    let t = Math.imul(a ^ (a >>> 15), 1 | a)
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

describe('THE FIBER-IDENTITY SWEEP: no arrangement remounts the log form or the voice keyer', () => {
  it('50 random moves with tier flips between them: the same two DOM nodes throughout, the keyer always leading', async () => {
    render(<Live />)
    await tier(1800)
    const log0 = document.querySelector('[data-testid="log-stub"]')!
    const vk0 = document.querySelector('[data-testid="vk-stub"]')!
    const ids = arrangeIds(PHONE_PANELS.arrange!)
    const moves: PaneMove[] = ['up', 'down', 'left', 'right']
    const next = rng(20260929)
    let applied = 0
    const seen = new Set<number>()
    for (let step = 0; step < 50; step++) {
      const id = ids[Math.floor(next() * ids.length)]
      const move = moves[Math.floor(next() * moves.length)]
      const before = JSON.stringify(api!.layout.place ?? null)
      act(() => api!.movePane!(id, move, shown))
      if (JSON.stringify(api!.layout.place ?? null) !== before) applied++
      // One, two or three tracks (classifyRegionCols: under 1080, under 1700, wider).
      if (step % 7 === 3) await tier([900, 1200, 1800][Math.floor(next() * 3)])
      const tracks = Number(region().getAttribute('data-cols')) as 1 | 2 | 3
      seen.add(tracks)
      expect(rendered(), `step ${step}: ${id} ${move} — the region is not the placement`).toEqual(expected(tracks))
      expect(document.querySelector('[data-testid="log-stub"]')!.isSameNode(log0), `step ${step}: ${id} ${move} remounted the log form`).toBe(true)
      expect(document.querySelector('[data-testid="vk-stub"]')!.isSameNode(vk0), `step ${step}: ${id} ${move} remounted the voice keyer`).toBe(true)
      expect(cols()[0].contains(vk0), `step ${step}: the voice keyer left the leading column`).toBe(true)
    }
    // The sweep must have MOVED things, or it proved nothing.
    expect([...seen].sort(), 'the flips never reached every tier').toEqual([1, 2, 3])
    expect(applied, 'too few of the random moves changed the placement').toBeGreaterThan(15)
  })
})

// ── THE LEFT SIDE (2026-10-03) ─────────────────────────────────────────────────────────────────────

/** The effective window width useViewport publishes on <html>; the side shows from 1280. A change
 *  reaches the cockpit through a MutationObserver, so it is flushed like a frame. */
async function windowWidth(px: number | null) {
  await act(async () => {
    if (px == null) document.documentElement.style.removeProperty('--vw-eff')
    else document.documentElement.style.setProperty('--vw-eff', `${px}px`)
    await Promise.resolve()
  })
}
afterEach(() => document.documentElement.style.removeProperty('--vw-eff'))

/** Phone with the two boards wired, so Spots and Needed can be shown (they ship hidden). */
function LiveFeeds() {
  const panels = usePanelLayout(PHONE_PANELS)
  api = panels
  return (
    <PhoneCockpit
      snap={makeSnap()}
      theme="dark"
      onWorkSpot={() => {}}
      spots={[]}
      panels={panels}
      spotsBoard={{ bandPlan: [], selectedCall: null, onSelect: () => {}, onWork: () => {} }}
      neededBoard={{ alerts: [], bandPlan: [], selectedCall: null, onQsy: () => {}, onSelect: () => {} }}
    />
  )
}
const side = () => document.querySelector('.cockpit-left')
const sideFrames = () => (side() ? framesIn(side()!.querySelector('.cockpit-left-col')!) : null)

describe('Phone renders the left side', () => {
  const RECORD = { v: 2, state: { spots: 'docked', needed: 'docked' }, share: {}, leftSide: ['spots', 'bandActivity'] }

  it('opens where the operator left it on a wide window: full height beside the scope, out of the region', async () => {
    localStorage.setItem(panelStorageKey('phone'), JSON.stringify(RECORD))
    await windowWidth(1600)
    render(<LiveFeeds />)
    await tier(1200)
    expect(sideFrames()).toEqual(['spots', 'bandActivity'])
    expect(rendered().flat(), 'a pane on the side is not in the region as well').not.toContain('bandActivity')
    expect(rendered().flat()).not.toContain('spots')
    // The shell: header, the row (side | stage), the dock — the dock after the row, full width.
    const shell = document.querySelector('main.phone-cockpit')!
    const row = shell.querySelector(':scope > .cockpit-leftrow')!
    expect(row, 'the side shows but the row is not a box').not.toBeNull()
    expect([...row.children].map((c) => c.className)).toEqual(['cockpit-left', 'cockpit-stage'])
    expect(row.nextElementSibling?.className).toMatch(/^cockpit-txdock/)
    // Nothing that stops a transmission is on the side; the strip is in the stage, under the scope.
    expect(side()!.querySelector('.cockpit-txstrip, .cockpit-txdock, .ph-ptt')).toBeNull()
    const strip = row.querySelector(':scope > .cockpit-stage > .cockpit-txstrip')!
    expect(strip.previousElementSibling?.matches('.pane-splitter'), 'the strip left its place under the scope').toBe(true)
  })

  it('below about 1280 px its panes stand in their usual columns, the record untouched — and come back when the window widens', async () => {
    localStorage.setItem(panelStorageKey('phone'), JSON.stringify(RECORD))
    await windowWidth(1600)
    render(<LiveFeeds />)
    await tier(1200)
    const stored = localStorage.getItem(panelStorageKey('phone'))
    expect(sideFrames()).toEqual(['spots', 'bandActivity'])
    await windowWidth(1279)
    expect(side(), 'the side still shows on a narrow window').toBeNull()
    expect(document.querySelectorAll('main.phone-cockpit > .cockpit-flat > .cockpit-flat').length, 'the wrappers kept a box').toBe(1)
    // Their usual columns: the record's placement (stock here), exactly as with no side at all.
    await tier(1200)
    expect(rendered()).toEqual([['bandActivity', 'voiceKeyer', 'receiver', 'transmitter', 'spots', 'needed'], ['log']])
    expect(localStorage.getItem(panelStorageKey('phone')), 'a narrow window rewrote the stored arrangement').toBe(stored)
    expect(api!.layout.leftSide).toEqual(['spots', 'bandActivity'])
    await windowWidth(1280)
    expect(sideFrames(), 'the side did not come back at 1280').toEqual(['spots', 'bandActivity'])
    expect(localStorage.getItem(panelStorageKey('phone'))).toBe(stored)
  })

  it('an old record (no left side) opens exactly as before on a wide window', async () => {
    localStorage.setItem(panelStorageKey('phone'), JSON.stringify({ v: 2, state: { spots: 'docked' }, share: {}, cols: { log: 480 } }))
    await windowWidth(2560)
    render(<LiveFeeds />)
    await tier(1800)
    expect(side()).toBeNull()
    expect(document.querySelector('.cockpit-leftrow, .cockpit-stage'), 'a wrapper took a box with no side to show').toBeNull()
    expect(rendered()).toEqual([['bandActivity', 'voiceKeyer', 'spots'], ['receiver', 'transmitter'], ['log']])
    expect(api!.layout).toEqual({ v: 2, state: { spots: 'docked' }, share: {}, cols: { log: 480 } })
  })

  it('a side whose panes are all hidden is not drawn, and the region takes them back on the tick', async () => {
    localStorage.setItem(panelStorageKey('phone'), JSON.stringify({ v: 2, state: {}, share: {}, leftSide: ['spots'] }))
    await windowWidth(1600)
    render(<LiveFeeds />)
    await tier(1200)
    // Spots ships hidden: nothing on the side is on screen, so there is no side.
    expect(side()).toBeNull()
    act(() => api!.setPanelState('spots', 'docked'))
    expect(sideFrames()).toEqual(['spots'])
  })

  it('the Spots / Needed divider is between them on the side, at any tier', async () => {
    localStorage.setItem(panelStorageKey('phone'), JSON.stringify({ v: 2, state: { spots: 'docked', needed: 'docked' }, share: {}, leftSide: ['spots', 'needed'] }))
    await windowWidth(1600)
    render(<LiveFeeds />)
    await tier(900)
    const col = side()!.querySelector('.cockpit-left-col')!
    expect([...col.children].map((c) => (c.getAttribute('role') === 'separator' ? 'seam' : c.getAttribute('data-pane')))).toEqual(['spots', 'seam', 'needed'])
    // A feed on the side is in a bounded column, never the stacking flow's content-height wrapper.
    expect(col.querySelector('.np-pane--stacked')).toBeNull()
  })
})

describe('THE FIBER-IDENTITY SWEEP, with the left side and the window crossing about 1280 px', () => {
  it('60 random moves, the side included, with window and tier flips between them: the log form, the keyer and the scope never remount', async () => {
    localStorage.setItem(panelStorageKey('phone'), JSON.stringify({ v: 2, state: { spots: 'docked', needed: 'docked' }, share: {}, leftSide: ['bandActivity'] }))
    await windowWidth(1600)
    render(<LiveFeeds />)
    await tier(1800)
    const log0 = document.querySelector('[data-testid="log-stub"]')!
    const vk0 = document.querySelector('[data-testid="vk-stub"]')!
    const scope0 = document.querySelector('[data-testid="scope-stub"]')!
    const shownAll = (id: PhonePanelId) => (SHOWN.has(id) || id === 'spots' || id === 'needed')
    const ids = arrangeIds(PHONE_PANELS.arrange!)
    const moves: PaneMove[] = ['up', 'down', 'left', 'right']
    const next = rng(20261003)
    let wide = true
    let sideSeen = 0
    let crossings = 0
    const records = new Set<string>()
    for (let step = 0; step < 60; step++) {
      const id = ids[Math.floor(next() * ids.length)]
      const move = moves[Math.floor(next() * moves.length)]
      act(() => api!.movePane!(id, move, shownAll, wide))
      records.add(JSON.stringify({ p: api!.layout.place ?? null, s: api!.layout.leftSide ?? null }))
      if (step % 5 === 2) {
        const w = [1100, 1279, 1280, 1600, 2560][Math.floor(next() * 5)]
        if (w >= 1280 !== wide) crossings++
        wide = w >= 1280
        await windowWidth(w)
      }
      if (step % 7 === 3) await tier([900, 1200, 1800][Math.floor(next() * 3)])
      // Where every pane stands: the side's panes on it (wide), every other in its column.
      const onSide = wide ? (api!.layout.leftSide ?? []).filter(shownAll) : []
      if (onSide.length > 0) sideSeen++
      expect(sideFrames() ?? [], `step ${step}: ${id} ${move}`).toEqual(onSide)
      const tracks = Number(region().getAttribute('data-cols')) as 1 | 2 | 3
      const want = regionGroups(PHONE_PANELS.arrange!, api!.layout.place, tracks, (x) => shownAll(x) && !onSide.includes(x))
        .filter((g) => g.col === 'log' || tracks === 3 || g.ids.length > 0)
        .map((g) => (g.col === 'log' ? [...g.ids, 'log'] : g.ids))
      expect(rendered(), `step ${step}: ${id} ${move} — the region is not the arrangement`).toEqual(want)
      expect(document.querySelector('[data-testid="log-stub"]')!.isSameNode(log0), `step ${step}: remounted the log form`).toBe(true)
      expect(document.querySelector('[data-testid="vk-stub"]')!.isSameNode(vk0), `step ${step}: remounted the voice keyer`).toBe(true)
      expect(document.querySelector('[data-testid="scope-stub"]')!.isSameNode(scope0), `step ${step}: remounted the scope`).toBe(true)
      // Never on the side: the keyer, the log form, a rig strip.
      expect(side()?.querySelector('[data-testid="vk-stub"], [data-testid="log-stub"], [data-pane="receiver"], [data-pane="transmitter"], [data-pane="rigscope"]') ?? null).toBeNull()
    }
    // The sweep must have used the side and crossed the width, or it proved nothing.
    expect(sideSeen, 'the side never showed').toBeGreaterThan(10)
    expect(crossings, 'the window never crossed about 1280 px').toBeGreaterThan(3)
    expect(records.size, 'too few of the random moves changed the arrangement').toBeGreaterThan(15)
    // And the stored side is still a list of the panes it may hold.
    for (const id of api!.layout.leftSide ?? []) expect(PHONE_PANELS.arrange!.leftSide).toContain(id)
    expect(placedColumns(PHONE_PANELS.arrange!, api!.layout.place).a).toContain('voiceKeyer')
  })
})
