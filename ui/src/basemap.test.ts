// THE BASE GEOGRAPHY: which Natural Earth scale a zoom draws, what the shipped files hold, and the
// two painters. The files are read as the app reads them (decodeBasemap on the shipped bytes), so
// every check here is a check of what scripts/gen-basemap.mjs actually produced: the tiling, the
// ring orientation d3 depends on, the coastline derived from the land, and real places.
import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest'
import { readFileSync } from 'node:fs'
import { geoArea, geoAzimuthalEquidistant, geoOrthographic, type GeoStream } from 'd3-geo'
import {
  FIFTY_FROM,
  PROJECTED_BUDGET,
  TEN_FROM,
  basemapAt,
  coarsestBasemap,
  decodeBasemap,
  loadBasemap,
  paintEquirect,
  paintProjected,
  pxPerDegree,
  riverRankFor,
  scaleFor,
  streamTiles,
  tileInRegion,
  visibleRegion,
  type Basemap,
  type BasemapScale,
  type BasemapTile,
  type Region,
} from './basemap'
import { makeProjection } from './mapGeo'

const file = (scale: BasemapScale) => new Uint8Array(readFileSync(new URL(`./data/basemap-${scale}.bin`, import.meta.url)))
const MAPS: Record<BasemapScale, Basemap> = {
  '110m': coarsestBasemap(),
  '50m': decodeBasemap(file('50m'), '50m'),
  '10m': decodeBasemap(file('10m'), '10m'),
}
const SCALES = ['110m', '50m', '10m'] as const

/** Twice the signed planar area of ring r of a tile, lat up: positive = anticlockwise. */
function area2(t: BasemapTile, r: number): number {
  let a = 0
  const s = t.parts[r]
  const e = t.parts[r + 1]
  for (let i = s, j = e - 1; i < e; j = i++) a += t.coords[2 * j] * t.coords[2 * i + 1] - t.coords[2 * i] * t.coords[2 * j + 1]
  return a
}

/** Even-odd containment in a polygon layer, by the tile that holds the point. */
function inside(layer: Basemap['land'], lon: number, lat: number): boolean {
  let hit = false
  for (const t of layer.tiles) {
    if (lon < t.west || lon >= t.east || lat < t.south || lat >= t.north) continue
    for (let r = 0; r + 1 < t.parts.length; r++) {
      for (let i = t.parts[r], j = t.parts[r + 1] - 1; i < t.parts[r + 1]; j = i++) {
        const xi = t.coords[2 * i]
        const yi = t.coords[2 * i + 1]
        const xj = t.coords[2 * j]
        const yj = t.coords[2 * j + 1]
        if (yi > lat !== yj > lat && lon < ((xj - xi) * (lat - yi)) / (yj - yi) + xi) hit = !hit
      }
    }
  }
  return hit
}

describe('which scale a zoom draws', () => {
  it('1:110m below FIFTY_FROM px/°, 1:50m below TEN_FROM, 1:10m from there', () => {
    expect([scaleFor(FIFTY_FROM - 0.01), scaleFor(FIFTY_FROM), scaleFor(TEN_FROM - 0.01), scaleFor(TEN_FROM)]).toEqual(['110m', '50m', '50m', '10m'])
  })

  it('reads the density off the projection the map draws with', () => {
    // A 1920-wide flat world map shows 360° across 1920 px.
    const world = makeProjection('world', { lat: 0, lon: 0 }, 1920, 960)
    expect(pxPerDegree(world)).toBeCloseTo(1920 / 360, 6)
    expect(scaleFor(pxPerDegree(world))).toBe('50m')
    // ...and switches to 1:10m once zoomed in past TEN_FROM / (1920/360) = 3.375×.
    expect(scaleFor(pxPerDegree(makeProjection('world', { lat: 0, lon: 0 }, 1920, 960, { zoom: 3.3, rotate: null, panX: 0, panY: 0 })))).toBe('50m')
    expect(scaleFor(pxPerDegree(makeProjection('world', { lat: 0, lon: 0 }, 1920, 960, { zoom: 3.4, rotate: null, panX: 0, panY: 0 })))).toBe('10m')
    // The 2-D globe filling a 1000 px box: radius 460 px per radian.
    expect(pxPerDegree(makeProjection('globe', { lat: 42, lon: -89 }, 1000, 1000))).toBeCloseTo((460 * Math.PI) / 180, 6)
    // A small embedded globe (the satellite detail) draws the bundled 1:110m.
    expect(scaleFor(pxPerDegree(makeProjection('globe', { lat: 42, lon: -89 }, 200, 200)))).toBe('110m')
  })

  it('shows more rivers as the map zooms in, never fewer', () => {
    const ranks = [1, 3, 6, 12, 30, 80].map(riverRankFor)
    expect(ranks).toEqual([...ranks].sort((a, b) => a - b))
    expect(ranks[0]).toBeLessThan(ranks[ranks.length - 1])
  })
})

