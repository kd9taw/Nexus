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
import { describe, it, expect, vi, beforeEach } from 'vitest'
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
import { COCKPIT_MAIN } from './appCockpits.testkit'
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

describe('never on a small window', () => {
  it('below lg: no rail, no NOW bar switch, and the ⊞ row keeps the choice and says why', async () => {
    localStorage.setItem('nexus.dashrail.sections', JSON.stringify({ cw: true }))
    windowOf(1280, 800) // 85 % → 1506 effective px: md
    await mountOn('cw')
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
