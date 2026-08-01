// @vitest-environment jsdom
//
// The hero sky dome, its rotator ghost, the pass timeline and the Doppler readout.
//
// What is pinned here is HONESTY, not layout. Every one of these assertions exists because the
// alternative is a picture that lies about the station:
//
//  - The ghost is the antenna's COMMANDED position. When the rotator is az-only the backend
//    reports elDeg as ABSENT because it never sent an elevation — drawing a ghost there would
//    put the antenna on the horizon in the operator's mind while it is actually pointing at a
//    bird 47° up. Az-only draws an azimuth SPOKE and says so; it never invents an elevation.
//    (The backend reports absence rather than a sentinel 0 precisely so this file can pin
//    behaviour instead of a guess: a real commanded el 0 — prepositioning on the horizon — and
//    "no elevation was ever sent" are different states and now look different on the wire.)
//  - `armed` sends no rotor commands at all, so there is no commanded position to draw, and
//    azDeg is absent rather than carrying the AOS azimuth dressed up as a command.
//  - A track running on a DIFFERENT bird must not decorate this one's dome.
//  - Doppler frequencies are nullable and null means "not tuning that leg": no zeros, no
//    placeholder dashes — a short line saying why instead.
//  - An SVG instrument is invisible to a screen reader, so the dome carries a DOM text
//    equivalent (az/el/range) and a role="img" label.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, waitFor } from '@testing-library/react'
import { SatellitesView } from './SatellitesView'
import type { SatDetail, SatPass, SatTrackStatus } from '../types'