describe('the shipped files', () => {
  it.each(SCALES)('%s: every layer is there, and every point lies in the tile that holds it', (scale) => {
    const m = MAPS[scale]
    for (const layer of [m.land, m.lakes, m.rivers, m.borders, m.states]) {
      expect(layer.tiles.length).toBeGreaterThan(0)
      for (const t of layer.tiles) {
        expect(t.parts[0]).toBe(0)
        expect(t.parts[t.parts.length - 1]).toBe(t.coords.length / 2)
        for (let i = 0; i < t.coords.length; i += 2) {
          if (t.coords[i] < t.west - 1e-4 || t.coords[i] > t.east + 1e-4 || t.coords[i + 1] < t.south - 1e-4 || t.coords[i + 1] > t.north + 1e-4) {
            throw new Error(`${scale}: point ${t.coords[i]},${t.coords[i + 1]} outside tile ${t.index}`)
          }
        }
      }
    }
    expect(m.rivers.tiles.every((t) => t.ranks && t.ranks.length === t.parts.length - 1)).toBe(true)
  })

  it.each(SCALES)('%s: exterior rings run clockwise and holes anticlockwise, as d3 reads a polygon', (scale) => {
    let polygons = 0
    for (const layer of [MAPS[scale].land, MAPS[scale].lakes]) {
      for (const t of layer.tiles) {
        const polys = t.polys!
        for (let p = 0; p + 1 < polys.length; p++) {
          polygons++
          expect(area2(t, polys[p]), `${scale} tile ${t.index} polygon ${p}`).toBeLessThan(0)
          for (let r = polys[p] + 1; r < polys[p + 1]; r++) expect(area2(t, r)).toBeGreaterThan(0)
        }
      }
    }
    expect(polygons).toBeGreaterThan(20)
  })

  it('d3 reads every 1:50m land and lake piece as the small polygon, never its complement', () => {
    // A ring the wrong way round is the whole sphere minus the island: it floods the globe.
    for (const layer of [MAPS['50m'].land, MAPS['50m'].lakes]) {
      for (const t of layer.tiles) {
        const polys = t.polys!
        for (let p = 0; p + 1 < polys.length; p++) {
          const rings: number[][][] = []
          for (let r = polys[p]; r < polys[p + 1]; r++) {
            const ring: number[][] = []
            for (let i = t.parts[r]; i < t.parts[r + 1]; i++) ring.push([t.coords[2 * i], t.coords[2 * i + 1]])
            ring.push(ring[0])
            rings.push(ring)
          }
          expect(geoArea({ type: 'Polygon', coordinates: rings })).toBeLessThan(2 * Math.PI)
        }
      }
    }
  })

  it.each(['50m', '10m'] as const)('%s: the coastline never runs along a tile cut — and the land rings do (the control)', (scale) => {
    const m = MAPS[scale]
    const onCut = (t: BasemapTile, a: number, b: number) => {
      const [x1, y1, x2, y2] = [t.coords[2 * a], t.coords[2 * a + 1], t.coords[2 * b], t.coords[2 * b + 1]]
      const step = m.tileDeg
      return (x1 === x2 && Math.abs((((x1 + 180) / step) % 1) - 0) < 1e-9) || (y1 === y2 && Math.abs((((y1 + 90) / step) % 1) - 0) < 1e-9)
    }
    let ringCuts = 0
    let shoreCuts = 0
    for (const t of [...m.land.tiles, ...m.lakes.tiles]) {
      for (let r = 0; r + 1 < t.parts.length; r++) {
        for (let i = t.parts[r]; i < t.parts[r + 1]; i++) if (onCut(t, i, i + 1 < t.parts[r + 1] ? i + 1 : t.parts[r])) ringCuts++
      }
      const sh = t.shore!
      for (let k = 0; k < sh.length; k += 3) {
        for (let i = sh[k]; i + 1 < sh[k] + sh[k + 1]; i++) if (onCut(t, i, i + 1)) shoreCuts++
      }
    }
    expect(ringCuts, 'CONTROL: the tile cuts are there to find').toBeGreaterThan(100)
    expect(shoreCuts).toBe(0)
  })

  it.each(SCALES)('%s: real places — Kansas and the Sahara are land, the mid-Atlantic is sea, Lake Michigan is a lake', (scale) => {
    const m = MAPS[scale]
    expect(inside(m.land, -98.5, 38.5)).toBe(true)
    expect(inside(m.land, 12, 23)).toBe(true)
    expect(inside(m.land, -30, 0)).toBe(false)
    expect(inside(m.lakes, -87, 43.5)).toBe(true)
    expect(inside(m.lakes, -98.5, 38.5)).toBe(false)
  })

  it('1:10m is the detailed one: several times the points of 1:50m, US states included', () => {
    const points = (m: Basemap) => [m.land, m.lakes, m.rivers, m.borders, m.states].flatMap((l) => l.tiles).reduce((n, t) => n + t.coords.length / 2, 0)
    expect(points(MAPS['10m'])).toBeGreaterThan(4 * points(MAPS['50m']))
    expect(points(MAPS['50m'])).toBeGreaterThan(4 * points(MAPS['110m']))
    for (const scale of SCALES) expect(MAPS[scale].states.tiles.length).toBeGreaterThan(0)
  })
})

