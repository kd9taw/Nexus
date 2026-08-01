// @vitest-environment jsdom
//
// Element CURRENCY honesty (phases 2+3 of the TLE overhaul).
//
// What is pinned here:
//
//  - The stale chip is a BUTTON now — the manual refresh exists (fetch_tles_now),
//    so the old "there is no manual refresh" tooltip line is dead. Clicking it
//    fires the refresh.
//  - The readiness rail carries a fifth ELEMENTS gate row: the physics the whole
//    chain runs on, with its own fix (refresh) when the frozen set is past the
//    14 d stale line. Five rows now — the four-gate era is over.
//  - Arming past 14 d gets the operator confirm (Refresh / Arm anyway / Cancel):
//    one STALE number, two consequences (badge at the section level, confirm at
//    the arm). 13 d arms without ceremony; 15 d asks first and arms ONLY on
//    "Arm anyway". The click is still the only consent that arms anything.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SatellitesView } from './SatellitesView'
import type { SatDetail, SatPass, SatTrackStatus, SatView } from '../types'
import type { TleStatus } from '../api'

const api = vi.hoisted(() => ({
  getSatellites: vi.fn((): Promise<SatView | null> => Promise.resolve(null)),
  getSatSchedule: vi.fn((): Promise<SatPass[]> => Promise.resolve([])),
  getSatPassNeeds: vi.fn((): Promise<SatPass[]> => Promise.resolve([])),
  getSatDetail: vi.fn(),
  getSettings: vi.fn(),
  setSettings: vi.fn((_s: unknown) => Promise.resolve({} as never)),
  setSatTransponder: vi.fn(() => Promise.resolve()),
  getSatTransponder: vi.fn(
    (): Promise<import('../types').SatTransponderHeld | null> => Promise.resolve(null),
  ),
  startSatTrack: vi.fn((): Promise<SatTrackStatus | null> => Promise.resolve(null)),
  stopSatTrack: vi.fn(() => Promise.resolve()),
  getSatTrackStatus: vi.fn((): Promise<SatTrackStatus | null> => Promise.resolve(null)),
  fetchTlesNow: vi.fn((): Promise<TleStatus> => Promise.resolve(freshStatus())),
}))
vi.mock('../api', () => api)
vi.mock('./MapView', () => ({ MapView: () => null }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

const NOW = Math.floor(Date.now() / 1000)

function freshStatus(): TleStatus {
  return {
    count: 97,
    usableCount: 97,
    fetchedAt: NOW,
    source: 'mirror',
    importedCount: 0,
    elementAgeDays: 0.3,
    blockedUntil: 0,
  }
}

const satView = (tleAgeDays: number): SatView => ({
  tleAgeDays,
  tleFetchedAt: NOW - 3600,
  tleSource: 'mirror',
  birds: [],
  passes: [],
})

const passRow = (over: Partial<SatPass> = {}): SatPass => ({
  name: 'RS-44',
  aosUnix: NOW + 720,
  losUnix: NOW + 1500,
  maxElDeg: 62,
  aosAzDeg: 100,
  losAzDeg: 260,
  status: 'alive',
  ...over,
})

/** RS-44 with one workable linear transponder; elementAgeDays per test. */
const detailWith = (elementAgeDays: number | null): SatDetail => ({
  name: 'RS-44',
  norad: 44909,
  status: 'alive',
  transmitters: [
    {
      description: 'SSB/CW linear transponder',
      alive: true,
      mode: 'LSB',
      uplinkLowHz: 145_965_000,
      downlinkLowHz: 435_640_000,
      invert: true,
      uplinkHighHz: 145_995_000,
      downlinkHighHz: 435_670_000,
      uplinkMode: 'LSB',
      downlinkMode: 'USB',
      kind: 'Transponder',
    },
  ],
  dataFetchedAt: 1_760_000_000,
  elementAgeDays,
  pass: passRow(),
  passTrack: [
    [NOW + 720, 100, 0],
    [NOW + 1100, 180, 62],
    [NOW + 1500, 260, 0],
  ],
})

const trackStatus = (over: Partial<SatTrackStatus> = {}): SatTrackStatus => ({
  name: 'RS-44',
  state: 'armed',
  mode: 'doppler-only',
  azDeg: null,
  elDeg: null,
  aosAzDeg: 100,
  satAzDeg: null,
  satElDeg: null,
  rangeKm: null,
  rangeRateKmS: null,
  downlinkHz: null,
  uplinkHz: null,
  downlinkShiftHz: null,
  uplinkShiftHz: null,
  transponder: null,
  transponderIndex: null,
  inverting: false,
  offsetHz: null,
  halfWidthHz: null,
  elementAgeDays: 1.2,
  elementEpochUnix: NOW - 104_000,
  aosUnix: NOW + 720,
  losUnix: NOW + 1500,
  ...over,
})

const mkSettings = (over: Record<string, unknown> = {}) => ({
  mygrid: 'EN52',
  rotatorModel: 0,
  rotatorHost: '',
  satDoppler: false,
  satVfoMap: 'off',
  ...over,
})

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('nexus.sats.chasing', JSON.stringify(['RS-44']))
  api.getSatellites.mockReset()
  api.getSatellites.mockImplementation(() => Promise.resolve(satView(0.4)))
  api.getSatSchedule.mockReset()
  api.getSatSchedule.mockImplementation(() => Promise.resolve([passRow()]))
  api.getSatPassNeeds.mockReset()
  api.getSatPassNeeds.mockImplementation(() => Promise.resolve([passRow()]))
  api.getSatDetail.mockReset()
  api.getSatDetail.mockImplementation(() => Promise.resolve(detailWith(0.4)))
  api.getSettings.mockReset()
  api.getSettings.mockImplementation(() => Promise.resolve(mkSettings()))
  api.setSettings.mockReset()
  api.setSatTransponder.mockReset()
  api.setSatTransponder.mockImplementation(() => Promise.resolve())
  api.getSatTransponder.mockReset()
  api.getSatTransponder.mockImplementation(() => Promise.resolve(null))
  api.startSatTrack.mockReset()
  api.startSatTrack.mockImplementation(() => Promise.resolve(trackStatus()))
  api.getSatTrackStatus.mockReset()
  api.getSatTrackStatus.mockImplementation(() => Promise.resolve(null))
  api.fetchTlesNow.mockReset()
  api.fetchTlesNow.mockImplementation(() => Promise.resolve(freshStatus()))
})
afterEach(cleanup)

