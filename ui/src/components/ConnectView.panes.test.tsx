// @vitest-environment jsdom
//
// CLOSE AND RESIZE ON THE CONNECT SURFACE (operator-approved 2026-09-13). A tester comparing
// Nexus with a wall-display app: Connect shows too much at once — close the panels you don't use,
// add them back when you choose, and make the map bigger.
//
// ⚠️ THE REAL ConnectView AND THE REAL MapView ARE MOUNTED HERE. Only the backend is stubbed.
// A stubbed frame would prove that a prop reaches it and nothing about what is actually on
// screen after a close and a restore — the seam this codebase has shipped a broken feature
// through before. Every assertion counts rendered frames, rails and separators.
//
// jsdom does not lay out, so the geometry half (no dead gap, no overflow, today's default
// rectangles unchanged) is measured in a real browser, not here. What IS here: structure,
// persistence, the menu round-trip, the pop-out's record, the keyboard path and the clamp.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { cleanup, render, fireEvent, screen, act, within } from '@testing-library/react'

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  getBandOutlook: vi.fn(async () => ({ bands: [], asOf: 0 })),
  getGettingOut: vi.fn(async () => null),
  getPathOutlook: vi.fn(async () => null),
  getSpaceWxScales: vi.fn(async () => ({ scales: null, alerts: [] })),
  getKc2gMuf: vi.fn(async () => []),
  getXrayNow: vi.fn(async () => null),
  getDxpedWindows: vi.fn(async () => []),
  getAurora: vi.fn(async () => null),
  getDeclination: vi.fn(async () => null),
  getPca: vi.fn(async () => null),
  getSatellites: vi.fn(async () => null),
  getLogStats: vi.fn(async () => null),
  getOtaMapSpots: vi.fn(async () => []),
  getContests: vi.fn(async () => []),
}))
import { ConnectView } from './ConnectView'
import { DEFAULT_SLOTS, SLOT_IDS, type SlotId } from '../features/connectConfig'
import { MAP_MIN, RAIL_MAX, RAIL_MIN } from '../features/connectRails'
import { CONNECT_PRESET_IDS, CONNECT_PRESETS, type ConnectPresetId } from '../features/connectPresets'
import { RemoteCollectionsContext, type RemoteCollections } from '../remote-web/collections'
import { StationDataContext } from '../stationAccess'

const RECORD = 'nexus.panels.connect.main'
const POPOUT_RECORD = 'nexus.panels.connect.connect'
const WIDTHS = 'nexus.connect.railWidths'
/** The 3-D globe's layer picks on this surface (features/globeLayers). */
const GLOBE = 'nexus.connect.globe3d.layers'

const props = {
  myGrid: 'EN52',
  theme: 'dark' as const,
  stations: [],
  prop: null,
  selectedCall: null,
  onSelectCall: () => {},
  needByCall: new Map(),
  needAlerts: [],
  amp: null,
}

async function mount() {
  let r!: ReturnType<typeof render>
  await act(async () => {
    r = render(<ConnectView {...props} />)
  })
  return r
}

const slotsOn = (root: ParentNode) =>
  [...root.querySelectorAll('.pane-frame')].map((f) => f.getAttribute('data-slot') as SlotId)
const grid = (c: HTMLElement) => c.querySelector('.connect') as HTMLElement
const rail = (c: HTMLElement, side: 'left' | 'right') =>
  c.querySelector(`.connect-rail[data-side="${side}"]`) as HTMLElement | null
const record = (key = RECORD) => JSON.parse(localStorage.getItem(key) ?? 'null')

/** Close one slot the way the operator does: the ✕ in that pane's own head. */
function closeSlot(c: HTMLElement, slot: SlotId) {
  const frame = c.querySelector(`.pane-frame[data-slot="${slot}"]`) as HTMLElement
  expect(frame, `slot ${slot} should be on screen before it is closed`).not.toBeNull()
  fireEvent.click(within(frame).getByRole('button', { name: /^Hide / }))
}

/** Controls OUTSIDE every pane frame, by accessible name — the screen that remains. */
function controlsOutsidePanes(c: HTMLElement): string[] {
  return [...c.querySelectorAll('button, select, input')]
    .filter((el) => !el.closest('.pane-frame') && !el.closest('.panels-menu'))
    .map((el) => el.getAttribute('aria-label') ?? el.getAttribute('title') ?? el.textContent ?? '')
    .sort()
}

// jsdom returns a 0×0 rect for everything, which the clamp (correctly) treats as "cannot
// measure — do not clamp". The resize tests give the grid and its rails a real box.
function fakeBoxes(connectW: number, railW = 300) {
  const orig = HTMLElement.prototype.getBoundingClientRect
  HTMLElement.prototype.getBoundingClientRect = function (this: HTMLElement) {
    if (this.classList.contains('connect')) return { x: 0, y: 0, left: 0, top: 0, width: connectW, height: 700, right: connectW, bottom: 700, toJSON() {} } as DOMRect
    if (this.classList.contains('connect-rail')) {
      const left = this.getAttribute('data-side') === 'left' ? 0 : connectW - railW
      return { x: left, y: 0, left, top: 0, width: railW, height: 600, right: left + railW, bottom: 600, toJSON() {} } as DOMRect
    }
    return orig.call(this)
  }
  return () => {
    HTMLElement.prototype.getBoundingClientRect = orig
  }
}

