// @vitest-environment jsdom
//
// THE TEMPO RAIL DIVIDERS (layout L1, PaneSeam) — the two separators between Tempo's stations
// rail, its conversation and its waterfall rail, mounted where they really render: inside App's
// three-pane workspace. They were pointer-only (`tabIndex −1`, no value); a keyboard or
// screen-reader operator could not resize either rail.
//
// The widths are App's own (usePaneWidths publishes them on <html>), so this mounts the real App
// on the Tempo view rather than the divider alone. The UI scale is pinned to 100 % so the rails'
// ceilings are plain numbers: jsdom's window is 1024 px wide, so the stations rail runs 220–410 px
// (≤ 40 %) and the waterfall rail 260–614 px (≤ 60 %).
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
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))

import App from './App'

const html = document.documentElement
const rail = (side: 'left' | 'right') => html.style.getPropertyValue(side === 'left' ? '--left-rail-w' : '--right-rail-w')
const sep = (side: 'left' | 'right') =>
  document.querySelector<HTMLElement>(`.layout[data-three-pane] > .pane-splitter.${side}`)!
const aria = (el: HTMLElement) => ['aria-valuenow', 'aria-valuemin', 'aria-valuemax'].map((a) => el.getAttribute(a))

beforeEach(() => {
  localStorage.clear()
  html.style.removeProperty('--left-rail-w')
  html.style.removeProperty('--right-rail-w')
  Object.defineProperty(window, 'innerWidth', { value: 1024, configurable: true, writable: true })
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
    expect(sep('right').getAttribute('aria-label')).toBe('waterfall panel width')
    expect(aria(sep('left'))).toEqual(['220', '220', '410'])
    expect(aria(sep('right'))).toEqual(['260', '260', '614'])
  })

  it('the arrow that points AWAY from a rail widens it — and the step is committed at once', async () => {
    await mountTempo()
    fireEvent.keyDown(sep('left'), { key: 'ArrowRight' })
    expect(rail('left')).toBe('236px')
    expect(localStorage.getItem('tempo-left-rail-w')).toBe('236')
    expect(sep('left').getAttribute('aria-valuenow')).toBe('236')
    // The waterfall rail's divider is on its LEFT edge: ArrowLeft moves it left, which widens it.
    fireEvent.keyDown(sep('right'), { key: 'ArrowLeft', shiftKey: true })
    expect(rail('right')).toBe('324px')
    expect(localStorage.getItem('tempo-right-rail-w')).toBe('324')
    fireEvent.keyDown(sep('right'), { key: 'ArrowRight' })
    expect(rail('right')).toBe('308px')
  })

  it('Home and End go to each rail’s own floor and ceiling', async () => {
    await mountTempo()
    fireEvent.keyDown(sep('left'), { key: 'End' })
    expect(rail('left')).toBe('410px')
    fireEvent.keyDown(sep('left'), { key: 'Home' })
    expect(rail('left')).toBe('220px')
    fireEvent.keyDown(sep('right'), { key: 'End' })
    expect(rail('right')).toBe('614px')
  })

  it('Backspace or a double-click resets THAT rail only — the other keeps its width', async () => {
    await mountTempo()
    fireEvent.keyDown(sep('left'), { key: 'End' })
    fireEvent.keyDown(sep('right'), { key: 'End' })
    fireEvent.keyDown(sep('left'), { key: 'Backspace' })
    expect(rail('left'), 'the stations rail is back at its default').toBe('220px')
    expect(rail('right'), 'resetting one rail moved the other').toBe('614px')
    fireEvent.doubleClick(sep('right'))
    expect(rail('right')).toBe('260px')
    expect(rail('left')).toBe('220px')
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
    expect(rail('left'), 'the drag did not paint the rail live').toBe('260px')
    expect(railWrites(), 'a move wrote the stored width').toBe(0)
    fireEvent.pointerUp(window, { clientX: 350, pointerId: 1 })
    expect(railWrites()).toBe(1)
    expect(localStorage.getItem('tempo-left-rail-w')).toBe('270')
    writes.mockRestore()
  })
})
