// @vitest-environment jsdom
//
// CONNECT'S MAP | BOTTOM STRIP DIVIDER (layout L7): the strip's height, which the sheet caps at 30 %
// of the effective viewport, becomes the operator's. The REAL ConnectView is mounted; only the
// backend is stubbed. jsdom lays nothing out, so the grid and the strip get stubbed boxes, and the
// strip's box follows the height the divider paints on the grid the way the implicit row does
// (`grid-auto-rows: var(--cn-strip-h, auto)`): a % of the grid once painted, its tallest pane (90
// px) until then. The real rectangles are measured in Chrome by the layout harness; the cascade
// is computed in connect-layout.test.ts.
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
import type { SlotId } from '../features/connectConfig'
import { pastTheSwitch } from './ConnectView.testkit'

// THE BUDGET (2026-10-09). The slowest case here, "sits inside the strip (above its panes on screen, out…", takes
// 0.39 s and 0.29 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const KEY = 'nexus.split.connect.strip'
const GRID_H = 700
const STRIP_CONTENT_H = 90

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

const grid = (c: HTMLElement) => c.querySelector('.connect') as HTMLElement
const strip = (c: HTMLElement) => c.querySelector('.connect-strip') as HTMLElement | null
const divider = () => screen.getByRole('separator', { name: 'Bottom panel row height' })
const aria = (el: HTMLElement) => ['aria-valuenow', 'aria-valuemin', 'aria-valuemax'].map((a) => Number(el.getAttribute(a)))
const painted = (c: HTMLElement) => grid(c).style.getPropertyValue('--cn-strip-h')

function closeSlot(c: HTMLElement, slot: SlotId) {
  const frame = c.querySelector(`.pane-frame[data-slot="${slot}"]`) as HTMLElement
  fireEvent.click(within(frame).getByRole('button', { name: /^Hide / }))
}

const realRect = HTMLElement.prototype.getBoundingClientRect
beforeEach(() => {
  localStorage.clear()
  pastTheSwitch()
  window.history.replaceState(null, '', '/')
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  HTMLElement.prototype.getBoundingClientRect = function (this: HTMLElement) {
    const box = (height: number) => ({ x: 0, y: 0, left: 0, top: 0, width: 1600, height, right: 1600, bottom: height, toJSON() {} }) as DOMRect
    if (this.classList.contains('connect')) return box(GRID_H)
    if (this.classList.contains('connect-strip')) {
      const v = this.parentElement?.style.getPropertyValue('--cn-strip-h') ?? ''
      if (v.endsWith('%')) return box((parseFloat(v) / 100) * GRID_H)
      if (v.endsWith('px')) return box(parseFloat(v))
      return box(STRIP_CONTENT_H)
    }
    return realRect.call(this)
  }
})
afterEach(() => {
  cleanup()
  HTMLElement.prototype.getBoundingClientRect = realRect
})

describe('the map | bottom strip divider', () => {
  it('sits inside the strip (above its panes on screen, out of flow) and announces the strip’s own height', async () => {
    const { container } = await mount()
    const d = divider()
    expect(d.parentElement).toBe(strip(container))
    expect(d.getAttribute('aria-orientation')).toBe('horizontal')
    expect(d.tabIndex).toBe(0)
    // Unsized: the strip's tallest pane, from a 4em floor (16 px font in jsdom) to half the grid.
    expect(aria(d)).toEqual([STRIP_CONTENT_H, 64, GRID_H / 2])
    // Nothing painted, nothing marked: the stock layout is untouched until the operator moves it.
    expect(painted(container)).toBe('')
    expect(strip(container)!.hasAttribute('data-sized')).toBe(false)
    // The panes are still the strip's only grid items: three, in their slots.
    expect([...strip(container)!.querySelectorAll(':scope > .pane-frame')].length).toBe(3)
  })

  it('moving it up grows the strip (the strip is below it), paints the grid’s row and marks the strip', async () => {
    const { container } = await mount()
    const d = divider()
    fireEvent.keyDown(d, { key: 'ArrowUp' })
    expect(parseFloat(painted(container))).toBeCloseTo(((STRIP_CONTENT_H + 16) / GRID_H) * 100, 6)
    expect(strip(container)!.hasAttribute('data-sized'), 'the sized rule lifts the pane cap').toBe(true)
    expect(parseFloat(localStorage.getItem(KEY)!)).toBeCloseTo(((STRIP_CONTENT_H + 16) / GRID_H) * 100, 6)
    // A big step down from 106 px would be 42 px: it stops at the 4em floor.
    fireEvent.keyDown(d, { key: 'ArrowDown', shiftKey: true })
    expect(parseFloat(painted(container))).toBeCloseTo((64 / GRID_H) * 100, 6)
  })

  it('End takes it to half the grid and no further; Home to its 4em floor; Backspace back to the stock height', async () => {
    const { container } = await mount()
    const d = divider()
    fireEvent.keyDown(d, { key: 'End' })
    expect(painted(container)).toBe('50%')
    fireEvent.keyDown(d, { key: 'ArrowUp' })
    expect(painted(container), 'the map keeps at least half').toBe('50%')
    fireEvent.keyDown(d, { key: 'Home' })
    expect(parseFloat(painted(container))).toBeCloseTo((64 / GRID_H) * 100, 6)
    fireEvent.keyDown(d, { key: 'Backspace' })
    expect(painted(container), 'the row is `auto` again').toBe('')
    expect(strip(container)!.hasAttribute('data-sized')).toBe(false)
    expect(localStorage.getItem(KEY)).toBe('')
  })

  it('a stored height is fitted into this grid on load, and the stored preference is never rewritten', async () => {
    localStorage.setItem(KEY, '80')
    const { container } = await mount()
    expect(painted(container), 'no more than half the grid').toBe('50%')
    expect(strip(container)!.hasAttribute('data-sized')).toBe(true)
    expect(localStorage.getItem(KEY)).toBe('80')
    expect(aria(divider())[0]).toBe(GRID_H / 2)
  })

  it('a window that never set a height opens exactly as before (a pre-L7 install)', async () => {
    const { container } = await mount()
    expect(painted(container)).toBe('')
    expect(localStorage.getItem(KEY)).toBeNull()
  })

  it('Reset layout puts the strip’s own height back too — the out-of-box state', async () => {
    localStorage.setItem(KEY, '30')
    const { container } = await mount()
    expect(painted(container)).toBe('30%')
    fireEvent.click(screen.getByRole('button', { name: /⊞ Panels/ }))
    fireEvent.click(screen.getByRole('button', { name: 'Reset layout' }))
    expect(painted(container)).toBe('')
    expect(strip(container)!.hasAttribute('data-sized')).toBe(false)
    expect(localStorage.getItem(KEY)).toBe('')
    // …and the divider after it reads the stock height, not the one it held.
    expect(aria(divider())[0]).toBe(STRIP_CONTENT_H)
  })

  it('a closed strip takes its divider with it: nothing is left to divide', async () => {
    const { container } = await mount()
    for (const s of ['bottom1', 'bottom2', 'bottom3'] as const) closeSlot(container, s)
    expect(strip(container)).toBeNull()
    expect(screen.queryByRole('separator', { name: 'Bottom panel row height' })).toBeNull()
  })
})