/** A d3 stream that only counts. */
function counter() {
  const c = { points: 0, lines: 0, polygons: 0 }
  const s: GeoStream = {
    point: () => void c.points++,
    lineStart: () => void c.lines++,
    lineEnd: () => {},
    polygonStart: () => void c.polygons++,
    polygonEnd: () => {},
  }
  return { c, s }
}

describe('what a projected map draws', () => {
  const tiles = MAPS['10m'].land.tiles
  it('skips the pieces out of view, keeps those in it (and everything with no region)', () => {
    // Lake Michigan's shore: a lake, not the sea, so its pieces are in the lakes layer.
    const near: Region = { kind: 'cap', lon: -87, lat: 43.5, radius: 3 }
    const far: Region = { kind: 'cap', lon: 100, lat: -40, radius: 3 }
    const michigan = MAPS['10m'].lakes.tiles.filter((t) => tileInRegion(t, near))
    expect(michigan.length).toBeGreaterThan(0)
    const a = counter()
    streamTiles(a.s, michigan, 'shore', near)
    const b = counter()
    streamTiles(b.s, michigan, 'shore', far)
    const all = counter()
    streamTiles(all.s, michigan, 'shore')
    expect(a.c.points).toBeGreaterThan(0)
    expect(b.c.points).toBe(0)
    expect(all.c.points).toBeGreaterThanOrEqual(a.c.points)
  })

  it('drops sub-pixel steps before projection, and never streams a ring below a triangle', () => {
    const full = counter()
    streamTiles(full.s, tiles, 'fill')
    const thin = counter()
    const rings: number[] = []
    let n = 0
    const s: GeoStream = { ...thin.s, point: () => void (thin.c.points++, n++), lineStart: () => void (n = 0), lineEnd: () => void rings.push(n) }
    streamTiles(s, tiles, 'fill', undefined, 0.05)
    expect(thin.c.points).toBeLessThan(full.c.points * 0.8)
    expect(thin.c.points).toBeGreaterThan(0)
    expect(Math.min(...rings)).toBeGreaterThanOrEqual(3)
  })

  it('a view as wide as the whole sphere is never culled', () => {
    const r = visibleRegion('aeqd', makeProjection('aeqd', { lat: 42, lon: -89 }, 1000, 1000), 1000, 1000)
    expect(r.kind).toBe('cap')
    expect(r.kind === 'cap' && r.radius).toBeGreaterThanOrEqual(180)
    expect(MAPS['50m'].land.tiles.every((t) => tileInRegion(t, r))).toBe(true)
  })
})

/** A canvas context that records the path verbs, fills and strokes, and the style at each. */
function recorder() {
  const log: string[] = []
  let segments = 0
  const ctx = {
    fillStyle: '',
    strokeStyle: '',
    lineWidth: 1,
    lineJoin: '',
    globalAlpha: 1,
    globalCompositeOperation: 'source-over',
    save: () => {},
    restore: () => {},
    transform: () => {},
    beginPath: () => {},
    moveTo: () => void segments++,
    lineTo: () => void segments++,
    closePath: () => {},
    fill: () => void log.push(`fill ${ctx.fillStyle}`),
    stroke: () => void log.push(`stroke ${ctx.strokeStyle}`),
    drawImage: () => void log.push('image'),
  }
  return { ctx: ctx as unknown as CanvasRenderingContext2D, log, segments: () => segments }
}
const INKS = { land: 'L', water: 'W', river: 'R', coast: 'C', state: 'S' }

