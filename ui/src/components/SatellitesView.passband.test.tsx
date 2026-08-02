// @vitest-environment jsdom
//
// The transponder passband strip: where the operator sits INSIDE the passband, both legs on the
// one axis they genuinely share (offset from centre — they are on different bands, so absolute
// frequency is not one).
//
// What is pinned here is the MEANING of the picture, not its layout:
//
//  - INVERSION IS THE GEOMETRY. On an inverting transponder the uplink sits at the negated
//    offset, so the two cursors must land on opposite sides of centre; non-inverting puts them
//    on the same side. That mirror is the whole lesson of the instrument — the single most
//    confusing thing in satellite operating, and invisible in a text readout.
//  - DOPPLER MOVES THE DIAL, NOT THE OPERATOR. Correction keeps your place in the passband
//    where you put it, so a changing shift must not slide the cursors. Sliding them would
//    animate a thing that is not moving.
//  - No offset = Doppler is not tuning: no strip at all (the Doppler readout above already
//    says why, and saying it twice is noise).
//  - No passband WIDTH (plenty of SatNOGS transmitter records have none) = no axis. You cannot
//    size an axis you do not have; one line says so and the exact offsets still print.
//  - Outside the passband the mark parks on the edge and the NUMBERS stay true.
//  - The colourblind secondary encoding is load-bearing, not decoration: --tx/--rx separate by
//    only ΔE 6.1 under deuteranopia in the light theme, which is permissible only because each
//    leg is on its own row with a text label and a direction glyph. A failure in that test is
//    an ACCESSIBILITY REGRESSION, not a cosmetic one.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import { SatellitesView } from './SatellitesView'
import type { SatDetail, SatTrackStatus } from '../types'

