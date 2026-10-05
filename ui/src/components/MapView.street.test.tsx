// @vitest-environment jsdom
//
// The 2-D map's overlays on the street map: the real MapView, with the street renderer replaced by a
// stand-in that hands MapView a camera and MapLibre-shaped pointer events (jsdom has no WebGL). What
// is pinned here is the bridge, not the drawing: the overlays are placed through the Mercator locked
// to the map's camera, input arrives through the map's own events, a double-click on a target keeps
// its meaning (and never transmits), the canvas is held on the map between redraws, and the
// street-scale rules (grid squares as outlines, the world-scale fields dimmed) apply only at street
// scale. Drawing is checked in a real browser.
import { act, cleanup, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { MapView } from './MapView'
import { bandLabelForMhz } from '../band'
import { gridToLatLon } from '../grid'
import type { OtaMapSpot, Station } from '../types'
import type { StreetPack } from '../features/streetPack'
import type { StreetCamera, StreetMapProps, StreetPointer } from './StreetMap'
import { t } from '../i18n'

vi.mock('../api', () => ({
  getAurora: vi.fn(async () => null),
  getDeclination: vi.fn(async () => null),
  getPca: vi.fn(async () => null),
  getSatellites: vi.fn(async () => null),
  getLogStats: vi.fn(async () => null),
  getOtaMapSpots: vi.fn(async () => []),
  // The transmit-capable surface, spied so a street double-click is PROVEN never to reach it.
  callStation: vi.fn(async () => ({})),
  setPtt: vi.fn(async () => ({})),
  setTxEnabled: vi.fn(async () => ({})),
  startCq: vi.fn(async () => ({})),
  callCq: vi.fn(async () => ({})),
  haltTx: vi.fn(async () => ({})),
}))
import { callCq, callStation, haltTx, setPtt, setTxEnabled, startCq } from '../api'

// The street renderer's stand-in: it keeps the props MapView gives it.
const fake = vi.hoisted(() => ({ props: null as null | Record<string, unknown> }))
vi.mock('./StreetMap', async () => {
  const { forwardRef } = await import('react')
  return {
    default: forwardRef<unknown, Record<string, unknown>>(function FakeStreet(props, _ref) {
      fake.props = props
      return <div className="street-map" data-testid="fake-street" />
    }),
  }
})
const streetProps = () => fake.props as unknown as StreetMapProps

class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}

/** A 2-D context that answers everything (see MapView.ota-doubleclick.test.tsx) and records the
 *  calls that say WHAT was drawn: rectangles, dots and text. */
let drawn: Array<[string, unknown[]]> = []
function recordingCtx(): CanvasRenderingContext2D {
  const self: object = new Proxy(function ctx() {}, {
    get(_t, prop) {
      if (prop === 'measureText') return () => ({ width: 10 })
      if (prop === 'rect' || prop === 'arc' || prop === 'fillText')
        return (...args: unknown[]) => {
          drawn.push([prop, args])
          return self
        }
      return self
    },
    set() {
      return true
    },
    apply() {
      return self
    },
  })
  return self as CanvasRenderingContext2D
}

const W = 1000
const H = 800
const MY_GRID = 'EN52'
const ME = gridToLatLon(MY_GRID)!

// MapLibre's projection for an unrotated camera centred on the canvas (its Web Mercator formula).
const mx = (lng: number) => (180 + lng) / 360
const my = (lat: number) => (180 - (180 / Math.PI) * Math.log(Math.tan(Math.PI / 4 + (lat * Math.PI) / 360))) / 360
const camera = (lon: number, lat: number, zoom: number, moving = false): StreetCamera => ({
  center: [lon, lat],
  zoom,
  bearing: 0,
  project: ([x, y]) => [(mx(x) - mx(lon)) * 512 * 2 ** zoom + W / 2, (my(y) - my(lat)) * 512 * 2 ** zoom + H / 2],
  moving,
})

const PACK: StreetPack = {
  id: '20261004-streets-200km-4250n08900w',
  name: 'EN52 200 km',
  bbox: [-90.2, 41.6, -87.8, 43.4],
  minZoom: 0,
  maxZoom: 14,
  detail: 'streets',
  bytes: 1_000_000,
  dataDate: '2026-10-04',
  sha256: 'ab'.repeat(32),
}

// A park with its own coordinates, a little north-east of the station.
const PARK: OtaMapSpot = {
  program: 'POTA',
  reference: 'K-1234',
  name: 'Test Park',
  activator: 'W1AW',
  freqMhz: 14.074,
  mode: 'FT8',
  lat: ME.lat + 0.01,
  lon: ME.lon + 0.02,
  approx: false,
  ageSecs: 30,
  newRef: true,
}

const STATION = { call: 'K9XYZ', grid: 'EN52', snr: -5, presence: 'active' } as unknown as Station

