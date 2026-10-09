// @vitest-environment jsdom
//
// THE ONE-TIME SWITCH TO FRAME + BAR (step 5, the operator's "Everyone, once", 2026-10-01): "All users
// switch to Frame + bar on update, with Standard one click away. This overrides layouts people chose."
// With the condition the operator was told: what each surface had is KEPT, one tap brings back exactly
// that (not just Standard), and the switch runs once, never again.
//
// ⚠️ The REAL ConnectView is mounted, over real storage; only the backend is stubbed. What is asserted is
// what the operator sees (the frames, their tabs, the bar, the picker's words) and what is stored, never
// a call into the switch. jsdom does not lay out; the geometry is measured in a real browser.
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
  getSolarIndices: vi.fn(async () => ({ days: [] })),
  getOpeningsLog: vi.fn(async () => []),
  getKpForecast: vi.fn(async () => null),
  // The profile's ui-state.json (features/durableStore), driven by the round-trip case.
  uiStateLoad: vi.fn(async () => ({})),
  uiStateSave: vi.fn(async () => true),
}))
import { uiStateLoad, uiStateSave } from '../api'
import { ConnectView } from './ConnectView'
import { DEFAULT_SLOTS, type PaneId, type SlotId } from '../features/connectConfig'
import { CONNECT_PRESETS, TV_FRAME_BAR, TV_PRESETS } from '../features/connectPresets'
import { __resetDurableForTest, flushDurable, loadDurable } from '../features/durableStore'

// THE BUDGET (2026-10-09). The slowest case here, "a fresh install opens in Frame + bar, and keeps…", takes 0.45 s
// and 0.43 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than one
// core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

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
const A = CONNECT_PRESETS.frameBar
const PICKS = ['Standard', 'Map first', 'List first', 'Dashboard', 'Frame', 'Frame + bar (default)']
const PICKS_KEPT = ['Standard', 'Your earlier layout', 'Map first', 'List first', 'Dashboard', 'Frame', 'Frame + bar (default)']

/** An operator's own arrangement, from before the update: two boxes swapped, a tab, a closed box, an uneven
 *  split, dragged widths and a bigger text size in one box. Every record a layout writes, plus a text size
 *  (which no layout writes). */
const OWN_SLOTS: Record<SlotId, PaneId> = { ...DEFAULT_SLOTS, left1: 'spacewx', bottom2: 'advisory' }
const OWN_TABS = { right2: ['outlook', 'clock'] as PaneId[] }
const OWN_SHARE = { left1: 1.4, left2: 0.6 }
const OWN_RAILS = { left: 304, right: 512 }
function seedOwn(surface = '') {
  const sfx = surface ? `.${surface}` : ''
  localStorage.setItem(`nexus.connect.config${sfx}`, JSON.stringify({ slots: OWN_SLOTS, tabs: OWN_TABS, rotate: {}, overlays: {} }))
  localStorage.setItem(
    `nexus.panels.connect.${surface || 'main'}`,
    JSON.stringify({ v: 1, state: { bottom3: 'removed' }, share: OWN_SHARE, scale: { right1: 1.25 } }),
  )
  localStorage.setItem(`nexus.connect.railWidths${sfx}`, JSON.stringify(OWN_RAILS))
}

async function mount(extra: Record<string, unknown> = {}) {
  let r!: ReturnType<typeof render>
  await act(async () => {
    r = render(<ConnectView {...props} station={{ call: 'KD9TAW', grid: 'EN52' }} {...extra} />)
  })
  return r
}
const header = (c: HTMLElement) => c.querySelector('.connect-header') as HTMLElement
const openPicker = (c: HTMLElement) => {
  if (!screen.queryByRole('group', { name: 'Layout' })) fireEvent.click(within(header(c)).getByRole('button', { name: 'Layout' }))
  return screen.getByRole('group', { name: 'Layout' })
}
const now = (c: HTMLElement) => openPicker(c).querySelector('.connect-layout-now')?.textContent
const choices = (c: HTMLElement) => within(openPicker(c)).getAllByRole('button').map((b) => b.textContent)
const pick = (c: HTMLElement, label: string) => fireEvent.click(within(openPicker(c)).getByRole('button', { name: label }))
/** Slot → the box on screen there, for every frame drawn. */
const onScreen = (c: HTMLElement) =>
  Object.fromEntries([...c.querySelectorAll('.pane-frame')].map((f) => [f.getAttribute('data-slot'), f.getAttribute('data-pane')]))