const api = vi.hoisted(() => ({
  // Typed: the stale-chip case below hands back a real SatView, and an
  // inferred `Promise<null>` would reject it at compile time.
  getSatellites: vi.fn((): Promise<import('../types').SatView | null> => Promise.resolve(null)),
  getSatSchedule: vi.fn(() => Promise.resolve([])),
  getSatPassNeeds: vi.fn((): Promise<SatPass[]> => Promise.resolve([])),
  getSatDetail: vi.fn(),
  getSettings: vi.fn(),
  setSettings: vi.fn(() => Promise.resolve({} as never)),
  setSatTransponder: vi.fn(() => Promise.resolve()),
  getSatTransponder: vi.fn((): Promise<import('../types').SatTransponderHeld | null> => Promise.resolve(null)),
  startSatTrack: vi.fn(() => Promise.resolve(null)),
  stopSatTrack: vi.fn(() => Promise.resolve()),
  // Typed: several cases below hand back a full status, and an inferred
  // `Promise<null>` would reject them at compile time.
  getSatTrackStatus: vi.fn((): Promise<SatTrackStatus | null> => Promise.resolve(null)),
}))
vi.mock('../api', () => api)
// The detail pane embeds the globe; it needs a canvas and nothing here is about the map.
vi.mock('./MapView', () => ({ MapView: () => null }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

const NOW = Math.floor(Date.now() / 1000)
const AOS = NOW - 300
const LOS = NOW + 300

/** A pass shaped like a real one: rises in the east, peaks, sets in the west. */
function samples(aos: number, los: number): [number, number, number][] {
  const out: [number, number, number][] = []
  const n = 20
  for (let i = 0; i <= n; i++) {
    out.push([
      Math.round(aos + ((los - aos) * i) / n),
      100 + (160 * i) / n,
      60 * Math.sin((Math.PI * i) / n),
    ])
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

/** A tracking status with everything the sky dome can draw; override per case. */
const status = (over: Partial<SatTrackStatus> = {}): SatTrackStatus => ({
  name: 'RS-44',
  state: 'tracking',
  mode: 'rotor+doppler',
  azDeg: 141,
  elDeg: 46,
  aosAzDeg: 100,
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
  rotatorModel: 2, // a rotor IS configured, so the track status is polled
  rotatorHost: '',
  satDoppler: false,
  satVfoMap: 'off',
  ...over,
})

beforeEach(() => {
  localStorage.clear()
  api.getSatellites.mockReset()
  api.getSatellites.mockImplementation(() => Promise.resolve(null))
  api.getSatDetail.mockReset()
  api.getSatDetail.mockImplementation(() => Promise.resolve(detail()))
  api.getSettings.mockReset()
  api.getSettings.mockImplementation(() => Promise.resolve(settings()))
  api.getSatTrackStatus.mockReset()
  api.getSatTrackStatus.mockImplementation(() => Promise.resolve(null))
})
afterEach(cleanup)

/** The dome element, once the detail load has settled. */
const dome = () => screen.findByRole('img')
/** The dome's enclosing block (svg + text readout). */
async function sky(): Promise<HTMLElement> {
  const el = (await dome()).closest('.sat-sky')
  expect(el).toBeTruthy()
  return el as HTMLElement
}

describe('the sky dome', () => {
  it('draws the pass and the bird while the pass is live', async () => {
    render(<SatellitesView focusSat="RS-44" />)
    const svg = await dome()
    expect(svg.getAttribute('aria-label')).toMatch(/RS-44/)
    // The bird is only drawn when it is actually up.
    expect(await screen.findByTestId('sat-bird')).toBeTruthy()
    // Elevation rings are labelled, not bare circles.
    expect(svg.textContent).toMatch(/30°/)
    expect(svg.textContent).toMatch(/60°/)
    expect(svg.textContent).toMatch(/N/)
  })

  it('draws no bird before AOS — a pass that has not started has no position', async () => {
    api.getSatDetail.mockImplementation(() =>
      Promise.resolve(
        detail({
          pass: {
            name: 'RS-44',
            aosUnix: NOW + 1800,
            losUnix: NOW + 2400,
            maxElDeg: 62,
            aosAzDeg: 100,
            losAzDeg: 260,
            status: 'alive',
          },
          passTrack: samples(NOW + 1800, NOW + 2400),
        }),
      ),
    )
    render(<SatellitesView focusSat="RS-44" />)
    await dome()
    expect(screen.queryByTestId('sat-bird')).toBeNull()
  })

  it('reads out az/el and range as DOM text, not just pixels', async () => {
    api.getSatTrackStatus.mockImplementation(() => Promise.resolve(status()))
    render(<SatellitesView focusSat="RS-44" />)
    await screen.findByTestId('sat-ghost')
    const text = (await sky()).textContent ?? ''
    expect(text).toMatch(/Satellite/)
    expect(text).toMatch(/az 143° el 47°/)
    expect(text).toMatch(/812 km/)
    expect(text).toMatch(/-5\.42 km\/s closing/)
  })

  it('omits range entirely when the backend has none (before AOS there is nothing to measure)', async () => {
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(status({ rangeKm: null, rangeRateKmS: null })),
    )
    render(<SatellitesView focusSat="RS-44" />)
    await screen.findByTestId('sat-ghost')
    const text = (await sky()).textContent ?? ''
    expect(text).not.toMatch(/Range/)
    expect(text).not.toMatch(/km/)
  })
})

describe('the rotator ghost', () => {
  it('draws the commanded position and the gap to the bird', async () => {
    api.getSatTrackStatus.mockImplementation(() => Promise.resolve(status()))
    render(<SatellitesView focusSat="RS-44" />)
    const ghost = await screen.findByTestId('sat-ghost')
    // A ring plus the dashed connector: the connector IS the tracking error.
    expect(ghost.querySelector('.sat-dome-ghost-ring')).toBeTruthy()
    expect(ghost.querySelector('.sat-dome-err')).toBeTruthy()
    const text = (await sky()).textContent ?? ''
    expect(text).toMatch(/az 141° el 46°/)
    // The error is the true angular separation, so the operator never subtracts
    // two rows of numbers by hand.
    expect(text).toMatch(/Δ 1\.[0-9]°/)
  })

  it('AZ-ONLY: never draws an elevation the rotator was never sent', async () => {
    // The backend reports elDeg absent after falling back to `point()`. Drawing a ghost dot
    // there would claim the antenna is on the horizon while the bird is 47° up.
    api.getSatTrackStatus.mockImplementation(() => Promise.resolve(status({ elDeg: null })))
    render(<SatellitesView focusSat="RS-44" />)
    expect(await screen.findByTestId('sat-ghost-az')).toBeTruthy()
    expect(screen.queryByTestId('sat-ghost')).toBeNull()
    const text = (await sky()).textContent ?? ''
    expect(text).toMatch(/elevation not commanded \(az-only rotator\)/)
    expect(text).toMatch(/az 141°/)
    expect(text).not.toMatch(/el 0°/)
    // No fabricated error either — there is no commanded elevation to compare.
    expect(text).not.toMatch(/Δ/)
  })

  it('ARMED: draws no ghost at all — the loop has commanded nothing yet', async () => {
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(
        status({
          state: 'armed',
          azDeg: null,
          elDeg: null,
          satAzDeg: null,
          satElDeg: null,
          rangeKm: null,
          rangeRateKmS: null,
        }),
      ),
    )
    render(<SatellitesView focusSat="RS-44" />)
    await dome()
    expect(screen.queryByTestId('sat-ghost')).toBeNull()
    expect(screen.queryByTestId('sat-ghost-az')).toBeNull()
    expect((await sky()).textContent).toMatch(/no rotor command sent yet/)
  })

  it('PREPOSITIONING: a genuinely commanded el 0 draws a real ghost on the horizon', async () => {
    // The case a sentinel-0 wire format could not express. Waiting on the AOS azimuth at the
    // horizon IS a command that was sent, and it must look different from an az-only rotor
    // that was never sent one — otherwise the operator cannot tell "parked, ready" from
    // "this rotator has no elevation at all".
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(
        status({
          state: 'prepositioning',
          azDeg: 100,
          elDeg: 0,
          satAzDeg: null,
          satElDeg: null,
          rangeKm: null,
          rangeRateKmS: null,
        }),
      ),
    )
    render(<SatellitesView focusSat="RS-44" />)
    expect(await screen.findByTestId('sat-ghost')).toBeTruthy()
    expect(screen.queryByTestId('sat-ghost-az')).toBeNull()
    const text = (await sky()).textContent ?? ''
    expect(text).toMatch(/az 100° el 0°/)
    expect(text).not.toMatch(/az-only/)
  })

  it('a track on another bird never decorates this dome', async () => {
    api.getSatTrackStatus.mockImplementation(() => Promise.resolve(status({ name: 'AO-91' })))
    render(<SatellitesView focusSat="RS-44" />)
    await dome()
    expect(screen.queryByTestId('sat-ghost')).toBeNull()
    expect(screen.queryByTestId('sat-ghost-az')).toBeNull()
    expect((await sky()).textContent).not.toMatch(/Antenna/)
  })

  it('the tracking badge does not print an elevation for an az-only rotor either', async () => {
    api.getSatTrackStatus.mockImplementation(() => Promise.resolve(status({ elDeg: null })))
    render(<SatellitesView focusSat="RS-44" />)
    const badge = await screen.findByTitle(/Auto-track is driving the rotor/)
    expect(badge.textContent).toMatch(/\(az only\)/)
    expect(badge.textContent).not.toMatch(/el 0°/)
  })

  it('the armed badge shows the RISE azimuth, never a command it withheld', async () => {
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(status({ state: 'armed', azDeg: null, elDeg: null, satAzDeg: null, satElDeg: null })),
    )
    render(<SatellitesView focusSat="RS-44" />)
    const badge = await screen.findByTitle(/has NOT been commanded yet/)
    expect(badge.textContent).toMatch(/armed RS-44/)
    expect(badge.textContent).toMatch(/rises az 100°/)
    expect(badge.textContent).not.toMatch(/cmd az/)
  })

  it('the PHASE word comes from the phase, not from whether a command exists', async () => {
    // Arming a pass that is ALREADY under way reports "tracking" before the
    // loop has commanded anything — and a rotor that stops answering mid-pass
    // keeps its phase while the command goes stale. Both are states where a
    // badge reading the phase off `azDeg` would announce "armed" while the
    // toast beside it says the pass is running: two contradictory claims on
    // screen, one of them false.
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(status({ state: 'tracking', azDeg: null, elDeg: null })),
    )
    render(<SatellitesView focusSat="RS-44" />)
    const badge = await screen.findByTitle(/has NOT been commanded yet/)
    expect(badge.textContent).toMatch(/tracking RS-44/)
    expect(badge.textContent).not.toMatch(/armed/)
    // …and it still refuses to print a command that was never sent.
    expect(badge.textContent).toMatch(/rises az 100°/)
    expect(badge.textContent).not.toMatch(/cmd az/)
  })
})

