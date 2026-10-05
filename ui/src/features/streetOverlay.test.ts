// The street map's overlay bridge: the Mercator locked to MapLibre's camera, the shift that holds a
// drawn picture on a moving map, and the street-scale rules (grid squares, the grid ladder, the
// world-scale fields). Drawing itself is checked in a real browser against MapLibre.
import { describe, expect, it } from 'vitest'
import {
  GRID_LABEL_MIN_PX,
  GRID_LINE_MIN_PX,
  STREET_SCALE_ZOOM,
  WORLD_PX,
  WORLD_SCALE_LAYERS,
  gridCells,
  gridEdges,
  gridLevelFor,
  locatorAt,
  locatorBounds,
  packCovers,
  sameStreetView,
  squareOfCentre,
  streetBounds,
  streetProjection,
  streetShift,
  streetViewOf,
  type StreetView,
} from './streetOverlay'
import type { StreetPack } from './streetPack'

// MapLibre's own transform for a camera with no rotation and no pitch (maplibre-gl 6:
// src/geo/mercator_coordinate.ts `mercatorXfromLng`/`mercatorYfromLat`, and a world
// `tileSize * scale` = 512 · 2^zoom px across in src/geo/transform_helper.ts): a position's world
// coordinate minus the centre's, plus where the centre is drawn. Written out here, apart from the
// d3 Mercator under test.
const mx = (lng: number) => (180 + lng) / 360
const my = (lat: number) => (180 - (180 / Math.PI) * Math.log(Math.tan(Math.PI / 4 + (lat * Math.PI) / 360))) / 360
function maplibreProject(v: StreetView, [lon, lat]: [number, number]): [number, number] {
  const size = 512 * 2 ** v.zoom
  return [(mx(lon) - mx(v.center[0])) * size + v.at[0], (my(lat) - my(v.center[1])) * size + v.at[1]]
}

const W = 1600
const H = 900
const view = (lon: number, lat: number, zoom: number): StreetView => ({ center: [lon, lat], zoom, at: [W / 2, H / 2] })

// Positions around a station in Kansas, one far off screen, and one in the far north.
const PLACES: Array<[number, number]> = [
  [-97.611, 38.84],
  [-97.5, 38.9],
  [-97.71, 38.77],
  [-96.0, 40.1],
  [-150.0, 61.2],
]

describe('streetProjection — the lock to MapLibre', () => {
  for (const zoom of [2, 6.5, 8, 12, 14.5, 17]) {
    it(`puts every position on MapLibre's pixel within 0.5 px at zoom ${zoom}`, () => {
      const v = view(-97.61, 38.84, zoom)
      const p = streetProjection(v, W, H)
      for (const ll of PLACES) {
        const [x, y] = p(ll)!
        const [ex, ey] = maplibreProject(v, ll)
        expect(Math.abs(x - ex)).toBeLessThan(0.5)
        expect(Math.abs(y - ey)).toBeLessThan(0.5)
      }
    })
  }

  it('follows the centre wherever MapLibre draws it (a container that is not centred on it)', () => {
    const v: StreetView = { center: [12.5, 41.9], zoom: 13.25, at: [311.5, 704.25] }
    const p = streetProjection(v, W, H)
    expect(p([12.5, 41.9])).toEqual([expect.closeTo(311.5, 6), expect.closeTo(704.25, 6)])
    const [x, y] = p([12.51, 41.89])!
    const [ex, ey] = maplibreProject(v, [12.51, 41.89])
    expect(Math.hypot(x - ex, y - ey)).toBeLessThan(0.5)
  })

  it('is sensitive: a scale off by a thousandth misses MapLibre by more than half a pixel', () => {
    const v = view(-97.61, 38.84, 14.5)
    const off = streetProjection(v, W, H)
    off.scale(off.scale() * 1.001)
    const [x] = off([-97.5, 38.9])!
    const [ex] = maplibreProject(v, [-97.5, 38.9])
    expect(Math.abs(x - ex)).toBeGreaterThan(0.5)
  })

  it('reads the centre pixel once, so a kept view still says where the map was', () => {
    let at: [number, number] = [800, 450]
    const cam = { center: [1, 2] as [number, number], zoom: 9, bearing: 0, project: () => at, moving: false }
    const v = streetViewOf(cam)
    at = [0, 0]
    expect(v.at).toEqual([800, 450])
    expect(sameStreetView(v, { ...v, at: [800, 450] })).toBe(true)
    expect(sameStreetView(v, { ...v, zoom: 9.01 })).toBe(false)
    expect(sameStreetView(v, null)).toBe(false)
  })

  it('clips paths just outside the map, so no clipped edge is stroked on screen', () => {
    const [[x0, y0], [x1, y1]] = streetProjection(view(0, 0, 10), W, H).clipExtent()!
    expect(x0).toBeLessThan(0)
    expect(y0).toBeLessThan(0)
    expect(x1).toBeGreaterThan(W)
    expect(y1).toBeGreaterThan(H)
  })

  it('names what the view shows, inside the one world MapLibre draws', () => {
    const b = streetBounds(streetProjection(view(-97.61, 38.84, 12), W, H), W, H)
    // 1600 px at zoom 12 is 1600 / (512 · 4096 / 360) degrees of longitude.
    expect(b.e - b.w).toBeCloseTo(1600 / ((WORLD_PX * 4096) / 360), 9)
    expect(b.w).toBeLessThan(-97.61)
    expect(b.s).toBeLessThan(38.84)
    expect(b.n).toBeGreaterThan(38.84)
    // At zoom 0 the 1600 px view is wider than the 512 px world: still the one world, not wrapped.
    const world = streetBounds(streetProjection(view(0, 0, 0), W, H), W, H)
    expect(world).toEqual({ w: -180, s: expect.closeTo(-85.0511, 3), e: 180, n: expect.closeTo(85.0511, 3) })
  })
})

