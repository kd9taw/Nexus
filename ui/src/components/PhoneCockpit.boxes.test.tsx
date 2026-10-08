// @vitest-environment jsdom
//
// PHONE'S BOXES (any pane in any area, 2026-10-07): up to six entries of the shared list in Phone's
// columns and on its left side, wherever ⊞ Panels ▸ Arrange puts them. Rendered with the REAL panel
// record and the real ⊞ menu: nobody's screen changes until a box is added, "+ Add a box" puts one at a
// column's foot, its picker and its ✕ work through the record, a window that lends no boxes (the hosted
// Remote page) draws none, and THE FIBER-IDENTITY SWEEP: no sequence of adds, picks, hides and moves,
// with tier and window flips between them, remounts the log form, the voice keyer or the scope. The
// box's body is stubbed (CockpitBox.test.tsx renders the real one); jsdom lays nothing out, so widths
// are stubbed as in PhoneCockpit.arrange.test.tsx, whose mocks this file follows.
import type { ReactNode } from 'react'
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen, within } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import type { AppSnapshot } from '../types'
import { BOX_IDS, PHONE_PANELS, boxEntries, panelStorageKey, usePanelLayout, type PanelLayoutApi, type PhonePanelId } from '../features/panelState'
import { arrangeIds, placedColumns, regionGroups, type PaneMove } from '../features/panelPlace'
import { SHARED_PANES } from '../features/sharedPanes'
import type { BoxSource } from './panes/CockpitBox'
import { t } from '../i18n'

vi.mock('../api', () => ({
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

// The header renders what the cockpit gives it to hold — the ⊞ menu, with Arrange in it — and nothing
// of its own (the transmit controls are swept in stop-line.test.tsx).
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: ({ actions }: { actions?: ReactNode }) => <header className="cockpit-header">{actions}</header>,
}))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="vk-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))
vi.mock('./SpotsPanel', () => ({ SpotsPanel: () => <div data-testid="spots-stub" /> }))
vi.mock('./NeededPanel', () => ({ NeededPanel: () => <div data-testid="needed-stub" /> }))
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
afterEach(() => {
  cleanup()
  document.documentElement.style.removeProperty('--vw-eff')
})

function stubWidth(el: Element, w: number) {
  Object.defineProperty(el, 'clientWidth', { configurable: true, get: () => w })
}
async function frame() {
  await act(async () => {
    await new Promise((r) => requestAnimationFrame(() => r(null)))
  })
}
async function windowWidth(px: number) {
  await act(async () => {
    document.documentElement.style.setProperty('--vw-eff', `${px}px`)
    await Promise.resolve()
  })
}

function makeSnap(): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.2, band: '20m', catOk: true, sideband: 'USB', sidebandOverride: null, rigMode: 'USB', transmitting: false,
      txEnabled: true, txAllowed: true, qsoRecording: false, rfPower: null, micGain: null, nrLevel: 0.3, agc: 'fast', nb: true,
      nr: true, notch: null, comp: null, vox: null, filterWidthHz: null, splitTxMhz: null, smeterDb: null, rxLevel: 0,
      phoneSegLo: null, phoneSegHi: null,
    },
  } as unknown as AppSnapshot
}

const SOURCE: BoxSource = { myGrid: 'EN52', theme: 'dark', stations: [], prop: null, needByCall: new Map() }
const BOARDS = {
  spotsBoard: { bandPlan: [], selectedCall: null, onSelect: () => {}, onWork: () => {} },
  neededBoard: { alerts: [], bandPlan: [], selectedCall: null, onQsy: () => {}, onSelect: () => {} },
}

let api: PanelLayoutApi<PhonePanelId> | null = null
/** Phone on the REAL panel record, lent the boxes' source as App lends it on the desktop (`lend`), or
 *  not, as on the hosted Remote page; `feeds` wires the Spots and Needed boards. */
function Live({ lend = true, feeds = false, source = SOURCE }: { lend?: boolean; feeds?: boolean; source?: BoxSource }) {
  const panels = usePanelLayout(PHONE_PANELS)
  api = panels
  return (
    <PhoneCockpit
      snap={makeSnap()}
      theme="dark"
      onWorkSpot={() => {}}
      spots={[]}
      panels={panels}
      boxes={lend ? source : undefined}
      {...(feeds ? BOARDS : {})}
    />
  )
}

