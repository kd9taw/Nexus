// @vitest-environment jsdom
//
// The transponder chooser remembers: the row picked LAST on a bird is the one "Work this pass"
// picks next time, and a ★ FAVORITE on a row wins over that. What is pinned here:
//
//  - Remember-last round trips across a remount (the chooser is rebuilt every time the section
//    opens), and the remembered row stays on screen past the first rows, marked.
//  - The ★ favorite beats the last pick, and starring a row never tunes the radio: it is
//    stored, nothing is sent.
//  - A saved transponder SatNOGS no longer lists as working falls back to the usual pick, and
//    the chooser says so in one quiet line. A beacon is never picked.
//  - Nothing here overrides a transponder the ENGINE is already holding, nor the operator's
//    explicit "None".
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SatellitesView } from './SatellitesView'
import type { SatDetail, SatPass, SatTrackStatus, SatTransmitter, SatView } from '../types'

const api = vi.hoisted(() => ({
  getSatellites: vi.fn((): Promise<SatView | null> => Promise.resolve(null)),
  getSatSchedule: vi.fn((): Promise<SatPass[]> => Promise.resolve([])),
  getSatPassNeeds: vi.fn((): Promise<SatPass[]> => Promise.resolve([])),
  getSatDetail: vi.fn(),
  getSettings: vi.fn(),
  setSettings: vi.fn((_s: unknown) => Promise.resolve({} as never)),
  confirmSatUplink: vi.fn((_m: unknown, _r?: number) => Promise.resolve({} as never)),
  setSatTransponder: vi.fn((_n: string, _i: number | null, _a?: boolean) => Promise.resolve()),
  getSatTransponder: vi.fn((): Promise<import('../types').SatTransponderHeld | null> => Promise.resolve(null)),
  startSatTrack: vi.fn((): Promise<SatTrackStatus | null> => Promise.resolve(null)),
  stopSatTrack: vi.fn(() => Promise.resolve()),
  getSatTrackStatus: vi.fn((): Promise<SatTrackStatus | null> => Promise.resolve(null)),
}))
vi.mock('../api', () => api)
vi.mock('./MapView', () => ({ MapView: () => null }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

const NOW = Math.floor(Date.now() / 1000)
const BIRD = 'ISS'

const passRow = (over: Partial<SatPass> = {}): SatPass => ({
  name: BIRD,
  aosUnix: NOW + 720,
  losUnix: NOW + 1500,
  maxElDeg: 62,
  aosAzDeg: 100,
  losAzDeg: 260,
  status: 'alive',
  ...over,
})

const tx = (over: Partial<SatTransmitter>): SatTransmitter => ({
  description: 'x',
  alive: true,
  mode: 'FM',
  uplinkLowHz: null,
  downlinkLowHz: null,
  invert: false,
  uplinkHighHz: null,
  downlinkHighHz: null,
  uplinkMode: null,
  downlinkMode: null,
  kind: 'Transceiver',
  ...over,
})

/** More workable rows than the chooser shows before "show all", with the one the operator
 *  works LAST: a dead entry first (the raw-index trap), then six alive rows. */
const FM = 'FM voice repeater'
const APRS = 'APRS digipeater'
const transmitters = (): SatTransmitter[] => [
  tx({ description: 'Old packet', alive: false, uplinkLowHz: 145_990_000, downlinkLowHz: 437_550_000 }),
  tx({ description: APRS, mode: 'AFSK', uplinkLowHz: 145_825_000, downlinkLowHz: 145_825_000 }),
  tx({ description: 'SSTV', kind: 'Transmitter', downlinkLowHz: 145_800_000 }),
  tx({ description: 'Telemetry', kind: 'Transmitter', mode: 'BPSK', downlinkLowHz: 437_800_000 }),
  tx({ description: 'Packet BBS', mode: 'AFSK', uplinkLowHz: 145_825_000, downlinkLowHz: 437_550_000 }),
  tx({ description: 'Crew contact', kind: 'Transmitter', downlinkLowHz: 145_800_000 }),
  tx({ description: FM, uplinkLowHz: 145_990_000, downlinkLowHz: 437_800_000 }),
]
const FM_INDEX = 6
const APRS_INDEX = 1

let list: SatTransmitter[] = transmitters()
const detail = (): SatDetail => ({
  name: BIRD,
  norad: 25544,
  status: 'alive',
  transmitters: list,
  dataFetchedAt: 1_760_000_000,
  pass: passRow(),
  passTrack: [
    [NOW + 720, 100, 0],
    [NOW + 1100, 180, 62],
    [NOW + 1500, 260, 0],
  ],
})

const trackStatus = (): SatTrackStatus => ({
  name: BIRD,
  state: 'armed',
  mode: 'doppler-only',
  dopplerDownlink: true,
  dopplerUplink: true,
  uplinkOffer: 'none',
  uplinkOfferMap: null,
  uplinkRadio: 'IC-9700',
  uplinkRadioId: 3,
  azDeg: null,
  elDeg: null,
  aosAzDeg: 100,
  maxElDeg: 45,
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
  elementEpochUnix: 1_785_442_400,
  aosUnix: NOW + 720,
  losUnix: NOW + 1500,
})

const mkView = (): SatView => ({
  tleAgeDays: 1,
  usableCount: 300,
  agingCount: 0,
  heldBackCount: 0,
  tleFetchedAt: NOW,
  tleSource: 'mirror',
  birds: [
    { name: BIRD, norad: 25544, lat: 0, lon: 0, altKm: 420, footprintKm: 2000, track: [], status: 'alive', amateur: true },
  ],
  passes: [passRow({ norad: 25544 })],
  excluded: [],
})

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('nexus.sats.chasing', JSON.stringify([BIRD]))
  list = transmitters()
  api.getSatellites.mockReset()
  api.getSatellites.mockImplementation(() => Promise.resolve(mkView()))
  api.getSatSchedule.mockReset()
  api.getSatSchedule.mockImplementation(() => Promise.resolve([passRow()]))
  api.getSatPassNeeds.mockReset()
  api.getSatPassNeeds.mockImplementation(() => Promise.resolve([passRow()]))
  api.getSatDetail.mockReset()
  api.getSatDetail.mockImplementation(() => Promise.resolve(detail()))
  api.getSettings.mockReset()
  api.getSettings.mockImplementation(() =>
    Promise.resolve({ mygrid: 'EN52', rotatorModel: 0, rotatorHost: '', satDopplerOff: false, satVfoMap: 'off', activeRadio: 3 }),
  )
  api.setSatTransponder.mockReset()
  api.setSatTransponder.mockImplementation(() => Promise.resolve())
  api.getSatTransponder.mockReset()
  api.getSatTransponder.mockImplementation(() => Promise.resolve(null))
  api.startSatTrack.mockReset()
  api.startSatTrack.mockImplementation(() => Promise.resolve(trackStatus()))
  api.getSatTrackStatus.mockReset()
  api.getSatTrackStatus.mockImplementation(() => Promise.resolve(null))
})
afterEach(cleanup)

