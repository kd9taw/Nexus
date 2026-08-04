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
import { SAT_ICON_RECTS, SAT_ICON_TILT_DEG } from '../features/satIcon'
import type { SatDetail, SatPass, SatTrackStatus } from '../types'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

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
  dopplerDownlink: true,
  dopplerUplink: true,
  uplinkOffer: 'none',
  uplinkOfferMap: null,
  uplinkRadio: 'IC-9700',
  uplinkRadioId: 1,
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
  rotatorModel: 2, // a rotor IS configured, so the track status is polled
  rotatorHost: '',
  satDopplerOff: false,
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

describe("the bird's persistent az/el tag", () => {
  // An operator turning a MANUAL az/el rotator by hand reads these two numbers
  // off the dome and turns the mast to match. A hover tooltip means holding the
  // mouse on a moving dot for the length of a pass, so az AND el ride ON the
  // bird for the whole pass. The <title> stays for assistive tech; it is no
  // longer the only place the numbers exist.

  /** The tag plate's box in viewBox units. */
  function tagBox(tag: Element) {
    const plate = tag.querySelector('.sat-dome-tag-plate')
    expect(plate).toBeTruthy()
    const n = (a: string) => Number(plate!.getAttribute(a))
    return { x: n('x'), y: n('y'), w: n('width'), h: n('height') }
  }

  /** Where the bird glyph sits, read off its own transform. */
  function birdAt(bird: Element): [number, number] {
    const icon = bird.querySelector('.sat-dome-bird-icon')
    expect(icon).toBeTruthy()
    const m = /translate\(([-\d.]+),\s*([-\d.]+)\)/.exec(icon!.getAttribute('transform') ?? '')
    expect(m).toBeTruthy()
    return [Number(m![1]), Number(m![2])]
  }

  it('prints az and el ON the bird, with no hover and no mouse at all', async () => {
    api.getSatTrackStatus.mockImplementation(() => Promise.resolve(status()))
    render(<SatellitesView focusSat="RS-44" />)
    const bird = await screen.findByTestId('sat-bird')
    const tag = bird.querySelector('[data-testid="sat-bird-tag"]')
    expect(tag).toBeTruthy()
    // Rendered graphics text, NOT a <title>: a tooltip is exactly what fails an
    // operator whose hands are on the mast.
    const lines = [...tag!.querySelectorAll('text')].map((t) => t.textContent ?? '')
    expect(lines.join(' ')).toMatch(/az 143°/)
    expect(lines.join(' ')).toMatch(/el 47°/)
    expect(tag!.querySelector('title')).toBeNull()
    // …and the hover equivalent survives alongside it.
    expect(bird.querySelector('title')?.textContent).toMatch(/az 143° el 47°/)
  })

  it('keeps both numbers inside the horizon box at every rim bearing', async () => {
    // The tag must survive the whole pass, and a pass ENDS on the rim. A tag
    // that runs off the east edge is unreadable at exactly the moment the
    // operator is swinging the mast to catch LOS. The bound is the HORIZON box
    // (centre 124 ± radius 100), not the viewBox: the margin outside it carries
    // the compass letters, which the opaque plate would otherwise cover.
    const LO = 24 // DOME_C - DOME_R
    const HI = 224 // DOME_C + DOME_R
    for (const az of [0, 90, 180, 270]) {
      api.getSatTrackStatus.mockImplementation(() =>
        Promise.resolve(status({ azDeg: null, elDeg: null, satAzDeg: az, satElDeg: 2 })),
      )
      render(<SatellitesView focusSat="RS-44" />)
      const bird = await screen.findByTestId('sat-bird')
      const box = tagBox(bird.querySelector('[data-testid="sat-bird-tag"]')!)
      expect(box.x, `az ${az} left edge`).toBeGreaterThanOrEqual(LO)
      expect(box.y, `az ${az} top edge`).toBeGreaterThanOrEqual(LO)
      expect(box.x + box.w, `az ${az} right edge`).toBeLessThanOrEqual(HI)
      expect(box.y + box.h, `az ${az} bottom edge`).toBeLessThanOrEqual(HI)
      cleanup()
    }
  })

  it('takes the side the rotator ghost is not on', async () => {
    // Near the rim the tag has one choice; in open sky it has two, and the one
    // it must not take is the one burying the ghost — the gap between bird and
    // ghost IS the tracking error the operator is judging.
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(status({ azDeg: 120, elDeg: 47, satAzDeg: 143, satElDeg: 47 })),
    )
    render(<SatellitesView focusSat="RS-44" />)
    const bird = await screen.findByTestId('sat-bird')
    const [bx] = birdAt(bird)
    const box = tagBox(bird.querySelector('[data-testid="sat-bird-tag"]')!)
    // az 120 is WEST of az 143 on a north-up dome, so the ghost is drawn to the
    // bird's right; the tag goes left.
    expect(box.x + box.w).toBeLessThanOrEqual(bx)
  })

  it("rides on the bird's right when that side is free", async () => {
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(status({ azDeg: 160, elDeg: 47, satAzDeg: 143, satElDeg: 47 })),
    )
    render(<SatellitesView focusSat="RS-44" />)
    const bird = await screen.findByTestId('sat-bird')
    const [bx] = birdAt(bird)
    expect(tagBox(bird.querySelector('[data-testid="sat-bird-tag"]')!).x).toBeGreaterThanOrEqual(bx)
  })

  it('draws the bird as the same spacecraft glyph the world map draws', async () => {
    // One shape definition, two renderers (canvas on the map, SVG here). A
    // second hand-tuned copy drifts on the first tweak and the operator learns
    // two marks for one object.
    render(<SatellitesView focusSat="RS-44" />)
    const bird = await screen.findByTestId('sat-bird')
    const icon = bird.querySelector('.sat-dome-bird-icon')
    expect(icon).toBeTruthy()
    expect(icon!.querySelectorAll('rect').length).toBe(SAT_ICON_RECTS.length)
    expect(icon!.getAttribute('transform')).toMatch(new RegExp(`rotate\\(${SAT_ICON_TILT_DEG}\\)`))
  })
})

