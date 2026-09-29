// @vitest-environment jsdom
//
// SATELLITES' COLUMN DIVIDER (layout L7): the boundary between the planning column (the schedule,
// the frequencies) and the pass column (the pass's two maps, the log, Birds). What the view wires:
// the divider in the grid, the stored split per window, a reset, and a window that never set one.
// jsdom lays nothing out, so the two columns get stubbed widths; the divider's own key and drag
// arithmetic is PaneSeam's (PaneSeam.test.tsx), the template the cascade's (view-grids.test.ts),
// and the rectangles Chrome's (the layout harness).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react'
import { SatellitesView } from './SatellitesView'
import type { AppSnapshot, SatView } from '../types'

const api = vi.hoisted(() => ({
  getSatellites: vi.fn(),
  getSatPassNeeds: vi.fn(() => Promise.resolve([])),
  getSatDetail: vi.fn(() => new Promise(() => {})),
  getSettings: vi.fn(),
  setSettings: vi.fn(() => Promise.resolve({} as never)),
  setPegLock: vi.fn(() => Promise.resolve()),
  confirmSatUplink: vi.fn(() => Promise.resolve()),
  setSatTransponder: vi.fn(() => Promise.resolve()),
  getSatTransponder: vi.fn(() => Promise.resolve(null)),
  startSatTrack: vi.fn(() => Promise.resolve(null)),
  stopSatTrack: vi.fn(() => Promise.resolve()),
  getSatTrackStatus: vi.fn(() => Promise.resolve(null)),
  fetchTlesNow: vi.fn(() => Promise.resolve(null)),
  fdLogManual: vi.fn(async () => ({})),
  contestLogManual: vi.fn(async () => ({})),
  logQso: vi.fn(async () => ({})),
  lookupPark: vi.fn(async () => null),
  lookupParkLive: vi.fn(async () => null),
  qrzLookup: vi.fn(async () => null),
  resolveEntity: vi.fn(async () => null),
  searchParks: vi.fn(async () => []),
  setCwPeerInfo: vi.fn(async () => {}),
}))
vi.mock('../api', () => api)
vi.mock('./MapView', () => ({ MapView: () => <div data-testid="mapview-stub" /> }))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: async <T,>(action: () => Promise<T>) => action(),
}))

const NOW = Math.floor(Date.now() / 1000)
const KEY = 'nexus.sats.columns'
const PLAN_W = 480
const SIDE_W = 590

const snap = (): AppSnapshot =>
  ({
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    hunt: null,
    fieldDay: null,
    link: { tier: 'TempoFast' },
    radio: { dialMhz: 435.64, band: '70cm', rigMode: 'USB', sideband: 'USB', catOk: true, transmitting: false, txEnabled: false, txAllowed: true },
  }) as unknown as AppSnapshot

const satView = (): SatView =>
  ({
    tleAgeDays: 1,
    usableCount: 300,
    agingCount: 0,
    heldBackCount: 0,
    tleFetchedAt: NOW,
    tleSource: 'mirror',
    birds: [{ name: 'RS-44', norad: 44909, lat: 0, lon: 0, altKm: 1200, footprintKm: 4000, track: [], status: 'alive', amateur: true }],
    passes: [{ name: 'RS-44', norad: 44909, aosUnix: NOW + 600, losUnix: NOW + 1200, maxElDeg: 62, aosAzDeg: 100, losAzDeg: 260 }],
    excluded: [],
  }) as unknown as SatView

const realRect = HTMLElement.prototype.getBoundingClientRect
beforeEach(() => {
  localStorage.clear()
  api.getSatellites.mockReset()
  api.getSatellites.mockImplementation(() => Promise.resolve(satView()))
  api.getSettings.mockReset()
  api.getSettings.mockImplementation(() => Promise.resolve({ mygrid: 'EN52', rotatorModel: 0, rotatorHost: '', satDopplerOff: true, satVfoMap: 'off' }))
  HTMLElement.prototype.getBoundingClientRect = function (this: HTMLElement) {
    const box = (left: number, width: number) => ({ x: left, y: 0, left, top: 0, width, height: 700, right: left + width, bottom: 700, toJSON() {} }) as DOMRect
    if (this.classList.contains('sats-plan')) return box(0, PLAN_W)
    if (this.classList.contains('sats-side')) return box(PLAN_W + 8, SIDE_W)
    return realRect.call(this)
  }
})
afterEach(() => {
  cleanup()
  HTMLElement.prototype.getBoundingClientRect = realRect
})