describe('the painters', () => {
  it('equirectangular: land, then the relief, then lakes, then the lines with the coast on top', () => {
    const { ctx, log } = recorder()
    const region: Region = { kind: 'rect', west: -180, south: -90, east: 180, north: 90 }
    paintEquirect(ctx, 5, 960, 480, { map: MAPS['50m'], region, inks: INKS, pxPerDeg: 5, coast: 1, states: 1 }, () => void log.push('relief'))
    expect(log).toEqual(['fill L', 'relief', 'fill W', 'stroke R', 'stroke S', 'stroke C', 'stroke C', 'stroke C'])
  })

  it('equirectangular: a layer turned off draws nothing', () => {
    const { ctx, log } = recorder()
    const region: Region = { kind: 'rect', west: -180, south: -90, east: 180, north: 90 }
    paintEquirect(ctx, 5, 960, 480, { map: MAPS['50m'], region, inks: INKS, pxPerDeg: 5, coast: 0, states: 0 })
    expect(log).toEqual(['fill L', 'fill W', 'stroke R'])
  })

  describe('projected: past the point budget, a 1:10m view falls back to 1:50m', () => {
    const realFetch = globalThis.fetch
    beforeAll(async () => {
      // loadBasemap fetches the asset URL; serve the shipped files from disk.
      globalThis.fetch = vi.fn(async (url: RequestInfo | URL) => {
        const name = String(url).match(/basemap-(50m|10m)/)![1] as '50m' | '10m'
        return new Response(file(name))
      }) as typeof fetch
      await loadBasemap('50m')
    })
    afterAll(() => {
      globalThis.fetch = realFetch
    })

    it('a narrow globe view draws 1:10m; the wide beam view at the same density does not', () => {
      expect(basemapAt('50m').scale).toBe('50m')
      const inks = INKS
      // The 2-D globe zoomed on the Great Lakes: a small cap, well under the budget.
      const ortho = geoOrthographic().rotate([87, -43.5]).clipAngle(90).translate([500, 500]).scale(5000)
      const narrow = visibleRegion('globe', ortho, 1000, 1000)
      const a = recorder()
      paintProjected(a.ctx, ortho, { map: MAPS['10m'], region: narrow, inks, pxPerDeg: pxPerDegree(ortho), coast: 1, states: 1 })
      const a50 = recorder()
      paintProjected(a50.ctx, ortho, { map: MAPS['50m'], region: narrow, inks, pxPerDeg: pxPerDegree(ortho), coast: 1, states: 1 })
      expect(a.segments(), 'the narrow view drew the 1:10m detail').toBeGreaterThan(3 * a50.segments())
      // The beam map reaching ~42° from the QTH, handed 1:10m: far over the budget.
      const aeqd = geoAzimuthalEquidistant().rotate([89, -42.5]).clipAngle(180).translate([500, 500]).scale(3000 / Math.PI)
      const wide = visibleRegion('aeqd', aeqd, 1000, 1000)
      const b = recorder()
      paintProjected(b.ctx, aeqd, { map: MAPS['10m'], region: wide, inks, pxPerDeg: pxPerDegree(aeqd), coast: 1, states: 1 })
      const b50 = recorder()
      paintProjected(b50.ctx, aeqd, { map: MAPS['50m'], region: wide, inks, pxPerDeg: pxPerDegree(aeqd), coast: 1, states: 1 })
      expect(b.segments()).toBe(b50.segments())
      expect(PROJECTED_BUDGET).toBeGreaterThan(0)
    })

    it('leaves the projection as it found it', () => {
      const proj = geoOrthographic().rotate([87, -43.5]).clipAngle(90).translate([500, 500]).scale(800)
      const before = proj.precision()
      const { ctx } = recorder()
      paintProjected(ctx, proj, { map: MAPS['50m'], region: visibleRegion('globe', proj, 1000, 1000), inks: INKS, pxPerDeg: pxPerDegree(proj), coast: 1, states: 1 })
      expect(proj.precision()).toBe(before)
    })
  })
})