describe('the rise and set marks carry their bearings', () => {
  // The same operator and the same problem as the bird's tag. A MANUAL rotator
  // is pre-pointed at the rise bearing before a pass and swung to the set
  // bearing to hold the end of one, and both numbers lived in a <title>. The
  // set mark is drawn hollow, and an SVG path with no fill answers the pointer
  // on its 1.4 u outline and nowhere else — so "hard to mouse over" was
  // literally true of that one mark and not of its filled twin.
  //
  // Both marks sit ON the horizon by construction, so these plates print an
  // AZIMUTH and no elevation: "el 0°" would be a restatement of the geometry,
  // not a reading. And each plate says WHICH mark it belongs to, because the
  // pair is read to decide which way to turn a mast — two bare bearings near
  // each other would be worse than the hover they replace.

  const RAD = Math.PI / 180
  /** The dome point for a bearing on the horizon (DOME_C 124, DOME_R 100). */
  const rimPt = (az: number): [number, number] => [
    124 + 100 * Math.sin(az * RAD),
    124 - 100 * Math.cos(az * RAD),
  ]
  type Rect = { x: number; y: number; w: number; h: number }
  /** A square around a mark, generous enough to catch a plate grazing it. */
  const around = ([x, y]: [number, number], r: number): Rect => ({ x: x - r, y: y - r, w: 2 * r, h: 2 * r })
  const overlaps = (a: Rect, b: Rect) =>
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h

  /** A tag's plate box in viewBox units. */
  function plate(tag: Element): Rect {
    const r = tag.querySelector('.sat-dome-tag-plate')
    expect(r).toBeTruthy()
    const n = (a: string) => Number(r!.getAttribute(a))
    return { x: n('x'), y: n('y'), w: n('width'), h: n('height') }
  }
  const lines = (tag: Element) => [...tag.querySelectorAll('text')].map((t) => t.textContent ?? '').join(' ')

  /** Both tags, once the detail load has settled. */
  async function tags() {
    const aos = await screen.findByTestId('sat-aos-tag')
    const los = await screen.findByTestId('sat-los-tag')
    return { aos, los }
  }

  /** The fixture pass with the two horizon bearings overridden. */
  const passAt = (aosAz: number, losAz: number) =>
    detail({
      pass: {
        name: 'RS-44',
        aosUnix: AOS,
        losUnix: LOS,
        maxElDeg: 62,
        aosAzDeg: aosAz,
        losAzDeg: losAz,
        status: 'alive',
      },
    })

  it('prints both bearings on the dome with no hover and no mouse at all', async () => {
    render(<SatellitesView focusSat="RS-44" />)
    const { aos, los } = await tags()
    // Rendered graphics text, NOT a <title>. This is the whole point: a tooltip
    // is unreachable for an operator with both hands on the mast.
    expect(lines(aos)).toMatch(/100° E/)
    expect(lines(los)).toMatch(/260° W/)
    expect(aos.querySelector('title')).toBeNull()
    expect(los.querySelector('title')).toBeNull()
  })

  it('says which mark each bearing belongs to', async () => {
    // Two bare bearings sitting near each other are worse than the hover they
    // replace — the operator is reading them to decide which way to turn.
    render(<SatellitesView focusSat="RS-44" />)
    const { aos, los } = await tags()
    expect(lines(aos)).toMatch(/AOS/)
    expect(lines(aos)).not.toMatch(/LOS/)
    expect(lines(los)).toMatch(/LOS/)
    expect(lines(los)).not.toMatch(/AOS/)
    // …and by the same ▲/▼ the readout under the dome already uses for these
    // two, so the plate maps to its mark by shape as well as by word.
    expect(lines(aos)).toMatch(/▲/)
    expect(lines(los)).toMatch(/▼/)
  })

  it('prints an azimuth and never an elevation — both marks ARE the horizon', async () => {
    render(<SatellitesView focusSat="RS-44" />)
    const { aos, los } = await tags()
    expect(lines(aos)).not.toMatch(/el /)
    expect(lines(los)).not.toMatch(/el /)
  })

  it('keeps the hover text, in addition — it was never the replacement', async () => {
    // Queried off the marks themselves: these <title>s hang inside a <path>,
    // which is not the `svg > title` shape findByTitle looks for.
    const { container } = render(<SatellitesView focusSat="RS-44" />)
    await tags()
    expect(container.querySelector('.sat-dome-aos > title')?.textContent).toMatch(
      /AOS — rises at 100° \(E\) \d\d:\d\d/,
    )
    expect(container.querySelector('.sat-dome-los > title')?.textContent).toMatch(
      /LOS — sets at 260° \(W\) \d\d:\d\d/,
    )
  })

  it('makes the see-through set mark hoverable over its whole body', async () => {
    // The root of "it's hard to mouse over": `fill:none` means the default
    // `visiblePainted` hit-tests the 1.4 u outline only. The filled rise mark
    // never had the problem, which is why only one of the two was hard to hit.
    const { container } = render(<SatellitesView focusSat="RS-44" />)
    await tags()
    const los = container.querySelector('.sat-dome-los')
    expect(los?.getAttribute('pointer-events')).toBe('all')
  })

  it('never lets a plate take the pointer off the mark it labels', async () => {
    const { container } = render(<SatellitesView focusSat="RS-44" />)
    const { aos, los } = await tags()
    expect(aos.getAttribute('pointer-events')).toBe('none')
    expect(los.getAttribute('pointer-events')).toBe('none')
    expect(container.querySelector('.sat-dome-los')).toBeTruthy()
  })

  it('renders before AOS too — the rise bearing is what you pre-point BY', async () => {
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
    const { aos, los } = await tags()
    expect(screen.queryByTestId('sat-bird')).toBeNull()
    expect(lines(aos)).toMatch(/100°/)
    expect(lines(los)).toMatch(/260°/)
  })

  it('keeps both plates inside the horizon box at every rim bearing', async () => {
    // The marks sit ON the rim, so a plate placed outward runs into the compass
    // letters and off the viewBox — the 24 u margin outside the horizon is
    // theirs. The bound is the horizon box, exactly as for the bird's tag.
    const LO = 24 // DOME_C - DOME_R
    const HI = 224 // DOME_C + DOME_R
    for (const az of [0, 45, 90, 135, 180, 225, 270, 315]) {
      api.getSatDetail.mockImplementation(() => Promise.resolve(passAt(az, (az + 180) % 360)))
      render(<SatellitesView focusSat="RS-44" />)
      const { aos, los } = await tags()
      for (const [what, box] of [
        [`aos az ${az}`, plate(aos)],
        [`los az ${(az + 180) % 360}`, plate(los)],
      ] as [string, Rect][]) {
        expect(box.x, `${what} left`).toBeGreaterThanOrEqual(LO)
        expect(box.y, `${what} top`).toBeGreaterThanOrEqual(LO)
        expect(box.x + box.w, `${what} right`).toBeLessThanOrEqual(HI)
        expect(box.y + box.h, `${what} bottom`).toBeLessThanOrEqual(HI)
      }
      cleanup()
    }
  })

  it('never buries either mark under a plate', async () => {
    // A readout that covers the thing it is reading out is a net loss: the
    // operator loses the bearing's position on the dome to gain its number.
    for (const [a, l] of [
      [100, 260],
      [0, 180],
      [90, 270],
      [100, 118], // a low grazing pass: both marks in the same corner
      [10, 350], // …and one straddling north
    ]) {
      api.getSatDetail.mockImplementation(() => Promise.resolve(passAt(a, l)))
      render(<SatellitesView focusSat="RS-44" />)
      const { aos, los } = await tags()
      for (const mark of [around(rimPt(a), 5), around(rimPt(l), 5)]) {
        expect(overlaps(plate(aos), mark), `aos plate over a mark (${a}/${l})`).toBe(false)
        expect(overlaps(plate(los), mark), `los plate over a mark (${a}/${l})`).toBe(false)
      }
      cleanup()
    }
  })

  it('keeps the two plates apart when the two marks are close together', async () => {
    // A low grazing pass rises and sets within a few tens of degrees of each
    // other. Two overlapping plates there would leave one bearing half-hidden
    // and the pair ambiguous — the exact failure the words are there to stop.
    for (const [a, l] of [
      [100, 118],
      [100, 100],
      [355, 5],
      [200, 215],
    ]) {
      api.getSatDetail.mockImplementation(() => Promise.resolve(passAt(a, l)))
      render(<SatellitesView focusSat="RS-44" />)
      const { aos, los } = await tags()
      expect(overlaps(plate(aos), plate(los)), `plates collide at ${a}/${l}`).toBe(false)
      cleanup()
    }
  })

  it("yields to the bird's live tag — the moving number holds its place", async () => {
    // At AOS the bird IS on the rise mark, so the two readouts want the same
    // patch of dome. The second-by-second number is the one being read right
    // then; the fixed reference bearing steps out of its way, not the reverse.
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(status({ azDeg: null, elDeg: null, satAzDeg: 100, satElDeg: 1 })),
    )
    render(<SatellitesView focusSat="RS-44" />)
    const bird = await screen.findByTestId('sat-bird')
    const birdTag = bird.querySelector('[data-testid="sat-bird-tag"]')
    expect(birdTag).toBeTruthy()
    const { aos, los } = await tags()
    expect(overlaps(plate(aos), plate(birdTag!)), 'aos plate over the bird tag').toBe(false)
    expect(overlaps(plate(los), plate(birdTag!)), 'los plate over the bird tag').toBe(false)
    // …and not over the bird itself either.
    expect(overlaps(plate(aos), around(rimPt(100), 8)), 'aos plate over the bird').toBe(false)
  })

  it('never buries the rotator ghost — the gap to it IS the tracking error', async () => {
    // Same rule the bird's tag follows. A plate parked on the commanded
    // position hides the one thing the ghost exists to show.
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(status({ azDeg: 100, elDeg: 3, satAzDeg: 104, satElDeg: 6 })),
    )
    render(<SatellitesView focusSat="RS-44" />)
    const ghost = await screen.findByTestId('sat-ghost')
    const ring = ghost.querySelector('.sat-dome-ghost-ring')!
    const at: [number, number] = [Number(ring.getAttribute('cx')), Number(ring.getAttribute('cy'))]
    const { aos, los } = await tags()
    expect(overlaps(plate(aos), around(at, 7)), 'aos plate over the ghost').toBe(false)
    expect(overlaps(plate(los), around(at, 7)), 'los plate over the ghost').toBe(false)
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
      Promise.resolve(settings({ satVfoMap: 'main-down-sub-up' })),
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
    api.getSettings.mockImplementation(() => Promise.resolve(settings({ satDopplerOff: true })))
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(status({ dopplerDownlink: false, dopplerUplink: false })),
    )
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

  it('names the unconfirmed uplink-only mapping when that is what is blocking it', async () => {
    // The one mapping that takes the receive dial away from Doppler. With it
    // unconfirmed for this radio, NEITHER leg is driven — and the readout says
    // which of the two facts is the blocking one.
    api.getSettings.mockImplementation(() =>
      Promise.resolve(settings({ satVfoMap: 'uplink-only' })),
    )
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(status({ dopplerDownlink: false, dopplerUplink: false, transponder: 'RS-44|linear' })),
    )
    const { container } = render(<SatellitesView focusSat="RS-44" />)
    await screen.findByTestId('sat-ghost')
    expect(container.querySelector('.sat-doppler')?.textContent).toMatch(
      /uplink-only mapping is not confirmed for this radio/,
    )
  })

  it('asks for a transponder when correction is on and none is held', async () => {
    api.getSettings.mockImplementation(() =>
      Promise.resolve(settings({ satVfoMap: 'main-down-sub-up' })),
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
      Promise.resolve({ tleAgeDays: 21, usableCount: 97, agingCount: 97, heldBackCount: 0, tleFetchedAt: 1_785_542_400, tleSource: 'mirror', birds: [], passes: [], excluded: [] }),
    )
    const { container } = render(<SatellitesView />)
    await waitFor(() => {
      const chip = container.querySelector('.sat-chip.stale')
      expect(chip?.textContent).toContain('TLE 21 days')
    })
  })
})