describe('streetShift — the picture held on a moving map between redraws', () => {
  it('moves what was drawn at one view onto the pixels of the next, exactly', () => {
    const drawn = view(-97.61, 38.84, 12)
    for (const live of [
      view(-97.58, 38.86, 12),
      view(-97.61, 38.84, 12.73),
      { center: [-97.4, 38.7] as [number, number], zoom: 10.2, at: [620, 380] as [number, number] },
    ]) {
      const t = streetShift(drawn, live)
      const before = streetProjection(drawn, W, H)
      const after = streetProjection(live, W, H)
      for (const ll of PLACES.slice(0, 4)) {
        const [x0, y0] = before(ll)!
        const [x1, y1] = after(ll)!
        expect(t.s * x0 + t.x).toBeCloseTo(x1, 5)
        expect(t.s * y0 + t.y).toBeCloseTo(y1, 5)
      }
    }
  })

  it('is no move at all for the view it was drawn at', () => {
    const v = view(30, -20, 13)
    expect(streetShift(v, v)).toEqual({ s: 1, x: expect.closeTo(0, 9), y: expect.closeTo(0, 9) })
  })
})

describe('grid squares at street scale', () => {
  it('reads a 4-, 6- or 8-character locator as its square', () => {
    expect(locatorBounds('EN52')).toEqual({ w: -90, s: 42, e: -88, n: 43 })
    const sub = locatorBounds('en52xa')!
    expect(sub.w).toBeCloseTo(-90 + 23 * (5 / 60), 12)
    expect(sub.e - sub.w).toBeCloseTo(5 / 60, 12)
    expect(sub.n - sub.s).toBeCloseTo(2.5 / 60, 12)
    const ext = locatorBounds('EN52XA25')!
    expect(ext.w).toBeCloseTo(sub.w + 2 * (0.5 / 60), 12)
    expect(ext.s).toBeCloseTo(sub.s + 5 * (0.25 / 60), 12)
    expect(ext.e - ext.w).toBeCloseTo(0.5 / 60, 12)
    for (const bad of ['', 'EN5', 'EN52x', 'ZZ00', 'EN52XA2', 'RR73XA2Z']) expect(locatorBounds(bad)).toBeNull()
  })

  // The backend's own placement (crates/propagation geo::maidenhead_to_latlon): the square's centre.
  const centreOf = (g: string): [number, number] => {
    const b = locatorBounds(g)!
    return [(b.s + b.n) / 2, (b.w + b.e) / 2]
  }

  it("recovers a spot's square from the centre the backend placed it at, 4 or 6 characters", () => {
    for (const g of ['EN52', 'JO01', 'QF56', 'AA00', 'RR99']) {
      const [lat, lon] = centreOf(g)
      expect(squareOfCentre(lat, lon)).toEqual(locatorBounds(g))
    }
    for (const g of ['EN52xa', 'DM79mr', 'JO01aa', 'QF56xx', 'FN31pr']) {
      const [lat, lon] = centreOf(g)
      const sq = squareOfCentre(lat, lon)!
      const want = locatorBounds(g)!
      expect(sq.w).toBeCloseTo(want.w, 9)
      expect(sq.n).toBeCloseTo(want.n, 9)
      expect(sq.e - sq.w).toBeCloseTo(5 / 60, 9)
    }
  })

  it('finds no square around a position that is not a centre (exact coordinates stay pins)', () => {
    expect(squareOfCentre(38.8401, -97.6112)).toBeNull()
    expect(squareOfCentre(42, -90)).toBeNull() // a corner, not a centre
  })

  it('names a cell at every rung, subsquares in lower case', () => {
    const [lat, lon] = centreOf('EN52XA25')
    expect(locatorAt(lat, lon, 2)).toBe('EN')
    expect(locatorAt(lat, lon, 4)).toBe('EN52')
    expect(locatorAt(lat, lon, 6)).toBe('EN52xa')
    expect(locatorAt(lat, lon, 8)).toBe('EN52xa25')
  })
})