async function mount() {
  const r = render(<SatellitesView snap={snap()} />)
  await waitFor(() => expect(document.querySelector('.sats-plan')).not.toBeNull())
  return r.container
}

const view = (c: HTMLElement) => c.querySelector<HTMLElement>('.sats-view')!
const divider = () => screen.getByRole('separator', { name: 'Schedule column / pass column' })
const token = (c: HTMLElement, v: 'a' | 'b') => view(c).style.getPropertyValue(`--sats-col-${v}`)

describe('Satellites’ column divider', () => {
  it('is the grid’s own child between the planning and pass columns, announcing the split on screen', async () => {
    const c = await mount()
    const d = divider()
    expect(d.parentElement).toBe(view(c))
    expect(d.className).toContain('sats-colseam')
    expect(d.getAttribute('aria-orientation')).toBe('vertical')
    // 480 of 1070: 45 %, between the two columns' 260 px floors (24 % … 76 %).
    await waitFor(() => expect(d.getAttribute('aria-valuenow')).toBe('45'))
    expect([d.getAttribute('aria-valuemin'), d.getAttribute('aria-valuemax')]).toEqual(['24', '76'])
  })

  it('a window that never set a split opens exactly as before: no tokens on the grid', async () => {
    const c = await mount()
    expect(token(c, 'a')).toBe('')
    expect(token(c, 'b')).toBe('')
    expect(localStorage.getItem(KEY)).toBeNull()
  })

  it('a key commits the split as two fr tokens and stores the first share for this window; Backspace puts the stock split back', async () => {
    const c = await mount()
    const d = divider()
    fireEvent.keyDown(d, { key: 'ArrowRight' })
    const a = 2 * (PLAN_W / (PLAN_W + SIDE_W) + 0.05)
    expect(parseFloat(token(c, 'a'))).toBeCloseTo(a, 9)
    expect(parseFloat(token(c, 'b'))).toBeCloseTo(2 - a, 9)
    expect(Number(localStorage.getItem(KEY))).toBeCloseTo(a, 9)
    fireEvent.keyDown(d, { key: 'Backspace' })
    expect(token(c, 'a')).toBe('')
    expect(localStorage.getItem(KEY)).toBe('')
  })

  it('Home and End stop at the columns’ floors, not at the divider’s generic ends', async () => {
    const c = await mount()
    fireEvent.keyDown(divider(), { key: 'Home' })
    expect(parseFloat(token(c, 'a'))).toBeCloseTo(2 * (260 / (PLAN_W + SIDE_W)), 9)
    fireEvent.keyDown(divider(), { key: 'End' })
    expect(parseFloat(token(c, 'b'))).toBeCloseTo(2 * (260 / (PLAN_W + SIDE_W)), 9)
  })

  it('a stored split opens where the operator left it; a hand-edited one is clamped, and a nonsense one is the stock split', async () => {
    localStorage.setItem(KEY, '1.2')
    let c = await mount()
    expect(token(c, 'a')).toBe('1.2fr')
    expect(token(c, 'b')).toBe('0.8fr')
    cleanup()
    localStorage.setItem(KEY, '5')
    c = await mount()
    expect(token(c, 'a')).toBe('1.85fr')
    cleanup()
    localStorage.setItem(KEY, 'wide')
    c = await mount()
    expect(token(c, 'a')).toBe('')
  })

  it('without a grid there is no planning column to divide, and no divider', async () => {
    api.getSettings.mockImplementation(() => Promise.resolve({ mygrid: '', rotatorModel: 0, rotatorHost: '', satDopplerOff: true, satVfoMap: 'off' }))
    render(<SatellitesView snap={snap()} />)
    await waitFor(() => expect(document.querySelector('.sats-empty')).not.toBeNull())
    expect(screen.queryByRole('separator', { name: 'Schedule column / pass column' })).toBeNull()
  })
})