describe('the pass timeline', () => {
  it('shows AOS, TCA and LOS with the max elevation', async () => {
    const { container } = render(<SatellitesView focusSat="RS-44" />)
    await dome()
    const tl = container.querySelector('.sat-timeline')
    expect(tl?.textContent).toMatch(/AOS \d\d:\d\d/)
    expect(tl?.textContent).toMatch(/TCA \d\d:\d\d/)
    expect(tl?.textContent).toMatch(/LOS \d\d:\d\d/)
    expect(tl?.textContent).toMatch(/max 62°/)
    expect(tl?.textContent).toMatch(/IN PASS/)
    // The live position is marked on the rail.
    expect(container.querySelector('.sat-tl-now')).toBeTruthy()
  })

  it('marks no TCA when there is no computed track to take it from', async () => {
    api.getSatDetail.mockImplementation(() => Promise.resolve(detail({ passTrack: [] })))
    const { container } = render(<SatellitesView focusSat="RS-44" />)
    await dome()
    const tl = container.querySelector('.sat-timeline')
    expect(tl?.textContent).not.toMatch(/TCA/)
    expect(tl?.textContent).toMatch(/max 62°/)
    expect(container.querySelector('.sat-tl-tca')).toBeNull()
  })
})

describe('the Doppler readout', () => {
  it('shows both legs, the per-leg shift and the inverting state', async () => {
    api.getSettings.mockImplementation(() =>
      Promise.resolve(settings({ satDoppler: true, satVfoMap: 'main-down-sub-up' })),
    )
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(
        status({
          downlinkHz: 435_640_120,
          uplinkHz: 145_965_880,
          downlinkShiftHz: -2310,
          uplinkShiftHz: 770,
          transponder: 'SSB/CW linear transponder',
          transponderIndex: 2,
          inverting: true,
        }),
      ),
    )
    const { container } = render(<SatellitesView focusSat="RS-44" />)
    await screen.findByTestId('sat-ghost')
    const dop = container.querySelector('.sat-doppler')
    expect(dop?.textContent).toMatch(/↓ Downlink/)
    expect(dop?.textContent).toMatch(/435\.64012 MHz/)
    expect(dop?.textContent).toMatch(/-2\.31 kHz/)
    expect(dop?.textContent).toMatch(/↑ Uplink/)
    expect(dop?.textContent).toMatch(/145\.96588 MHz/)
    expect(dop?.textContent).toMatch(/\+770 Hz/)
    expect(dop?.textContent).toMatch(/INVERTING/)
    expect(dop?.textContent).toMatch(/SSB\/CW linear transponder/)
  })

  it('shows NOTHING but a reason when Doppler is off — no zeros, no dashes', async () => {
    api.getSatTrackStatus.mockImplementation(() => Promise.resolve(status()))
    const { container } = render(<SatellitesView focusSat="RS-44" />)
    await screen.findByTestId('sat-ghost')
    const dop = container.querySelector('.sat-doppler')
    expect(dop?.textContent).toMatch(/Doppler is off/)
    // Not one frequency, not one placeholder row: the legs are absent, not blank.
    // (The em dash in the sentence above is prose — what must never appear is a
    // dash or a zero standing in the PLACE of a number.)
    expect(dop?.querySelector('.sat-dop-legs')).toBeNull()
    expect(dop?.textContent).not.toMatch(/Hz/)
    expect(dop?.textContent).not.toMatch(/0\.00000/)
  })

  it('names the VFO mapping as the reason when that is what is blocking it', async () => {
    api.getSettings.mockImplementation(() => Promise.resolve(settings({ satDoppler: true })))
    api.getSatTrackStatus.mockImplementation(() => Promise.resolve(status()))
    const { container } = render(<SatellitesView focusSat="RS-44" />)
    await screen.findByTestId('sat-ghost')
    expect(container.querySelector('.sat-doppler')?.textContent).toMatch(/VFO mapping is Off/)
  })

  it('asks for a transponder when both switches are on and none is held', async () => {
    api.getSettings.mockImplementation(() =>
      Promise.resolve(settings({ satDoppler: true, satVfoMap: 'main-down-sub-up' })),
    )
    api.getSatTrackStatus.mockImplementation(() => Promise.resolve(status()))
    const { container } = render(<SatellitesView focusSat="RS-44" />)
    await screen.findByTestId('sat-ghost')
    expect(container.querySelector('.sat-doppler')?.textContent).toMatch(/No transponder selected/)
  })

  it('is absent entirely when nothing is tracking this bird', async () => {
    const { container } = render(<SatellitesView focusSat="RS-44" />)
    await dome()
    expect(container.querySelector('.sat-doppler')).toBeNull()
  })
})

