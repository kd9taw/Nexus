// @vitest-environment jsdom
//
// THE DASHBOARD RAIL, rendered: its stock boxes, a box closed and brought back from the rail's own ⊞,
// every box in the Connect registry placed in it with no transmit control among them, the width divider
// (fitted on load, moved by keys), and a crashing box costing the rail and nothing else.
// The one-poll claim with Connect beside it is DashRail.feeds.test.tsx; the rail beside the real
// cockpits (off by default, per section, the doors, the stop line) is App.dashRail.test.tsx.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { AppSnapshot, NeedAlert, OtaSpot, PropagationSnapshot, SpotRow } from '../types'

// The boards the window lends the Spots and POTA/SOTA boxes, so the sweep reads their controls too.
// No call or comment here carries a standalone "CQ" or "TX": a row's accessible name can carry its
// comment, and the sweep's words are whole words, so such a spot would fail it on data.
const fx = vi.hoisted(() => ({
  clockThrows: false,
  ota: [
    {
      program: 'POTA', reference: 'US-1000', name: 'Test park', activator: 'K9ABC', freqKhz: 14285, mode: 'SSB',
      spotter: null, comment: 'thanks for the park', grid: null, newPark: false, bandOpen: false, huntedToday: false,
    },
  ] as OtaSpot[],
}))

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
    // The POTA/SOTA box's own reads (it self-fetches, as its screen does).
    getOtaSpots: vi.fn(async (program: string) => (program === 'POTA' ? fx.ota : [])),
    getActivation: vi.fn(async () => ({ program: null, reference: null, qsoCount: 0 })),
    parksCount: vi.fn(async () => 0),
    huntedParksCount: vi.fn(async () => 0),
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

import { OwnedDashRail } from './DashRail.testkit'
import { publishDashRailSwitch } from './dashRailSwitch'
import { PANE_IDS } from '../features/connectConfig'
import { APP_SNAPSHOT } from '../appCockpits.testkit'

const LIVE: PropagationSnapshot = {
  advisory: { headline: 'Bands are fair', bands: [], banners: [] },
  openings: [],
  dxpeditions: { workableNow: [], upcoming: [] },
  spaceWx: { sfi: 97, kp: 2, aIndex: 7, xrayClass: 'B3.1-class', flare: false, solarWind: null },
  source: 'live',
  asOf: Math.floor(Date.now() / 1000),
} as unknown as PropagationSnapshot

const SPOT = {
  call: 'K1CW', entity: 'United States', zone: 5, state: null, band: '20m', freqMhz: 14.025, mode: 'CW', submode: 'CW',
  spotter: 'W3LPL', corroborators: [], ageSecs: 30, comment: 'up 1', licensed: true, spotterLocal: true,
} as unknown as SpotRow

const NEED = {
  call: 'K1CW', entity: 'United States', band: '20m', zone: 5, tags: ['NewBand'], priority: 50,
  headline: 'New band — United States 20m', mode: 'CW', freqMhz: 14.025,
} as unknown as NeedAlert