const workButtons = () => screen.findAllByTitle(/^Work this pass/)
const card = (description: string) => screen.findByLabelText(`Work ${description}`)
const star = (description: string) => screen.findByRole('button', { name: `Favorite transponder: ${description}` })
const showAll = async () => fireEvent.click(await screen.findByText(/^show all \d+ ▾$/))

describe('remember the last pick', () => {
  it('the row picked last is what "Work this pass" picks next time, and it stays on screen', async () => {
    const first = render(<SatellitesView focusSat={BIRD} />)
    // Past the first rows: today the operator has to open "show all" for it.
    await card(APRS)
    expect(screen.queryByLabelText(`Work ${FM}`)).toBeNull()
    await showAll()
    fireEvent.click(await card(FM))
    await waitFor(() => expect(api.setSatTransponder).toHaveBeenCalledWith(BIRD, FM_INDEX, false))
    first.unmount()

    // NEXT TIME (the section is rebuilt; the engine released the hold at LOS).
    api.setSatTransponder.mockClear()
    render(<SatellitesView focusSat={BIRD} />)
    // On screen without "show all", marked, and NOT selected — opening the bird tunes nothing.
    const fm = await card(FM)
    expect((fm as HTMLInputElement).checked).toBe(false)
    expect(fm.closest('.sat-tp-card')?.textContent).toMatch(/last used/)
    expect(api.setSatTransponder).not.toHaveBeenCalled()
    fireEvent.click((await workButtons())[0])
    await waitFor(() => expect(api.setSatTransponder).toHaveBeenCalledWith(BIRD, FM_INDEX, true))
    await waitFor(() => expect(api.startSatTrack).toHaveBeenCalledWith(BIRD, NOW + 720))
  })

  it('CONTROL: with nothing saved, "Work this pass" picks the first workable row as before', async () => {
    render(<SatellitesView />)
    fireEvent.click((await workButtons())[0])
    await waitFor(() => expect(api.setSatTransponder).toHaveBeenCalledWith(BIRD, APRS_INDEX, true))
  })

  it('an auto pick is not remembered as the operator’s', async () => {
    const first = render(<SatellitesView />)
    fireEvent.click((await workButtons())[0])
    await waitFor(() => expect(api.setSatTransponder).toHaveBeenCalledWith(BIRD, APRS_INDEX, true))
    first.unmount()
    expect(localStorage.getItem('nexus.sats.transponders')).toBeNull()
  })

  it('a refused pick is not remembered', async () => {
    api.setSatTransponder.mockImplementation(() => Promise.reject(new Error('no radio')))
    render(<SatellitesView focusSat={BIRD} />)
    await showAll()
    fireEvent.click(await card(FM))
    await waitFor(() => expect(api.setSatTransponder).toHaveBeenCalled())
    await new Promise((r) => setTimeout(r, 0))
    expect(localStorage.getItem('nexus.sats.transponders')).toBeNull()
  })
})