describe('rotor-less honesty — the Antenna row', () => {
  it('never promises a rotor command on a track with no rotor half', async () => {
    // A doppler-only track will NEVER command a rotor — the DTO `mode` is the
    // engine's own answer. "armed — no rotor command sent yet" there promises
    // an antenna event that cannot come; the row must be ABSENT (the rail's
    // Rotor row already carries the "no rotor in this track" story).
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(status({ mode: 'doppler-only', azDeg: null, elDeg: null })),
    )
    render(<SatellitesView focusSat="RS-44" />)
    await screen.findByTestId('sat-bird')
    const text = (await sky()).textContent ?? ''
    expect(text).toMatch(/Satellite/) // the bird's own readout still renders
    expect(text).not.toMatch(/Antenna/)
    expect(text).not.toMatch(/no rotor command/)
  })

  it('keeps the armed promise when the track really does hold the rotor', async () => {
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(status({ state: 'armed', azDeg: null, elDeg: null })),
    )
    render(<SatellitesView focusSat="RS-44" />)
    await dome()
    expect((await sky()).textContent).toMatch(/no rotor command sent yet/)
  })
})

describe('the passline beyond the detail window', () => {
  // get_sat_detail's pass window is 24 h; the schedule sees 48 h. A clicked
  // 24–48 h row used to render "no pass over you in the next 24 h" directly
  // under the row promising that exact pass — the pane must cite the schedule's
  // own AOS instead of contradicting it. (Root fix — one shared constant — is
  // backend; this pins the honest fallback.)
  const beyondRow: SatPass = {
    name: 'RS-44',
    aosUnix: NOW + 30 * 3600,
    losUnix: NOW + 30 * 3600 + 600,
    maxElDeg: 44,
    aosAzDeg: 100,
    losAzDeg: 260,
    status: 'alive',
  }

  it('cites the 48 h schedule AOS instead of contradicting the row that opened it', async () => {
    localStorage.setItem('nexus.sats.chasing', JSON.stringify(['RS-44']))
    api.getSatPassNeeds.mockImplementation(() => Promise.resolve([beyondRow]))
    api.getSatDetail.mockImplementation(() =>
      Promise.resolve(detail({ pass: null, passTrack: [] })),
    )
    const { container } = render(<SatellitesView focusSat="RS-44" />)
    await waitFor(() => {
      const line = container.querySelector('.sat-passline')
      expect(line?.textContent).toMatch(/next pass over you rises/)
      expect(line?.textContent).toMatch(/in 30\.\d h/)
      // …and it makes no 24 h claim beside that countdown: detail refreshes a
      // minute behind the schedule, and a pass drifting across the window
      // boundary must not render a sentence that argues with its own numbers
      // ("no pass in the next 24 h — next rises … (in 23.9 h)").
      expect(line?.textContent).not.toMatch(/next 24 h/)
    })
  })

  it('keeps the plain no-pass line when the schedule knows nothing either', async () => {
    api.getSatDetail.mockImplementation(() =>
      Promise.resolve(detail({ pass: null, passTrack: [] })),
    )
    const { container } = render(<SatellitesView focusSat="RS-44" />)
    await waitFor(() => {
      const line = container.querySelector('.sat-passline')
      expect(line?.textContent).toMatch(/no pass over you in the next 24 h/)
      expect(line?.textContent).not.toMatch(/rises/)
    })
  })
})

describe('the stale-TLE chip', () => {
  // The chip family speaks uppercase (one CSS voice: INVERTING, ALIVE, STALE).
  // A single-letter unit does not survive that voice — "21 d" rendered "21 D"
  // is a wrong unit, not a style — so the markup spells the unit out and the
  // transform has nothing to mis-case.
  it('spells the age unit so the uppercase chip voice cannot re-case it', async () => {
    api.getSatellites.mockImplementation(() =>
      Promise.resolve({ tleAgeDays: 21, tleFetchedAt: 1_785_542_400, tleSource: 'mirror', birds: [], passes: [], excluded: [] }),
    )
    const { container } = render(<SatellitesView />)
    await waitFor(() => {
      const chip = container.querySelector('.sat-chip.stale')
      expect(chip?.textContent).toContain('TLE 21 days')
    })
  })
})