const workButtons = () => screen.findAllByTitle(/^Work this pass/)

describe('the stale chip is the refresh button', () => {
  it('renders the >14 d chip as a BUTTON that fires the manual refresh', async () => {
    api.getSatellites.mockImplementation(() => Promise.resolve(satView(20.2)))
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(container.querySelector('.sat-chip.stale')).toBeTruthy())
    const chip = container.querySelector('.sat-chip.stale')!
    // A button, not a span: the manual refresh EXISTS now.
    expect(chip.tagName).toBe('BUTTON')
    // The old "there is no manual refresh" line is dead — the tooltip must
    // never claim the control it sits on does not exist.
    expect(chip.getAttribute('title') ?? '').not.toMatch(/no manual refresh/i)
    expect(chip.getAttribute('title') ?? '').toMatch(/refresh/i)
    fireEvent.click(chip)
    await waitFor(() => expect(api.fetchTlesNow).toHaveBeenCalledTimes(1))
  })

  it('renders no chip (and no dead control) while the elements are fresh', async () => {
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(api.getSatellites).toHaveBeenCalled())
    expect(container.querySelector('.sat-chip.stale')).toBeNull()
  })
})

describe('the Elements gate row', () => {
  it('is the fifth rail row, ready (●) on a fresh frozen set', async () => {
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(trackStatus({ mode: 'pass-only', elementAgeDays: 1.2 })),
    )
    render(<SatellitesView focusSat="RS-44" />)
    const rail = await screen.findByTestId('sat-rail')
    expect(rail.querySelectorAll('.sat-rail-dot').length).toBe(5)
    const row = Array.from(rail.querySelectorAll('.sat-rail-row')).find((r) =>
      /Elements/.test(r.textContent ?? ''),
    )
    expect(row).toBeTruthy()
    expect(row?.querySelector('.sat-rail-dot')?.textContent).toBe('●')
    expect(row?.textContent).toMatch(/1\.2 d old/)
  })

  it('goes hollow past 14 d with its own refresh fix', async () => {
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(trackStatus({ mode: 'pass-only', elementAgeDays: 20.4 })),
    )
    render(<SatellitesView focusSat="RS-44" />)
    const rail = await screen.findByTestId('sat-rail')
    const row = Array.from(rail.querySelectorAll('.sat-rail-row')).find((r) =>
      /Elements/.test(r.textContent ?? ''),
    )
    expect(row).toBeTruthy()
    expect(row?.querySelector('.sat-rail-dot')?.textContent).toBe('○')
    expect(row?.textContent).toMatch(/20 days old/)
    const fix = Array.from(row!.querySelectorAll('button')).find((b) =>
      /refresh/i.test(b.textContent ?? ''),
    )
    expect(fix).toBeTruthy()
    // The fix is honest about the freeze: this armed pass keeps its set.
    expect(fix!.title).toMatch(/re-arm/i)
    fireEvent.click(fix!)
    await waitFor(() => expect(api.fetchTlesNow).toHaveBeenCalledTimes(1))
  })
})

describe('the 14 d arm confirm', () => {
  it('asks before arming on 15-day elements, and arms only on "Arm anyway"', async () => {
    api.getSatDetail.mockImplementation(() => Promise.resolve(detailWith(15.3)))
    render(<SatellitesView />)
    fireEvent.click((await workButtons())[0])
    // The confirm appears — naming the age — and NOTHING has armed yet.
    const armAnyway = await screen.findByRole('button', { name: /arm anyway/i })
    expect(screen.getByRole('button', { name: /refresh/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /cancel/i })).toBeTruthy()
    expect(api.startSatTrack).not.toHaveBeenCalled()
    fireEvent.click(armAnyway)
    await waitFor(() => expect(api.startSatTrack).toHaveBeenCalledWith('RS-44', NOW + 720))
  })

  it('Cancel arms nothing', async () => {
    api.getSatDetail.mockImplementation(() => Promise.resolve(detailWith(15.3)))
    render(<SatellitesView />)
    fireEvent.click((await workButtons())[0])
    fireEvent.click(await screen.findByRole('button', { name: /cancel/i }))
    await new Promise((r) => setTimeout(r, 50))
    expect(api.startSatTrack).not.toHaveBeenCalled()
  })

  it('13-day elements arm without ceremony — the confirm starts at the STALE line', async () => {
    api.getSatDetail.mockImplementation(() => Promise.resolve(detailWith(13.0)))
    render(<SatellitesView />)
    fireEvent.click((await workButtons())[0])
    await waitFor(() => expect(api.startSatTrack).toHaveBeenCalledWith('RS-44', NOW + 720))
    expect(screen.queryByRole('button', { name: /arm anyway/i })).toBeNull()
  })
})
