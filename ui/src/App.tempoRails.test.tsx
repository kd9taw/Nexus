// @vitest-environment jsdom
//
// THE TEMPO RAIL DIVIDERS (layout L1, PaneSeam) — the two separators between Tempo's stations
// rail, its conversation and its waterfall rail, mounted where they really render: inside App's
// three-pane workspace. They were pointer-only (`tabIndex −1`, no value); a keyboard or
// screen-reader operator could not resize either rail.
//
// The widths are App's own (usePaneWidths publishes them on <html>), so this mounts the real App
// on the Tempo view rather than the divider alone. The window is 1920 px and the UI scale pinned to
// 100 %, so the numbers are plain: the stations rail runs 220–768 px (≤ 40 %), the waterfall rail
// 260–1152 px (≤ 60 %), both start at their defaults (346 / 422), and the two together share
// 1920 − 160 − 360 = 1400 px, so the conversation between them never drops below 360 (L1-1).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor, fireEvent } from '@testing-library/react'
import type { AppSnapshot } from './types'

const snapshot = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  mode: 'Normal',
  radio: {
    dialMhz: 14.078,
    band: '20m',
    catOk: true,
    sideband: 'USB',
    transmitting: false,
    txEnabled: false,
    txAllowed: true,
    rxOffsetHz: 1500,
    txOffsetHz: 1500,
    txLevel: 0.5,
    slot: 0,
  },
  aiCw: { enabled: false, status: '', text: '' },
  link: { tier: 'TempoFast', periodSecs: 15, snrDb: -8, dtSec: 0.1, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
  stations: [],
  conversations: [],
  activePeer: null,
  qso: null,
  fieldDay: null,
  recentDecodes: [],
  harqRescues: 0,
} as unknown as AppSnapshot

// Every export auto-stubbed from the real module (App.js8workspace.test.tsx's pattern); the few App
// needs a SHAPE from on mount are given one.
vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    // A computer running its own station: the backend answers None, and the banner asks once. The
    // catch-all's `{}` would read as driving a remote station, asked every half second.
    clientStatus: vi.fn(async () => null),
    askLog: vi.fn(async () => {
      throw new Error('no log in this test')
    }),
    getSnapshot: vi.fn(async () => snapshot),
    subscribeSnapshot: vi.fn(() => () => {}),
    getAwards: vi.fn(async () => ({ achievements: [] })),
    getJourney: vi.fn(async () => ({ firsts: [], feats: [], ladders: [] })),
    getSettings: vi.fn(async () => null),
    getBandPlan: vi.fn(async () => []),
    getLicensedBandPlan: vi.fn(async () => []),
    getFdRuleset: vi.fn(async () => null),
    logOperators: vi.fn(async () => []),
    logActivations: vi.fn(async () => []),
    radioLaunchInfo: vi.fn(async () => ({ showPicker: false })),
    uiStateLoad: vi.fn(async () => ({})),
    uiStateSave: vi.fn(async () => ({})),
    getAllSpots: vi.fn(async () => []),
    getNeedAlerts: vi.fn(async () => []),
    getPropagation: vi.fn(async () => null),
    getFeedHealth: vi.fn(async () => null),
    getXrayNow: vi.fn(async () => null),
    getDxpedWindows: vi.fn(async () => []),
    getSatSchedule: vi.fn(async () => []),
    getSatTrackStatus: vi.fn(async () => null),
    getIssPass: vi.fn(async () => null),
    getTleStatus: vi.fn(async () => null),
    setOperatingMode: vi.fn(async () => snapshot),
    setArea: vi.fn(async () => snapshot),
    appVersion: vi.fn(async () => '0.0.0-test'),
  }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
// The stub keeps the real root's class and ref: Tempo's strip divider sizes the Waterfall's own root (2026-10-26).
vi.mock('./components/Waterfall', () => ({
  Waterfall: ({ stripRef }: { stripRef?: import('react').Ref<HTMLDivElement> }) => <div className="waterfall-wrap" ref={stripRef} data-testid="waterfall" />,
}))

import App from './App'

// THE BUDGET (2026-10-04). The App this file mounts is real work, and it scales with the CPU a test gets: the
// slowest test takes 0.47 s on a quiet box, 2.3–2.4 s with a fifth of a CPU and 4.5–4.9 s with a tenth, against
// vitest's 5 s default. 15 s is over twice the tenth; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const html = document.documentElement
const rail = (side: 'left' | 'right') => html.style.getPropertyValue(side === 'left' ? '--left-rail-w' : '--right-rail-w')
const sep = (side: 'left' | 'right') =>
  document.querySelector<HTMLElement>(`.layout[data-three-pane] > .pane-splitter.${side}`)!