let wDesc: PropertyDescriptor | undefined
let hDesc: PropertyDescriptor | undefined
beforeEach(() => {
  localStorage.clear()
  drawn = []
  fake.props = null
  ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RO
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(recordingCtx())
  wDesc = Object.getOwnPropertyDescriptor(Element.prototype, 'clientWidth')
  hDesc = Object.getOwnPropertyDescriptor(Element.prototype, 'clientHeight')
  Object.defineProperty(Element.prototype, 'clientWidth', { configurable: true, get: () => W })
  Object.defineProperty(Element.prototype, 'clientHeight', { configurable: true, get: () => H })
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  if (wDesc) Object.defineProperty(Element.prototype, 'clientWidth', wDesc)
  if (hDesc) Object.defineProperty(Element.prototype, 'clientHeight', hDesc)
})

async function mountStreet(over: Partial<Parameters<typeof MapView>[0]> = {}) {
  const onWorkSpot = vi.fn()
  const onSelectCall = vi.fn()
  const view = render(
    <MapView
      myGrid={MY_GRID}
      theme="dark"
      stations={[]}
      prop={null}
      selectedCall={null}
      onSelectCall={onSelectCall}
      needByCall={new Map()}
      intent="pota"
      projection="street"
      streetPack={PACK}
      ota={[PARK]}
      onWorkSpot={onWorkSpot}
      {...over}
    />,
  )
  await screen.findByTestId('fake-street')
  return { ...view, onWorkSpot, onSelectCall, canvas: view.container.querySelector('canvas') as HTMLCanvasElement }
}

/** The pixel MapLibre puts a position on, for a camera. */
const px = (cam: StreetCamera, lat: number, lon: number) => cam.project([lon, lat])

function pointer(type: StreetPointer['type'], [x, y]: [number, number]) {
  const e = { type, x, y, preventDefault: vi.fn() }
  act(() => streetProps().onPointer?.(e))
  return e
}

describe('MapView on Street — the map under the overlays', () => {
  it('mounts the street map under the overlay canvas, which lets the pointer through to it', async () => {
    const { canvas, container } = await mountStreet()
    const wrap = container.querySelector('.map-canvas-wrap')!
    expect(wrap.firstElementChild?.getAttribute('data-testid')).toBe('fake-street')
    expect(canvas.style.pointerEvents).toBe('none')
    // The station's own square holds it, so the map opens there.
    expect(streetProps().center).toEqual([ME.lon, ME.lat])
  })

  it('keeps the pointer on the overlay canvas everywhere else', () => {
    const { container } = render(
      <MapView
        myGrid={MY_GRID}
        theme="dark"
        stations={[]}
        prop={null}
        selectedCall={null}
        onSelectCall={() => {}}
        needByCall={new Map()}
        intent="pota"
        projection="world"
      />,
    )
    expect((container.querySelector('canvas') as HTMLCanvasElement).style.pointerEvents).toBe('')
    expect(container.querySelector('[data-testid="fake-street"]')).toBeNull()
  })

  it('draws nothing until the map says where it is', async () => {
    await mountStreet({ stations: [STATION] })
    expect(drawn.filter(([k]) => k === 'arc' || k === 'rect')).toHaveLength(0)
  })
})

