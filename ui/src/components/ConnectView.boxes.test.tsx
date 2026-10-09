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
import { pastTheSwitch } from './ConnectView.testkit'

// THE BUDGET (2026-10-09). The slowest case here, "A+ grows THAT pane’s text a step at a time, keeps the…", takes
// 0.41 s and 0.32 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

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
  pastTheSwitch()
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
    // The default layout: Best band (left, top) is a row of the pane-grid table; Chase (right, top)
    // has a section of its own.
    expect([DEFAULT_SLOTS.left1, DEFAULT_SLOTS.right1], 'control: the default layout').toEqual(['advisory', 'chase'])
    let a = link(openMenu(container, 'left1'))!
    expect(a.textContent).toBe('?Best band in the manual')
    expect(a.getAttribute('href')).toBe('https://hamradiotools.io/manual/connect#the-pane-grid')
    expect(a.getAttribute('target')).toBe('_blank')
    expect(a.getAttribute('rel')).toBe('noreferrer')
    fireEvent.keyDown(screen.getByRole('menu'), { key: 'Escape' })
    a = link(openMenu(container, 'right1'))!
    expect(a.getAttribute('href')).toBe('https://hamradiotools.io/manual/connect#chase-whats-workable-now')
  })

  it('Bands for you, in the default layout, links to its row in the pane grid', async () => {
    const { container } = await mount()
    expect(DEFAULT_SLOTS.left2).toBe('bandTiles')
    const menu = openMenu(container, 'left2')
    expect(link(menu)?.getAttribute('href')).toBe('https://hamradiotools.io/manual/connect#the-pane-grid')
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

describe('tabs — several panes in one slot', () => {
  const CONFIG = 'nexus.connect.config'
  const cfg = () => JSON.parse(localStorage.getItem(CONFIG) ?? 'null')
  const tabsIn = (c: HTMLElement, s: SlotId) => within(frameOf(c, s)).queryAllByRole('tab')
  const tabNames = (c: HTMLElement, s: SlotId) => tabsIn(c, s).map((b) => b.textContent)
  const selected = (c: HTMLElement, s: SlotId) => tabsIn(c, s).find((b) => b.getAttribute('aria-selected') === 'true')?.textContent
  const paneIn = (c: HTMLElement, s: SlotId) => frameOf(c, s)?.getAttribute('data-pane')
  /** ⋯ ▸ Add a tab ▸ <pane>, the way a pointer does it. */
  function openAddTab(c: HTMLElement, s: SlotId) {
    const menu = openMenu(c, s)
    fireEvent.click(within(menu).getByRole('menuitem', { name: /Add a tab/ }))
    const menus = screen.getAllByRole('menu')
    expect(menus.length, 'the submenu opened').toBe(2)
    return menus[1]
  }
  const addTab = (c: HTMLElement, s: SlotId, name: string) =>
    fireEvent.click(within(openAddTab(c, s)).getByRole('menuitem', { name }))
  const layoutNow = () => {
    if (!screen.queryByRole('group', { name: 'Layout' })) fireEvent.click(screen.getByRole('button', { name: /⊞ Panels/ }))
    return screen.getByRole('group', { name: 'Layout' }).querySelector('.connect-layout-now')?.textContent
  }

  it('a layout saved before tabs loads exactly as today: every head is its title, no tab strip anywhere', async () => {
    localStorage.setItem(CONFIG, JSON.stringify({ slots: { ...DEFAULT_SLOTS, left1: 'greyline' }, overlays: {} }))
    const { container } = await mount()
    for (const s of SLOT_IDS) {
      const head = frameOf(container, s).querySelector('.pane-head')!
      expect([...head.children].map((el) => el.className), s).toEqual(['pane-title', 'pane-acts'])
      expect(frameOf(container, s).querySelector('[role="tablist"], [role="tabpanel"]'), s).toBeNull()
    }
    expect(paneIn(container, 'left1')).toBe('greyline')
    expect(cfg().tabs, 'loading rewrote nothing').toBeUndefined()
  })

  it('⋯ ▸ Add a tab puts a second pane in the slot and shows it: the title becomes a tab strip and the body its tabpanel', async () => {
    const { container } = await mount()
    addTab(container, 'left2', 'Clock')
    expect(tabNames(container, 'left2')).toEqual(['Bands for you', 'Clock'])
    expect(selected(container, 'left2')).toBe('Clock')
    expect(paneIn(container, 'left2')).toBe('clock')
    const strip = within(frameOf(container, 'left2')).getByRole('tablist')
    expect(strip.getAttribute('aria-label')).toBe('Panes in this slot')
    const body = bodyOf(container, 'left2')
    const shown = tabsIn(container, 'left2').find((b) => b.getAttribute('aria-selected') === 'true')!
    expect(body.getAttribute('role')).toBe('tabpanel')
    expect(body.getAttribute('aria-labelledby')).toBe(shown.id)
    expect(shown.getAttribute('aria-controls')).toBe(body.id)
    expect(cfg().slots.left2).toBe('clock')
    expect(cfg().tabs).toEqual({ left2: ['bandTiles', 'clock'] })
    expect(screen.queryByRole('menu'), 'the menu closed: the slot changed under it').toBeNull()
  })

  it('a click on a tab shows it, and the slot reopens on the tab that was showing', async () => {
    const first = await mount()
    addTab(first.container, 'right2', 'Greyline')
    fireEvent.click(within(frameOf(first.container, 'right2')).getByRole('tab', { name: 'Band Outlook' }))
    expect(paneIn(first.container, 'right2')).toBe('outlook')
    first.unmount()
    const { container } = await mount()
    expect(selected(container, 'right2')).toBe('Band Outlook')
    expect(tabNames(container, 'right2')).toEqual(['Band Outlook', 'Greyline'])
  })

  it('the keyboard: only the shown tab is in the Tab order, ←/→ wrap and show, Home/End go to the ends', async () => {
    const { container } = await mount()
    addTab(container, 'bottom1', 'Clock')
    addTab(container, 'bottom1', 'Greyline')
    expect(tabNames(container, 'bottom1')).toEqual(['Openings', 'Clock', 'Greyline'])
    const order = () => tabsIn(container, 'bottom1').map((b) => b.tabIndex)
    expect(order()).toEqual([-1, -1, 0])
    const tab = (name: string) => within(frameOf(container, 'bottom1')).getByRole('tab', { name })
    fireEvent.keyDown(tab('Greyline'), { key: 'ArrowRight' })
    expect(paneIn(container, 'bottom1'), '→ wraps to the first').toBe('openings')
    expect(document.activeElement).toBe(tab('Openings'))
    expect(order()).toEqual([0, -1, -1])
    fireEvent.keyDown(tab('Openings'), { key: 'ArrowLeft' })
    expect(paneIn(container, 'bottom1'), '← wraps to the last').toBe('greyline')
    fireEvent.keyDown(tab('Greyline'), { key: 'Home' })
    expect(paneIn(container, 'bottom1')).toBe('openings')
    fireEvent.keyDown(tab('Openings'), { key: 'End' })
    expect(paneIn(container, 'bottom1')).toBe('greyline')
    expect(document.activeElement).toBe(tab('Greyline'))
  })

  it('the picker replaces the SHOWN tab, in its place', async () => {
    const { container } = await mount()
    addTab(container, 'left2', 'Clock')
    fireEvent.change(frameOf(container, 'left2').querySelector('select')!, { target: { value: 'insights' } })
    expect(tabNames(container, 'left2')).toEqual(['Bands for you', 'Insights'])
    expect(selected(container, 'left2')).toBe('Insights')
  })

  it('⋯ ▸ Remove takes the shown pane out; with one pane left the head is its title again', async () => {
    const { container } = await mount()
    addTab(container, 'left2', 'Clock')
    let menu = openMenu(container, 'left2')
    fireEvent.click(within(menu).getByRole('menuitem', { name: 'Remove Clock from this slot' }))
    expect(tabsIn(container, 'left2')).toEqual([])
    expect(frameOf(container, 'left2').querySelector('.pane-title')?.textContent).toBe('Bands for you')
    expect(cfg().tabs).toEqual({})
    menu = openMenu(container, 'left2')
    expect(within(menu).queryByRole('menuitem', { name: /^Remove / }), 'a slot’s only pane has no Remove — ✕ closes the slot').toBeNull()
  })

  it('a pane is in one slot at most: a tab added here MOVES from the slot it was in', async () => {
    localStorage.setItem(CONFIG, JSON.stringify({ slots: DEFAULT_SLOTS, tabs: { right2: ['outlook', 'clock'] }, overlays: {} }))
    const { container } = await mount()
    expect(tabNames(container, 'right2'), 'control: a stored tab strip loads').toEqual(['Band Outlook', 'Clock'])
    addTab(container, 'left2', 'Clock')
    expect(tabNames(container, 'left2')).toEqual(['Bands for you', 'Clock'])
    expect(tabsIn(container, 'right2'), 'Clock left the other slot').toEqual([])
    expect(paneIn(container, 'right2')).toBe('outlook')
  })

  it('another slot’s only pane is not offered — it would leave that slot empty', async () => {
    const { container } = await mount()
    const sub = openAddTab(container, 'left2')
    const offered = within(sub).getAllByRole('menuitem').map((i) => i.textContent)
    expect(offered).toContain('Clock')
    expect(offered, 'Space Wx is the only pane of the bottom row’s middle slot').not.toContain('Space Wx')
    expect(offered, 'already here').not.toContain('Bands for you')
  })

  it('Reset layout is one pane per slot again, and Undo brings the tabs back', async () => {
    const { container } = await mount()
    addTab(container, 'left2', 'Clock')
    fireEvent.click(screen.getByRole('button', { name: /⊞ Panels/ }))
    fireEvent.click(screen.getByRole('button', { name: 'Reset layout' }))
    for (const s of SLOT_IDS) expect(tabsIn(container, s), s).toEqual([])
    expect(cfg().tabs).toEqual({})
    fireEvent.click(screen.getByRole('button', { name: 'Undo last change' }))
    expect(tabNames(container, 'left2')).toEqual(['Bands for you', 'Clock'])
    expect(selected(container, 'left2')).toBe('Clock')
  })

  it('with tabs the Layout menu reads Custom; a layout is one pane per slot, reads as itself, and Undo brings the tabs back', async () => {
    const { container } = await mount()
    expect(layoutNow()).toBe('Standard')
    fireEvent.keyDown(screen.getByRole('group', { name: 'Layout' }), { key: 'Escape' })
    addTab(container, 'left2', 'Clock')
    expect(layoutNow()).toBe('Custom')
    fireEvent.click(within(screen.getByRole('group', { name: 'Layout' })).getByRole('button', { name: 'Dashboard' }))
    expect(layoutNow()).toBe('Dashboard')
    for (const s of SLOT_IDS) expect(tabsIn(container, s), s).toEqual([])
    fireEvent.click(screen.getByRole('button', { name: 'Undo last change' }))
    expect(tabNames(container, 'left2')).toEqual(['Bands for you', 'Clock'])
    expect(layoutNow()).toBe('Custom')
  })

  it('a pane’s text size belongs to its slot: it applies to whichever tab is shown', async () => {
    const { container } = await mount()
    addTab(container, 'left2', 'Clock')
    fireEvent.click(item(openMenu(container, 'left2'), /Larger text/))
    fireEvent.keyDown(screen.getByRole('menu'), { key: 'Escape' })
    expect(factorOf(container, 'left2')).toBe('1.1')
    fireEvent.click(within(frameOf(container, 'left2')).getByRole('tab', { name: 'Bands for you' }))
    expect(factorOf(container, 'left2')).toBe('1.1')
  })
})

describe('auto-rotate — a slot’s tabs in turn, on the dashboard window and the TV page only', () => {
  const CONFIG = 'nexus.connect.config'
  const cfg = () => JSON.parse(localStorage.getItem(CONFIG) ?? 'null')
  const paneIn = (c: HTMLElement, s: SlotId) => frameOf(c, s)?.getAttribute('data-pane')
  const store = (rotate?: Record<string, number>) =>
    localStorage.setItem(
      CONFIG,
      JSON.stringify({ slots: DEFAULT_SLOTS, tabs: { left2: ['bandTiles', 'clock', 'greyline'] }, ...(rotate ? { rotate } : {}), overlays: {} }),
    )
  async function mountRotating(autoRotate = true) {
    let r!: ReturnType<typeof render>
    await act(async () => {
      r = render(<ConnectView {...props} autoRotate={autoRotate} />)
    })
    return r
  }
  const tick = async (ms: number) => {
    await act(async () => {
      vi.advanceTimersByTime(ms)
    })
  }

  beforeEach(() => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval'] })
  })
  afterEach(() => {
    vi.useRealTimers()
  })

  it('shows the next tab every interval, round the slot and back to the first', async () => {
    store({ left2: 10 })
    const { container } = await mountRotating()
    expect(paneIn(container, 'left2')).toBe('bandTiles')
    await tick(9_999)
    expect(paneIn(container, 'left2'), 'not before the interval').toBe('bandTiles')
    await tick(1)
    expect(paneIn(container, 'left2')).toBe('clock')
    await tick(10_000)
    expect(paneIn(container, 'left2')).toBe('greyline')
    await tick(10_000)
    expect(paneIn(container, 'left2')).toBe('bandTiles')
  })

  it('a re-render mid-interval does not restart the count (the window re-renders on every snapshot)', async () => {
    store({ left2: 10 })
    const { container, rerender } = await mountRotating()
    await tick(6_000)
    await act(async () => {
      rerender(<ConnectView {...props} stations={[]} autoRotate />)
    })
    await tick(4_000)
    expect(paneIn(container, 'left2')).toBe('clock')
  })

  it('pauses while the pointer is over the slot, and starts a fresh interval when it leaves', async () => {
    store({ left2: 10 })
    const { container } = await mountRotating()
    fireEvent.pointerEnter(frameOf(container, 'left2'))
    await tick(60_000)
    expect(paneIn(container, 'left2'), 'paused under the pointer').toBe('bandTiles')
    fireEvent.pointerLeave(frameOf(container, 'left2'))
    await tick(9_999)
    expect(paneIn(container, 'left2')).toBe('bandTiles')
    await tick(1)
    expect(paneIn(container, 'left2')).toBe('clock')
  })

  it('pauses while the slot has the keyboard focus', async () => {
    store({ left2: 10 })
    const { container } = await mountRotating()
    const tab = within(frameOf(container, 'left2')).getByRole('tab', { name: 'Bands for you' })
    fireEvent.keyDown(document.body, { key: 'Tab' }) // the keyboard is what moves focus here
    act(() => tab.focus())
    await tick(60_000)
    expect(paneIn(container, 'left2'), 'paused while focused').toBe('bandTiles')
    act(() => tab.blur())
    await tick(10_000)
    expect(paneIn(container, 'left2')).toBe('clock')
  })

  it('the focus a mouse click leaves behind does not hold the slot once the pointer has gone', async () => {
    store({ left2: 10 })
    const { container } = await mountRotating()
    const f = frameOf(container, 'left2')
    const tab = within(f).getByRole('tab', { name: 'Clock' })
    fireEvent.pointerEnter(f)
    fireEvent.pointerDown(tab, { button: 0, pointerType: 'mouse' })
    act(() => tab.focus())
    fireEvent.click(tab)
    expect(paneIn(container, 'left2'), 'control: the click showed the tab').toBe('clock')
    fireEvent.pointerLeave(f)
    expect(document.activeElement, 'the clicked tab keeps the focus').toBe(tab)
    await tick(10_000)
    expect(paneIn(container, 'left2')).toBe('greyline')
  })

  it('pauses while the slot’s ⋯ menu is open', async () => {
    store({ left2: 10 })
    const { container } = await mountRotating()
    openMenu(container, 'left2')
    await tick(60_000)
    expect(paneIn(container, 'left2')).toBe('bandTiles')
  })

  it('is OFF by default: a slot with tabs and no interval stays where it is', async () => {
    store()
    const { container } = await mountRotating()
    await tick(300_000)
    expect(paneIn(container, 'left2')).toBe('bandTiles')
  })

  it('⋯ ▸ Rotate the tabs: the operator picks the interval, and Off stops it', async () => {
    store()
    const { container } = await mountRotating()
    let menu = openMenu(container, 'left2')
    expect(within(menu).getByText('Rotate the tabs')).toBeTruthy()
    const choice = (name: string) => within(menu).getByRole('menuitemradio', { name })
    expect(choice('Off').getAttribute('aria-checked')).toBe('true')
    expect(within(menu).getAllByRole('menuitemradio').map((i) => i.textContent?.replace('●', '').trim())).toEqual([
      'Off',
      '10 s',
      '15 s',
      '30 s',
      '1 min',
      '2 min',
    ])
    fireEvent.click(choice('15 s'))
    expect(cfg().rotate).toEqual({ left2: 15 })
    await tick(15_000)
    expect(paneIn(container, 'left2')).toBe('clock')
    menu = openMenu(container, 'left2')
    expect(within(menu).getByRole('menuitemradio', { name: '15 s' }).getAttribute('aria-checked')).toBe('true')
    fireEvent.click(within(menu).getByRole('menuitemradio', { name: 'Off' }))
    expect(cfg().rotate).toEqual({})
    await tick(60_000)
    expect(paneIn(container, 'left2')).toBe('clock')
  })

  it('a slot with one pane offers no rotation', async () => {
    store()
    const { container } = await mountRotating()
    expect(within(openMenu(container, 'left1')).queryByText('Rotate the tabs')).toBeNull()
  })

  it('NEVER in the main window: no choice in the menu, and a stored interval does not rotate', async () => {
    store({ left2: 10 })
    const { container } = await mountRotating(false)
    await tick(120_000)
    expect(paneIn(container, 'left2')).toBe('bandTiles')
    expect(within(openMenu(container, 'left2')).queryByText('Rotate the tabs')).toBeNull()
  })

  it('a slot back to one pane forgets its interval; Reset layout clears it, and Undo brings it back', async () => {
    store({ left2: 10 })
    const { container } = await mountRotating()
    fireEvent.click(screen.getByRole('button', { name: /⊞ Panels/ }))
    fireEvent.click(screen.getByRole('button', { name: 'Reset layout' }))
    expect(cfg().rotate).toEqual({})
    fireEvent.click(screen.getByRole('button', { name: 'Undo last change' }))
    expect(cfg().rotate).toEqual({ left2: 10 })
    await tick(10_000)
    expect(paneIn(container, 'left2')).toBe('clock')
    // Remove two of the three tabs: one pane left, nothing to rotate.
    fireEvent.click(within(openMenu(container, 'left2')).getByRole('menuitem', { name: /^Remove / }))
    fireEvent.click(within(openMenu(container, 'left2')).getByRole('menuitem', { name: /^Remove / }))
    expect(cfg().tabs).toEqual({})
    expect(cfg().rotate).toEqual({})
  })
})
