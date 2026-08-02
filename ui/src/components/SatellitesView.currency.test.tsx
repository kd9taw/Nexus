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
import { render, screen, cleanup, fireEvent, waitFor, within } from '@testing-library/react'
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
    agingCount: 0,
    heldBackCount: 0,
    fetchedAt: NOW,
    source: 'mirror',
    importedCount: 0,
    elementAgeDays: 0.3,
    blockedUntil: 0,
  }
}

const satView = (tleAgeDays: number): SatView => ({
  tleAgeDays,
  usableCount: 97,
  agingCount: 0,
  heldBackCount: 0,
  tleFetchedAt: NOW - 3600,
  tleSource: 'mirror',
  birds: [],
  passes: [],
  excluded: [],
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
  dopplerDownlink: true,
  dopplerUplink: true,
  uplinkOffer: 'none',
  uplinkOfferMap: null,
  uplinkRadio: 'IC-9700',
  uplinkRadioId: 1,
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
  satDopplerOff: false,
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

describe('the section always has a way to refresh elements', () => {
  // The amber chip WAS the section's manual refresh. Under median semantics
  // it correctly disappears in normal operation — which took the only
  // in-section refresh with it (what was left: the per-bird arm-confirm, the
  // armed rail past 14 d, and Settings ▸ Radio ▸ Orbital elements). A control
  // that exists only while something is wrong is not a control.
  it('a current catalog keeps a quiet refresh control, and no warning chip', async () => {
    const { container } = render(<SatellitesView />)
    const btn = await screen.findByRole('button', { name: /refresh elements/i })
    // Quiet: it must not borrow the amber warning chip's look or its claim.
    expect(container.querySelector('.sat-chip.stale')).toBeNull()
    expect(btn.textContent).not.toMatch(/stale/i)
    fireEvent.click(btn)
    await waitFor(() => expect(api.fetchTlesNow).toHaveBeenCalledTimes(1))
  })

  // The other end of the same rule: with nothing cached (a first launch, or a
  // shack that has never had the network) fetching elements is the only thing
  // there is to do here, so the control must be reachable then above all.
  it('no elements at all still offers the fetch', async () => {
    api.getSatellites.mockImplementation(() => Promise.resolve(null))
    render(<SatellitesView />)
    fireEvent.click(await screen.findByRole('button', { name: /refresh elements/i }))
    await waitFor(() => expect(api.fetchTlesNow).toHaveBeenCalledTimes(1))
  })

  it('a stale catalog refreshes from the amber chip — one control, never two', async () => {
    api.getSatellites.mockImplementation(() => Promise.resolve(satView(20.2)))
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(container.querySelector('.sat-chip.stale')).toBeTruthy())
    expect(screen.queryByRole('button', { name: /refresh elements/i })).toBeNull()
  })
})

// FINDING: `heldBackCount` reached exactly one surface (the Settings line),
// so "30 of 367 held back" and "51 of 100 held back" rendered identically
// where the operator actually is. The excluded birds are listed individually
// only when STARRED or matched by the search box, so by default they appear
// nowhere at all.
describe('the held-back birds are visible on the Satellites screen', () => {
  const bands = (over: Partial<SatView>): SatView => ({ ...satView(0.2), ...over })

  it('names the sitting-out birds and their share, by default', async () => {
    api.getSatellites.mockImplementation(() =>
      Promise.resolve(bands({ usableCount: 337, agingCount: 0, heldBackCount: 30 })),
    )
    render(<SatellitesView />)
    const note = await screen.findByTestId('sat-element-bands')
    expect(note.textContent).toBe('367 birds · 30 sit out past 30 d')
  })

  it('a drifting set does NOT read like a few slow-cadence birds', async () => {
    api.getSatellites.mockImplementation(() =>
      Promise.resolve(bands({ usableCount: 100, agingCount: 49, heldBackCount: 40 })),
    )
    render(<SatellitesView />)
    const note = await screen.findByTestId('sat-element-bands')
    expect(note.textContent).toBe('140 birds · 49 past 14 d · 40 sit out past 30 d')
  })

  it('a clean catalog says nothing — a zero is not news', async () => {
    render(<SatellitesView />)
    await waitFor(() => expect(api.getSatellites).toHaveBeenCalled())
    expect(screen.queryByTestId('sat-element-bands')).toBeNull()
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
    // Scoped to the confirm: the header carries its own refresh control now,
    // so an unscoped /refresh/i names two different buttons.
    const confirm = within(screen.getByRole('dialog'))
    expect(confirm.getByRole('button', { name: /refresh/i })).toBeTruthy()
    expect(confirm.getByRole('button', { name: /cancel/i })).toBeTruthy()
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
