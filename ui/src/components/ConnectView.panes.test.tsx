// @vitest-environment jsdom
//
// CLOSE AND RESIZE ON THE CONNECT SURFACE (operator-approved 2026-09-13). A tester comparing
// Nexus with OpenHamClock: Connect shows too much at once — close the panels you don't use,
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
  getLog: vi.fn(async () => []),
  getLogStats: vi.fn(async () => null),
  getOtaMapSpots: vi.fn(async () => []),
  getContests: vi.fn(async () => []),
}))
import { ConnectView } from './ConnectView'
import { SLOT_IDS, type SlotId } from '../features/connectConfig'
import { MAP_MIN, RAIL_MAX, RAIL_MIN } from '../features/connectRails'

const RECORD = 'nexus.panels.connect.main'
const POPOUT_RECORD = 'nexus.panels.connect.connect'
const WIDTHS = 'nexus.connect.railWidths'

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
      expect(JSON.parse(localStorage.getItem(WIDTHS)!)).toEqual({ left: 316 })
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
      expect(grid(container).style.getPropertyValue('--cn-rail-l')).toBe(`${1024 - MAP_MIN - 300}px`)
      expect(JSON.parse(localStorage.getItem(WIDTHS)!), 'a re-clamp must not overwrite the choice').toEqual({ left: 5000 })
    } finally {
      restore()
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