beforeEach(() => {
  localStorage.clear()
  window.history.replaceState(null, '', '/')
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(() => {
  cleanup()
  window.history.replaceState(null, '', '/')
})

describe('unconfigured, Connect is exactly today’s layout', () => {
  it('renders every slot in both rails and the strip, stores nothing, overrides no size', async () => {
    const { container } = await mount()
    expect(slotsOn(container).sort()).toEqual([...SLOT_IDS].sort())
    expect(grid(container).getAttribute('data-rails')).toBe('both')
    expect(slotsOn(rail(container, 'left')!)).toEqual(['left1', 'left2'])
    expect(slotsOn(rail(container, 'right')!)).toEqual(['right1', 'right2'])
    expect(slotsOn(container.querySelector('.connect-strip')!)).toEqual(['bottom1', 'bottom2', 'bottom3'])
    expect(grid(container).style.getPropertyValue('--cn-rail-l')).toBe('')
    expect(grid(container).style.getPropertyValue('--cn-rail-r')).toBe('')
    expect(localStorage.getItem(RECORD), 'mounting must not write a layout').toBeNull()
    expect(localStorage.getItem(WIDTHS), 'mounting must not write a width').toBeNull()
    expect(screen.getByRole('button', { name: '⊞ Panels' })).toBeTruthy()
  })
})

describe('closing and restoring panes', () => {
  it('a pane closed from its own frame is unmounted and its rail sibling takes the column', async () => {
    const { container } = await mount()
    closeSlot(container, 'left1')
    expect(slotsOn(container)).not.toContain('left1')
    expect(slotsOn(rail(container, 'left')!)).toEqual(['left2'])
    expect(grid(container).getAttribute('data-rails')).toBe('both')
    // A split handle between one pane and nothing would be a control that does nothing.
    expect(screen.queryByRole('separator', { name: /split between the left panels/i })).toBeNull()
    expect(screen.getByRole('separator', { name: /split between the right panels/i })).toBeTruthy()
    expect(record().state).toEqual({ left1: 'removed' })
    expect(screen.getByRole('button', { name: '⊞ Panels · 1 hidden' })).toBeTruthy()
  })

  it('the ⊞ Panels menu lists what is hidden, and ticking it brings the same pane back', async () => {
    const { container } = await mount()
    const title = container.querySelector('.pane-frame[data-slot="left1"] .pane-title')!.textContent!
    closeSlot(container, 'left1')
    fireEvent.click(screen.getByRole('button', { name: /⊞ Panels/ }))
    const box = screen.getByRole('checkbox', { name: `${title} · left, top` }) as HTMLInputElement
    expect(box.checked).toBe(false)
    fireEvent.click(box)
    expect(slotsOn(container).sort()).toEqual([...SLOT_IDS].sort())
    expect(container.querySelector('.pane-frame[data-slot="left1"] .pane-title')!.textContent).toBe(title)
  })

  it('closing every pane in a rail collapses the rail; closing everything leaves the map', async () => {
    const { container } = await mount()
    closeSlot(container, 'left1')
    closeSlot(container, 'left2')
    expect(rail(container, 'left')).toBeNull()
    expect(grid(container).getAttribute('data-rails')).toBe('right')
    expect(screen.queryByRole('separator', { name: /left panel column/i })).toBeNull()

    closeSlot(container, 'right1')
    closeSlot(container, 'right2')
    expect(rail(container, 'right')).toBeNull()
    expect(grid(container).getAttribute('data-rails')).toBe('none')

    for (const s of ['bottom1', 'bottom2', 'bottom3'] as const) closeSlot(container, s)
    expect(container.querySelector('.connect-strip'), 'an empty strip would be a dead row').toBeNull()
    expect(slotsOn(container)).toEqual([])
    expect(container.querySelector('.connect-map .map-view')).not.toBeNull()
    expect(screen.getByRole('button', { name: '⊞ Panels · 7 hidden' })).toBeTruthy()
  })

  it('Reset layout puts every pane back', async () => {
    const { container } = await mount()
    for (const s of SLOT_IDS) closeSlot(container, s)
    fireEvent.click(screen.getByRole('button', { name: /⊞ Panels/ }))
    fireEvent.click(screen.getByRole('button', { name: 'Reset layout' }))
    expect(slotsOn(container).sort()).toEqual([...SLOT_IDS].sort())
    expect(grid(container).getAttribute('data-rails')).toBe('both')
    expect(record().state).toEqual({})
  })

  it('Reset layout returns Connect to the out-of-box state — default pane in every slot too', async () => {
    // Operator ruling 2026-09-13: Reset layout is "exactly as installed" — which pane sits in
    // each slot, every pane open, default rail widths and splits.
    const restore = fakeBoxes(1280)
    try {
      const { container } = await mount()
      const paneIn = (s: SlotId) => container.querySelector(`.pane-frame[data-slot="${s}"]`)?.getAttribute('data-pane')
      const defaults = Object.fromEntries(SLOT_IDS.map((s) => [s, paneIn(s)]))
      expect(defaults.left1, 'control: the default layout is on screen').toBe(DEFAULT_SLOTS.left1)

      // 1. swap a slot's pane from its picker
      const picker = container.querySelector('.pane-frame[data-slot="left1"] select') as HTMLSelectElement
      fireEvent.change(picker, { target: { value: 'greyline' } })
      expect(paneIn('left1')).toBe('greyline')
      // 2. close a pane
      closeSlot(container, 'right2')
      // 3. drag a rail width
      const sep = screen.getByRole('separator', { name: 'Left panel column width' })
      fireEvent.pointerDown(sep, { button: 0, clientX: 300, pointerId: 1 })
      fireEvent.pointerMove(window, { clientX: 400, pointerId: 1 })
      fireEvent.pointerUp(window, { clientX: 400, pointerId: 1 })
      expect(grid(container).style.getPropertyValue('--cn-rail-l'), 'control: the drag took').toBe('400px')
      // …and a split
      fireEvent.keyDown(screen.getByRole('separator', { name: 'Split between the left panels' }), { key: 'ArrowDown' })

      fireEvent.click(screen.getByRole('button', { name: /⊞ Panels/ }))
      fireEvent.click(screen.getByRole('button', { name: 'Reset layout' }))

      for (const s of SLOT_IDS) expect(paneIn(s), `slot ${s} after Reset`).toBe(DEFAULT_SLOTS[s])
      expect(grid(container).style.getPropertyValue('--cn-rail-l')).toBe('')
      expect(JSON.parse(localStorage.getItem(WIDTHS) ?? '{}')).toEqual({})
      expect(record().state).toEqual({})
      expect(record().share).toEqual({})
      expect(JSON.parse(localStorage.getItem('nexus.connect.config')!).slots).toEqual(DEFAULT_SLOTS)

      // Undo after Reset puts back what Reset took: the swapped pane and the closed pane.
      // Width drags are not undo steps (operator ruling), so the width stays at its default.
      fireEvent.click(screen.getByRole('button', { name: 'Undo last change' }))
      expect(paneIn('left1')).toBe('greyline')
      expect(paneIn('right2'), 'the pane closed before Reset is closed again').toBeUndefined()
      expect(grid(container).style.getPropertyValue('--cn-rail-l')).toBe('')
      expect(JSON.parse(localStorage.getItem('nexus.connect.config')!).slots.left1).toBe('greyline')
    } finally {
      restore()
    }
  })

  it('a change made after Reset is what Undo reverts — never the slots from before Reset', async () => {
    const { container } = await mount()
    const paneIn = (s: SlotId) => container.querySelector(`.pane-frame[data-slot="${s}"]`)?.getAttribute('data-pane')
    fireEvent.change(container.querySelector('.pane-frame[data-slot="left1"] select')!, { target: { value: 'greyline' } })
    fireEvent.click(screen.getByRole('button', { name: /⊞ Panels/ }))
    fireEvent.click(screen.getByRole('button', { name: 'Reset layout' }))
    closeSlot(container, 'bottom1')
    fireEvent.click(screen.getByRole('button', { name: 'Undo last change' }))
    expect(paneIn('bottom1'), 'the close is undone').toBe(DEFAULT_SLOTS.bottom1)
    expect(paneIn('left1'), 'Reset’s slot snapshot is gone once another change lands').toBe(DEFAULT_SLOTS.left1)
  })

  it('a closed pane stays closed across a remount', async () => {
    const first = await mount()
    closeSlot(first.container, 'right2')
    first.unmount()
    const { container } = await mount()
    expect(slotsOn(container)).not.toContain('right2')
  })

  it('hiding every pane leaves every control outside the panes on screen', async () => {
    // THE STOP LINE, Connect's half: Connect renders no transmit control at all (its panes'
    // ▶ Work QSYs and opens a cockpit; the TopBar's TX cluster is outside this view), so there
    // is no stop control here to lose. What a hide may never do is reach past its own pane —
    // this computes that against the rendered screen, singly and with everything closed.
    const { container } = await mount()
    const before = controlsOutsidePanes(container)
    expect(before.length, 'control: the map toolbar and the header are on screen').toBeGreaterThan(3)
    closeSlot(container, 'left1')
    expect(controlsOutsidePanes(container)).toEqual(before)
    for (const s of SLOT_IDS.filter((s) => s !== 'left1')) closeSlot(container, s)
    expect(controlsOutsidePanes(container)).toEqual(before)
  })
})

describe('the pop-out keeps its own record', () => {
  it('inherits the main window’s layout on first open, then writes only its own', async () => {
    localStorage.setItem(RECORD, JSON.stringify({ v: 1, state: { left1: 'removed' }, share: {} }))
    window.history.replaceState(null, '', '/?panel=connect')
    const { container } = await mount()
    expect(slotsOn(container), 'first open inherits the main window’s hidden pane').not.toContain('left1')
    closeSlot(container, 'right1')
    expect(record(POPOUT_RECORD).state).toEqual({ left1: 'removed', right1: 'removed' })
    expect(record(RECORD).state, 'the main window’s layout is untouched').toEqual({ left1: 'removed' })
  })
})

describe('resizing the rails', () => {
  it('each rail width separator is focusable, labelled and keyboard-resizable, and double-click resets it', async () => {
    const restore = fakeBoxes(1280)
    try {
      const { container } = await mount()
      const sep = screen.getByRole('separator', { name: 'Left panel column width' })
      expect(sep.tabIndex).toBe(0)
      expect(sep.getAttribute('aria-orientation')).toBe('vertical')
      expect(sep.getAttribute('aria-valuenow')).toBe('300')

      fireEvent.keyDown(sep, { key: 'ArrowRight' })
      expect(grid(container).style.getPropertyValue('--cn-rail-l')).toBe('316px')
      // `last` records which rail the operator set most recently: it keeps its width when a
      // smaller window cannot hold both preferences (features/connectRails fitRails).
      expect(JSON.parse(localStorage.getItem(WIDTHS)!)).toEqual({ left: 316, last: 'left' })
      expect(sep.getAttribute('aria-valuenow')).toBe('316')

      // End = as wide as this box allows: the room minus the right rail's 300.
      fireEvent.keyDown(sep, { key: 'End' })
      expect(grid(container).style.getPropertyValue('--cn-rail-l')).toBe(`${Math.min(RAIL_MAX, 1280 - MAP_MIN - 300)}px`)
      fireEvent.keyDown(sep, { key: 'Home' })
      expect(grid(container).style.getPropertyValue('--cn-rail-l')).toBe(`${RAIL_MIN}px`)

      fireEvent.doubleClick(sep)
      expect(grid(container).style.getPropertyValue('--cn-rail-l')).toBe('')
      expect(JSON.parse(localStorage.getItem(WIDTHS) ?? '{}')).toEqual({})

      // The right rail's handle sits on its LEFT edge, so ArrowLeft is what widens it.
      const right = screen.getByRole('separator', { name: 'Right panel column width' })
      fireEvent.keyDown(right, { key: 'ArrowLeft' })
      expect(grid(container).style.getPropertyValue('--cn-rail-r')).toBe('316px')
    } finally {
      restore()
    }
  })

  it('a saved oversized width is clamped on load against the current box, and the preference is kept', async () => {
    localStorage.setItem(WIDTHS, JSON.stringify({ left: 5000 }))
    const restore = fakeBoxes(1024)
    try {
      const { container } = await mount()
      // jsdom carries no sheet, so padding and gaps read as 0: the room is 1024 − MAP_MIN. The
      // stored side is the one the operator set, so it keeps what the floored other rail leaves.
      expect(grid(container).style.getPropertyValue('--cn-rail-l')).toBe(`${1024 - MAP_MIN - RAIL_MIN}px`)
      expect(grid(container).style.getPropertyValue('--cn-rail-r')).toBe(`${RAIL_MIN}px`)
      expect(JSON.parse(localStorage.getItem(WIDTHS)!), 'a re-clamp must not overwrite the choice').toEqual({ left: 5000 })
    } finally {
      restore()
    }
  })

  it('with BOTH widths stored and oversized, a keyboard step moves only its own rail and never overruns the map', async () => {
    // D1 (found in a real browser, 2026-09-13): 600/500 fitted to 368/307 on load, then one
    // ArrowRight on the left rail put the right rail back to its raw stored 500 — map 86.9 px,
    // ten map controls unreachable — and left aria-valuenow above aria-valuemax.
    localStorage.setItem(WIDTHS, JSON.stringify({ left: 600, right: 500 }))
    // jsdom carries no sheet: padding and gaps read 0 and the tier default reads the hook's 300.
    // A FRACTIONAL box, as a real one is (1024×768 measures 674.9 of room): a whole-pixel room
    // hides the round-down rule that keeps a fitted pair from overrunning it.
    const W = 955.5
    const JSDOM_DEFAULT = 300
    const restore = fakeBoxes(W)
    try {
      const { container } = await mount()
      const px = (v: string) => {
        const s = grid(container).style.getPropertyValue(v)
        return s ? parseFloat(s) : JSDOM_DEFAULT // unset ⇒ the rail renders the tier default
      }
      const room = W - MAP_MIN
      const assertFits = (when: string) => {
        const l = px('--cn-rail-l')
        const r = px('--cn-rail-r')
        expect(l + r, `${when}: rails ${l}+${r} overrun the room ${room}`).toBeLessThanOrEqual(room)
        for (const sep of screen.getAllByRole('separator', { name: /panel column width/ })) {
          const now = Number(sep.getAttribute('aria-valuenow'))
          expect(now, `${when}: ${sep.getAttribute('aria-label')} valuenow`).toBeGreaterThanOrEqual(Number(sep.getAttribute('aria-valuemin')))
          expect(now, `${when}: ${sep.getAttribute('aria-label')} valuenow`).toBeLessThanOrEqual(Number(sep.getAttribute('aria-valuemax')))
        }
        return { l, r }
      }
      // The exact load numbers are fitRails' business (connectRails.test.ts); what is on screen
      // here is the pair fitting, and then what one step does to it.
      const loaded = assertFits('load')
      expect(loaded.l, 'control: the stored pair really was squeezed on load').toBeLessThan(600)
      expect(loaded.r, 'control: the stored pair really was squeezed on load').toBeLessThan(500)

      const left = screen.getByRole('separator', { name: 'Left panel column width' })
      fireEvent.keyDown(left, { key: 'ArrowRight' })
      expect(assertFits('ArrowRight'), 'no room to grow: nothing moves, least of all the right rail').toEqual(loaded)

      fireEvent.keyDown(left, { key: 'ArrowLeft' })
      expect(assertFits('ArrowLeft')).toEqual({ l: loaded.l - 16, r: loaded.r })
      expect(JSON.parse(localStorage.getItem(WIDTHS)!).right, 'the right rail keeps its preference').toBe(500)

      fireEvent.doubleClick(left)
      assertFits('double-click reset')
      expect(px('--cn-rail-r'), 'resetting one rail leaves the other where it is').toBe(loaded.r)
    } finally {
      restore()
    }
  })

  it('re-fits when the viewport TIER flips, not only when the grid box resizes', async () => {
    // A window resize fires the grid's ResizeObserver BEFORE useViewport's next frame moves
    // data-viewport, so that fit reads the OLD tier's --cn-rail default. When the tier flip then
    // changes the default without changing the box, nothing re-fits: widening sm → md here grows
    // an unsized rail 248 → 300 beside a stored 450 in a 748 room — 2 px past the map's floor.
    // jsdom resolves a sheet's custom property through [data-viewport] (probed), so the sheet the
    // tier default lives in is modelled exactly.
    const sheet = document.createElement('style')
    sheet.textContent = ".connect { --cn-rail: 300px; } [data-viewport='sm'] .connect { --cn-rail: 248px; }"
    document.head.appendChild(sheet)
    const html = document.documentElement
    const vpBefore = html.getAttribute('data-viewport')
    html.setAttribute('data-viewport', 'sm')
    localStorage.setItem(WIDTHS, JSON.stringify({ right: 450, last: 'right' }))
    const W = 1028.5 // jsdom: room = W − MAP_MIN = 748.5
    const restore = fakeBoxes(W)
    try {
      const { container } = await mount()
      const shownWidth = (v: string, tierDefault: number) => {
        const s = grid(container).style.getPropertyValue(v)
        return s ? parseFloat(s) : tierDefault
      }
      const room = W - MAP_MIN
      // control: at sm the pair fits with the left rail on the stylesheet default
      expect(grid(container).style.getPropertyValue('--cn-rail-l')).toBe('')
      expect(shownWidth('--cn-rail-l', 248) + shownWidth('--cn-rail-r', 248)).toBeLessThanOrEqual(room)

      await act(async () => {
        html.setAttribute('data-viewport', 'md')
        // one MutationObserver turn + a React commit
        await new Promise((r) => setTimeout(r, 0))
      })
      const l = shownWidth('--cn-rail-l', 300)
      const r = shownWidth('--cn-rail-r', 300)
      expect(l + r, `after sm → md the rails render ${l}+${r} in a ${room} room — the map is below its floor`).toBeLessThanOrEqual(room)
      expect(r, 'the stored rail was set last, so it keeps its width').toBe(450)
    } finally {
      restore()
      sheet.remove()
      if (vpBefore == null) html.removeAttribute('data-viewport')
      else html.setAttribute('data-viewport', vpBefore)
    }
  })

  it('the rail split separator moves the seam with the arrow keys and double-click puts it back', async () => {
    const { container } = await mount()
    const seam = screen.getByRole('separator', { name: 'Split between the left panels' })
    expect(seam.tabIndex).toBe(0)
    expect(seam.getAttribute('aria-orientation')).toBe('horizontal')
    expect(seam.getAttribute('aria-valuenow')).toBe('50')

    fireEvent.keyDown(seam, { key: 'ArrowDown' })
    const share = record().share
    expect(share.left1).toBeCloseTo(1.1)
    expect(share.left2).toBeCloseTo(0.9)
    const top = container.querySelector('.pane-frame[data-slot="left1"]') as HTMLElement
    expect(parseFloat(top.style.getPropertyValue('--connect-share'))).toBeCloseTo(1.1)
    expect(seam.getAttribute('aria-valuenow')).toBe('55')

    fireEvent.doubleClick(seam)
    expect(record().share.left1).toBe(1)
    expect(record().share.left2).toBe(1)
  })
})

// LAYOUT PRESETS (the UI redesign, 2026-09-26): Map first · List first · Dashboard, in the ⊞ Panels
// menu. Additive — the approved default stays the default and nothing changes until a tap — and
// read back from the records they write, so a moved or resized pane reads Custom and nothing
// snaps back. The preset table itself is guarded in features/connectPresets.test.ts.
describe('layout presets — ⊞ Panels ▸ Layout', () => {
  const LABEL: Record<ConnectPresetId, string> = { mapFirst: 'Map first', listFirst: 'List first', dashboard: 'Dashboard', frame: 'Frame' }
  const CONFIG = 'nexus.connect.config'
  const layouts = () => screen.getByRole('group', { name: 'Layout' })
  const openMenu = () => {
    if (!screen.queryByRole('group', { name: 'Layout' })) fireEvent.click(screen.getByRole('button', { name: /⊞ Panels/ }))
  }
  const chip = (label: string) => {
    openMenu()
    return within(layouts()).getByRole('button', { name: label })
  }
  const pick = (label: string) => fireEvent.click(chip(label))
  /** What the picker says is on screen. */
  const layoutNow = () => {
    openMenu()
    return layouts().querySelector('.connect-layout-now')?.textContent
  }
  const paneIn = (c: HTMLElement, s: SlotId) => c.querySelector(`.pane-frame[data-slot="${s}"]`)?.getAttribute('data-pane')
  const stored = (key: string) => JSON.parse(localStorage.getItem(key) ?? 'null')

  it('nothing picked: the picker reads Standard, the approved default is on screen, and nothing is written', async () => {
    const { container } = await mount()
    expect(layoutNow()).toBe('Standard')
    for (const id of CONNECT_PRESET_IDS) expect(chip(LABEL[id]).getAttribute('aria-pressed'), id).toBe('false')
    for (const s of SLOT_IDS) expect(paneIn(container, s), s).toBe(DEFAULT_SLOTS[s])
    expect(localStorage.getItem(CONFIG), 'opening the menu must not write a layout').toBeNull()
    expect(localStorage.getItem(RECORD)).toBeNull()
    expect(localStorage.getItem(WIDTHS)).toBeNull()
  })

  for (const id of CONNECT_PRESET_IDS) {
    it(`${LABEL[id]} applies on a tap — its slots, closed panes and widths — reads back as ${LABEL[id]}, and survives a remount`, async () => {
      const restore = fakeBoxes(1920)
      try {
        // The operator's own map setup for the intent in use: a Flat map coloured by signal — both
        // unlike the POTA/SOTA defaults, so an overwrite would show.
        localStorage.setItem('nexus.connect.intent', 'pota')
        localStorage.setItem('nexus.connect.intents', JSON.stringify({ pota: { map: 'world', colorBy: 'snr' } }))
        // …and the 3-D globe's own layer picks, which a layout could reach the same way.
        const globe = JSON.stringify({ spots: false, aurora: true, sats: false })
        localStorage.setItem(GLOBE, globe)
        const { container } = await mount()
        // Taken AFTER mount: MapView writes its layer table into the record when it mounts.
        const intents = localStorage.getItem('nexus.connect.intents')
        expect(JSON.parse(intents!).pota.map, 'control: the setup survived the mount').toBe('world')
        pick(LABEL[id])
        const p = CONNECT_PRESETS[id]
        const shown = SLOT_IDS.filter((s) => !p.hidden.includes(s))
        expect(slotsOn(container).sort()).toEqual([...shown].sort())
        for (const s of shown) expect(paneIn(container, s), s).toBe(p.slots[s])
        expect(stored(CONFIG).slots).toEqual(p.slots)
        expect(stored(RECORD).state).toEqual(Object.fromEntries(p.hidden.map((s) => [s, 'removed'])))
        expect(stored(RECORD).share).toEqual({})
        expect(stored(WIDTHS)).toEqual(p.rails)
        expect(grid(container).style.getPropertyValue('--cn-rail-l')).toBe(`${p.rails.left}px`)
        expect(grid(container).style.getPropertyValue('--cn-rail-r')).toBe(`${p.rails.right}px`)
        expect(layoutNow()).toBe(LABEL[id])
        expect(chip(LABEL[id]).getAttribute('aria-pressed')).toBe('true')
        // The map's own choices are not the layout's to change — except the layers a layout turns ON
        // (Frame: the satellites, the operator's pick), and nothing else about either map.
        expect(localStorage.getItem('nexus.connect.intent')).toBe('pota')
        const on = (p as { mapLayers?: readonly string[] }).mapLayers ?? []
        if (on.length === 0) {
          expect(localStorage.getItem('nexus.connect.intents'), 'the 2-D map’s record').toBe(intents)
          expect(localStorage.getItem(GLOBE), 'the 3-D globe’s record').toBe(globe)
        } else {
          const want = JSON.parse(intents!)
          for (const k of on) want.pota.layers[k] = { ...want.pota.layers[k], visible: true }
          expect(stored('nexus.connect.intents'), 'the 2-D map: only the layout’s layers turned on').toEqual(want)
          const now = stored(GLOBE)
          expect([now.spots, now.aurora], 'the 3-D globe: the operator’s own picks kept').toEqual([false, true])
          for (const k of on) expect(now[k], `the 3-D globe: ${k} turned on`).toBe(true)
        }

        cleanup()
        const again = await mount()
        expect(slotsOn(again.container).sort()).toEqual([...shown].sort())
        expect(layoutNow()).toBe(LABEL[id])
      } finally {
        restore()
      }
    })
  }

  it('after a preset, moving or resizing a pane reads Custom, and nothing snaps back', async () => {
    const restore = fakeBoxes(1920)
    try {
      const first = await mount()
      pick('Map first')
      expect(layoutNow(), 'control: the preset is on screen').toBe('Map first')
      fireEvent.keyDown(screen.getByRole('separator', { name: 'Left panel column width' }), { key: 'ArrowRight' })
      expect(layoutNow(), 'a rail width step').toBe('Custom')
      expect(chip('Map first').getAttribute('aria-pressed')).toBe('false')

      pick('Map first')
      expect(layoutNow(), 'a second tap puts it back — the only way it comes back').toBe('Map first')
      fireEvent.change(first.container.querySelector('.pane-frame[data-slot="left1"] select')!, { target: { value: 'greyline' } })
      expect(layoutNow(), 'a pane picked into a slot').toBe('Custom')

      // Nothing remembers the pick to snap back to: a remount keeps the operator's arrangement.
      cleanup()
      const again = await mount()
      expect(paneIn(again.container, 'left1')).toBe('greyline')
      expect(layoutNow()).toBe('Custom')

      pick('Dashboard')
      closeSlot(again.container, 'bottom3')
      expect(layoutNow(), 'a pane closed').toBe('Custom')
      pick('Dashboard')
      fireEvent.keyDown(screen.getByRole('separator', { name: 'Split between the left panels' }), { key: 'ArrowDown' })
      expect(layoutNow(), 'a split moved').toBe('Custom')
    } finally {
      restore()
    }
  })

  it('over the operator’s own arrangement, the menu says what a tap costs, and Undo puts all of it back — widths included', async () => {
    const restore = fakeBoxes(1920)
    try {
      const { container } = await mount()
      fireEvent.change(container.querySelector('.pane-frame[data-slot="left1"] select')!, { target: { value: 'greyline' } })
      closeSlot(container, 'right2')
      fireEvent.keyDown(screen.getByRole('separator', { name: 'Left panel column width' }), { key: 'ArrowRight' })
      const before = { slots: stored(CONFIG).slots, record: stored(RECORD), widths: stored(WIDTHS), left: grid(container).style.getPropertyValue('--cn-rail-l') }
      expect(before.left, 'control: the width step took').toBe('316px')
      expect(layoutNow()).toBe('Custom')
      expect(within(layouts()).getByText('Picking one replaces your own arrangement. Undo last change puts it back.')).toBeTruthy()

      pick('List first')
      expect(layoutNow()).toBe('List first')
      expect(within(layouts()).queryByText(/Picking one replaces/), 'no warning once nothing of the operator’s is on screen').toBeNull()

      fireEvent.click(screen.getByRole('button', { name: 'Undo last change' }))
      expect(stored(CONFIG).slots).toEqual(before.slots)
      expect(stored(RECORD)).toEqual(before.record)
      expect(stored(WIDTHS)).toEqual(before.widths)
      expect(grid(container).style.getPropertyValue('--cn-rail-l')).toBe(before.left)
      expect(paneIn(container, 'left1')).toBe('greyline')
      expect(paneIn(container, 'right2'), 'the pane closed before the preset is closed again').toBeUndefined()
      expect(layoutNow()).toBe('Custom')
    } finally {
      restore()
    }
  })

  it('a preset’s widths are clamped on load: List first saved on a big window fits the 1024 floor and still reads List first', async () => {
    let restore = fakeBoxes(1920)
    const first = await mount()
    pick('List first')
    first.unmount()
    restore()
    restore = fakeBoxes(1024)
    try {
      const { container } = await mount()
      const l = parseFloat(grid(container).style.getPropertyValue('--cn-rail-l'))
      const r = parseFloat(grid(container).style.getPropertyValue('--cn-rail-r'))
      // jsdom carries no sheet, so padding and gaps read 0 and the room is the box less the map floor.
      expect(l + r, `rails ${l}+${r} leave the map less than its floor`).toBeLessThanOrEqual(1024 - MAP_MIN)
      expect(l, 'control: the stored width really was squeezed').toBeLessThan(CONNECT_PRESETS.listFirst.rails.left!)
      expect(r).toBeLessThan(CONNECT_PRESETS.listFirst.rails.right!)
      expect(stored(WIDTHS), 'a clamp never rewrites the preference').toEqual(CONNECT_PRESETS.listFirst.rails)
      expect(layoutNow()).toBe('List first')
    } finally {
      restore()
    }
  })

  it('the Remote Connect page has the same picker, and a preset applies there', async () => {
    const source = { client: { supports: () => true } } as unknown as RemoteCollections
    let r!: ReturnType<typeof render>
    await act(async () => {
      r = render(
        <StationDataContext.Provider value={false}>
          <RemoteCollectionsContext.Provider value={source}>
            <ConnectView {...props} />
          </RemoteCollectionsContext.Provider>
        </StationDataContext.Provider>,
      )
    })
    expect(r.container.querySelector('.connect-header [role="status"]'), 'control: this is the Remote page').not.toBeNull()
    expect(layoutNow()).toBe('Standard')
    pick('List first')
    expect(slotsOn(r.container).sort()).toEqual(['left1', 'left2', 'right1', 'right2'])
    expect(layoutNow()).toBe('List first')
  })

  it('a pop-out’s preset is its own: the main window’s arrangement is not touched', async () => {
    window.history.replaceState(null, '', '/?panel=connect')
    const { container } = await mount()
    pick('Map first')
    expect(slotsOn(container).sort()).toEqual(['left1', 'left2', 'right1', 'right2'])
    expect(stored('nexus.connect.config.connect').slots).toEqual(CONNECT_PRESETS.mapFirst.slots)
    expect(stored(POPOUT_RECORD).state).toEqual({ bottom1: 'removed', bottom2: 'removed', bottom3: 'removed' })
    expect(stored('nexus.connect.railWidths.connect')).toEqual(CONNECT_PRESETS.mapFirst.rails)
    expect(localStorage.getItem(CONFIG), 'the main window’s placement').toBeNull()
    expect(localStorage.getItem(RECORD), 'the main window’s record').toBeNull()
    expect(localStorage.getItem(WIDTHS), 'the main window’s widths').toBeNull()
  })

  // FRAME TURNS THE SATELLITES ON (the operator's pick: "satellites on in the Frame layout"). The one
  // reach a layout has into the map: it turns layers ON, on the map on screen and the one behind the
  // picker, and touches nothing else. The REAL MapView answers here, so these are its own boxes.
  describe('Frame turns the satellites on, and nothing else about the map', () => {
    const SATS = 'Satellites (amateur)'
    /** Every box in the map's Layers panel, by name: what the map shows. */
    const mapBoxes = (c: HTMLElement) =>
      Object.fromEntries(
        [...c.querySelectorAll('.map-layers input[type="checkbox"]')].map((b) => [b.closest('label')!.textContent!.trim(), (b as HTMLInputElement).checked]),
      )

    it('picking Frame ticks Satellites on the map on screen and in the 3-D globe’s record, and no other box', async () => {
      const restore = fakeBoxes(1920)
      try {
        const { container } = await mount()
        const before = mapBoxes(container)
        expect(Object.keys(before).length, 'CONTROL: the map’s Layers panel is on screen').toBeGreaterThan(10)
        expect(before[SATS], 'CONTROL: off by default').toBe(false)
        pick('Frame')
        const after = mapBoxes(container)
        expect(after[SATS], 'Frame did not turn the satellites on').toBe(true)
        expect({ ...after, [SATS]: false }, 'Frame moved another box').toEqual(before)
        expect(stored(GLOBE)?.sats, 'the 3-D globe behind the picker').toBe(true)
      } finally {
        restore()
      }
    })

    it('keeps the operator’s own picks: a layer they turned off stays off, one they turned on stays on', async () => {
      const restore = fakeBoxes(1920)
      try {
        const { container } = await mount()
        fireEvent.click(within(container.querySelector('.map-layers')!).getByRole('checkbox', { name: 'Band heat (openings)' }))
        fireEvent.click(within(container.querySelector('.map-layers')!).getByRole('checkbox', { name: 'Aurora oval' }))
        const before = mapBoxes(container)
        expect([before['Band heat (openings)'], before['Aurora oval']], 'CONTROL: the operator’s picks took').toEqual([false, true])
        pick('Frame')
        const after = mapBoxes(container)
        expect([after['Band heat (openings)'], after['Aurora oval'], after[SATS]]).toEqual([false, true, true])
      } finally {
        restore()
      }
    })

    it('turned off again, the satellites stay off — the layout still reads Frame, after a remount too', async () => {
      const restore = fakeBoxes(1920)
      try {
        const first = await mount()
        pick('Frame')
        fireEvent.click(within(first.container.querySelector('.map-layers')!).getByRole('checkbox', { name: SATS }))
        expect(mapBoxes(first.container)[SATS]).toBe(false)
        expect(layoutNow(), 'a layer is not part of the layout: unticking one is not a change to it').toBe('Frame')
        cleanup()
        const again = await mount()
        expect(mapBoxes(again.container)[SATS], 'nothing turned it back on').toBe(false)
        expect(layoutNow()).toBe('Frame')
      } finally {
        restore()
      }
    })

    it('Undo last change takes the satellites back off with the layout, on both maps', async () => {
      const restore = fakeBoxes(1920)
      try {
        const { container } = await mount()
        pick('Frame')
        expect(mapBoxes(container)[SATS], 'CONTROL').toBe(true)
        fireEvent.click(screen.getByRole('button', { name: 'Undo last change' }))
        expect(mapBoxes(container)[SATS], 'Undo left the satellites on').toBe(false)
        expect(stored(GLOBE)?.sats, 'Undo left them on the 3-D globe').toBe(false)
        expect(layoutNow()).toBe('Standard')
      } finally {
        restore()
      }
    })

    it('Undo leaves the satellites on when they were on before the tap', async () => {
      const restore = fakeBoxes(1920)
      try {
        const { container } = await mount()
        fireEvent.click(within(container.querySelector('.map-layers')!).getByRole('checkbox', { name: SATS }))
        expect(mapBoxes(container)[SATS], 'CONTROL: the operator turned them on first').toBe(true)
        pick('Frame')
        fireEvent.click(screen.getByRole('button', { name: 'Undo last change' }))
        expect(mapBoxes(container)[SATS], 'Undo turned off what the operator had on').toBe(true)
        expect(layoutNow(), 'CONTROL: the Undo did happen').toBe('Standard')
      } finally {
        restore()
      }
    })
  })
})