/* ===================== THE 25% INSTRUCTION, COMPUTED ======================
 * Operator, 2026-08-03: "the actual aos, los and az, el text could be made
 * smaller by 25%, please do that."
 *
 * ⚠️ THIS CANNOT BE CHECKED BY LOOKING AT ONE NUMBER, WHICH IS WHY IT IS HERE
 * AND NOT A CSS PRESENCE TEST. The on-dome az/el and the two rim bearings are
 * SVG text in viewBox units, so what the operator sees is
 *
 *     rendered px = fontSize attribute × (rendered dome px ÷ 248)
 *
 * and the second factor lives in a CSS `max-width` on `.sat-dome`. The two
 * numbers are ONE decision: raise the constant without capping the dome and the
 * text gets BIGGER; cap the dome without raising the constant and it collapses
 * to ~9 px, which is under every size token the app ships. So this guard reads
 * the unit off the RENDERED marks (jsdom gives the real attribute the component
 * emits) and the cap off the SHEET (resolving the winner by specificity then
 * source order, against the real element), and asserts on the product.
 *
 * THE BASELINE it is measured against is what shipped before: TAG_FS 10 on a
 * dome that rendered 458 px wide at the supported 1024×768 floor — 18.5
 * effective px, the largest type in the whole section and bigger than the <h1>.
 * That is what the operator was looking at when he asked. */
