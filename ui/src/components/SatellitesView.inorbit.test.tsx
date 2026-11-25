// @vitest-environment jsdom
//
// SatNOGS renamed a working bird's status from "alive" to "in orbit" in
// 2026-08, and the mirror carries the word through as it arrived — measured
// 2026-11-24, every live bird in the published catalog reads "in orbit" and
// none reads "alive". Each surface that keys on a live bird must read the new
// word exactly as the old one: the schedule's alive chip, the strip's "reported
// alive" clause and its ranking bonus, and the open bird's status chip tone.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor } from '@testing-library/react'
import { SatellitesView } from './SatellitesView'
import type { SatDetail, SatPass, SatTrackStatus, SatView } from '../types'

const api = vi.hoisted(() => ({
  getSatellites: vi.fn((): Promise<SatView | null> => Promise.resolve(null)),
  getSatSchedule: vi.fn((): Promise<SatPass[]> => Promise.resolve([])),
  getSatPassNeeds: vi.fn((_names: string[], _hours: number): Promise<SatPass[]> =>
    Promise.resolve([]),
  ),
  getSatDetail: vi.fn((_n: string): Promise<SatDetail | null> => Promise.resolve(null)),
  getSettings: vi.fn(),
  setSettings: vi.fn(() => Promise.resolve({} as never)),
  setSatTransponder: vi.fn(() => Promise.resolve()),
  getSatTransponder: vi.fn(
    (): Promise<import('../types').SatTransponderHeld | null> => Promise.resolve(null),
  ),
  startSatTrack: vi.fn((): Promise<SatTrackStatus | null> => Promise.resolve(null)),
  stopSatTrack: vi.fn(() => Promise.resolve()),
  getSatTrackStatus: vi.fn((): Promise<SatTrackStatus | null> => Promise.resolve(null)),
  fetchTlesNow: vi.fn(),
}))
vi.mock('../api', () => api)
vi.mock('./MapView', () => ({ MapView: () => null }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

const NOW = Math.floor(Date.now() / 1000)

type Bird = SatView['birds'][number]
const bird = (name: string, norad: number, over: Partial<Bird> = {}): Bird => ({
  name,
  norad,
  lat: 0,
  lon: 0,
  altKm: 500,
  footprintKm: 2000,
  track: [],
  status: 'alive',
  amateur: true,
  ...over,
})

const pass = (
  name: string,
  norad: number,
  maxElDeg: number,
  aosOffsetMin: number,
  over: Partial<SatPass> = {},
): SatPass => ({
  name,
  norad,
  aosUnix: NOW + aosOffsetMin * 60,
  losUnix: NOW + aosOffsetMin * 60 + 600,
  maxElDeg,
  aosAzDeg: 10,
  losAzDeg: 190,
  ...over,
})

const mkView = (birds: Bird[], passes: SatPass[]): SatView => ({
  tleAgeDays: 1,
  usableCount: 300,
  agingCount: 0,
  heldBackCount: 0,
  tleFetchedAt: NOW,
  tleSource: 'mirror',
  birds,
  passes,
  excluded: [],
})

const settings = () => ({
  mygrid: 'EN52',
  rotatorModel: 0,
  rotatorHost: '',
  satDoppler: false,
  satVfoMap: 'off',
})

const detail = (name: string): SatDetail => ({
  name,
  norad: null,
  status: 'alive',
  transmitters: [],
  dataFetchedAt: null,
  pass: null,
  passTrack: [],
})

/** The strip's labelled groups, in render order. */
const groups = (c: HTMLElement) => Array.from(c.querySelectorAll('.sats-best-group'))
const groupRows = (g: Element) => Array.from(g.querySelectorAll('.sats-best-row'))
const stripText = (c: HTMLElement) => c.querySelector('.sats-best')?.textContent ?? ''

beforeEach(() => {
  localStorage.clear()
  api.getSatellites.mockReset()
  api.getSatPassNeeds.mockReset()
  api.getSatDetail.mockReset()
  api.getSatDetail.mockImplementation((n: string) => Promise.resolve(detail(n)))
  api.getSettings.mockReset()
  api.getSettings.mockImplementation(() => Promise.resolve(settings()))
  api.getSatTrackStatus.mockReset()
  api.getSatTrackStatus.mockImplementation(() => Promise.resolve(null))
})
afterEach(cleanup)

const IN_ORBIT = 'in orbit'

describe('a post-rename "in orbit" bird reads as alive on every surface', () => {
  it('the schedule row carries the alive chip, and the strip says "reported alive"', async () => {
    localStorage.setItem('nexus.sats.chasing', JSON.stringify(['RS-44']))
    api.getSatellites.mockImplementation(() =>
      Promise.resolve(mkView([bird('RS-44', 44909, { status: IN_ORBIT })], [pass('RS-44', 44909, 40, 10)])),
    )
    api.getSatPassNeeds.mockImplementation(() =>
      Promise.resolve([pass('RS-44', 44909, 40, 10, { status: IN_ORBIT })]),
    )
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(container.querySelector('.sats-sched tbody tr .sat-star')).toBeTruthy())
    const chip = container.querySelector('.sats-sched .sat-chip')
    expect(chip, 'the schedule row lost its alive chip').toBeTruthy()
    expect(chip!.classList.contains('alive')).toBe(true)
    await waitFor(() => expect(groups(container).length).toBeGreaterThan(0))
    expect(groupRows(groups(container)[0])[0].textContent).toMatch(/reported alive/)
  })

  it('Best ranks an "in orbit" bird with the alive bonus', async () => {
    const names = ['NEAR-1', 'NEAR-2', 'ORBIT', 'NOWORD']
    localStorage.setItem('nexus.sats.chasing', JSON.stringify(names))
    // NEAR-1/2 are the two soonest (Next). ORBIT at 40° with SatNOGS saying
    // "in orbit" vs NOWORD at 45° with no status: the +8 alive bonus puts
    // ORBIT first in Best (40+8 > 45), exactly as "alive" always did.
    const passes = [
      pass('NEAR-1', 90001, 12, 5),
      pass('NEAR-2', 90002, 12, 6),
      pass('ORBIT', 90003, 40, 120),
      pass('NOWORD', 90004, 45, 240),
    ]
    api.getSatellites.mockImplementation(() =>
      Promise.resolve(mkView(passes.map((p) => bird(p.name, p.norad!, { status: null })), passes)),
    )
    api.getSatPassNeeds.mockImplementation(() =>
      Promise.resolve(passes.map((p) => (p.name === 'ORBIT' ? { ...p, status: IN_ORBIT } : p))),
    )
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(groups(container).length).toBe(2))
    await waitFor(() => expect(stripText(container)).toMatch(/reported alive/))
    const best = groupRows(groups(container)[1]).map((r) => r.querySelector('b')?.textContent)
    expect(best).toEqual(['ORBIT', 'NOWORD'])
  })

  it("the open bird's status chip takes the alive tone, not the dead one", async () => {
    api.getSatellites.mockImplementation(() =>
      Promise.resolve(mkView([bird('RS-44', 44909, { status: IN_ORBIT })], [pass('RS-44', 44909, 40, 10)])),
    )
    api.getSatPassNeeds.mockImplementation(() => Promise.resolve([]))
    api.getSatDetail.mockImplementation((n: string) => Promise.resolve({ ...detail(n), status: IN_ORBIT }))
    const { container } = render(<SatellitesView focusSat="RS-44" />)
    await waitFor(() => expect(container.querySelector('.sats-arm-id .sat-chip')).toBeTruthy())
    const chip = container.querySelector('.sats-arm-id .sat-chip')!
    expect(chip.textContent).toBe(IN_ORBIT)
    expect(chip.classList.contains('alive')).toBe(true)
    expect(chip.classList.contains('dead')).toBe(false)
  })

  it('a ★ bird whose launch failed gets the dead chip and says so, like a re-entry', async () => {
    localStorage.setItem('nexus.sats.chasing', JSON.stringify(['LOST-1']))
    api.getSatellites.mockImplementation(() =>
      Promise.resolve(mkView([bird('LOST-1', 99005, { status: null })], [pass('LOST-1', 99005, 40, 10)])),
    )
    api.getSatPassNeeds.mockImplementation(() =>
      Promise.resolve([pass('LOST-1', 99005, 40, 10, { status: 'launch failed' })]),
    )
    const { container } = render(<SatellitesView />)
    await waitFor(() => expect(container.querySelector('.sats-sched .sat-chip')).toBeTruthy())
    const chip = container.querySelector('.sats-sched .sat-chip')!
    expect(chip.classList.contains('dead')).toBe(true)
    expect(chip.textContent).toBe('launch failed')
    await waitFor(() => expect(stripText(container)).toMatch(/LAUNCH FAILED/))
  })
})
