// @vitest-environment jsdom
//
// THE DASHBOARD RAIL IN THE REAL APP — the operator's picks, by what is on the screen:
//   · OFF in every operating cockpit until the operator turns it on ("nobody's cockpit narrows on
//     update");
//   · one click from ⊞ Panels or the NOW bar, remembered PER SECTION, across a relaunch;
//   · never on a small window: below `lg` there is no rail and the NOW bar offers no switch that
//     would change nothing, while the ⊞ row keeps the choice and says why nothing appears;
//   · never beside a view that is not an operating cockpit (Connect already is these boxes; Tempo's
//     conversation fits its own two rails against the whole window);
//   · a click in the rail never selects a station app-wide — the selected station is the one a CW
//     macro's `!` sends — and a Spots row in it works the spot through the board's own Work.
// The one-poll claim is DashRail.feeds.test.tsx; each cockpit's stop controls beside the rail are
// DashRail.stopLine.test.tsx.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  const { appApiAnswers } = await import('./appCockpits.testkit')
  return { ...auto, ...appApiAnswers() }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))
vi.mock('./components/MapView', () => ({ MapView: () => <div data-testid="map" /> }))

import * as api from './api'
import App from './App'
import { APP_SNAPSHOT, COCKPIT_MAIN } from './appCockpits.testkit'
import { DASH_RAIL_SECTIONS } from './features/dashRail'

// Each case mounts the real App (this file mounts it more than once per case); under the full suite's
// load that outruns vitest's default 5 s per test, which is a budget, not a claim about the app.
vi.setConfig({ testTimeout: 30_000 })

const SECTIONS_ON = { phone: true, cw: true, rtty: true, psk: true, sstv: true, aprs: true, js8: true, connect: true }

/** A window of this size (auto zoom never upscales: 1920×1080 is 100 %, `lg`). */
function windowOf(w: number, h: number): void {
  Object.defineProperty(window, 'innerWidth', { value: w, configurable: true })
  Object.defineProperty(window, 'innerHeight', { value: h, configurable: true })
}

async function mountOn(view: string, area: 'dx' | 'msg' = 'dx'): Promise<void> {
  localStorage.setItem('nexus.workspace', area)
  window.location.hash = `#${view}`
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
  // The class the rail keys on is published a frame after mount (useViewport).
  await waitFor(() => expect(document.documentElement.getAttribute('data-viewport')).not.toBeNull())
  await act(async () => {})
}

const railEl = () => document.querySelector<HTMLElement>('.dash-rail')
const shellMarked = () => document.querySelector('.shell')?.hasAttribute('data-dash-rail') ?? false
const nowSwitch = () => within(document.querySelector<HTMLElement>('.now-bar')!).queryByRole('button', { name: 'Dashboard' })
/** The visible cockpit's ⊞ Panels menu, opened. Hidden keep-alive cockpits are out of the a11y tree. */
function openPanels(): HTMLElement {
  const buttons = screen.getAllByRole('button', { name: /^⊞ Panels/ }).filter((b) => !railEl()?.contains(b))
  expect(buttons, 'the cockpit on screen has one ⊞ Panels menu').toHaveLength(1)
  fireEvent.click(buttons[0])
  return buttons[0].closest<HTMLElement>('.panels-menu')!
}
/** ModeNav's button for a section, by its visible label. */
function navTo(label: string): void {
  const btn = [...document.querySelectorAll<HTMLButtonElement>('.mode-nav .mode-btn')].find(
    (b) => b.querySelector('.mode-label')?.textContent === label,
  )
  expect(btn, `no navigation button labelled ${label}`).toBeTruthy()
  fireEvent.click(btn!)
}

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: SECTIONS_ON }))
  document.documentElement.removeAttribute('data-viewport')
  windowOf(1920, 1080)
  vi.mocked(api.selectPeer).mockClear()
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) =>
    ({
      matches: false,
      media: q,
      addEventListener() {},
      removeEventListener() {},
      addListener() {},
      removeListener() {},
    }) as unknown as MediaQueryList) as typeof window.matchMedia
})