const isBox = (id: string) => (BOX_IDS as readonly string[]).includes(id)
const region = () => document.querySelector('.cockpit-panes')!
const cols = () => [...region().querySelectorAll(':scope > .cockpit-col')] as HTMLElement[]
const framesIn = (col: Element) => [...col.querySelectorAll(':scope > .pane-frame')].map((f) => f.getAttribute('data-pane'))
const rendered = () => cols().map(framesIn)
const side = () => document.querySelector('.cockpit-left')
const sideFrames = () => (side() ? framesIn(side()!.querySelector('.cockpit-left-col')!) : [])
const boxFrame = (b: string) => document.querySelector(`.pane-frame[data-pane="${b}"]`) as HTMLElement | null
const bodyOf = (b: string) => boxFrame(b)?.querySelector('[data-testid^="box-body-"]')?.getAttribute('data-testid')
async function tier(width: number) {
  stubWidth(region(), width)
  act(() => fire!())
  await frame()
}
function openMenu() {
  fireEvent.click(document.querySelector('.panels-menu-btn')!)
}
const addTo = (area: 'a' | 'b' | 'log' | 'side') => fireEvent.click(screen.getByRole('button', { name: t(`panels.box.add.${area}.aria` as const) }))
/** What this Phone shows of its own (no native scope; the boards only with `feeds`). */
const OWN = new Set<PhonePanelId>(['bandActivity', 'voiceKeyer', 'receiver', 'transmitter'])
const FIRST = SHARED_PANES.map((e) => e.id)
const paneOf = (entry: string) => SHARED_PANES.find((e) => e.id === entry)!.pane