describe('the AOS/LOS/az/el type is 25% smaller than it was', () => {
  // Resolved off the vitest cwd (ui/) rather than `import.meta.url`: this file
  // runs in the jsdom environment, where `import.meta.url` is not a file: URL.
  const SHEET = readFileSync(resolve(process.cwd(), 'src/styles.css'), 'utf8')

  /** Brace-aware rule walk, descending into at-rule blocks. */
  function rules(css: string, out: { sel: string; body: string; n: number }[] = [], c = { n: 0 }) {
    let i = 0
    let selStart = 0
    while (i < css.length) {
      if (css[i] === '{') {
        const sel = css.slice(selStart, i).trim()
        i++
        const start = i
        let depth = 1
        while (i < css.length && depth > 0) {
          if (css[i] === '{') depth++
          else if (css[i] === '}') depth--
          i++
        }
        const body = css.slice(start, i - 1)
        if (sel.startsWith('@')) rules(body, out, c)
        else {
          c.n++
          for (const one of sel.split(',').map((x) => x.trim().replace(/\s+/g, ' '))) {
            if (one) out.push({ sel: one, body, n: c.n })
          }
        }
        selStart = i
      } else i++
    }
    return out
  }
  const RULES = rules(SHEET.replace(/\/\*[\s\S]*?\*\//g, ''))
  const spec = (s: string) => (s.match(/\.[a-z][\w-]*|\[[^\]]+\]|#[a-z][\w-]*/g) ?? []).length
  /** The winning declaration for `prop` among the selectors that MATCH `el`. */
  function computed(el: Element, prop: string): string | null {
    let win: { v: string; s: number; n: number } | null = null
    const re = new RegExp(`(?:^|;)\\s*${prop}\\s*:\\s*([^;]+)`, 'g')
    for (const r of RULES) {
      let hit = false
      try {
        hit = el.matches(r.sel)
      } catch {
        hit = false
      }
      if (!hit) continue
      const all = [...r.body.matchAll(re)]
      if (!all.length) continue
      const v = all[all.length - 1][1].trim()
      const s = spec(r.sel)
      if (!win || s > win.s || (s === win.s && r.n >= win.n)) win = { v, s, n: r.n }
    }
    return win?.v ?? null
  }
  /** `min(100%, calc(F * var(--vh-eff, …)))` → F. */
  function vhFraction(v: string): number {
    const m = v.match(/calc\(\s*([0-9.]+)\s*\*\s*var\(--vh-eff/)
    expect(
      m,
      `\`${v}\` is not a --vh-eff-relative cap — a raw vh is blind to .app's zoom`,
    ).toBeTruthy()
    return Number(m![1])
  }

  /** Effective viewport height at the supported 1024×768 floor: useScale fits a
   *  1200×900 natural box into 1024×768 at 85%, so the app sees 1205×904. */
  const VH_EFF_AT_FLOOR = 904
  /** What shipped before this change, at that same floor. */
  const OLD_UNIT = 10
  const OLD_DOME_PX = 458

  /** The rendered px of the bird's own az/el plate at the supported floor. */
  async function renderedPx(): Promise<number> {
    const bird = await screen.findByTestId('sat-bird-tag')
    const domeEl = (await dome()) as unknown as Element
    const unit = Number(bird.querySelector('text')!.getAttribute('font-size'))
    expect(unit, 'the plate text carries no fontSize attribute').toBeGreaterThan(0)
    // ⚠️ RESOLVED AGAINST THE LIVE ELEMENT. `.sat-sky.live .sat-dome` is (0,3,0);
    // a cap written only on `.sat-dome` is (0,1,0) and would lose for the whole
    // of every live pass — i.e. exactly when the size matters. That is this
    // sheet's documented failure mode, so the fixture pass is in progress and
    // the winner is computed against the element that actually renders.
    expect(domeEl.closest('.sat-sky')!.className).toContain('live')
    const cap = computed(domeEl, 'max-width')
    expect(cap, 'the dome declares no max-width that wins for a LIVE pass').not.toBeNull()
    return unit * ((vhFraction(cap!) * VH_EFF_AT_FLOOR) / 248)
  }

  it('lands on the quarter he asked for, at the supported floor', async () => {
    render(<SatellitesView focusSat="RS-44" />)
    const before = OLD_UNIT * (OLD_DOME_PX / 248)
    const after = await renderedPx()
    const cut = 1 - after / before
    expect(
      cut,
      `the on-dome az/el renders ${after.toFixed(1)} px against ${before.toFixed(1)} px before ` +
        `(${(cut * 100).toFixed(0)}% smaller). He asked for 25%.`,
    ).toBeGreaterThan(0.2)
    // ⚠️ THE UPPER BOUND IS 36%, NOT 25%, AND THE OVERSHOOT IS DELIBERATE — it
    // is written here rather than quietly allowed. He asked for two things in
    // one sentence: this text 25% smaller, and the dome it lives on "reduced in
    // size". The dome cap is what delivers the second, and it is also the second
    // factor in this text's rendered size, so the two instructions pull the same
    // number. Measured in headless Chrome against the real sheet, a cap that
    // landed the type on exactly 25% (0.31·--vh-eff) left the pass column five
    // effective pixels over its box at the 1024×768 floor — the whole no-scroll
    // promise missing by a third of a text line. 0.28 closes it and costs the
    // type another seven points. The band stops at 36% because past that the
    // type is heading under `--fs-micro`, which the guard below refuses outright.
    expect(cut, 'smaller than asked — this is the number a mast is turned by').toBeLessThan(0.36)
  })

  it('is still the section’s most prominent number, not its smallest', async () => {
    // The honest ceiling on the cut. `--fs-micro` (11 px) is the smallest size
    // token the app ships, and this is what a manual-rotor operator reads
    // mid-turn with both hands busy. A "25%" delivered by taking it under the
    // app's own floor would be the wrong trade — and it is the trade a dome cap
    // makes silently if the plate constants are not raised to meet it.
    render(<SatellitesView focusSat="RS-44" />)
    expect(await renderedPx()).toBeGreaterThanOrEqual(11)
  })

  it('the two rim bearings take exactly the same size as the bird’s own plate', async () => {
    // One instrument, one type size. The rise/set bearings are read WITH the
    // live az/el to decide which way to turn a mast; two sizes there would read
    // as two different kinds of number.
    render(<SatellitesView focusSat="RS-44" />)
    const bird = await screen.findByTestId('sat-bird-tag')
    const unit = bird.querySelector('text')!.getAttribute('font-size')
    for (const id of ['sat-aos-tag', 'sat-los-tag']) {
      const t = screen.getByTestId(id).querySelector('text')!
      expect(t.getAttribute('font-size'), `${id} drifted off the bird plate's size`).toBe(unit)
    }
  })

  it('the ring and compass labels came down with them', async () => {
    // Same instrument, same instruction. These are `font-size` in CSS but INSIDE
    // the SVG, so they are viewBox units too and scale with the dome exactly as
    // the plates do — which means the dome cap alone would have shrunk them ~39%
    // while the plates stayed put. They go UP for the same reason TAG_FS did.
    render(<SatellitesView focusSat="RS-44" />)
    await dome()
    const ring = document.querySelector('.sat-dome-ringlabel')!
    const comp = document.querySelector('.sat-dome-compass')!
    expect(
      Number(computed(ring, 'font-size')!.replace('px', '')),
      'the ring labels were not re-scaled with the dome',
    ).toBeGreaterThan(9)
    expect(
      Number(computed(comp, 'font-size')!.replace('px', '')),
      'the compass letters were not re-scaled with the dome',
    ).toBeGreaterThan(11)
  })
})
