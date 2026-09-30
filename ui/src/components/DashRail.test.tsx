// @vitest-environment jsdom
//
// THE DASHBOARD RAIL, rendered: its stock boxes, a box closed and brought back from the rail's own ⊞,
// every box in the Connect registry placed in it with no transmit control among them, the width divider
// (fitted on load, moved by keys), and a crashing box costing the rail and nothing else.
// The one-poll claim with Connect beside it is DashRail.feeds.test.tsx; the rail beside the real
// cockpits (off by default, per section, the doors, the stop line) is App.dashRail.test.tsx.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { PropagationSnapshot } from '../types'

const fx = vi.hoisted(() => ({ clockThrows: false }))

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  return {
    ...auto,
    getGettingOut: vi.fn(async () => ({
      count: 2,
      maxKm: 3200,
      reports: [
        { call: 'K1ABC', octant: 'E', km: 1500, band: '20m', snr: -12 },
        { call: 'G4XYZ', octant: 'NE', km: 3200, band: '20m', snr: -18 },
      ],
    })),
    getBandOutlook: vi.fn(async () => ({ bands: [], mufNow: 0 })),
    getSpaceWxScales: vi.fn(async () => ({ scales: { r: 0, s: 0, g: 0 }, alerts: [] })),
    getKc2gMuf: vi.fn(async () => []),
    getXrayNow: vi.fn(async () => ({ flux: 1e-7, asOf: 0 })),
    getDxpedWindows: vi.fn(async () => []),
    getKpForecast: vi.fn(async () => ({ points: [] })),
    getSolarIndices: vi.fn(async () => ({ days: [] })),
    getPathOutlook: vi.fn(async () => null),
    getDxccEntityLocations: vi.fn(async () => []),
    getOpeningsLog: vi.fn(async () => []),
    getSatellites: vi.fn(async () => null),
    getContests: vi.fn(async () => []),
    getSpectrumRow: vi.fn(() => Promise.reject(new Error('no spectrum here'))),
    // A configured rotator, so its box draws its own ■ STOP (the rotator's, never a transmitter's).
    getSettings: vi.fn(async () => ({ rotatorModel: 202, rotatorHost: '' })),
    readRotator: vi.fn(async () => 180),
    readRotatorState: vi.fn(async () => null),
    getDeclination: vi.fn(async () => null),
    getSatTrackStatus: vi.fn(async () => null),
  }
})
vi.mock('./prop/ClockPane', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./prop/ClockPane')>()
  return {
    ...actual,
    ClockPane: (p: Parameters<typeof actual.ClockPane>[0]) => {
      if (fx.clockThrows) throw new Error('a box broke')
      return actual.ClockPane(p)
    },
  }
})

import { DashRail } from './DashRail'
import { publishDashRailSwitch } from './dashRailSwitch'
import { PANE_IDS } from '../features/connectConfig'

const LIVE: PropagationSnapshot = {
  advisory: { headline: 'Bands are fair', bands: [], banners: [] },
  openings: [],
  dxpeditions: { workableNow: [], upcoming: [] },
  spaceWx: { sfi: 97, kp: 2, aIndex: 7, xrayClass: 'B3.1-class', flare: false, solarWind: null },
  source: 'live',
  asOf: Math.floor(Date.now() / 1000),
} as unknown as PropagationSnapshot

const props = (over: Partial<Parameters<typeof DashRail>[0]> = {}) => ({
  section: 'operate',
  myGrid: 'EN52',
  theme: 'dark' as const,
  stations: [],
  prop: LIVE,
  needByCall: new Map(),
  onHide: vi.fn(),
  ...over,
})

const rail = () => document.querySelector<HTMLElement>('.dash-rail')!
const boxes = () => [...rail().querySelectorAll<HTMLElement>('.dash-rail-col > .pane-frame')].map((f) => f.dataset.pane)