describe('the ★ favorite', () => {
  it('beats the last pick, and starring it sends nothing to the radio', async () => {
    const first = render(<SatellitesView focusSat={BIRD} />)
    await showAll()
    const s = await star(FM)
    expect(s.getAttribute('aria-pressed')).toBe('false')
    fireEvent.click(s)
    expect((await star(FM)).getAttribute('aria-pressed')).toBe('true')
    // Starring is not picking: nothing tuned, the row not selected.
    expect(api.setSatTransponder).not.toHaveBeenCalled()
    expect(((await card(FM)) as HTMLInputElement).checked).toBe(false)
    // The operator then works the APRS digipeater once: that is the LAST pick.
    fireEvent.click(await card(APRS))
    await waitFor(() => expect(api.setSatTransponder).toHaveBeenCalledWith(BIRD, APRS_INDEX, false))
    first.unmount()

    api.setSatTransponder.mockClear()
    render(<SatellitesView focusSat={BIRD} />)
    // The favorite is on screen without "show all", starred.
    expect((await star(FM)).getAttribute('aria-pressed')).toBe('true')
    fireEvent.click((await workButtons())[0])
    await waitFor(() => expect(api.setSatTransponder).toHaveBeenCalledWith(BIRD, FM_INDEX, true))
  })

  it('a beacon has no star: it cannot be worked', async () => {
    render(<SatellitesView focusSat={BIRD} />)
    await showAll()
    await star(FM)
    expect(screen.queryByRole('button', { name: 'Favorite transponder: SSTV' })).toBeNull()
  })
})

describe('a saved transponder that is gone', () => {
  it('falls back to the usual pick and says so quietly', async () => {
    localStorage.setItem(
      'nexus.sats.transponders',
      JSON.stringify({ ISS: { fav: { up: 145_990_000, down: 437_800_000, mode: 'FM', label: FM } } }),
    )
    list = transmitters().filter((t) => t.description !== FM)
    render(<SatellitesView focusSat={BIRD} />)
    expect(await screen.findByText(/“FM voice repeater”.*not listed as working/)).toBeTruthy()
    fireEvent.click((await workButtons())[0])
    await waitFor(() => expect(api.setSatTransponder).toHaveBeenCalledWith(BIRD, APRS_INDEX, true))
  })

  it('never picks it once SatNOGS marks it dead', async () => {
    localStorage.setItem(
      'nexus.sats.transponders',
      JSON.stringify({ ISS: { fav: { up: 145_990_000, down: 437_800_000, mode: 'FM', label: FM } } }),
    )
    list = transmitters().map((t) => (t.description === FM ? { ...t, alive: false } : t))
    render(<SatellitesView />)
    fireEvent.click((await workButtons())[0])
    await waitFor(() => expect(api.setSatTransponder).toHaveBeenCalledWith(BIRD, APRS_INDEX, true))
  })

  it('CONTROL: no line while the saved one is listed', async () => {
    localStorage.setItem(
      'nexus.sats.transponders',
      JSON.stringify({ ISS: { fav: { up: 145_990_000, down: 437_800_000, mode: 'FM', label: FM } } }),
    )
    render(<SatellitesView focusSat={BIRD} />)
    await star(FM)
    expect(screen.queryByText(/not listed as working/)).toBeNull()
  })
})

describe('it never takes the radio from anything already holding it', () => {
  beforeEach(() => {
    localStorage.setItem(
      'nexus.sats.transponders',
      JSON.stringify({ ISS: { fav: { up: 145_990_000, down: 437_800_000, mode: 'FM', label: FM } } }),
    )
  })

  it('a transponder the ENGINE holds on this bird stands over the favorite', async () => {
    api.getSatTransponder.mockImplementation(() =>
      Promise.resolve({ name: BIRD, index: APRS_INDEX, description: APRS, binding: null }),
    )
    render(<SatellitesView />)
    fireEvent.click((await workButtons())[0])
    await waitFor(() => expect(api.startSatTrack).toHaveBeenCalled())
    expect(api.setSatTransponder).not.toHaveBeenCalled()
  })

  it('the operator’s explicit None stands over the favorite', async () => {
    render(<SatellitesView focusSat={BIRD} />)
    fireEvent.click(await card(FM))
    await waitFor(() => expect(api.setSatTransponder).toHaveBeenCalledWith(BIRD, FM_INDEX, false))
    fireEvent.click(await screen.findByLabelText('Work no transponder — leave the dial to me'))
    await waitFor(() => expect(api.setSatTransponder).toHaveBeenCalledWith(BIRD, null, false))
    api.setSatTransponder.mockClear()
    fireEvent.click((await workButtons())[0])
    await waitFor(() => expect(api.startSatTrack).toHaveBeenCalled())
    expect(api.setSatTransponder).not.toHaveBeenCalled()
  })
})