describe('off until the operator turns it on', () => {
  it.each(DASH_RAIL_SECTIONS.map((s) => [s]))('%s: no rail on arrival, and the NOW bar offers it', async (section) => {
    await mountOn(section)
    expect(document.documentElement.getAttribute('data-viewport'), 'this window is lg').toBe('lg')
    // The cockpit really mounted: a crash panel beside no rail would pass everything below.
    expect(document.querySelector(COCKPIT_MAIN[section]), 'the cockpit did not render').not.toBeNull()
    expect(document.querySelector('.view-crash')).toBeNull()
    expect(railEl(), 'a rail appeared that nobody turned on').toBeNull()
    expect(shellMarked()).toBe(false)
    expect(nowSwitch()?.getAttribute('aria-pressed')).toBe('false')
  })
})

describe('one click from ⊞ Panels or the NOW bar, remembered per section', () => {
  it('the ⊞ Panels row turns it on for this section only, and it comes back after a relaunch', async () => {
    await mountOn('cw')
    const menu = openPanels()
    const row = within(menu).getByRole('checkbox', { name: 'Dashboard rail' })
    expect((row as HTMLInputElement).checked).toBe(false)
    fireEvent.click(row)
    await waitFor(() => expect(railEl()).not.toBeNull())
    expect(shellMarked(), 'the shell does not say the rail is taking width').toBe(true)
    expect(JSON.parse(localStorage.getItem('nexus.dashrail.sections')!)).toEqual({ cw: true })
    navTo('Phone')
    await waitFor(() => expect(railEl(), 'the rail followed to a section it was never turned on for').toBeNull())
    navTo('CW')
    await waitFor(() => expect(railEl()).not.toBeNull())
    cleanup()
    await mountOn('cw')
    expect(railEl(), 'the choice did not survive a relaunch').not.toBeNull()
  })

  it('the NOW bar’s switch shows and hides it, and says which', async () => {
    await mountOn('phone')
    fireEvent.click(nowSwitch()!)
    await waitFor(() => expect(railEl()).not.toBeNull())
    expect(nowSwitch()?.getAttribute('aria-pressed')).toBe('true')
    fireEvent.click(nowSwitch()!)
    await waitFor(() => expect(railEl()).toBeNull())
    expect(nowSwitch()?.getAttribute('aria-pressed')).toBe('false')
  })

  it('its own ✕ turns it off for this section', async () => {
    localStorage.setItem('nexus.dashrail.sections', JSON.stringify({ rtty: true }))
    await mountOn('rtty')
    expect(railEl()).not.toBeNull()
    fireEvent.click(within(railEl()!).getByRole('button', { name: 'Hide the dashboard rail' }))
    await waitFor(() => expect(railEl()).toBeNull())
    expect(JSON.parse(localStorage.getItem('nexus.dashrail.sections')!)).toEqual({ rtty: false })
  })
})

describe('each cockpit keeps its own rail, and each starts from today’s', () => {
  // The operator's "Per cockpit" (2026-10-07): FT's rail can differ from Phone's, and each starts from the
  // rail every cockpit shared before. The records are App's (components/DashRail `useDashRail`).
  const railBoxes = () => [...railEl()!.querySelectorAll<HTMLElement>('.dash-rail-col > .pane-frame')].map((f) => f.dataset.pane)
  it('a pick and a close in Phone’s rail stay in Phone’s; CW’s still shows today’s rail, and both survive a relaunch', async () => {
    localStorage.setItem('nexus.dashrail.sections', JSON.stringify({ phone: true, cw: true }))
    localStorage.setItem('nexus.dashrail.config', JSON.stringify({ slots: { rail1: 'clock', rail2: 'spacewx', rail3: 'needed', rail4: 'getout' } }))
    await mountOn('phone')
    await waitFor(() => expect(railEl()).not.toBeNull())
    expect(railBoxes(), 'Phone’s first open is not today’s rail').toEqual(['clock', 'spacewx', 'needed', 'getout'])
    fireEvent.change(railEl()!.querySelectorAll('select.pane-pick')[1], { target: { value: 'selection' } })
    fireEvent.click(within(railEl()!.querySelector<HTMLElement>('[data-pane="getout"]')!).getByRole('button', { name: /^Hide/ }))
    await act(async () => {})
    expect(railBoxes()).toEqual(['clock', 'selection', 'needed'])
    navTo('CW')
    await waitFor(() => expect(document.querySelector('main.cw-cockpit')).not.toBeNull())
    await waitFor(() => expect(railBoxes()).toEqual(['clock', 'spacewx', 'needed', 'getout']))
    cleanup()
    await mountOn('phone')
    await waitFor(() => expect(railEl()).not.toBeNull())
    expect(railBoxes(), 'Phone’s own rail did not survive a relaunch').toEqual(['clock', 'selection', 'needed'])
  })
})