const aria = (el: HTMLElement) => ['aria-valuenow', 'aria-valuemin', 'aria-valuemax'].map((a) => el.getAttribute(a))

beforeEach(() => {
  localStorage.clear()
  html.style.removeProperty('--left-rail-w')
  html.style.removeProperty('--right-rail-w')
  Object.defineProperty(window, 'innerWidth', { value: 1920, configurable: true, writable: true })
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
afterEach(cleanup)

/** Mount App on Tempo ('chat' needs the 'msg' workspace) at a pinned 100 % UI scale. */
async function mountTempo(): Promise<void> {
  localStorage.setItem('nexus-ui-scale-mode', '100')
  localStorage.setItem('nexus.workspace', 'msg')
  window.location.hash = '#chat'
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
  await waitFor(() => expect(document.querySelector('.layout[data-three-pane]')).not.toBeNull())
}

describe('the Tempo rail dividers answer the keyboard (PaneSeam)', () => {
  it('both are focusable separators named for what they size, announcing the width in CSS px', async () => {
    await mountTempo()
    expect(sep('left').tabIndex, 'a divider only a mouse can reach').toBe(0)
    expect(sep('right').tabIndex, 'a divider only a mouse can reach').toBe(0)
    expect(sep('left').getAttribute('aria-label')).toBe('stations panel width')
    // The right rail holds Band Activity since the waterfall became a strip across the top (2026-10-26).
    expect(sep('right').getAttribute('aria-label')).toBe('band activity panel width')
    // Each ceiling is its own share of the window or the room beside the other rail, whichever is less.
    expect(aria(sep('left'))).toEqual(['346', '220', '768'])
    expect(aria(sep('right'))).toEqual(['422', '260', '1054'])
  })

  it('the arrow that points AWAY from a rail widens it — and the step is committed at once', async () => {
    await mountTempo()
    fireEvent.keyDown(sep('left'), { key: 'ArrowRight' })
    expect(rail('left')).toBe('362px')
    expect(localStorage.getItem('tempo-left-rail-w')).toBe('362')
    expect(sep('left').getAttribute('aria-valuenow')).toBe('362')
    // The waterfall rail's divider is on its LEFT edge: ArrowLeft moves it left, which widens it.
    fireEvent.keyDown(sep('right'), { key: 'ArrowLeft', shiftKey: true })
    expect(rail('right')).toBe('486px')
    expect(localStorage.getItem('tempo-right-rail-w')).toBe('486')
    fireEvent.keyDown(sep('right'), { key: 'ArrowRight' })
    expect(rail('right')).toBe('470px')
  })

  it('Home and End go to each rail’s own floor and ceiling', async () => {
    await mountTempo()
    fireEvent.keyDown(sep('left'), { key: 'End' })
    expect(rail('left')).toBe('768px')
    fireEvent.keyDown(sep('left'), { key: 'Home' })
    expect(rail('left')).toBe('220px')
    fireEvent.keyDown(sep('right'), { key: 'End' })
    expect(rail('right')).toBe('1152px')
  })

  it('End on both rails stops where the conversation would drop below its floor (L1-1)', async () => {
    await mountTempo()
    fireEvent.keyDown(sep('left'), { key: 'End' })
    fireEvent.keyDown(sep('right'), { key: 'End' })
    // 768 + 632 = 1400: the conversation keeps exactly its 360 px.
    expect([rail('left'), rail('right')]).toEqual(['768px', '632px'])
    expect(sep('right').getAttribute('aria-valuemax'), 'the ceiling it announces is the room it has').toBe('632')
    // …and the stations rail moving back gives the waterfall rail its room again.
    fireEvent.keyDown(sep('left'), { key: 'Home' })
    expect(sep('right').getAttribute('aria-valuemax')).toBe('1152')
  })

  it('Backspace or a double-click resets THAT rail only — the other keeps its width', async () => {
    await mountTempo()
    fireEvent.keyDown(sep('left'), { key: 'End' })
    fireEvent.keyDown(sep('right'), { key: 'End' })
    fireEvent.keyDown(sep('left'), { key: 'Backspace' })
    expect(rail('left'), 'the stations rail is back at its default').toBe('346px')
    expect(rail('right'), 'resetting one rail moved the other').toBe('632px')
    fireEvent.doubleClick(sep('right'))
    expect(rail('right')).toBe('422px')
    expect(rail('left')).toBe('346px')
  })

  it('widths stored by an earlier build are restored where they were left', async () => {
    localStorage.setItem('tempo-left-rail-w', '300')
    localStorage.setItem('tempo-right-rail-w', '400')
    await mountTempo()
    expect(rail('left')).toBe('300px')
    expect(rail('right')).toBe('400px')
    expect(sep('left').getAttribute('aria-valuenow')).toBe('300')
    expect(sep('right').getAttribute('aria-valuenow')).toBe('400')
  })

  it('a drag commits ONCE, on release — the moves only paint', async () => {
    await mountTempo()
    const writes = vi.spyOn(Storage.prototype, 'setItem')
    const railWrites = () => writes.mock.calls.filter(([k]) => k === 'tempo-left-rail-w').length
    fireEvent.pointerDown(sep('left'), { clientX: 300, pointerId: 1, button: 0 })
    fireEvent.pointerMove(window, { clientX: 320, pointerId: 1 })
    fireEvent.pointerMove(window, { clientX: 340, pointerId: 1 })
    expect(rail('left'), 'the drag did not paint the rail live').toBe('386px')
    expect(railWrites(), 'a move wrote the stored width').toBe(0)
    fireEvent.pointerUp(window, { clientX: 350, pointerId: 1 })
    expect(railWrites()).toBe(1)
    expect(localStorage.getItem('tempo-left-rail-w')).toBe('396')
    writes.mockRestore()
  })
})

// ── THE WATERFALL STRIP (2026-10-26: "width and height to be consistent") ───────────────────────────
// Every mode's waterfall starts at the same full width and 25 % of its shell; Tempo's left the right rail
// for a strip across the top of the workspace, sized by its own divider as FT's is, stored per surface.
describe('Tempo’s waterfall is a full-width strip with a height divider', () => {
  const grid = () => document.querySelector<HTMLElement>('.layout[data-three-pane]')!
  // The keep-alive cockpits (hidden hosts) draw waterfalls of their own: Tempo's is the one in its grid.
  const strip = () => grid().querySelector<HTMLElement>('[data-testid="waterfall"]')!
  const wfSep = () => document.querySelector<HTMLElement>('.layout[data-three-pane] > .pane-splitter.tempo-wf-seam')!
  function layOut(height: number) {
    const real = HTMLElement.prototype.getBoundingClientRect
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
      if (this.matches('.layout[data-three-pane]')) return { top: 0, left: 0, width: 1600, height, right: 1600, bottom: height, x: 0, y: 0, toJSON: () => ({}) } as DOMRect
      return real.call(this)
    })
  }
  afterEach(() => vi.restoreAllMocks())

  it('the waterfall is the grid’s own strip, not in the right rail — Band Activity is', async () => {
    await mountTempo()
    expect(strip().parentElement, 'the waterfall is not a strip of the workspace').toBe(grid())
    expect(document.querySelector('.right-rail [data-testid="waterfall"]'), 'the waterfall is still in the right rail').toBeNull()
    expect(grid().querySelectorAll('[data-testid="waterfall"]'), 'one waterfall on Tempo').toHaveLength(1)
    expect(document.querySelector('.right-rail')!.textContent, 'Band Activity left the right rail').toMatch(/Band Activity/)
    expect(wfSep().getAttribute('aria-label')).toBe('waterfall height')
    expect(wfSep().getAttribute('role')).toBe('separator')
  })

  it('starts at 25 % of the workspace, the common default, and a step is stored as the operator’s', async () => {
    layOut(1000)
    await mountTempo()
    expect(aria(wfSep())).toEqual(['250', '128', '346'])
    expect(grid().style.getPropertyValue('--tempo-wf-h')).toBe('25%')
    expect(localStorage.getItem('nexus.split.tempo.waterfall'), 'an untouched default writes nothing').toBeNull()
    fireEvent.keyDown(wfSep(), { key: 'ArrowDown' })
    expect(localStorage.getItem('nexus.split.tempo.waterfall')).toBe(String((266 / 1000) * 100))
    fireEvent.keyDown(wfSep(), { key: 'Backspace' })
    expect(grid().style.getPropertyValue('--tempo-wf-h')).toBe('25%')
  })

  it('a height the operator stored wins over the default, and loading never rewrites it', async () => {
    localStorage.setItem('nexus.split.tempo.waterfall', '30')
    layOut(1000)
    await mountTempo()
    expect(wfSep().getAttribute('aria-valuenow')).toBe('300')
    expect(grid().style.getPropertyValue('--tempo-wf-h')).toBe('30%')
    expect(localStorage.getItem('nexus.split.tempo.waterfall')).toBe('30')
  })
})