describe("MapView on Street — the overlays follow the map's camera", () => {
  it('finds a park where MapLibre draws it, and a double-click there works it without zooming', async () => {
    const { onWorkSpot } = await mountStreet()
    const cam = camera(ME.lon, ME.lat, 12)
    act(() => streetProps().onCamera?.(cam))
    const at = px(cam, PARK.lat, PARK.lon)
    // Off the centre, so only a projection locked to this camera finds it.
    expect(Math.hypot(at[0] - W / 2, at[1] - H / 2)).toBeGreaterThan(100)

    const dbl = pointer('dblclick', at)
    expect(dbl.preventDefault).toHaveBeenCalledTimes(1)
    expect(onWorkSpot).toHaveBeenCalledWith({
      call: PARK.activator,
      band: bandLabelForMhz(PARK.freqMhz),
      mode: PARK.mode,
      freqMhz: PARK.freqMhz,
      program: PARK.program,
      reference: PARK.reference,
    })
    for (const tx of [callStation, setPtt, setTxEnabled, startCq, callCq, haltTx]) expect(tx).not.toHaveBeenCalled()
  })

  it('leaves a double-click on empty map to MapLibre, which zooms', async () => {
    const { onWorkSpot } = await mountStreet()
    act(() => streetProps().onCamera?.(camera(ME.lon, ME.lat, 12)))
    const dbl = pointer('dblclick', [20, 20])
    expect(dbl.preventDefault).not.toHaveBeenCalled()
    expect(onWorkSpot).not.toHaveBeenCalled()
  })

  it("hovers through the map's own events, and the map's cursor says the target is clickable", async () => {
    await mountStreet()
    const cam = camera(ME.lon, ME.lat, 12)
    act(() => streetProps().onCamera?.(cam))
    pointer('move', px(cam, PARK.lat, PARK.lon))
    expect(document.querySelector('.map-hover')?.textContent).toContain(PARK.reference)
    expect(streetProps().cursor).toBe('pointer')
    pointer('out', [0, 0])
    expect(document.querySelector('.map-hover')).toBeNull()
    expect(streetProps().cursor).toBeUndefined()
  })

  it("selects a station with the map's own click, where its square's name is", async () => {
    const { onSelectCall } = await mountStreet({ stations: [STATION] })
    const cam = camera(ME.lon, ME.lat, 9)
    act(() => streetProps().onCamera?.(cam))
    pointer('click', px(cam, ME.lat, ME.lon))
    expect(onSelectCall).toHaveBeenCalledWith('K9XYZ')
  })

  it('holds the drawn picture on the map until the redraw lands, then lets go', async () => {
    const { canvas } = await mountStreet()
    act(() => streetProps().onCamera?.(camera(ME.lon, ME.lat, 12)))
    expect(canvas.style.transform).toBe('')
    // The map pans 100 px east: everything drawn moves 100 px left, in the frame MapLibre draws.
    const panned = camera(ME.lon + 100 / ((512 * 2 ** 12) / 360), ME.lat, 12)
    let between = ''
    act(() => {
      streetProps().onCamera?.(panned)
      between = canvas.style.transform
    })
    expect(between).toMatch(/^translate\(-100(\.0+\d*)?px, 0px\) scale\(1\)$|^translate\(-99\.9999\d*px, 0px\) scale\(1\)$/)
    // The redraw at the new camera replaces the moved picture.
    expect(canvas.style.transform).toBe('')
  })

  it('clears the overlays when the map goes', async () => {
    const { canvas } = await mountStreet()
    act(() => streetProps().onCamera?.(camera(ME.lon, ME.lat, 12)))
    act(() => streetProps().onCamera?.(null))
    expect(canvas.style.transform).toBe('')
  })
})

// While the map moves (a drag, an animated pan or zoom, the drift after a drag), the overlays are not
// redrawn: the picture drawn when it started is held on the map by the canvas's transform, and the
// redraw comes once, when the map stops (operator ruling 2026-10-05). Each full redraw draws the
// station's own grid square once, the only rectangle here (the park is a pin), so counting rectangles
// counts redraws.
describe('MapView on Street — a gesture holds the picture and redraws once, at its end', () => {
  const redraws = () => drawn.filter(([k]) => k === 'rect').length
  /** The canvas's transform as numbers: a point drawn at p shows at s · p + (x, y). */
  const shiftOf = (canvas: HTMLCanvasElement) => {
    const m = /^translate\(([-\d.e]+)px, ([-\d.e]+)px\) scale\(([-\d.e]+)\)$/.exec(canvas.style.transform)
    return m ? { x: Number(m[1]), y: Number(m[2]), s: Number(m[3]) } : null
  }

  it('pans by moving the drawn picture with the map, redrawing nothing until the map stops, then once', async () => {
    const { canvas } = await mountStreet()
    act(() => streetProps().onCamera?.(camera(ME.lon, ME.lat, 8)))
    expect(redraws()).toBe(1)
    drawn = []
    const lonPerPx = 360 / (512 * 2 ** 8)
    for (let k = 1; k <= 12; k++) {
      // The map moves 25 px east per frame: what was drawn moves 25 px left, frame by frame.
      act(() => streetProps().onCamera?.(camera(ME.lon + 25 * k * lonPerPx, ME.lat, 8, true)))
      const t = shiftOf(canvas)
      expect(t).not.toBeNull()
      expect(t!.s).toBe(1)
      expect(t!.x).toBeCloseTo(-25 * k, 6)
      expect(t!.y).toBeCloseTo(0, 6)
    }
    expect(redraws()).toBe(0)
    act(() => streetProps().onCamera?.(camera(ME.lon + 300 * lonPerPx, ME.lat, 8)))
    expect(redraws()).toBe(1)
    expect(canvas.style.transform).toBe('')
  })

  it("zooms by scaling the drawn picture about the map's centre, and redraws once at the end", async () => {
    const { canvas } = await mountStreet()
    act(() => streetProps().onCamera?.(camera(ME.lon, ME.lat, 8)))
    drawn = []
    for (const z of [8.25, 8.5, 8.75, 9]) {
      act(() => streetProps().onCamera?.(camera(ME.lon, ME.lat, z, true)))
      // The centre stays put and everything else spreads from it: s = 2^Δzoom, about (W/2, H/2).
      const s = 2 ** (z - 8)
      const t = shiftOf(canvas)!
      expect(t.s).toBeCloseTo(s, 6)
      expect(t.x).toBeCloseTo((W / 2) * (1 - s), 4)
      expect(t.y).toBeCloseTo((H / 2) * (1 - s), 4)
    }
    expect(drawn.filter(([k]) => k === 'rect')).toHaveLength(0)
    act(() => streetProps().onCamera?.(camera(ME.lon, ME.lat, 9)))
    expect(redraws()).toBe(1)
    expect(canvas.style.transform).toBe('')
  })

  it('finds a target where the moving map shows it: a double-click mid-pan works the park', async () => {
    const { onWorkSpot } = await mountStreet()
    act(() => streetProps().onCamera?.(camera(ME.lon, ME.lat, 12)))
    const live = camera(ME.lon + 150 / ((512 * 2 ** 12) / 360), ME.lat - 40 / ((512 * 2 ** 12) / 360), 12, true)
    act(() => streetProps().onCamera?.(live))
    const dbl = pointer('dblclick', px(live, PARK.lat, PARK.lon))
    expect(dbl.preventDefault).toHaveBeenCalledTimes(1)
    expect(onWorkSpot).toHaveBeenCalledWith(expect.objectContaining({ call: PARK.activator, reference: PARK.reference }))
    for (const tx of [callStation, setPtt, setTxEnabled, startCq, callCq, haltTx]) expect(tx).not.toHaveBeenCalled()
  })

  it('reads no hover while the map moves, as the other maps do during a drag, and reads it again once it stops', async () => {
    await mountStreet()
    act(() => streetProps().onCamera?.(camera(ME.lon, ME.lat, 12)))
    const live = camera(ME.lon + 60 / ((512 * 2 ** 12) / 360), ME.lat, 12, true)
    act(() => streetProps().onCamera?.(live))
    pointer('move', px(live, PARK.lat, PARK.lon))
    expect(document.querySelector('.map-hover')).toBeNull()
    const still = { ...live, moving: false }
    act(() => streetProps().onCamera?.(still))
    pointer('move', px(still, PARK.lat, PARK.lon))
    expect(document.querySelector('.map-hover')?.textContent).toContain(PARK.reference)
  })
})