beforeEach(() => {
  localStorage.clear()
  fx.clockThrows = false
  Object.defineProperty(window, 'innerWidth', { value: 1920, configurable: true })
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})

describe('the stock rail', () => {
  it('shows the operator’s four boxes, top to bottom, each in its own slot', () => {
    render(<DashRail {...props()} />)
    expect(boxes()).toEqual(['clock', 'bandTiles', 'spacewx', 'getout'])
    expect(rail().getAttribute('aria-label')).toBe('Dashboard rail')
  })

  it('a box’s ✕ closes its slot and the rail’s own ⊞ brings the same box back', () => {
    render(<DashRail {...props()} />)
    fireEvent.click(within(rail()).getByRole('button', { name: 'Hide Space Wx' }))
    expect(boxes()).toEqual(['clock', 'bandTiles', 'getout'])
    fireEvent.click(within(rail()).getByRole('button', { name: /⊞ Panels · 1 hidden/ }))
    fireEvent.click(within(rail()).getByRole('checkbox', { name: 'Space Wx · lower middle' }))
    expect(boxes()).toEqual(['clock', 'bandTiles', 'spacewx', 'getout'])
  })

  it('its own ⊞ offers no "Dashboard rail" row, even while a cockpit’s switch is published', () => {
    // Its ✕ is the off switch here; a row turning the rail off from inside the rail would be a second
    // copy of it, reading like a box.
    publishDashRailSwitch({ on: true, fits: true, set: () => {} })
    render(<DashRail {...props()} />)
    fireEvent.click(within(rail()).getByRole('button', { name: /^⊞ Panels/ }))
    expect(within(rail()).queryByRole('checkbox', { name: 'Dashboard rail' }), 'the rail offers its own switch').toBeNull()
    expect(within(rail()).getByRole('checkbox', { name: 'Clock · top' })).toBeTruthy()
  })

  it('every closed slot still leaves the way back on screen', () => {
    render(<DashRail {...props()} />)
    for (const name of ['Hide Clock', 'Hide Bands for you', 'Hide Space Wx', 'Hide Getting Out']) {
      fireEvent.click(within(rail()).getByRole('button', { name }))
    }
    expect(boxes()).toEqual([])
    expect(within(rail()).getByRole('button', { name: /⊞ Panels · 4 hidden/ })).toBeTruthy()
    expect(within(rail()).getByRole('button', { name: 'Hide the dashboard rail' })).toBeTruthy()
  })
})

describe('the rail renders no transmit control, whichever box is in which slot', () => {
  // A transmit control, by its accessible name: the stop-line vocabulary and every sender's words.
  const TX = /\b(ptt|tx|transmit\w*|key(ing)?|unkey|tune|cq|send\w*|halt|abort)\b/i
  const STOP = /\bstop\b/i
  const ROLES = ['button', 'checkbox', 'switch', 'menuitem', 'radio', 'combobox', 'slider'] as const
  const controls = (name?: RegExp) =>
    ROLES.flatMap((role) => within(rail()).queryAllByRole(role, name ? { name } : undefined))

  // All 26 boxes, four at a time: every one of them sits in the rail once.
  const groups: string[][] = []
  for (let i = 0; i < PANE_IDS.length; i += 4) groups.push([...PANE_IDS.slice(i, i + 4)])

  it.each(groups.map((g) => [g.join(', '), g]))('%s', async (_name, group) => {
    const pad = PANE_IDS.filter((p) => !group.includes(p))
    const slots = [...group, ...pad].slice(0, 4)
    localStorage.setItem(
      'nexus.dashrail.config',
      JSON.stringify({ slots: { rail1: slots[0], rail2: slots[1], rail3: slots[2], rail4: slots[3] } }),
    )
    render(<DashRail {...props()} />)
    await act(async () => {})
    expect(boxes(), 'the rail did not place the boxes asked for').toEqual(slots)
    // A box that crashed would show the crash panel and no controls at all — a pass proving nothing.
    expect(rail().querySelector('.view-crash'), 'a box crashed, so the sweep would read nothing').toBeNull()
    expect(controls().length, 'the rail drew no control at all — the sweep is reading nothing').toBeGreaterThan(0)
    expect(controls(TX).map((el) => el.textContent), 'a transmit control in the dashboard rail').toEqual([])
    // A control named for a STOP may only be the rotator's, which stops the antenna: the stop line's
    // census never counts it, and it keys nothing.
    const stops = within(rail()).queryAllByRole('button', { name: STOP })
    for (const el of stops) {
      expect(el.closest('[data-pane]')?.getAttribute('data-pane'), `"${el.textContent}" is a stop outside the rotor box`).toBe('rotor')
    }
    // …and that allowance is read, not assumed: with the rotor box in the rail, its STOP is there.
    if (group.includes('rotor')) {
      await waitFor(() => expect(within(rail()).queryAllByRole('button', { name: STOP }).length).toBeGreaterThan(0))
    }
  })
})

