// @vitest-environment jsdom
//
// THE QO-100 ROW. The Phone and CW band picker ends with a "QO-100 satellite" row that OPENS THE
// SATELLITES SCREEN on the bird rather than tuning anything itself: that screen already works the
// narrowband transponder (linear, the 13 cm / 3 cm licence gate, the uplink left alone unless the
// operator mapped one), and a band list holding a second copy of that state would drift from it.
//
// Pinned here:
//  - the row is offered only when the host can open the Satellites screen, and a pick hands it
//    QO-100's NORAD id without asking the engine to change band;
//  - the Satellites screen selects the bird by that NORAD id under whatever name the catalogue
//    gives it (Celestrak's is "ES'HAIL 2"), and only once, so a list refresh never takes the
//    pane back from a bird the operator chose since.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import type { AppSnapshot, SatView } from '../types'

const api = vi.hoisted(() => ({
  getLicensedBandPlan: vi.fn(async () => [
    { band: '3cm', group: 'UHF', dialMhz: 10489.7, mode: 'USB', label: '3cm', note: '', tx: true },
  ]),
  pickBand: vi.fn(async () => ({})),
  getSatellites: vi.fn((): Promise<SatView | null> => Promise.resolve(null)),
  getSatSchedule: vi.fn(() => Promise.resolve([])),
  getSatPassNeeds: vi.fn(() => Promise.resolve([])),
  getSatDetail: vi.fn((_name: string) => new Promise(() => {})),
  getSettings: vi.fn(() => new Promise(() => {})),
  setSettings: vi.fn((_s: unknown) => Promise.resolve({} as never)),
  setSatTransponder: vi.fn(() => Promise.resolve()),
  getSatTransponder: vi.fn(() => Promise.resolve(null)),
  startSatTrack: vi.fn(() => Promise.resolve(null)),
  stopSatTrack: vi.fn(() => Promise.resolve()),
  getSatTrackStatus: vi.fn(() => Promise.resolve(null)),
  fetchTlesNow: vi.fn(() => Promise.resolve(null)),
}))
vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  ...api,
}))
vi.mock('./MapView', () => ({ MapView: () => null }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

import { BandPicker, QO100_NORAD } from './BandPicker'
import { SatellitesView } from './SatellitesView'

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

const snap = {
  activeRadioId: 1,
  radio: { band: '3cm', dialMhz: 10489.7, catOk: true, txAllowed: true, operatingMode: 'phone', source: 'native' },
} as unknown as AppSnapshot

const settle = async () => {
  for (let i = 0; i < 3; i++) await act(async () => { await Promise.resolve() })
}
const openMenu = () => {
  fireEvent.keyDown(screen.getByRole('button', { name: /band/i }), { key: 'Enter' })
  return screen.getByRole('menu')
}
const rows = (menu: HTMLElement) => within(menu).getAllByRole('menuitemradio').map((el) => el.textContent ?? '')

describe('the band picker QO-100 row', () => {
  it('opens the Satellites screen on QO-100 and tunes nothing itself', async () => {
    const open = vi.fn()
    render(<BandPicker snap={snap} mode="phone" onOpenSatellite={open} />)
    await settle()
    const menu = openMenu()
    const qo = within(menu).getAllByRole('menuitemradio').find((el) => el.textContent?.startsWith('QO-100 satellite'))
    expect(qo, `rows: ${rows(menu).join(' | ')}`).toBeTruthy()
    fireEvent.click(qo!)
    expect(open).toHaveBeenCalledWith(43700)
    expect(QO100_NORAD).toBe(43700)
    expect(api.pickBand).not.toHaveBeenCalled()
  })

  it('is absent where the host cannot open the Satellites screen', async () => {
    render(<BandPicker snap={snap} mode="phone" />)
    await settle()
    expect(rows(openMenu()).some((r) => r.startsWith('QO-100'))).toBe(false)
  })

  it('control: a band row still picks the band', async () => {
    render(<BandPicker snap={snap} mode="phone" onOpenSatellite={vi.fn()} />)
    await settle()
    const menu = openMenu()
    fireEvent.click(within(menu).getAllByRole('menuitemradio').find((el) => el.textContent?.startsWith('3cm'))!)
    expect(api.pickBand).toHaveBeenCalledWith('3cm', 'phone')
  })
})

const view = (birds: { name: string; norad: number }[]): SatView =>
  ({
    tleAgeDays: 1,
    usableCount: birds.length,
    agingCount: 0,
    heldBackCount: 0,
    tleFetchedAt: 0,
    tleSource: 'bundled',
    birds: birds.map((b) => ({ ...b, lat: 0, lon: 26, altKm: 35786, footprintKm: 9000, track: [] })),
    passes: [],
    excluded: [],
  }) as SatView

describe('the Satellites screen focused by NORAD id', () => {
  it("selects QO-100 under the catalogue's own name", async () => {
    api.getSatellites.mockImplementation(() =>
      Promise.resolve(view([{ name: 'ISS', norad: 25544 }, { name: "ES'HAIL 2", norad: 43700 }])),
    )
    render(<SatellitesView focusNorad={QO100_NORAD} />)
    await settle()
    expect(api.getSatDetail).toHaveBeenCalledWith("ES'HAIL 2")
    expect(api.getSatDetail).not.toHaveBeenCalledWith('ISS')
  })

  it('selects nothing for an id the list does not carry', async () => {
    api.getSatellites.mockImplementation(() => Promise.resolve(view([{ name: 'ISS', norad: 25544 }])))
    render(<SatellitesView focusNorad={QO100_NORAD} />)
    await settle()
    expect(api.getSatDetail).not.toHaveBeenCalled()
  })
})