const api = vi.hoisted(() => ({
  getSatellites: vi.fn(() => Promise.resolve(null)),
  getSatSchedule: vi.fn(() => Promise.resolve([])),
  getSatPassNeeds: vi.fn(() => Promise.resolve([])),
  getSatDetail: vi.fn(),
  getSettings: vi.fn(),
  setSettings: vi.fn(() => Promise.resolve({} as never)),
  setSatTransponder: vi.fn(() => Promise.resolve()),
  getSatTransponder: vi.fn((): Promise<import('../types').SatTransponderHeld | null> => Promise.resolve(null)),
  startSatTrack: vi.fn(() => Promise.resolve(null)),
  stopSatTrack: vi.fn(() => Promise.resolve()),
  getSatTrackStatus: vi.fn((): Promise<SatTrackStatus | null> => Promise.resolve(null)),
}))
vi.mock('../api', () => api)
// The detail pane embeds the globe; it needs a canvas and nothing here is about the map.
vi.mock('./MapView', () => ({ MapView: () => null }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

const NOW = Math.floor(Date.now() / 1000)
const AOS = NOW - 300
const LOS = NOW + 300

const detail = (): SatDetail => ({
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
  passTrack: [
    [AOS, 100, 0],
    [NOW, 180, 62],
    [LOS, 260, 0],
  ],
})

/** A tracking status with Doppler live on an inverting linear transponder: the
 * operator is 3.2 kHz UP the passband, which is ±12.5 kHz wide. */
const status = (over: Partial<SatTrackStatus> = {}): SatTrackStatus => ({
  name: 'RS-44',
  state: 'tracking',
  mode: 'rotor+doppler',
  dopplerDownlink: true,
  dopplerUplink: true,
  uplinkOffer: 'none',
  uplinkOfferMap: null,
  uplinkRadio: 'IC-9700',
  uplinkRadioId: 1,
  azDeg: 141,
  elDeg: 46,
  aosAzDeg: 100,
  satAzDeg: 143,
  satElDeg: 47,
  rangeKm: 812,
  rangeRateKmS: -5.42,
  downlinkHz: 435_643_320,
  uplinkHz: 145_962_680,
  downlinkShiftHz: -2310,
  uplinkShiftHz: 770,
  transponder: 'SSB/CW linear transponder',
  transponderIndex: 2,
  inverting: true,
  offsetHz: 3200,
  halfWidthHz: 12_500,
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
  satDopplerOff: false,
  satVfoMap: 'main-down-sub-up',
  ...over,
})

beforeEach(() => {
  localStorage.clear()
  api.getSatellites.mockClear()
  api.getSatDetail.mockReset()
  api.getSatDetail.mockImplementation(() => Promise.resolve(detail()))
  api.getSettings.mockReset()
  api.getSettings.mockImplementation(() => Promise.resolve(settings()))
  api.getSatTrackStatus.mockReset()
  api.getSatTrackStatus.mockImplementation(() => Promise.resolve(status()))
})
afterEach(cleanup)

/** The strip, once the detail load has settled. */
const strip = () => screen.findByTestId('sat-passband')

/** Where a cursor was DRAWN, in viewBox units (the lane translates the mark). */
async function cursorX(which: 'down' | 'up'): Promise<number> {
  const g = await screen.findByTestId(`sat-pb-${which}`)
  const m = /translate\(\s*(-?[\d.]+)/.exec(g.getAttribute('transform') ?? '')
  expect(m, `no translate on the ${which} cursor`).toBeTruthy()
  return Number(m![1])
}

/** The 0 kHz hairline's x — read off the drawing rather than hard-coded, so
 * the axis can be re-proportioned without rewriting every assertion. */
function centreX(container: HTMLElement): number {
  const line = container.querySelector('.sat-pb-centre')
  expect(line).toBeTruthy()
  return Number(line!.getAttribute('x1'))
}

describe('the passband strip — inversion is the geometry', () => {
  it('INVERTING: the two cursors land on OPPOSITE sides of centre', async () => {
    const { container } = render(<SatellitesView focusSat="RS-44" />)
    await strip()
    const c = centreX(container)
    const down = await cursorX('down')
    const up = await cursorX('up')
    // +3.2 kHz on the downlink, so the uplink is at −3.2 kHz: tune up, transmit down.
    expect(down).toBeGreaterThan(c)
    expect(up).toBeLessThan(c)
    // Mirrored about centre, not merely on opposite sides.
    expect(down - c).toBeCloseTo(c - up, 6)
  })

  it('NON-INVERTING: both cursors land on the SAME side of centre', async () => {
    api.getSatTrackStatus.mockImplementation(() => Promise.resolve(status({ inverting: false })))
    const { container } = render(<SatellitesView focusSat="RS-44" />)
    await strip()
    const c = centreX(container)
    expect(await cursorX('down')).toBeGreaterThan(c)
    expect(await cursorX('up')).toBeGreaterThan(c)
    expect(await cursorX('down')).toBeCloseTo(await cursorX('up'), 6)
  })

  it('says which kind of transponder it is in WORDS as well as in the drawing', async () => {
    render(<SatellitesView focusSat="RS-44" />)
    expect((await strip()).textContent).toMatch(/inverting — tune up, transmit down/)
    cleanup()
    api.getSatTrackStatus.mockImplementation(() => Promise.resolve(status({ inverting: false })))
    render(<SatellitesView focusSat="RS-44" />)
    expect((await strip()).textContent).toMatch(/non-inverting — both legs move the same way/)
  })
})

describe('the passband strip — accessibility', () => {
  // A FAILURE HERE IS AN ACCESSIBILITY REGRESSION, NOT A COSMETIC ONE. --tx and --rx are
  // ΔE 6.1 apart under deuteranopia in the light theme; the marks are only distinguishable
  // because each leg has its own row, a text label, and a direction glyph.
  it('ACCESSIBILITY: each leg carries a text label AND a direction glyph — the colourblind secondary encoding for --tx/--rx', async () => {
    render(<SatellitesView focusSat="RS-44" />)
    const el = await strip()
    const text = el.textContent ?? ''
    expect(text).toMatch(/↓ Downlink/)
    expect(text).toMatch(/↑ Uplink/)
    // Both marks exist and are separately identified — colour is never the only channel.
    expect(el.querySelector('.sat-pb-mark.rx')).toBeTruthy()
    expect(el.querySelector('.sat-pb-mark.tx')).toBeTruthy()
    // And the shape differs too: the pointers aim at their track from opposite sides.
    const downTri = (await screen.findByTestId('sat-pb-down')).querySelector('path')
    const upTri = (await screen.findByTestId('sat-pb-up')).querySelector('path')
    expect(downTri?.getAttribute('d')).not.toBe(upTri?.getAttribute('d'))
  })

  it('carries a text equivalent with the exact numbers, and it is NOT an aria-live region', async () => {
    render(<SatellitesView focusSat="RS-44" />)
    const el = await strip()
    const dl = el.querySelector('.sat-pb-readout')
    expect(dl?.textContent).toMatch(/435643320 Hz/)
    expect(dl?.textContent).toMatch(/145962680 Hz/)
    expect(dl?.textContent).toMatch(/offset \+3\.20 kHz/)
    expect(dl?.textContent).toMatch(/offset -3\.20 kHz/)
    // These change on every poll: announcing them would talk over the operator all pass.
    expect(dl?.getAttribute('aria-live')).toBeNull()
    expect(el.querySelector('[aria-live]')).toBeNull()
    // The drawing itself is labelled for a screen reader.
    expect(el.querySelector('svg')?.getAttribute('aria-label')).toMatch(/inverting/)
  })

  it('gives each cursor a ≥24 px hit target and a tooltip with the exact Hz', async () => {
    render(<SatellitesView focusSat="RS-44" />)
    const hit = (await screen.findByTestId('sat-pb-down')).querySelector('.sat-pb-hit')
    expect(Number(hit?.getAttribute('width'))).toBeGreaterThanOrEqual(24)
    expect(Number(hit?.getAttribute('height'))).toBeGreaterThanOrEqual(24)
    expect(hit?.querySelector('title')?.textContent).toMatch(/435643320 Hz/)
    expect(hit?.querySelector('title')?.textContent).toMatch(/offset \+3\.20 kHz/)
  })
})

describe('the passband strip — honesty', () => {
  it('renders NOTHING when there is no offset — Doppler is not tuning, and the readout says why', async () => {
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(
        status({
          downlinkHz: null,
          uplinkHz: null,
          downlinkShiftHz: null,
          uplinkShiftHz: null,
          transponder: null,
          transponderIndex: null,
          inverting: false,
          offsetHz: null,
          halfWidthHz: null,
        }),
      ),
    )
    const { container } = render(<SatellitesView focusSat="RS-44" />)
    // The Doppler readout above still renders: that is the one place the reason belongs.
    await screen.findByTestId('sat-ghost')
    expect(container.querySelector('.sat-doppler')).toBeTruthy()
    expect(screen.queryByTestId('sat-passband')).toBeNull()
  })

  it('draws NO passband at zero width — and blames neither cause, because it cannot tell', async () => {
    api.getSatTrackStatus.mockImplementation(() => Promise.resolve(status({ halfWidthHz: 0 })))
    render(<SatellitesView focusSat="RS-44" />)
    const el = await strip()
    // No axis: you cannot size one you do not have.
    expect(el.querySelector('svg')).toBeNull()
    expect(el.querySelector('.sat-pb-track')).toBeNull()
    expect(screen.queryByTestId('sat-pb-down')).toBeNull()
    // Zero width means EITHER an FM channel (SO-50 and the rest of the easy
    // birds) OR a linear transponder whose SatNOGS record has no upper edge.
    // They are identical on the wire, so the line must name both rather than
    // blame the database for a satellite that simply has no passband.
    expect(el.textContent).toMatch(/No passband to tune inside/)
    expect(el.textContent).toMatch(/single channel/)
    expect(el.textContent).toMatch(/SatNOGS carries no width/)
    // The offsets are still known, so they are still printed — exactly, in Hz.
    expect(el.querySelector('.sat-pb-readout')?.textContent).toMatch(/435643320 Hz/)
    expect(el.textContent).not.toMatch(/kHz from centre/) // no invented extent
  })

  it('treats a null width the same as a zero one — never a zero-wide axis', async () => {
    api.getSatTrackStatus.mockImplementation(() => Promise.resolve(status({ halfWidthHz: null })))
    render(<SatellitesView focusSat="RS-44" />)
    const el = await strip()
    expect(el.querySelector('svg')).toBeNull()
    expect(el.textContent).toMatch(/No passband to tune inside/)
  })

  it('parks a cursor on the edge when the operator is outside the passband, but prints the TRUE offset', async () => {
    // 30 kHz up a ±12.5 kHz passband: drawable only at the edge.
    api.getSatTrackStatus.mockImplementation(() => Promise.resolve(status({ offsetHz: 30_000 })))
    const { container } = render(<SatellitesView focusSat="RS-44" />)
    const el = await strip()
    const c = centreX(container)
    const down = await cursorX('down')
    const track = el.querySelector('.sat-pb-track')!
    const right = Number(track.getAttribute('x')) + Number(track.getAttribute('width'))
    expect(down).toBeLessThanOrEqual(right)
    expect(down - c).toBeCloseTo(right - c, 6) // pinned to the edge, not past it
    // The numbers do not lie about it, and the tooltip says the mark is parked.
    expect(el.querySelector('.sat-pb-readout')?.textContent).toMatch(/offset \+30\.00 kHz/)
    expect(
      (await screen.findByTestId('sat-pb-down')).querySelector('title')?.textContent,
    ).toMatch(/outside the passband/)
  })

  it('DOPPLER DOES NOT SLIDE THE BAND: a different shift, same offset, same cursor', async () => {
    // Doppler moves the DIAL so the operator's place in the passband stays put. If a changing
    // shift moved a cursor, the instrument would animate something that is not moving.
    const { container } = render(<SatellitesView focusSat="RS-44" />)
    await strip()
    const before = await cursorX('down')
    const c = centreX(container)
    cleanup()
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(
        status({ downlinkHz: 435_651_900, downlinkShiftHz: 6290, uplinkShiftHz: -2100 }),
      ),
    )
    const { container: c2 } = render(<SatellitesView focusSat="RS-44" />)
    await strip()
    expect(await cursorX('down')).toBeCloseTo(before, 6)
    expect(centreX(c2)).toBeCloseTo(c, 6)
    // …and the new dial frequency IS shown, in the row's direct label.
    expect((await strip()).textContent).toMatch(/435\.65190 MHz/)
  })

  it('labels each row with its own live dial frequency and signed shift', async () => {
    render(<SatellitesView focusSat="RS-44" />)
    const el = await strip()
    expect(el.textContent).toMatch(/↓ Downlink 435\.64332 MHz -2\.31 kHz/)
    expect(el.textContent).toMatch(/↑ Uplink 145\.96268 MHz \+770 Hz/)
  })
})
