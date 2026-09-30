// @vitest-environment jsdom
//
// A CONNECT PANE'S OWN OPTIONS — the ⋯ menu in every pane head (2026-09-29, the operator's picks):
// the pane's text size (A− / A+, 80–160 %) and "? … in the manual", its manual section.
//
// ⚠️ THE REAL ConnectView, THE REAL PaneFrame AND THE REAL Radix MENU ARE MOUNTED. Only the backend
// is stubbed. Every assertion reads the rendered screen or the stored record — a stubbed frame
// would prove a prop arrives and nothing about what the operator sees. Whether the words really
// grow is computed against the sheet in connect/PaneFrame.textScale.test.tsx; geometry was measured
// in Chrome (the report).
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest'
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
import { installExternalLinkInterceptor } from '../externalLinks'

const RECORD = 'nexus.panels.connect.main'

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

const frameOf = (c: HTMLElement, s: SlotId) => c.querySelector(`.pane-frame[data-slot="${s}"]`) as HTMLElement
const bodyOf = (c: HTMLElement, s: SlotId) => frameOf(c, s).querySelector('.pane-body') as HTMLElement
const factorOf = (c: HTMLElement, s: SlotId) => bodyOf(c, s).style.getPropertyValue('--box-text-scale')
const record = () => JSON.parse(localStorage.getItem(RECORD) ?? 'null')

/** Open a pane's ⋯ menu the way a pointer does (Radix opens on pointerdown). */
function openMenu(c: HTMLElement, s: SlotId) {
  const trigger = within(frameOf(c, s)).getByRole('button', { name: /^Options for / })
  fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false, pointerType: 'mouse' })
  return screen.getByRole('menu')
}
const item = (menu: HTMLElement, name: RegExp) => within(menu).getByRole('menuitem', { name })

beforeAll(() => {
  // Radix DropdownMenu measures and captures pointers; jsdom has neither (TopBar.help.test.tsx).
  Element.prototype.hasPointerCapture = () => false
  Element.prototype.setPointerCapture = () => {}
  Element.prototype.releasePointerCapture = () => {}
  Element.prototype.scrollIntoView = () => {}
})
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

describe('a pane’s text size — ⋯ ▸ A− / A+', () => {
  it('an older layout (no sizes stored) opens every pane at the app’s size: no factor on any body, nothing written', async () => {
    localStorage.setItem(RECORD, JSON.stringify({ v: 2, state: { bottom3: 'removed' }, share: {} }))
    const { container } = await mount()
    for (const s of SLOT_IDS.filter((s) => s !== 'bottom3')) expect(factorOf(container, s), s).toBe('')
    expect(record(), 'mounting rewrote the record').toEqual({ v: 2, state: { bottom3: 'removed' }, share: {} })
  })

  it('every pane has one ⋯, after the picker and before ✕ — the picker is still the frame’s first select', async () => {
    const { container } = await mount()
    for (const s of SLOT_IDS) {
      const acts = frameOf(container, s).querySelector('.pane-acts')!
      const kinds = [...acts.children].map((el) => (el.matches('select') ? 'picker' : el.classList.contains('pane-menu') ? '⋯' : el.classList.contains('pane-close') ? '✕' : el.className))
      expect(kinds, s).toEqual(['picker', '⋯', '✕'])
      expect(frameOf(container, s).querySelector('select'), s).toBe(acts.querySelector('.pane-pick'))
    }
  })

  it('A+ grows THAT pane’s text a step at a time, keeps the menu open reading the size back, and stores it per slot', async () => {
    const { container } = await mount()
    const menu = openMenu(container, 'left1')
    expect(within(menu).getByText('Text size: 100%')).toBeTruthy()
    fireEvent.click(item(menu, /Larger text/))
    expect(factorOf(container, 'left1')).toBe('1.1')
    expect(screen.getByRole('menu'), 'A+ keeps the menu open for the next press').toBe(menu)
    expect(within(menu).getByText('Text size: 110%')).toBeTruthy()
    fireEvent.click(item(menu, /Larger text/))
    fireEvent.click(item(menu, /Larger text/))
    expect(factorOf(container, 'left1')).toBe('1.3')
    expect(record().scale).toEqual({ left1: 1.3 })
    for (const s of SLOT_IDS.filter((s) => s !== 'left1')) expect(factorOf(container, s), `${s} is not the pane that was sized`).toBe('')
  })

  it('A− stops at 80 % and A+ at 160 %: the item at the end of the range is disabled', async () => {
    localStorage.setItem(RECORD, JSON.stringify({ v: 2, state: {}, share: {}, scale: { right1: 1.6 } }))
    const { container } = await mount()
    expect(factorOf(container, 'right1'), 'a stored size applies on load').toBe('1.6')
    let menu = openMenu(container, 'right1')
    expect(item(menu, /Larger text/).getAttribute('aria-disabled')).toBe('true')
    expect(item(menu, /Smaller text/).getAttribute('aria-disabled')).toBeNull()
    fireEvent.keyDown(menu, { key: 'Escape' })
    cleanup()

    const again = await mount()
    menu = openMenu(again.container, 'left2')
    fireEvent.click(item(menu, /Smaller text/))
    fireEvent.click(item(menu, /Smaller text/))
    expect(factorOf(again.container, 'left2')).toBe('0.8')
    expect(item(menu, /Smaller text/).getAttribute('aria-disabled')).toBe('true')
    expect(within(menu).getByText('Text size: 80%')).toBeTruthy()
  })

  it('a stored size out of range is clamped on load', async () => {
    localStorage.setItem(RECORD, JSON.stringify({ v: 2, state: {}, share: {}, scale: { left1: 7, left2: 0.2 } }))
    const { container } = await mount()
    expect(factorOf(container, 'left1')).toBe('1.6')
    expect(factorOf(container, 'left2')).toBe('0.8')
  })

  it('the size survives a remount; Reset layout puts every pane back at 100 %, and Undo brings it back', async () => {
    const first = await mount()
    fireEvent.click(item(openMenu(first.container, 'bottom2'), /Larger text/))
    first.unmount()
    const { container } = await mount()
    expect(factorOf(container, 'bottom2')).toBe('1.1')
    fireEvent.click(screen.getByRole('button', { name: /⊞ Panels/ }))
    fireEvent.click(screen.getByRole('button', { name: 'Reset layout' }))
    expect(factorOf(container, 'bottom2')).toBe('')
    expect(record().scale).toBeUndefined()
    fireEvent.click(screen.getByRole('button', { name: 'Undo last change' }))
    expect(factorOf(container, 'bottom2')).toBe('1.1')
  })

  it('a layout picked from ⊞ Panels keeps the panes’ text sizes (a layout moves panes, it does not resize words)', async () => {
    const { container } = await mount()
    fireEvent.click(item(openMenu(container, 'left1'), /Larger text/))
    fireEvent.keyDown(screen.getByRole('menu'), { key: 'Escape' })
    fireEvent.click(screen.getByRole('button', { name: /⊞ Panels/ }))
    fireEvent.click(within(screen.getByRole('group', { name: 'Layout' })).getByRole('button', { name: 'Frame' }))
    expect(within(screen.getByRole('group', { name: 'Layout' })).getByText('Frame', { selector: '.connect-layout-now' }), 'control: the layout applied').toBeTruthy()
    expect(factorOf(container, 'left1')).toBe('1.1')
    expect(record().scale).toEqual({ left1: 1.1 })
  })

  it('the keyboard reaches it: Enter on ⋯ opens the menu, and Enter on an item steps the size', async () => {
    const { container } = await mount()
    const trigger = within(frameOf(container, 'right2')).getByRole('button', { name: /^Options for / })
    trigger.focus()
    fireEvent.keyDown(trigger, { key: 'Enter' })
    const menu = screen.getByRole('menu')
    fireEvent.keyDown(item(menu, /Larger text/), { key: 'Enter' })
    expect(factorOf(container, 'right2')).toBe('1.1')
  })

  it('the pane head keeps the app’s size: the factor sits on the body only', async () => {
    localStorage.setItem(RECORD, JSON.stringify({ v: 2, state: {}, share: {}, scale: { left1: 1.5 } }))
    const { container } = await mount()
    const f = frameOf(container, 'left1')
    expect(f.style.getPropertyValue('--box-text-scale')).toBe('')
    expect((f.querySelector('.pane-head') as HTMLElement).style.getPropertyValue('--box-text-scale')).toBe('')
    expect(factorOf(container, 'left1')).toBe('1.5')
  })
})