const tabNames = (c: HTMLElement, s: SlotId) =>
  [...c.querySelectorAll(`.pane-frame[data-slot="${s}"] [role="tab"]`)].map((b) => b.textContent)
const bar = (c: HTMLElement) => c.querySelector('.connect-shell > .dash-bar')
const stored = (key: string) => JSON.parse(localStorage.getItem(key) ?? 'null')

beforeEach(() => {
  localStorage.clear()
  __resetDurableForTest()
  window.history.replaceState(null, '', '/')
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(() => {
  cleanup()
  __resetDurableForTest()
  window.history.replaceState(null, '', '/')
})

describe('the one-time switch to Frame + bar', () => {
  it('a fresh install opens in Frame + bar, and keeps nothing: Standard is the way back', async () => {
    const { container } = await mount()
    expect(now(container)).toBe('Frame + bar (default)')
    expect(onScreen(container)).toEqual({ left1: 'bandTiles', left2: 'openings', right1: 'chase', right2: 'getout' })
    expect(tabNames(container, 'left1')).toHaveLength(A.tabs!.left1!.length)
    expect(bar(container), 'the bar across the top').not.toBeNull()
    expect(choices(container)).toEqual(PICKS)
    expect(stored('nexus.connect.switch.main')).toEqual({ v: 1, kept: null })
  })

  it('an operator’s own layout is switched once, and one tap brings back exactly what they had', async () => {
    seedOwn()
    const { container } = await mount()
    expect(now(container)).toBe('Frame + bar (default)')
    expect(onScreen(container)).toEqual({ left1: 'bandTiles', left2: 'openings', right1: 'chase', right2: 'getout' })
    // The text size is no part of a layout: it rode through the switch.
    expect(stored('nexus.panels.connect.main').scale).toEqual({ right1: 1.25 })
    expect(choices(container)).toEqual(PICKS_KEPT)

    pick(container, 'Your earlier layout')
    expect(now(container)).toBe('Your earlier layout')
    const { bottom3: _closed, ...open } = OWN_SLOTS
    expect(onScreen(container)).toEqual(open)
    expect(tabNames(container, 'right2')).toEqual(['Band Outlook', 'Clock'])
    expect(bar(container)).toBeNull()
    expect(stored('nexus.connect.config')).toMatchObject({ slots: OWN_SLOTS, tabs: OWN_TABS })
    expect(stored('nexus.panels.connect.main')).toMatchObject({ state: { bottom3: 'removed' }, share: OWN_SHARE, scale: { right1: 1.25 } })
    expect(stored('nexus.connect.railWidths')).toEqual(OWN_RAILS)
    // One undoable step, like any layout's: Undo goes back to Frame + bar.
    fireEvent.click(within(openPicker(container).parentElement!).getByRole('button', { name: 'Undo last change' }))
    expect(now(container)).toBe('Frame + bar (default)')
  })

  it('never runs again: reopened after going back, the earlier layout stays, and so does Standard', async () => {
    seedOwn()
    const first = await mount()
    pick(first.container, 'Your earlier layout')
    first.unmount()
    const again = await mount()
    expect(now(again.container)).toBe('Your earlier layout')
    pick(again.container, 'Standard')
    again.unmount()
    const third = await mount()
    expect(now(third.container)).toBe('Standard')
    expect(bar(third.container)).toBeNull()
    expect(stored('nexus.connect.switch.main').kept).toMatchObject({ slots: OWN_SLOTS, tabs: OWN_TABS })
  })

  it('survives the profile round-trip: ui-state.json carries the record, so a reinstall or a restore never switches twice', async () => {
    const files: Array<Record<string, string>> = []
    vi.mocked(uiStateSave).mockImplementation(async (state) => {
      files.push({ ...state })
      return true
    })
    await loadDurable()
    seedOwn()
    const first = await mount()
    expect(now(first.container)).toBe('Frame + bar (default)')
    first.unmount()
    await flushDurable()
    const file = files[files.length - 1]
    expect(JSON.parse(file['nexus.connect.switch.main']).kept).toMatchObject({ slots: OWN_SLOTS })

    // A reinstall, or the backup restored on another computer: the browser's store is gone, and the profile's
    // file comes back. Switched a second time, Conditions would open in Frame + bar again, bar and tabs drawn.
    localStorage.clear()
    __resetDurableForTest()
    vi.mocked(uiStateLoad).mockResolvedValueOnce(file)
    await loadDurable()
    const { container } = await mount()
    expect(bar(container), 'switched a second time').toBeNull()
    expect(tabNames(container, 'left1')).toEqual([])
    // …and the layout it kept came back with the profile: still one tap.
    pick(container, 'Your earlier layout')
    expect(stored('nexus.connect.config')).toMatchObject({ slots: OWN_SLOTS, tabs: OWN_TABS })
    expect(stored('nexus.connect.railWidths')).toEqual(OWN_RAILS)
  })

  it('a dashboard window with no layout of its own follows the main window, and writes only its record', async () => {
    seedOwn()
    ;(await mount()).unmount() // the main window's switch
    window.history.replaceState(null, '', '/?panel=connect')
    const { container } = await mount({ hostBar: true, autoRotate: true })
    expect(now(container)).toBe('Frame + bar (default)')
    const OWN_LAYOUTS = ['nexus.connect.config.connect', 'nexus.panels.connect.connect', 'nexus.connect.railWidths.connect', 'nexus.connect.bar.connect']
    for (const own of OWN_LAYOUTS) expect(localStorage.getItem(own), `${own}: still reading the main window's`).toBeNull()
    // It showed the main window's layout before the update too, so that is its way back.
    expect(stored('nexus.connect.switch.connect')).toEqual(stored('nexus.connect.switch.main'))
    expect(choices(container)).toEqual(PICKS_KEPT)
    // The host draws the bar; the view draws no second one.
    expect(bar(container)).toBeNull()
  })

  it('after the main window went back to its layout, a dashboard window opened for the first time follows it: no second switch', async () => {
    seedOwn()
    const main = await mount()
    pick(main.container, 'Your earlier layout')
    main.unmount()
    window.history.replaceState(null, '', '/?panel=connect')
    const { container } = await mount({ hostBar: true, autoRotate: true })
    expect(now(container)).toBe('Your earlier layout')
    expect(localStorage.getItem('nexus.connect.config.connect')).toBeNull()
  })

  it('a dashboard window with a layout of its own is switched once too, and keeps its own', async () => {
    ;(await mount()).unmount()
    seedOwn('connect')
    window.history.replaceState(null, '', '/?panel=connect')
    const { container } = await mount({ hostBar: true, autoRotate: true })
    expect(now(container)).toBe('Frame + bar (default)')
    expect(stored('nexus.connect.config.connect').slots).toEqual(A.slots)
    pick(container, 'Your earlier layout')
    expect(stored('nexus.connect.config.connect')).toMatchObject({ slots: OWN_SLOTS, tabs: OWN_TABS })
    expect(stored('nexus.connect.railWidths.connect')).toEqual(OWN_RAILS)
    // The main window's record is the main window's: never written from here.
    expect(stored('nexus.connect.switch.main')).toEqual({ v: 1, kept: null })
  })
})

describe('the TV page: its own Frame + bar, without the boxes it can never fill', () => {
  // The previous build's seed (tv/main.tsx), as every TV browser that has opened the page holds it.
  const OLD_SEED = { left1: 'advisory', left2: 'bandTiles', right1: 'insights', right2: 'kpOutlook', bottom1: 'openings', bottom2: 'spacewx', bottom3: 'beacons' }
  const tv = { hostBar: true, autoRotate: true, presets: TV_PRESETS }

  it('a TV browser that never opened the page starts in it', async () => {
    const { container } = await mount(tv)
    expect(now(container)).toBe('Frame + bar (default)')
    expect(onScreen(container)).toEqual({ left1: 'bandTiles', left2: 'openings', right1: 'spacewx', right2: 'getout' })
    expect(tabNames(container, 'right1')).toEqual(['Space Wx', 'Kp outlook'])
    expect(stored('nexus.connect.config').slots).toEqual(TV_FRAME_BAR.slots)
    expect(choices(container)).toEqual(PICKS)
  })

  it('one that already had the old seed switches once and keeps it, one tap back', async () => {
    localStorage.setItem('nexus.connect.config', JSON.stringify({ slots: OLD_SEED, overlays: {} }))
    // A TV that has opened the page before has run the one-time Chase promotion (connectConfig) already.
    localStorage.setItem('nexus.connect.chaseDefault.v1', '1')
    const { container } = await mount(tv)
    expect(now(container)).toBe('Frame + bar (default)')
    pick(container, 'Your earlier layout')
    expect(onScreen(container)).toEqual(OLD_SEED)
  })
})
