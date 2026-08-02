// @vitest-environment jsdom
//
// HOW HIGH IS THE BIRD — the altitude readout, in the Birds list and on the sky dome.
//
// The operator flies SSB birds with a hand-cranked az/el rotator, and altitude is the number
// that tells him what kind of pass he is about to work: a 600 km LEO screams over in ten
// minutes with Doppler running fast, while RS-44 near apogee loiters for the better part of an
// hour with a shift that barely moves. The backend has computed it for every bird all along
// (SatView.birds[].altKm, and now the live track badge) and no surface showed it.
//
// What is pinned here is that the number is SHOWN and that it cannot be mistaken for RANGE:
//
//  - Range is how far the bird is from the operator, altitude is how far it is from the
//    ground. On the sky dome they sit in the same list, one above the other, so both rows are
//    named. On the map hover line — where the neighbouring station line ends in a bare
//    "1,234 km" meaning distance-from-you — the figure carries an explicit "alt".
//  - Absent is ABSENT. A bird nothing carries elements for has no subpoint, and before AOS the
//    track badge has computed no position; neither renders as 0 km, which would put a
//    satellite on the ground.
//  - It is the LIVE number, never a nominal orbit height: an elliptical bird's altitude varies
//    by hundreds of km across one orbit, and the varying value is the whole point.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { SatellitesView } from './SatellitesView'
import type { SatDetail, SatPass, SatTrackStatus, SatView } from '../types'