describe('MapView on Street — the street-scale rules', () => {
  const rects = () => drawn.filter(([k]) => k === 'rect').map(([, a]) => a as number[])

  it("draws a decoded station as its grid square's outline at street scale, named at the centre", async () => {
    await mountStreet({ stations: [STATION] })
    const cam = camera(ME.lon, ME.lat, 8)
    drawn = []
    act(() => streetProps().onCamera?.(cam))
    // EN52 is 2° wide: 728 px at zoom 8, centred on the camera.
    const sq = rects().find(([, , w]) => Math.abs(w - (2 * 512 * 256) / 360) < 0.5)
    expect(sq).toBeDefined()
    const [x, , w] = sq!
    expect(x + w / 2).toBeCloseTo(W / 2, 3)
    expect(drawn.some(([k, a]) => k === 'fillText' && a[0] === 'K9XYZ')).toBe(true)
  })

  it('keeps the dot below street scale', async () => {
    await mountStreet({ stations: [STATION] })
    drawn = []
    act(() => streetProps().onCamera?.(camera(ME.lon, ME.lat, 6)))
    expect(rects()).toHaveLength(0)
    expect(drawn.some(([k, a]) => k === 'arc' && Math.abs((a[0] as number) - W / 2) < 0.5)).toBe(true)
  })

  it('keeps a park with its own coordinates a pin, never a square', async () => {
    await mountStreet()
    drawn = []
    act(() => streetProps().onCamera?.(camera(ME.lon, ME.lat, 12)))
    // The station's own square is the only one: the park is not grid-placed.
    expect(rects().every(([, , w]) => w > 1000)).toBe(true)
  })

  it('dims the world-scale fields above street scale, saying why, and leaves them on', async () => {
    await mountStreet()
    act(() => streetProps().onCamera?.(camera(ME.lon, ME.lat, 12)))
    const dimmed = [...document.querySelectorAll('.map-layer.street-dim')]
    expect(dimmed).toHaveLength(5)
    for (const row of dimmed) expect(row.getAttribute('title')).toBe(t('map.street.worldScale'))
    const muf = dimmed.find((r) => r.textContent?.includes(t('map.layer.muf.label')))!
    expect((muf.querySelector('input[type="checkbox"]') as HTMLInputElement).checked).toBe(true)

    act(() => streetProps().onCamera?.(camera(ME.lon, ME.lat, 6)))
    expect(document.querySelectorAll('.map-layer.street-dim')).toHaveLength(0)
  })
})