const props = (over: Partial<Parameters<typeof OwnedDashRail>[0]> = {}) => ({
  section: 'operate',
  myGrid: 'EN52',
  theme: 'dark' as const,
  stations: [],
  prop: LIVE,
  needByCall: new Map(),
  spotsFeed: { rows: [SPOT], board: { bandPlan: [], selectedCall: null, myGrid: 'EN52', onSelect: vi.fn(), onWork: vi.fn() } },
  otaBoard: { snap: APP_SNAPSHOT as unknown as AppSnapshot, onHunt: vi.fn(), onSnap: vi.fn() },
  neededBoard: { alerts: [NEED], bandPlan: [], selectedCall: null, myGrid: 'EN52', onQsy: vi.fn(), onSelect: vi.fn(), onWork: vi.fn() },
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
    render(<OwnedDashRail {...props()} />)
    expect(boxes()).toEqual(['clock', 'bandTiles', 'spacewx', 'getout'])
    expect(rail().getAttribute('aria-label')).toBe('Dashboard rail')
  })

  it('a box’s ✕ closes its slot and the rail’s own ⊞ brings the same box back', () => {
    render(<OwnedDashRail {...props()} />)
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
    render(<OwnedDashRail {...props()} />)
    fireEvent.click(within(rail()).getByRole('button', { name: /^⊞ Panels/ }))
    expect(within(rail()).queryByRole('checkbox', { name: 'Dashboard rail' }), 'the rail offers its own switch').toBeNull()
    expect(within(rail()).getByRole('checkbox', { name: 'Clock · top' })).toBeTruthy()
  })

  it('every closed slot still leaves the way back on screen', () => {
    render(<OwnedDashRail {...props()} />)
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

  // Every box, four at a time: each of them sits in the rail once, the boards with their wiring lent.
  const groups: string[][] = []
  for (let i = 0; i < PANE_IDS.length; i += 4) groups.push([...PANE_IDS.slice(i, i + 4)])

  it.each(groups.map((g) => [g.join(', '), g]))('%s', async (_name, group) => {
    const pad = PANE_IDS.filter((p) => !group.includes(p))
    const slots = [...group, ...pad].slice(0, 4)
    localStorage.setItem(
      'nexus.dashrail.config',
      JSON.stringify({ slots: { rail1: slots[0], rail2: slots[1], rail3: slots[2], rail4: slots[3] } }),
    )
    render(<OwnedDashRail {...props()} />)
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

describe('the Needed box beside a cockpit', () => {
  it('keeps the rail’s own selection: a row’s select stays in the rail, and its Work is the board’s', async () => {
    localStorage.setItem(
      'nexus.dashrail.config',
      JSON.stringify({ slots: { rail1: 'clock', rail2: 'needed', rail3: 'spacewx', rail4: 'getout' } }),
    )
    const p = props()
    render(<OwnedDashRail {...p} />)
    await act(async () => {})
    const row = [...rail().querySelectorAll<HTMLElement>('[data-pane="needed"] .np-row')].find((r) =>
      r.querySelector('.np-call')?.textContent?.includes('K1CW'),
    )
    expect(row, 'the rail’s Needed box does not list the need').toBeTruthy()
    fireEvent.click(row!)
    expect(p.neededBoard.onSelect, 'the click used the app-wide select, which a CW macro sends').not.toHaveBeenCalled()
    expect(p.neededBoard.onWork).toHaveBeenCalledWith(NEED)
    expect(row!.classList.contains('selected'), 'the rail’s own selection marks the row').toBe(true)
  })
})

describe('a box’s own text size in the rail — ⋯ ▸ A− / A+', () => {
  const frame = (slot: string) => rail().querySelector<HTMLElement>(`.pane-frame[data-slot="${slot}"]`)!
  const factor = (slot: string) => frame(slot).querySelector<HTMLElement>('.pane-body')!.style.getPropertyValue('--box-text-scale')
  // Radix opens its menu on pointerdown, and measures and captures pointers, which jsdom lacks
  // (ConnectView.boxes.test.tsx's helper).
  const larger = (slot: string) => {
    Element.prototype.hasPointerCapture = () => false
    Element.prototype.setPointerCapture = () => {}
    Element.prototype.releasePointerCapture = () => {}
    Element.prototype.scrollIntoView = () => {}
    const trigger = within(frame(slot)).getByRole('button', { name: /^Options for / })
    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false, pointerType: 'mouse' })
    fireEvent.click(within(screen.getByRole('menu')).getByRole('menuitem', { name: /Larger text/ }))
    fireEvent.keyDown(screen.getByRole('menu'), { key: 'Escape' })
  }

  it('A+ grows that box’s words and no other’s, and the rail remembers it', () => {
    render(<OwnedDashRail {...props()} />)
    expect(factor('rail3'), 'control: a box opens at the app’s size').toBe('')
    larger('rail3')
    expect(factor('rail3'), 'A+ did not reach the rail’s box').toBe('1.1')
    expect(factor('rail2'), 'A+ reached another box').toBe('')
    cleanup()
    render(<OwnedDashRail {...props()} />)
    expect(factor('rail3'), 'the size did not survive a remount').toBe('1.1')
  })

  it('the rail’s own Reset puts every box back at the app’s size, and its Undo brings the size back', () => {
    render(<OwnedDashRail {...props()} />)
    larger('rail1')
    expect(factor('rail1')).toBe('1.1')
    fireEvent.click(within(rail()).getByRole('button', { name: /^⊞ Panels/ }))
    fireEvent.click(within(rail()).getByRole('button', { name: 'Reset layout' }))
    expect(factor('rail1'), 'Reset kept the size').toBe('')
    fireEvent.click(within(rail()).getByRole('button', { name: 'Undo last change' }))
    expect(factor('rail1'), 'Undo lost the size').toBe('1.1')
  })
})

describe('the width divider', () => {
  const seam = () => within(rail()).getByRole('separator', { name: 'Dashboard rail width' })
  const width = () => rail().style.getPropertyValue('--dash-rail-w')

  it('opens at the default and moves the way the arrows point, stored as the preference', () => {
    render(<OwnedDashRail {...props()} />)
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
    render(<OwnedDashRail {...props()} />)
    expect(width()).toBe('402px')
    expect(localStorage.getItem('nexus.dashrail.width'), 'the fit rewrote the stored preference').toBe('700')
  })

  it('re-fits when the window is resized', async () => {
    localStorage.setItem('nexus.dashrail.width', '700')
    render(<OwnedDashRail {...props()} />)
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
      render(<OwnedDashRail {...props({ onHide })} />)
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