describe('never on a small window', () => {
  it('below lg: no rail, no NOW bar switch, and the ⊞ row keeps the choice and says why (a cockpit with no box columns)', async () => {
    localStorage.setItem('nexus.dashrail.sections', JSON.stringify({ rtty: true }))
    windowOf(1280, 800) // 85 % → 1506 effective px: md
    await mountOn('rtty')
    expect(document.documentElement.getAttribute('data-viewport')).toBe('md')
    expect(railEl(), 'the rail rendered below lg').toBeNull()
    expect(shellMarked()).toBe(false)
    expect(nowSwitch(), 'the NOW bar offers a switch that would change nothing').toBeNull()
    const menu = openPanels()
    const row = within(menu).getByRole('checkbox', { name: 'Dashboard rail' }) as HTMLInputElement
    expect(row.checked, 'the choice was lost on a small window').toBe(true)
    expect(row.getAttribute('aria-describedby')).toBeTruthy()
    expect(menu.textContent).toContain('Needs a larger window')
  })
})

describe('on a window too small for the rail, its boxes stand in the cockpit’s columns', () => {
  // The operator's "They move into the columns" (2026-10-07): beside FT, Phone, CW and JS8, below `lg` the rail's
  // boxes stand at the foot of the column the cockpit's own boxes stand in until placed, until the window is wide
  // enough again; the record is never rewritten by a width. Their picker is the rail's; they carry no ✕ (a slot
  // closed there could come back only from the rail's own ⊞, which a window this size does not show).
  const SLOTS = { rail1: 'clock', rail2: 'spacewx', rail3: 'needed', rail4: 'getout' }
  const foldedIn = (main: string) =>
    [...document.querySelectorAll<HTMLElement>(`${main} .pane-frame[data-pane^="rail"]`)].map(
      (f) => `${f.dataset.pane}:${f.querySelector('[data-box]')?.getAttribute('data-box')}`,
    )
  const resize = async (w: number, h: number) => {
    windowOf(w, h)
    await act(async () => {
      window.dispatchEvent(new Event('resize'))
    })
  }

  it('CW: at the foot of the leading column, with no ✕; the NOW bar and the ⊞ row say so; the rail comes back on a larger window and folds again', async () => {
    localStorage.setItem('nexus.dashrail.sections', JSON.stringify({ cw: true }))
    localStorage.setItem('nexus.dashrail.config', JSON.stringify({ slots: SLOTS }))
    windowOf(1280, 800)
    await mountOn('cw')
    expect(document.documentElement.getAttribute('data-viewport')).toBe('md')
    expect(railEl(), 'the rail rendered below lg').toBeNull()
    await waitFor(() => expect(foldedIn('main.cw-cockpit')).toEqual(['rail1:clock', 'rail2:spacewx', 'rail3:neededBoard', 'rail4:getout']))
    const frames = [...document.querySelectorAll<HTMLElement>('main.cw-cockpit .pane-frame[data-pane^="rail"]')]
    const lead = frames[0].closest('.cockpit-col')!
    expect(lead, 'not in the leading column').toBe(document.querySelector('main.cw-cockpit .cockpit-panes > .cockpit-col'))
    expect(frames.every((f) => f.parentElement === lead), 'the boxes are spread over the columns').toBe(true)
    expect([...lead.children].slice(-4), 'not at the foot of the column').toEqual(frames)
    for (const f of frames) expect(within(f).queryByRole('button', { name: /^Hide/ }), 'a folded box offers a ✕').toBeNull()
    expect(nowSwitch()?.getAttribute('aria-pressed'), 'the NOW bar does not offer the switch that moves them').toBe('true')
    const menu = openPanels()
    expect((within(menu).getByRole('checkbox', { name: 'Dashboard rail' }) as HTMLInputElement).checked).toBe(true)
    expect(menu.textContent).toContain('stand at the foot of this screen')
    fireEvent.click(document.body)
    // A larger window: the rail stands beside the cockpit again, and the columns let them go.
    await resize(1366, 768)
    await waitFor(() => expect(railEl()).not.toBeNull())
    expect(foldedIn('main.cw-cockpit')).toEqual([])
    expect([...railEl()!.querySelectorAll<HTMLElement>('.dash-rail-col > .pane-frame')].map((f) => f.dataset.pane)).toEqual(['clock', 'spacewx', 'needed', 'getout'])
    await resize(1280, 800)
    await waitFor(() => expect(railEl()).toBeNull())
    expect(foldedIn('main.cw-cockpit')).toHaveLength(4)
    expect(JSON.parse(localStorage.getItem('nexus.dashrail.config')!), 'a width rewrote the rail’s record').toEqual({ slots: SLOTS })
  })

  it('FT: at the foot of the side rail, in FT’s arranged columns; turned off, FT draws today’s tree again', async () => {
    localStorage.setItem('nexus.dashrail.sections', JSON.stringify({ operate: true }))
    localStorage.setItem('nexus.dashrail.config', JSON.stringify({ slots: SLOTS }))
    windowOf(1280, 800)
    await mountOn('operate')
    const lower = () => document.querySelector<HTMLElement>('.operate-host:not([hidden]) main.operate-cockpit .cockpit-lower')!
    await waitFor(() => expect(foldedIn('.operate-host:not([hidden]) main.operate-cockpit')).toHaveLength(4))
    expect(lower().hasAttribute('data-arranged')).toBe(true)
    const side = lower().querySelector<HTMLElement>(':scope > aside.op-stack')!
    expect(side, 'FT’s side rail is not drawn').not.toBeNull()
    expect([...side.querySelectorAll<HTMLElement>(':scope > .pane-frame[data-pane^="rail"]')].map((f) => f.dataset.pane)).toEqual(['rail1', 'rail2', 'rail3', 'rail4'])
    // A pick in a folded box is the rail's: its slot takes it.
    const pick = side.querySelector<HTMLSelectElement>('.pane-frame[data-pane="rail2"] select.pane-pick')!
    fireEvent.change(pick, { target: { value: 'bandTiles' } })
    await act(async () => {})
    expect(foldedIn('.operate-host:not([hidden]) main.operate-cockpit')[1]).toBe('rail2:bandTiles')
    expect(JSON.parse(localStorage.getItem('nexus.dashrail.config')!).sections.operate.rail2).toBe('bandTiles')
    // Off from the NOW bar: the boxes go, and with nothing else arranged FT is today's tree.
    fireEvent.click(nowSwitch()!)
    await act(async () => {})
    expect(foldedIn('.operate-host:not([hidden]) main.operate-cockpit')).toEqual([])
    expect(lower().hasAttribute('data-arranged')).toBe(false)
  })

  // FT draws today's tree while the rail stands beside it and its arranged columns while the rail's boxes stand in it, so
  // crossing the line remounts FT's panes, once each way. What the operator set in them must be what they set, by value,
  // on both sides — none of it the pane's default, or a reset would look the same.
  describe('FT keeps what the operator set in its panes, each way across the line', () => {
    const station = (call: string, snr: number, heardCount: number) => ({
      call, grid: 'EN52', snr, lastHeardSlot: 0, heardCount, presence: 'heard', worked: false,
    })
    const decode = (from: string, snr: number, freqHz: number) => ({
      from, snr, dtSec: 0.2, freqHz, message: `CQ ${from} FN31`, isCq: true, directedToMe: false, worked: false, tier: 'FT8', rv: 0,
    })
    const SNAP = {
      ...APP_SNAPSHOT,
      stations: [station('K1AAA', -20, 4), station('W2BBB', -5, 5), station('N3CCC', -12, 1)],
      // Heard in this order, so Time (the default) reads K1AAA, W2BBB, N3CCC.
      recentDecodes: [decode('K1AAA', -20, 900), decode('W2BBB', -5, 1700), decode('N3CCC', -12, 1300)],
    }
    const FT = '.operate-host:not([hidden]) main.operate-cockpit'
    const ft = () => document.querySelector<HTMLElement>(FT)!
    const arranged = () => ft().querySelector('.cockpit-lower')!.hasAttribute('data-arranged')
    const firstWord = (el: Element) => el.getAttribute('aria-label')?.split(/[ ,]/)[0]
    // Band Activity is the full decode window; Rx Frequency is the compact one, with no sort.
    const bandSort = () => ft().querySelector<HTMLSelectElement>('.operate-decodes:not(.compact) .od-sort select')!
    const bandRows = () => [...ft().querySelectorAll('.operate-decodes:not(.compact) .decode-row')].map(firstWord)
    const rosterSort = () => ft().querySelector('.operate-roster .or-th.active')?.textContent
    const rosterRows = () => [...ft().querySelectorAll('.operate-roster .or-row:not(.or-header)')].map(firstWord)
    const stations = () => ft().querySelector<HTMLElement>('.station-list')!
    const stationsChip = () => within(stations()).getAllByRole('tab').find((c) => c.getAttribute('aria-selected') === 'true')?.textContent
    const stationsSearch = () => within(stations()).getByRole('searchbox', { name: 'Search stations' }) as HTMLInputElement
    const stationsRows = () =>
      within(stations()).queryAllByTitle(/^Double-click to work /).map((el) => el.getAttribute('title')!.replace('Double-click to work ', ''))
    /** Across the line to 1280×800 and back to 1366×768, checking `seen()` against what the operator set at each stop. */
    const across = async (seen: () => unknown, set: unknown) => {
      await resize(1280, 800)
      await waitFor(() => expect(foldedIn(FT)).toHaveLength(4))
      expect(arranged(), 'below lg FT draws its arranged columns').toBe(true)
      expect(seen(), 'below lg').toEqual(set)
      await resize(1366, 768)
      await waitFor(() => expect(railEl()).not.toBeNull())
      expect(arranged(), 'beside the rail FT draws today’s tree').toBe(false)
      expect(seen(), 'back beside the rail').toEqual(set)
    }

    // App takes its snapshot from each of these, and the area sync and the mode assert run on load.
    const answer = (snap: unknown) => {
      for (const f of [api.getSnapshot, api.setArea, api.setOperatingMode]) vi.mocked(f).mockImplementation(async () => snap as never)
    }

    beforeEach(() => {
      answer(SNAP)
      localStorage.setItem('nexus.dashrail.sections', JSON.stringify({ operate: true }))
      localStorage.setItem('nexus.dashrail.config', JSON.stringify({ slots: SLOTS }))
      windowOf(1366, 768)
    })
    afterEach(() => answer(APP_SNAPSHOT))

    it('Roster: the Call Roster’s sort and Band Activity’s', async () => {
      localStorage.setItem('nexus.operateLayout', 'roster')
      await mountOn('operate')
      await waitFor(() => expect(railEl()).not.toBeNull())
      expect(arranged()).toBe(false)
      await waitFor(() => expect(rosterRows()).toHaveLength(3))
      expect(rosterSort(), 'the Call Roster’s default').toBe('Need ▼')
      expect(bandSort().value, 'Band Activity’s default').toBe('time')
      // Call, then Call again: descending.
      fireEvent.click(within(ft().querySelector<HTMLElement>('.operate-roster')!).getByRole('button', { name: /^Call( [▲▼])?$/ }))
      fireEvent.click(within(ft().querySelector<HTMLElement>('.operate-roster')!).getByRole('button', { name: /^Call( [▲▼])?$/ }))
      fireEvent.change(bandSort(), { target: { value: 'snr' } })
      const seen = () => ({ roster: rosterSort(), rosterRows: rosterRows(), band: bandSort().value, bandRows: bandRows() })
      const set = { roster: 'Call ▼', rosterRows: ['W2BBB', 'N3CCC', 'K1AAA'], band: 'snr', bandRows: ['W2BBB', 'N3CCC', 'K1AAA'] }
      expect(seen(), 'the operator’s picks did not take').toEqual(set)
      await across(seen, set)
    })

    it('Classic: Band Activity’s sort, and the Stations list’s chip and search', async () => {
      localStorage.setItem('nexus.operateLayout', 'classic')
      await mountOn('operate')
      await waitFor(() => expect(railEl()).not.toBeNull())
      expect(arranged()).toBe(false)
      await waitFor(() => expect(stationsRows()).toHaveLength(3))
      expect(stationsChip(), 'the Stations list’s default').toBe('All')
      fireEvent.change(bandSort(), { target: { value: 'freq' } })
      fireEvent.click(within(stations()).getByRole('tab', { name: 'Beaconing' }))
      fireEvent.change(stationsSearch(), { target: { value: 'W2' } })
      const seen = () => ({ band: bandSort().value, bandRows: bandRows(), chip: stationsChip(), search: stationsSearch().value, rows: stationsRows() })
      const set = { band: 'freq', bandRows: ['K1AAA', 'N3CCC', 'W2BBB'], chip: 'Beaconing', search: 'W2', rows: ['W2BBB'] }
      expect(seen(), 'the operator’s picks did not take').toEqual(set)
      await across(seen, set)
    })
  })
})

