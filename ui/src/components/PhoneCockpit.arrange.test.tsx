// @vitest-environment jsdom
//
// PHONE, ARRANGED (layout L3): the pane region renders the panel record's placement — each pane in its
// column, in its order — at every tier, and THE FIBER-IDENTITY SWEEP: under any sequence of moves and
// tier flips the log form and the voice keyer are never remounted (a remount of the keyer stops its
// over and discards its recording; of the log form, loses a half-typed contact). jsdom lays nothing
// out: widths are stubbed as in PhoneCockpit.structure.test.tsx, whose mocks this file shares.
import { useRef } from 'react'
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import type { AppSnapshot } from '../types'
import { PHONE_PANELS, panelStorageKey, usePanelLayout, type PanelLayoutApi, type PhonePanelId } from '../features/panelState'
import { arrangeIds, regionGroups, type PaneMove } from '../features/panelPlace'

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
    for (let step = 0; step < 50; step++) {
      const id = ids[Math.floor(next() * ids.length)]
      const move = moves[Math.floor(next() * moves.length)]
      const before = JSON.stringify(api!.layout.place ?? null)
      act(() => api!.movePane!(id, move, shown))
      if (JSON.stringify(api!.layout.place ?? null) !== before) applied++
      if (step % 7 === 3) await tier(next() < 0.5 ? 1200 : 1800)
      const tracks = Number(region().getAttribute('data-cols')) as 1 | 2 | 3
      expect(rendered(), `step ${step}: ${id} ${move} — the region is not the placement`).toEqual(expected(tracks))
      expect(document.querySelector('[data-testid="log-stub"]')!.isSameNode(log0), `step ${step}: ${id} ${move} remounted the log form`).toBe(true)
      expect(document.querySelector('[data-testid="vk-stub"]')!.isSameNode(vk0), `step ${step}: ${id} ${move} remounted the voice keyer`).toBe(true)
      expect(cols()[0].contains(vk0), `step ${step}: the voice keyer left the leading column`).toBe(true)
    }
    // The sweep must have MOVED things, or it proved nothing.
    expect(applied, 'too few of the random moves changed the placement').toBeGreaterThan(15)
  })
})
