// @vitest-environment jsdom
//
// THE CONNECT POP-OUT IS A DASHBOARD WINDOW: the clock and space-weather bar
// across its top — the station from the shared snapshot, the day's indices from the
// propagation poll the window already runs — with Connect under it, and the "Stay behind"
// toggle in the bar where the platform offers it. No other pop-out grows one.
//
// ConnectView is stubbed: it has its own suites, and what is under test is what this router
// puts around it.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'

vi.mock('./components/ConnectView', () => ({
  ConnectView: () => <main className="layout single" data-testid="connect-view" />,
}))
vi.mock('./components/NeededPanel', () => ({
  NeededPanel: () => <div className="panel" data-testid="needed" />,
}))
vi.mock('./useFeatures', () => ({
  readEnabledModes: vi.fn(() => ({ cw: true, phone: true })),
}))

const fx = vi.hoisted(() => ({
  SNAP: {
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    stations: [],
    activePeer: null,
    conversations: [],
    link: { tier: 'FT8' },
    radio: { amp: null, band: '20m' },
  },
  PROP: {
    advisory: { headline: '', bands: [], banners: [] },
    openings: [],
    dxpeditions: { workableNow: [], upcoming: [] },
    spaceWx: { sfi: 97, kp: 2, aIndex: 7, xrayClass: 'B3.1-class', flare: false, solarWind: null },
    source: 'live',
    asOf: 0,
  },
}))

vi.mock('./api', () => ({
  subscribeSnapshot: vi.fn((cb: (s: unknown) => void) => {
    cb(fx.SNAP)
    return () => {}
  }),
  getBandPlan: vi.fn(() => Promise.resolve([])),
  getPropagation: vi.fn(() => Promise.resolve(fx.PROP)),
  getNeedAlerts: vi.fn(() => Promise.resolve([])),
  // The Connect window polls the spot list for its Spots box (the band-map pop-outs' 15 s poll).
  getAllSpots: vi.fn(() => Promise.resolve([])),
  getSettings: vi.fn(() => Promise.resolve(null)),
  selectPeer: vi.fn(() => Promise.resolve(null)),
  pointRotatorAtCall: vi.fn(() => Promise.resolve(null)),
  workSpot: vi.fn(() => Promise.resolve(null)),
  setFrequency: vi.fn(() => Promise.resolve(null)),
  getWindowBehind: vi.fn(() => Promise.resolve({ supported: true, on: false })),
  setWindowBehind: vi.fn((on: boolean) => Promise.resolve({ supported: true, on })),
}))

import { DetachedPanel } from './DetachedPanel'
import { getWindowBehind, setWindowBehind } from './api'

async function mount(panel: string) {
  let r!: ReturnType<typeof render>
  await act(async () => {
    r = render(<DetachedPanel panel={panel} />)
  })
  return r
}

beforeEach(() => {
  vi.mocked(getWindowBehind).mockClear()
  vi.mocked(setWindowBehind).mockClear()
})
afterEach(() => cleanup())

describe('the Connect pop-out', () => {
  it('wears the dashboard bar across its top, above Connect, inside the zoomed window tree', async () => {
    const { container } = await mount('connect')
    const shell = container.querySelector('.app.detached') as HTMLElement
    const bar = shell.querySelector(':scope > .dash-bar') as HTMLElement
    expect(bar, 'the bar is a child of the window tree').not.toBeNull()
    const connect = screen.getByTestId('connect-view')
    // Above Connect, not beside or under it.
    expect(bar.compareDocumentPosition(connect) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    expect(connect.parentElement).toBe(shell)
  })

  it('shows the station from the shared snapshot and the indices from its own propagation poll', async () => {
    await mount('connect')
    const bar = document.querySelector('.dash-bar') as HTMLElement
    expect(bar.querySelector('.dash-call')?.textContent).toBe('KD9TAW')
    expect(bar.querySelector('.dash-grid')?.textContent).toBe('EN52')
    const sfi = [...bar.querySelectorAll('.dash-index')].find((li) => li.querySelector('.dash-index-k')?.textContent === 'SFI')
    expect(sfi?.querySelector('.dash-index-v')?.textContent).toBe('97')
  })

  it('carries the Stay behind toggle, which asks the window to stay behind', async () => {
    await mount('connect')
    const btn = screen.getByRole('button', { name: 'Stay behind' })
    expect(btn.closest('.dash-bar'), 'in the bar').not.toBeNull()
    await act(async () => {
      fireEvent.click(btn)
    })
    expect(vi.mocked(setWindowBehind)).toHaveBeenCalledWith(true)
    expect(btn.getAttribute('aria-pressed')).toBe('true')
  })

  it('no toggle where the platform does not offer staying behind', async () => {
    vi.mocked(getWindowBehind).mockResolvedValueOnce({ supported: false, on: false })
    await mount('connect')
    expect(document.querySelector('.dash-bar'), 'control: the bar is there').not.toBeNull()
    expect(screen.queryByRole('button', { name: 'Stay behind' })).toBeNull()
  })
})

describe('the other pop-outs', () => {
  it('grow no bar and never ask about staying behind', async () => {
    await mount('needed')
    expect(screen.getByTestId('needed'), 'control: the Needed board rendered').toBeTruthy()
    expect(document.querySelector('.dash-bar')).toBeNull()
    expect(vi.mocked(getWindowBehind)).not.toHaveBeenCalled()
  })
})