describe('Phone with boxes', () => {
  it('with no box in the record, the screen is exactly what it was, lent the boxes or not', async () => {
    const shapes: string[] = []
    for (const lend of [false, true]) {
      render(<Live lend={lend} />)
      for (const w of [900, 1200, 1800]) {
        await tier(w)
        shapes.push(`${w}:${JSON.stringify(rendered())}:${region().getAttribute('data-cols')}`)
      }
      expect(document.querySelector('.pane-frame[data-pane^="box"]'), 'a box on a stock screen').toBeNull()
      cleanup()
    }
    expect(shapes.slice(3)).toEqual(shapes.slice(0, 3))
  })

  it('“+ Add a box” in ⊞ Arrange puts one at the foot of that column, showing the first entry not on screen', async () => {
    render(<Live />)
    await tier(1800)
    openMenu()
    addTo('b')
    expect(rendered()).toEqual([['bandActivity', 'voiceKeyer'], ['receiver', 'transmitter', 'box1'], ['log']])
    expect(bodyOf('box1')).toBe(`box-body-${paneOf(FIRST[0])}`)
    addTo('a')
    expect(rendered()[0]).toEqual(['bandActivity', 'voiceKeyer', 'box2'])
    expect(bodyOf('box2')).toBe(`box-body-${paneOf(FIRST[1])}`)
  })

  it('a box’s picker shows another entry, and an entry on screen elsewhere moves here', async () => {
    render(<Live />)
    await tier(1800)
    act(() => api!.addBox!('b'))
    act(() => api!.addBox!('b'))
    const pick = (b: string) => boxFrame(b)!.querySelector('select')!
    fireEvent.change(pick('box1'), { target: { value: 'clock' } })
    expect(bodyOf('box1')).toBe('box-body-clock')
    fireEvent.change(pick('box2'), { target: { value: 'clock' } })
    expect(bodyOf('box2')).toBe('box-body-clock')
    expect(bodyOf('box1'), 'the box that showed it took the other one’s').toBe(`box-body-${paneOf(FIRST[1])}`)
  })

  it('picking the Spots board while Phone’s own Spots pane shows it moves the board into the box', async () => {
    render(<Live feeds />)
    await tier(1800)
    act(() => api!.setPanelState('spots', 'docked'))
    act(() => api!.addBox!('b'))
    expect(document.querySelector('.pane-frame[data-pane="spots"]')).not.toBeNull()
    fireEvent.change(boxFrame('box1')!.querySelector('select')!, { target: { value: 'spotsBoard' } })
    expect(document.querySelector('.pane-frame[data-pane="spots"]'), 'the Spots board is on screen twice').toBeNull()
    expect(bodyOf('box1')).toBe('box-body-spots')
  })

  it('✕ hides a box, and Undo brings it back showing what it showed', async () => {
    render(<Live />)
    await tier(1800)
    act(() => api!.addBox!('a'))
    act(() => api!.setBox!('box1', 'pota'))
    fireEvent.click(screen.getByRole('button', { name: t('pane.hide.aria', { title: 'POTA / SOTA' }) }))
    expect(boxFrame('box1')).toBeNull()
    act(() => api!.undo())
    expect(bodyOf('box1')).toBe('box-body-pota')
  })

  it('a window that lends no boxes (the hosted Remote page) draws none and offers none, whatever the record says', async () => {
    localStorage.setItem(panelStorageKey('phone'), JSON.stringify({ v: 2, state: { box1: 'docked' }, share: {}, boxes: { box1: 'clock' } }))
    render(<Live lend={false} />)
    await tier(1800)
    expect(boxFrame('box1')).toBeNull()
    openMenu()
    expect(screen.queryByRole('button', { name: t('panels.box.add.a.aria') })).toBeNull()
  })

  it('in the stacking flow a list box is capped and scrolls inside itself; at the bounded tiers it fills', async () => {
    render(<Live />)
    act(() => api!.addBox!('a'))
    act(() => api!.setBox!('box1', 'getout'))
    await tier(900)
    expect(region().getAttribute('data-flow')).toBe('stack')
    expect(boxFrame('box1')!.querySelector('.box-body')!.classList.contains('box-body--stacked')).toBe(true)
    await tier(1200)
    expect(boxFrame('box1')!.querySelector('.box-body')!.classList.contains('box-body--stacked')).toBe(false)
  })

  it('a box on the left side stands beside the scope on a wide window, in its column on a narrow one, the record untouched', async () => {
    await windowWidth(1600)
    render(<Live />)
    await tier(1200)
    openMenu()
    addTo('side')
    expect(sideFrames()).toEqual(['box1'])
    const stored = JSON.stringify(api!.layout)
    await windowWidth(1100)
    expect(side()).toBeNull()
    expect(rendered()[0]).toContain('box1')
    expect(JSON.stringify(api!.layout)).toBe(stored)
    await windowWidth(1600)
    expect(sideFrames()).toEqual(['box1'])
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
  // App lends them (RailLink `folded`) when the rail is on and the window is below lg: Phone stands them at the
  // foot of column 1, where its own boxes stand until placed, at every tier. Their picker is the rail's, and they
  // carry no ✕ (the rail's own switch takes them away).
  it('stand at the foot of the leading column at every tier, with no ✕, and their picker is the rail’s', async () => {
    const pick = vi.fn()
    const folded: BoxSource = {
      ...SOURCE,
      rail: {
        shows: ['clock', 'spacewx'],
        take: vi.fn(),
        folded: { boxes: [{ slot: 'rail1', pane: 'clock', entry: 'clock' }, { slot: 'rail2', pane: 'spacewx', entry: 'spacewx' }], pick },
      },
    }
    render(<Live source={folded} />)
    await frame()
    for (const w of [700, 1300, 1800]) {
      await tier(w)
      expect(framesIn(cols()[0]).slice(-2), `at ${w} px`).toEqual(['rail1', 'rail2'])
    }
    expect(within(boxFrame('rail1')!).queryByRole('button', { name: /^Hide/ })).toBeNull()
    expect(bodyOf('rail2')).toBe(`box-body-${paneOf('spacewx')}`)
    fireEvent.change(boxFrame('rail2')!.querySelector('select.pane-pick')!, { target: { value: 'getout' } })
    expect(pick).toHaveBeenCalledWith('rail2', 'getout')
  })
})

describe('THE FIBER-IDENTITY SWEEP, with boxes: no add, pick, hide or move remounts the log form, the keyer or the scope', () => {
  it('80 random box and pane acts, with window and tier flips between them', async () => {
    await windowWidth(1600)
    render(<Live />)
    await tier(1800)
    const log0 = document.querySelector('[data-testid="log-stub"]')!
    const vk0 = document.querySelector('[data-testid="vk-stub"]')!
    const scope0 = document.querySelector('[data-testid="scope-stub"]')!
    const ids = arrangeIds(PHONE_PANELS.arrange!)
    const moves: PaneMove[] = ['up', 'down', 'left', 'right']
    const areas = ['a', 'b', 'log', 'side'] as const
    const next = rng(20261007)
    const pick = <T,>(xs: readonly T[]) => xs[Math.floor(next() * xs.length)]
    let wide = true
    const seen = { adds: 0, picks: 0, hides: 0, moves: 0, sided: 0, boxesShown: 0, tiers: new Set<number>() }
    for (let step = 0; step < 80; step++) {
      const entries = boxEntries(PHONE_PANELS, api!.layout)
      const shownBoxes = Object.keys(entries) as PhonePanelId[]
      const isShown = (id: PhonePanelId) => (isBox(id) ? entries[id] != null : OWN.has(id))
      const roll = next()
      const before = JSON.stringify(api!.layout)
      if (roll < 0.3) act(() => api!.addBox!(pick(areas)))
      else if (roll < 0.45 && shownBoxes.length > 0) act(() => api!.setBox!(pick(shownBoxes), pick(FIRST)))
      else if (roll < 0.55 && shownBoxes.length > 0) act(() => api!.setPanelState(pick(shownBoxes), 'removed'))
      else act(() => api!.movePane!(pick(ids), pick(moves), isShown, wide))
      if (JSON.stringify(api!.layout) !== before) {
        if (roll < 0.3) seen.adds++
        else if (roll < 0.45) seen.picks++
        else if (roll < 0.55) seen.hides++
        else seen.moves++
      }
      if (step % 5 === 2) {
        const w = pick([1100, 1279, 1280, 1600, 2560])
        wide = w >= 1280
        await windowWidth(w)
      }
      if (step % 7 === 3) await tier(pick([900, 1200, 1800]))
      // Where everything stands: the side's panes and boxes on it (wide), everything else in its column.
      const now = boxEntries(PHONE_PANELS, api!.layout)
      const shownNow = (id: PhonePanelId) => (isBox(id) ? now[id] != null : OWN.has(id))
      const onSide = wide ? (api!.layout.leftSide ?? []).filter(shownNow) : []
      if (onSide.length > 0) seen.sided++
      seen.boxesShown += Object.keys(now).length
      expect(sideFrames(), `step ${step}`).toEqual(onSide)
      const tracks = Number(region().getAttribute('data-cols')) as 1 | 2 | 3
      seen.tiers.add(tracks)
      const want = regionGroups(PHONE_PANELS.arrange!, api!.layout.place, tracks, (x) => shownNow(x) && !onSide.includes(x))
        .filter((g) => g.col === 'log' || tracks === 3 || g.ids.length > 0)
        .map((g) => (g.col === 'log' ? [...g.ids, 'log'] : g.ids))
      expect(rendered(), `step ${step} — the region is not the arrangement`).toEqual(want)
      // Every box on screen shows what the record says.
      for (const [b, e] of Object.entries(now)) expect(bodyOf(b), `step ${step}: ${b}`).toBe(`box-body-${paneOf(e!)}`)
      // A bounded tier never holds a track with nothing in it.
      if (tracks > 1) for (const c of cols()) expect(c.children.length, `step ${step}: an empty track`).toBeGreaterThan(0)
      expect(document.querySelector('[data-testid="log-stub"]')!.isSameNode(log0), `step ${step}: remounted the log form`).toBe(true)
      expect(document.querySelector('[data-testid="vk-stub"]')!.isSameNode(vk0), `step ${step}: remounted the voice keyer`).toBe(true)
      expect(document.querySelector('[data-testid="scope-stub"]')!.isSameNode(scope0), `step ${step}: remounted the scope`).toBe(true)
      expect(cols()[0].contains(vk0), `step ${step}: the voice keyer left the leading column`).toBe(true)
    }
    // The sweep must have done each kind of thing, or it proved nothing.
    expect(seen.adds, 'too few adds').toBeGreaterThan(8)
    expect(seen.picks, 'too few picks').toBeGreaterThan(3)
    expect(seen.hides, 'too few hides').toBeGreaterThan(3)
    expect(seen.moves, 'too few moves').toBeGreaterThan(10)
    expect(seen.sided, 'nothing ever stood on the side').toBeGreaterThan(5)
    expect(seen.boxesShown, 'the boxes were rarely on screen').toBeGreaterThan(100)
    expect([...seen.tiers].sort()).toEqual([1, 2, 3])
    expect(placedColumns(PHONE_PANELS.arrange!, api!.layout.place).a).toContain('voiceKeyer')
  })
})