describe('a pane’s manual link — ⋯ ▸ ? … in the manual', () => {
  const link = (menu: HTMLElement) => within(menu).queryByRole('menuitem', { name: / in the manual$/ }) as HTMLAnchorElement | null

  it('a pane the manual describes links to its own section, opening in the browser', async () => {
    const { container } = await mount()
    // The default layout: Conditions (left, top) is a row of the pane-grid table; Chase (right, top)
    // has a section of its own.
    expect([DEFAULT_SLOTS.left1, DEFAULT_SLOTS.right1], 'control: the default layout').toEqual(['advisory', 'chase'])
    let a = link(openMenu(container, 'left1'))!
    expect(a.textContent).toBe('?Conditions in the manual')
    expect(a.getAttribute('href')).toBe('https://hamradiotools.io/manual/connect#the-pane-grid')
    expect(a.getAttribute('target')).toBe('_blank')
    expect(a.getAttribute('rel')).toBe('noreferrer')
    fireEvent.keyDown(screen.getByRole('menu'), { key: 'Escape' })
    a = link(openMenu(container, 'right1'))!
    expect(a.getAttribute('href')).toBe('https://hamradiotools.io/manual/connect#chase-whats-workable-now')
  })

  it('a pane the manual does not describe has no link — Bands for you, in the default layout', async () => {
    const { container } = await mount()
    expect(DEFAULT_SLOTS.left2).toBe('bandTiles')
    const menu = openMenu(container, 'left2')
    expect(link(menu)).toBeNull()
    expect(item(menu, /Larger text/), 'the rest of its menu is there').toBeTruthy()
  })

  it('the link follows the pane in the slot: pick another pane and the link is its section', async () => {
    const { container } = await mount()
    fireEvent.change(frameOf(container, 'left1').querySelector('select')!, { target: { value: 'amp' } })
    expect(link(openMenu(container, 'left1'))!.getAttribute('href')).toBe('https://hamradiotools.io/manual/connect#the-amplifier-pane')
  })

  it('a click and Enter both reach the app’s external-link path (the browser opens it, not the window)', async () => {
    const opened: string[] = []
    const uninstall = installExternalLinkInterceptor((url) => opened.push(url))
    try {
      const { container } = await mount()
      fireEvent.click(link(openMenu(container, 'bottom2'))!)
      expect(opened, 'a click').toEqual(['https://hamradiotools.io/manual/connect#the-pane-grid'])
      const a = link(openMenu(container, 'bottom1'))!
      fireEvent.keyDown(a, { key: 'Enter' })
      expect(opened, 'Enter').toEqual(['https://hamradiotools.io/manual/connect#the-pane-grid', 'https://hamradiotools.io/manual/connect#read-an-opening'])
    } finally {
      uninstall()
    }
  })
})