describe('the grid ladder', () => {
  // Pixels per degree of longitude at a MapLibre zoom.
  const ppd = (zoom: number) => (WORLD_PX * 2 ** zoom) / 360

  it('draws lines down to squares at street scale, subsquares by zoom 10 and extended squares by 12', () => {
    expect(gridLevelFor(ppd(STREET_SCALE_ZOOM), GRID_LINE_MIN_PX)?.chars).toBe(4)
    expect(gridLevelFor(ppd(10), GRID_LINE_MIN_PX)?.chars).toBe(6)
    expect(gridLevelFor(ppd(12), GRID_LINE_MIN_PX)?.chars).toBe(8)
  })

  it('names 6-character subsquares from zoom 10 and 8-character squares near zoom 13', () => {
    expect(gridLevelFor(ppd(8), GRID_LABEL_MIN_PX)?.chars).toBe(4)
    expect(gridLevelFor(ppd(10), GRID_LABEL_MIN_PX)?.chars).toBe(6)
    expect(gridLevelFor(ppd(12), GRID_LABEL_MIN_PX)?.chars).toBe(6)
    expect(gridLevelFor(ppd(13), GRID_LABEL_MIN_PX)?.chars).toBe(8)
  })

  it('has nothing to draw when even a field is narrower than the floor', () => {
    expect(gridLevelFor(0.5, GRID_LINE_MIN_PX)).toBeNull()
  })

  it('lists the edges and the named cells inside a view', () => {
    const sub = { chars: 6 as const, lon: 5 / 60, lat: 2.5 / 60 }
    // Salina, Kansas (EM18eu) and the subsquares around it.
    const { lons, lats } = gridEdges(sub, -97.7, 38.8, -97.52, 38.9)
    expect(lons).toEqual([expect.closeTo(-97 - 8 * (5 / 60), 9), expect.closeTo(-97 - 7 * (5 / 60), 9)])
    expect(lats).toEqual([expect.closeTo(38 + 20 * (2.5 / 60), 9), expect.closeTo(38 + 21 * (2.5 / 60), 9)])
    const cells = gridCells(sub, -97.7, 38.8, -97.52, 38.9)
    expect(cells).toHaveLength(3 * 3)
    expect(cells.map((c) => c.name)).toContain('EM18eu')
  })
})

describe('what dims at street scale, and where a pack opens', () => {
  it('dims exactly the planet-scale fields, never a station or path layer', () => {
    expect([...WORLD_SCALE_LAYERS].sort()).toEqual(['aurora', 'flare', 'heat', 'muf', 'pca'])
  })

  const pack = (bbox: [number, number, number, number]): StreetPack => ({
    id: 'p',
    name: 'p',
    bbox,
    minZoom: 0,
    maxZoom: 14,
    detail: 'streets',
    bytes: 1,
    dataDate: '2026-10-04',
    sha256: '00',
  })

  it("knows whether a pack's square holds a position, across the 180° meridian too", () => {
    expect(packCovers(pack([-98.2, 38.4, -97, 39.3]), 38.84, -97.61)).toBe(true)
    expect(packCovers(pack([-98.2, 38.4, -97, 39.3]), 40, -97.61)).toBe(false)
    expect(packCovers(pack([179, -18, -179, -17]), -17.5, 179.5)).toBe(true)
    expect(packCovers(pack([179, -18, -179, -17]), -17.5, -179.5)).toBe(true)
    expect(packCovers(pack([179, -18, -179, -17]), -17.5, 0)).toBe(false)
  })
})