describe('the width divider', () => {
  const seam = () => within(rail()).getByRole('separator', { name: 'Dashboard rail width' })
  const width = () => rail().style.getPropertyValue('--dash-rail-w')

  it('opens at the default and moves the way the arrows point, stored as the preference', () => {
    render(<DashRail {...props()} />)
    expect(width()).toBe('300px')
    expect(seam().getAttribute('aria-valuenow')).toBe('300')
    // Its handle is on the rail's LEFT edge: the left arrow widens the rail.
    fireEvent.keyDown(seam(), { key: 'ArrowLeft' })
    expect(width()).toBe('316px')
    expect(localStorage.getItem('nexus.dashrail.width')).toBe('316')
    fireEvent.keyDown(seam(), { key: 'End' })
    expect(seam().getAttribute('aria-valuenow')).toBe('715') // 1920 − the 1024×768 floor window
    fireEvent.keyDown(seam(), { key: 'Home' })
    expect(width()).toBe('200px')
    fireEvent.keyDown(seam(), { key: 'Backspace' })
    expect(width()).toBe('300px')
    expect(localStorage.getItem('nexus.dashrail.width')).toBe('')
  })

  it('a width stored on a wide window is fitted on a narrower one, and the preference is kept', () => {
    localStorage.setItem('nexus.dashrail.width', '700')
    Object.defineProperty(window, 'innerWidth', { value: 1607, configurable: true })
    render(<DashRail {...props()} />)
    expect(width()).toBe('402px')
    expect(localStorage.getItem('nexus.dashrail.width'), 'the fit rewrote the stored preference').toBe('700')
  })

  it('re-fits when the window is resized', async () => {
    localStorage.setItem('nexus.dashrail.width', '700')
    render(<DashRail {...props()} />)
    expect(width()).toBe('700px')
    Object.defineProperty(window, 'innerWidth', { value: 1607, configurable: true })
    act(() => {
      window.dispatchEvent(new Event('resize'))
    })
    await waitFor(() => expect(width()).toBe('402px'))
  })
})

describe('a crash in a box costs the rail, not the cockpit', () => {
  it('the rail keeps its place and width, and its way out turns the rail off', () => {
    fx.clockThrows = true
    const onHide = vi.fn()
    const err = vi.spyOn(console, 'error').mockImplementation(() => {})
    try {
      render(<DashRail {...props({ onHide })} />)
      expect(rail(), 'the crash took the rail’s box with it').not.toBeNull()
      expect(rail().style.getPropertyValue('--dash-rail-w')).toBe('300px')
      expect(screen.getByRole('alert').textContent).toContain('The dashboard rail hit an error')
      fireEvent.click(within(rail()).getByRole('button', { name: 'Hide the dashboard rail' }))
      expect(onHide).toHaveBeenCalledTimes(1)
    } finally {
      err.mockRestore()
    }
  })
})