describe('only beside an operating cockpit', () => {
  it.each([
    ['connect', 'dx'],
    ['chat', 'msg'],
    ['settings', 'dx'],
  ] as const)('%s: no rail, whatever is stored, and no switch', async (view, area) => {
    localStorage.setItem(
      'nexus.dashrail.sections',
      JSON.stringify(Object.fromEntries([...DASH_RAIL_SECTIONS, 'connect', 'chat', 'settings'].map((s) => [s, true]))),
    )
    await mountOn(view, area)
    expect(railEl()).toBeNull()
    expect(nowSwitch()).toBeNull()
  })
})

describe('a click in the rail never changes the station the cockpit is working', () => {
  it('a Getting Out row selects inside the rail only — no app-wide select, which a CW macro would send', async () => {
    localStorage.setItem('nexus.dashrail.sections', JSON.stringify({ cw: true }))
    // The Selection box in the top slot, so the rail's own selection is visible.
    localStorage.setItem(
      'nexus.dashrail.config',
      JSON.stringify({ slots: { rail1: 'selection', rail2: 'bandTiles', rail3: 'spacewx', rail4: 'getout' } }),
    )
    await mountOn('cw')
    const rail = railEl()!
    await waitFor(() => expect(within(rail).getByText('K1ABC')).toBeTruthy())
    vi.mocked(api.selectPeer).mockClear()
    fireEvent.click(within(rail).getByText('K1ABC'))
    await act(async () => {})
    expect(api.selectPeer, 'a click in the rail selected a station app-wide').not.toHaveBeenCalled()
    expect(rail.querySelector('[data-pane="selection"] .cs-call')?.textContent).toBe('K1ABC')
  })

  it('a Spots row selects inside the rail too, then works the spot through the board’s own Work, keying nothing', async () => {
    // A row click on the Spots board is a select and a Work. In the rail the select is the rail's own;
    // the Work is the board's (a QSY and its cockpit), after which that cockpit arms the station it
    // was handed, as a Work from every board does.
    const K1CW = {
      call: 'K1CW', entity: 'United States', zone: 5, state: null, band: '20m', freqMhz: 14.025, mode: 'CW',
      submode: 'CW', spotter: 'W3LPL', corroborators: [], ageSecs: 30, comment: '', licensed: true, spotterLocal: true,
    }
    vi.mocked(api.getAllSpots).mockResolvedValue([K1CW] as unknown as Awaited<ReturnType<typeof api.getAllSpots>>)
    try {
      localStorage.setItem('nexus.dashrail.sections', JSON.stringify({ cw: true }))
      localStorage.setItem(
        'nexus.dashrail.config',
        JSON.stringify({ slots: { rail1: 'selection', rail2: 'spots', rail3: 'spacewx', rail4: 'getout' } }),
      )
      await mountOn('cw')
      const rail = railEl()!
      const row = await waitFor(() => {
        const r = [...rail.querySelectorAll<HTMLElement>('[data-pane="spots"] .sp-row')].find(
          (x) => x.querySelector('.np-call')?.textContent === 'K1CW',
        )
        if (!r) throw new Error('the rail’s Spots box does not list the spot')
        return r
      })
      // The transmit verbs a Work could reach; a switch keys only when set ON (a release is a `false`).
      const VERBS = ['sendCw', 'atuTune', 'callStation', 'startCq'] as const
      const SWITCHES = ['setPtt', 'setTune', 'setTxEnabled'] as const
      for (const fn of [api.selectPeer, api.workSpot, ...VERBS.map((v) => api[v]), ...SWITCHES.map((v) => api[v])]) {
        vi.mocked(fn).mockClear()
      }
      await act(async () => {
        fireEvent.click(row)
      })
      await waitFor(() => expect(api.workSpot).toHaveBeenCalled())
      await act(async () => {})
      expect(document.querySelector('.view-crash'), 'the cockpit the Work opened crashed').toBeNull()
      expect(vi.mocked(api.workSpot).mock.calls, 'the Work is not the board’s own').toEqual([['cw', 14.025, '20m', 'K1CW', undefined]])
      const worked = vi.mocked(api.workSpot).mock.invocationCallOrder[0]
      expect(
        vi.mocked(api.selectPeer).mock.invocationCallOrder.filter((n) => n < worked),
        'the click selected the station app-wide',
      ).toEqual([])
      expect(rail.querySelector('[data-pane="selection"] .cs-call')?.textContent, 'the rail’s own selection').toBe('K1CW')
      expect(row.classList.contains('selected'), 'the rail’s Spots box marks the row it selected').toBe(true)
      for (const v of VERBS) expect(api[v], `the Work keyed through ${v}`).not.toHaveBeenCalled()
      for (const v of SWITCHES) {
        expect(vi.mocked(api[v]).mock.calls.filter(([on]) => on === true), `the Work turned ${v} on`).toEqual([])
      }
    } finally {
      vi.mocked(api.getAllSpots).mockImplementation(async () => [])
    }
  })

  it('a Needed row selects inside the rail too, then works the need through the board’s own Work, keying nothing', async () => {
    // The Needed board's row click is the Spots board's: a select and a Work. In the rail the select is
    // the rail's own; the Work is the board's (handleWorkNeeded: a QSY and its cockpit).
    const K1CW = {
      call: 'K1CW', entity: 'United States', band: '20m', zone: 5, tags: ['NewBand'], priority: 50,
      headline: 'New band — United States 20m', mode: 'CW', freqMhz: 14.025,
    }
    vi.mocked(api.getNeedAlerts).mockResolvedValue([K1CW] as unknown as Awaited<ReturnType<typeof api.getNeedAlerts>>)
    try {
      localStorage.setItem('nexus.dashrail.sections', JSON.stringify({ cw: true }))
      localStorage.setItem(
        'nexus.dashrail.config',
        JSON.stringify({ slots: { rail1: 'clock', rail2: 'needed', rail3: 'spacewx', rail4: 'getout' } }),
      )
      await mountOn('cw')
      const rail = railEl()!
      const row = await waitFor(() => {
        const r = [...rail.querySelectorAll<HTMLElement>('[data-pane="needed"] .np-row')].find((x) =>
          x.querySelector('.np-call')?.textContent?.includes('K1CW'),
        )
        if (!r) throw new Error('the rail’s Needed box does not list the need')
        return r
      })
      const VERBS = ['sendCw', 'atuTune', 'callStation', 'startCq'] as const
      const SWITCHES = ['setPtt', 'setTune', 'setTxEnabled'] as const
      for (const fn of [api.selectPeer, api.workSpot, ...VERBS.map((v) => api[v]), ...SWITCHES.map((v) => api[v])]) {
        vi.mocked(fn).mockClear()
      }
      await act(async () => {
        fireEvent.click(row)
      })
      await waitFor(() => expect(api.workSpot).toHaveBeenCalled())
      await act(async () => {})
      expect(document.querySelector('.view-crash'), 'the cockpit the Work opened crashed').toBeNull()
      expect(vi.mocked(api.workSpot).mock.calls, 'the Work is not the board’s own').toEqual([['cw', 14.025, '20m', 'K1CW', undefined]])
      const worked = vi.mocked(api.workSpot).mock.invocationCallOrder[0]
      expect(
        vi.mocked(api.selectPeer).mock.invocationCallOrder.filter((n) => n < worked),
        'the click selected the station app-wide',
      ).toEqual([])
      expect(row.classList.contains('selected'), 'the rail’s Needed box marks the row it selected').toBe(true)
      for (const v of VERBS) expect(api[v], `the Work keyed through ${v}`).not.toHaveBeenCalled()
      for (const v of SWITCHES) {
        expect(vi.mocked(api[v]).mock.calls.filter(([on]) => on === true), `the Work turned ${v} on`).toEqual([])
      }
    } finally {
      vi.mocked(api.getNeedAlerts).mockImplementation(async () => [])
    }
  })

  it('a POTA / SOTA box’s HUNT tags the hunt and works the activator through the board’s own path, keying nothing', async () => {
    const K9ABC = {
      program: 'POTA', reference: 'US-1000', name: 'Test park', activator: 'K9ABC', freqKhz: 14285, mode: 'SSB',
      spotter: null, comment: null, grid: null, newPark: false, bandOpen: false, huntedToday: false,
    }
    vi.mocked(api.getOtaSpots).mockImplementation((async (program: string) => (program === 'POTA' ? [K9ABC] : [])) as never)
    try {
      localStorage.setItem('nexus.dashrail.sections', JSON.stringify({ cw: true }))
      localStorage.setItem(
        'nexus.dashrail.config',
        JSON.stringify({ slots: { rail1: 'clock', rail2: 'pota', rail3: 'spacewx', rail4: 'getout' } }),
      )
      await mountOn('cw')
      const hunt = await waitFor(() => within(railEl()!).getByRole('button', { name: 'Hunt K9ABC' }))
      for (const fn of [api.selectPeer, api.workSpot, api.setHuntTarget, api.setPtt, api.setTxEnabled]) vi.mocked(fn).mockClear()
      await act(async () => {
        fireEvent.click(hunt)
      })
      await waitFor(() => expect(api.workSpot).toHaveBeenCalled())
      expect(vi.mocked(api.setHuntTarget).mock.calls, 'the hunt was not tagged as the board tags it').toEqual([['K9ABC', 'POTA', 'US-1000']])
      expect(vi.mocked(api.workSpot).mock.calls, 'the QSY is not the board’s own').toEqual([['phone', 14.285, '20m', 'K9ABC', undefined]])
      for (const v of ['setPtt', 'setTxEnabled'] as const) {
        expect(vi.mocked(api[v]).mock.calls.filter(([on]) => on === true), `the hunt turned ${v} on`).toEqual([])
      }
    } finally {
      vi.mocked(api.getOtaSpots).mockImplementation(async () => [])
    }
  })
})