const api = vi.hoisted(() => ({
  getSatellites: vi.fn((): Promise<SatView | null> => Promise.resolve(null)),
  getSatSchedule: vi.fn((): Promise<SatPass[]> => Promise.resolve([])),
  getSatPassNeeds: vi.fn((): Promise<SatPass[]> => Promise.resolve([])),
  getSatDetail: vi.fn((): Promise<SatDetail | null> => Promise.resolve(null)),
  getSettings: vi.fn(),
  setSettings: vi.fn((_s: unknown) => Promise.resolve({} as never)),
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
// The detail pane embeds the globe; it needs a canvas and nothing here is about the map canvas.
vi.mock('./MapView', () => ({ MapView: () => null }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

const NOW = Math.floor(Date.now() / 1000)
const AOS = NOW - 300
const LOS = NOW + 300

type Bird = SatView['birds'][number]
const bird = (name: string, norad: number, altKm: number): Bird => ({
  name,
  norad,
  lat: 0,
  lon: 0,
  altKm,
  footprintKm: 2000,
  track: [],
  status: 'alive',
  amateur: true,
})

/** RS-44 high on its ellipse, SO-50 a low circular LEO — the contrast the number exists for. */
const view = (over: Partial<SatView> = {}): SatView => ({
  tleAgeDays: 1,
  usableCount: 300,
  agingCount: 0,
  heldBackCount: 0,
  tleFetchedAt: NOW - 3600,
  tleSource: 'mirror',
  birds: [bird('RS-44', 44909, 1234.4), bird('SO-50', 27607, 629.6)],
  passes: [],
  excluded: [],
  ...over,
})

/** (unix, az, el) samples across a pass, so the dome has a curve to draw. */
function samples(aos: number, los: number): [number, number, number][] {
  const out: [number, number, number][] = []
  for (let i = 0; i <= 20; i++) {
    out.push([Math.round(aos + ((los - aos) * i) / 20), 100 + (160 * i) / 20, 60 * Math.sin((Math.PI * i) / 20)])
  }
  return out
}

const detail = (over: Partial<SatDetail> = {}): SatDetail => ({
  name: 'RS-44',
  norad: 44909,
  status: 'alive',
  transmitters: [],
  dataFetchedAt: 1_760_000_000,
  pass: {
    name: 'RS-44',
    aosUnix: AOS,
    losUnix: LOS,
    maxElDeg: 62,
    aosAzDeg: 100,
    losAzDeg: 260,
    status: 'alive',
  },
  passTrack: samples(AOS, LOS),
  ...over,
})

/** A live tracking badge with everything the dome readout can show. */
const status = (over: Partial<SatTrackStatus> = {}): SatTrackStatus => ({
  name: 'RS-44',
  state: 'tracking',
  mode: 'rotor+doppler',
  dopplerDownlink: true,
  dopplerUplink: true,
  uplinkOffer: 'none',
  uplinkOfferMap: null,
  uplinkRadio: '',
  uplinkRadioId: 0,
  azDeg: 141,
  elDeg: 46,
  aosAzDeg: 100,
  maxElDeg: 45,
  satAzDeg: 143,
  satElDeg: 47,
  rangeKm: 812,
  rangeRateKmS: -5.42,
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
  aosUnix: AOS,
  losUnix: LOS,
  ...over,
})

const settings = (over: Record<string, unknown> = {}) => ({
  mygrid: 'EN52',
  rotatorModel: 2, // a rotor IS configured, so the track badge is polled
  rotatorHost: '',
  satDoppler: false,
  satVfoMap: 'off',
  ...over,
})

beforeEach(() => {
  localStorage.clear()
  api.getSatellites.mockReset()
  api.getSatellites.mockImplementation(() => Promise.resolve(view()))
  api.getSatSchedule.mockReset()
  api.getSatSchedule.mockImplementation(() => Promise.resolve([]))
  api.getSatPassNeeds.mockReset()
  api.getSatPassNeeds.mockImplementation(() => Promise.resolve([]))
  api.getSatDetail.mockReset()
  api.getSatDetail.mockImplementation(() => Promise.resolve(null))
  api.getSettings.mockReset()
  api.getSettings.mockImplementation(() => Promise.resolve(settings()))
  api.getSatTrackStatus.mockReset()
  api.getSatTrackStatus.mockImplementation(() => Promise.resolve(null))
})
afterEach(cleanup)

/** The row in the Birds list for `name` (the list is the only surface rendering it here). */
async function birdRow(name: string): Promise<HTMLElement> {
  const el = (await screen.findByRole('button', { name })).closest('li')
  expect(el, `no Birds row for ${name}`).toBeTruthy()
  return el as HTMLElement
}

describe('the Birds list carries each bird’s altitude', () => {
  it('shows how high every placed bird is, right now', async () => {
    render(<SatellitesView />)
    expect((await birdRow('RS-44')).textContent).toMatch(/alt 1234 km/)
    // The low LEO reads differently — the number is per-bird truth, not a constant.
    expect((await birdRow('SO-50')).textContent).toMatch(/alt 630 km/)
  })

  it('LABELS it — an unlabelled figure in a satellite row reads as range', async () => {
    render(<SatellitesView />)
    const text = (await birdRow('RS-44')).textContent ?? ''
    expect(text).toMatch(/alt 1234 km/)
    expect(text.replace('alt 1234 km', '')).not.toMatch(/km/)
  })

  it('shows no altitude for a bird nothing carries elements for — absent, not 0 km', async () => {
    api.getSatellites.mockImplementation(() =>
      Promise.resolve(
        view({
          birds: [bird('RS-44', 44909, 1234.4)],
          // No elements at all: there is no subpoint, so there is no altitude
          // to state. The row still exists (it is searchable and starrable) —
          // it simply says nothing it does not know.
          excluded: [{ name: 'AO-7', norad: 7530, status: 'alive', amateur: true, reason: 'noElements' }],
        }),
      ),
    )
    render(<SatellitesView />)
    await birdRow('RS-44')
    // Excluded birds are listed when a search names them.
    fireEvent.change(screen.getByPlaceholderText('search…'), { target: { value: 'AO-7' } })
    const text = (await birdRow('AO-7')).textContent ?? ''
    expect(text).not.toMatch(/km/)
    expect(text).not.toMatch(/alt/)
  })
})

describe('the sky-dome readout carries the bird’s altitude beside its range', () => {
  /** The dome's enclosing block (svg + the text readout), once detail has settled. */
  async function sky(): Promise<HTMLElement> {
    const el = (await screen.findByRole('img')).closest('.sat-sky')
    expect(el).toBeTruthy()
    return el as HTMLElement
  }

  beforeEach(() => {
    api.getSatDetail.mockImplementation(() => Promise.resolve(detail()))
  })

  it('names both numbers, so range and altitude cannot be confused', async () => {
    api.getSatTrackStatus.mockImplementation(() => Promise.resolve(status({ altKm: 1234.6 })))
    render(<SatellitesView focusSat="RS-44" />)
    await screen.findByTestId('sat-ghost')
    const text = (await sky()).textContent ?? ''
    expect(text).toMatch(/Altitude/)
    expect(text).toMatch(/1235 km/)
    // …and the range row it sits beside is untouched and still its own thing.
    expect(text).toMatch(/Range/)
    expect(text).toMatch(/812 km/)
  })

  it('omits the altitude row when the track has computed none — never 0 km', async () => {
    // Before AOS the tick commands nothing and computes no position. A dome
    // reading "0 km" there would put the bird on the ground.
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(status({ altKm: null, rangeKm: null, rangeRateKmS: null })),
    )
    render(<SatellitesView focusSat="RS-44" />)
    await screen.findByTestId('sat-ghost')
    const text = (await sky()).textContent ?? ''
    expect(text).not.toMatch(/Altitude/)
    expect(text).not.toMatch(/km/)
  })
})
