// THE BASE GEOGRAPHY — land, lakes, rivers, country borders and US state lines from Natural Earth
// (public domain) at its three published scales, the right one picked by zoom, drawn the same way on
// the flat map, the 2-D globe and beam map, and the texture of the 3-D globe.
//
// THE THREE SCALES. 1:110m is bundled as a module (28 KB), so the first frame always has land.
// 1:50m (260 KB) and 1:10m (1.6 MB) are separate assets, fetched the first time a map is drawn at a
// zoom that wants them — the 10m file only once someone zooms in. `scaleFor` picks by on-screen
// density (layout px per degree of latitude, from the projection's own scale): each scale is used
// from about where its detail stops being sub-pixel. Until a finer file has arrived the next coarser
// one is drawn, so a map never waits on a fetch.
//
// THE FILES (`scripts/gen-basemap.mjs` writes them and documents the format; keep the reader below
// in step with it). Integers on a grid of `units` per degree, varint deltas, cut into tiles (10° at
// 1:10m, 30° at 1:50m, one tile at 1:110m) so a zoomed map projects only what it can see.
//
// THE COASTLINE IS NOT STORED: it is the outline of the land and lake polygons less the edges the data
// invented — the antimeridian cut, Antarctica's closure along the pole and the tile cuts — and every
// invented edge has both its ends on one tile boundary line, so `decode` drops exactly those. The
// coast and the land fill are therefore the same points at every zoom, and the cuts never show:
// the land of all the visible tiles is always filled as one path.
//
// TWO WAYS TO DRAW. The flat world map is equirectangular, so its projection is a plain scale and
// offset: `paintEquirect` draws each tile from a cached Path2D in lon/lat under a canvas transform,
// with no per-point JavaScript at all, and lays the shaded relief under the lines. The globe and the
// beam map are not linear, so `paintProjected` streams the visible tiles through the d3 projection
// (its clipping and resampling included) into the canvas, as `geoPath` would, without building any
// GeoJSON. The 3-D globes draw the same lines over NASA's pictures of the Earth
// (features/globeBasemap.ts).
import type { GeoProjection, GeoStream } from 'd3-geo'
import coarsest from './data/basemap-110m'
import url50m from './data/basemap-50m.bin?url'
import url10m from './data/basemap-10m.bin?url'

export type BasemapScale = '110m' | '50m' | '10m'

/** One tile of one layer. Points are lon/lat pairs in degrees. */
export interface BasemapTile {
  /** row * (360 / tileDeg) + col, counted from lon -180 and lat -90. */
  index: number
  west: number
  south: number
  east: number
  north: number
  coords: Float32Array
  /** Where each ring (polygon layers) or line starts, in points; one more entry, the point count. */
  parts: Uint32Array
  /** Polygon layers: where each polygon starts, in rings (its first ring is the exterior); one more
   *  entry, the ring count. */
  polys?: Uint32Array
  /** Ranked lines (rivers): each line's Natural Earth scalerank, lower = more important. */
  ranks?: Uint8Array
  /** Polygon layers: the outline to stroke, as [firstPoint, pointCount, closed] triples. */
  shore?: Uint32Array
  /** For culling a projected map piece by piece (see `CapTest`): per polygon (`fillCaps`), shore run
   *  (`shoreCaps`) or line (`lineCaps`), the centre of its lon/lat box as a unit vector and the
   *  cosine and sine of an angular radius that holds the whole piece — five numbers each. */
  fillCaps?: Float32Array
  shoreCaps?: Float32Array
  lineCaps?: Float32Array
}

export interface BasemapLayer {
  tiles: BasemapTile[]
}

export interface Basemap {
  scale: BasemapScale
  tileDeg: number
  land: BasemapLayer
  lakes: BasemapLayer
  rivers: BasemapLayer
  borders: BasemapLayer
  states: BasemapLayer
}

// ── Reading a file ─────────────────────────────────────────────────────────────────────────────

const LAYER_NAMES = ['land', 'lakes', 'rivers', 'borders', 'states'] as const

/** Decode one basemap file. Throws on anything that is not one. */
export function decodeBasemap(bytes: Uint8Array, scale: BasemapScale): Basemap {
  let at = 0
  const uint = (): number => {
    let v = 0
    let mul = 1
    for (;;) {
      if (at >= bytes.length) throw new Error('basemap: truncated')
      const b = bytes[at++]
      v += (b & 0x7f) * mul
      if (b < 0x80) return v
      mul *= 128
    }
  }
  const sint = (): number => {
    const z = uint()
    return z % 2 === 1 ? -(z + 1) / 2 : z / 2
  }
  if (String.fromCharCode(bytes[0], bytes[1], bytes[2], bytes[3]) !== 'NXBM' || bytes[4] !== 1) {
    throw new Error('basemap: not a version-1 basemap file')
  }
  at = 5
  const units = uint()
  const tileDeg = uint()
  const cols = 360 / tileDeg
  const step = tileDeg * units
  const out: Partial<Record<(typeof LAYER_NAMES)[number], BasemapLayer>> = {}
  const layerCount = uint()
  for (let l = 0; l < layerCount; l++) {
    const nameLen = uint()
    let name = ''
    for (let i = 0; i < nameLen; i++) name += String.fromCharCode(bytes[at++])
    const kind = uint()
    const tileCount = uint()
    const tiles: BasemapTile[] = []
    for (let t = 0; t < tileCount; t++) {
      const index = uint()
      const partCount = uint()
      const ringStarts: number[] = [0]
      const polyStarts: number[] = [0]
      const ranks: number[] = []
      let points = 0
      for (let p = 0; p < partCount; p++) {
        if (kind === 0) {
          const rings = uint()
          for (let r = 0; r < rings; r++) ringStarts.push((points += uint()))
          polyStarts.push(ringStarts.length - 1)
        } else {
          if (kind === 2) ranks.push(uint())
          ringStarts.push((points += uint()))
        }
      }
      const col = index % cols
      const row = Math.floor(index / cols)
      const ints = new Int32Array(points * 2)
      let x = col * step
      let y = row * step
      for (let i = 0; i < points; i++) {
        x += sint()
        y += sint()
        ints[2 * i] = x
        ints[2 * i + 1] = y
      }
      const coords = new Float32Array(points * 2)
      for (let i = 0; i < points; i++) {
        coords[2 * i] = ints[2 * i] / units - 180
        coords[2 * i + 1] = ints[2 * i + 1] / units - 90
      }
      const tile: BasemapTile = {
        index,
        west: col * tileDeg - 180,
        south: row * tileDeg - 90,
        east: (col + 1) * tileDeg - 180,
        north: (row + 1) * tileDeg - 90,
        coords,
        parts: Uint32Array.from(ringStarts),
      }
      if (kind === 0) {
        tile.polys = Uint32Array.from(polyStarts)
        tile.shore = shoreRuns(ints, tile.parts, step)
        const polys = tile.polys
        tile.fillCaps = capsOf(coords, polys.length - 1, (p) => [tile.parts[polys[p]], tile.parts[polys[p] + 1]])
        const sh = tile.shore
        tile.shoreCaps = capsOf(coords, sh.length / 3, (k) => [sh[3 * k], sh[3 * k] + sh[3 * k + 1]])
      } else {
        tile.lineCaps = capsOf(coords, tile.parts.length - 1, (l) => [tile.parts[l], tile.parts[l + 1]])
      }
      if (kind === 2) tile.ranks = Uint8Array.from(ranks)
      tiles.push(tile)
    }
    out[name as (typeof LAYER_NAMES)[number]] = { tiles }
  }
  for (const n of LAYER_NAMES) if (!out[n]) throw new Error(`basemap: no ${n} layer`)
  return { scale, tileDeg, ...(out as Record<(typeof LAYER_NAMES)[number], BasemapLayer>) }
}

/** The coastline of a polygon layer: every ring edge except those whose two ends lie on one tile
 *  boundary line (the edges the data invented). The writer starts a ring just after an invented
 *  edge when it has one, so a run never wraps; a ring with none is one closed run. */
function shoreRuns(ints: Int32Array, parts: Uint32Array, step: number): Uint32Array {
  const runs: number[] = []
  const cut = (a: number, b: number): boolean =>
    (ints[2 * a] === ints[2 * b] && ints[2 * a] % step === 0) ||
    (ints[2 * a + 1] === ints[2 * b + 1] && ints[2 * a + 1] % step === 0)
  for (let r = 0; r + 1 < parts.length; r++) {
    const s = parts[r]
    const e = parts[r + 1]
    let any = false
    for (let i = s; i < e; i++) if (cut(i, i + 1 < e ? i + 1 : s)) any = true
    if (!any) {
      runs.push(s, e - s, 1)
      continue
    }
    let start = -1
    for (let i = s; i < e; i++) {
      const isCut = cut(i, i + 1 < e ? i + 1 : s)
      if (!isCut && start < 0) start = i
      if (isCut && start >= 0) {
        runs.push(start, i - start + 1, 0)
        start = -1
      }
    }
    if (start >= 0) runs.push(start, e - start, 0)
  }
  return Uint32Array.from(runs)
}

/** A culling cap per piece: the piece is the points [from, to) that `range(i)` names (a polygon's
 *  exterior ring holds its holes, so the exterior is enough). */
function capsOf(coords: Float32Array, count: number, range: (i: number) => [number, number]): Float32Array {
  const out = new Float32Array(count * 5)
  for (let i = 0; i < count; i++) {
    const [a, b] = range(i)
    let w = Infinity
    let e = -Infinity
    let s = Infinity
    let n = -Infinity
    for (let j = a; j < b; j++) {
      const x = coords[2 * j]
      const y = coords[2 * j + 1]
      if (x < w) w = x
      if (x > e) e = x
      if (y < s) s = y
      if (y > n) n = y
    }
    const lon = (w + e) / 2
    const lat = (s + n) / 2
    const corners = [[w, s], [e, s], [w, n], [e, n], [lon, s], [lon, n], [w, lat], [e, lat]]
    const r = (Math.max(...corners.map(([x, y]) => arcDeg(lon, lat, x, y))) + 0.01) * RAD
    out[5 * i] = Math.cos(lat * RAD) * Math.cos(lon * RAD)
    out[5 * i + 1] = Math.cos(lat * RAD) * Math.sin(lon * RAD)
    out[5 * i + 2] = Math.sin(lat * RAD)
    out[5 * i + 3] = Math.cos(r)
    out[5 * i + 4] = Math.sin(r)
  }
  return out
}

// ── Which scale, and loading it ────────────────────────────────────────────────────────────────

/** Below this many layout px per degree the map draws 1:110m; below `TEN_FROM`, 1:50m. 1:50m's
 *  detail is about 0.12° (13 km) and 1:10m's about 0.025°, so each takes over a little before the
 *  coarser one's corners would show as a pixel step. */
export const FIFTY_FROM = 2
export const TEN_FROM = 18

/** The scale the map should draw at `pxPerDeg` layout px per degree. */
export function scaleFor(pxPerDeg: number): BasemapScale {
  return pxPerDeg >= TEN_FROM ? '10m' : pxPerDeg >= FIFTY_FROM ? '50m' : '110m'
}

/** Layout px per degree at the projection's centre (`scale` is px per radian for all three of the
 *  map's projections). */
export function pxPerDegree(proj: GeoProjection): number {
  return (proj.scale() * Math.PI) / 180
}

const URLS: Record<Exclude<BasemapScale, '110m'>, string> = { '50m': url50m, '10m': url10m }
const loaded = new Map<BasemapScale, Basemap>()
const loading = new Map<BasemapScale, Promise<Basemap | null>>()
const listeners = new Set<() => void>()

function bytesOfBase64(b64: string): Uint8Array {
  const bin = atob(b64)
  const out = new Uint8Array(bin.length)
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i)
  return out
}

/** The 1:110m map, always there (decoded on first use). */
export function coarsestBasemap(): Basemap {
  let m = loaded.get('110m')
  if (!m) {
    m = decodeBasemap(bytesOfBase64(coarsest), '110m')
    loaded.set('110m', m)
  }
  return m
}

/** Fetch a finer scale once; resolves null if it cannot be had (no network stack in a test, a
 *  missing asset) — the map then keeps drawing the coarser one. */
export function loadBasemap(scale: BasemapScale): Promise<Basemap | null> {
  if (scale === '110m') return Promise.resolve(coarsestBasemap())
  const have = loaded.get(scale)
  if (have) return Promise.resolve(have)
  let p = loading.get(scale)
  if (!p) {
    p = (async () => {
      try {
        const res = await fetch(URLS[scale])
        if (!res.ok) return null
        const m = decodeBasemap(new Uint8Array(await res.arrayBuffer()), scale)
        loaded.set(scale, m)
        for (const f of [...listeners]) f()
        return m
      } catch {
        return null
      }
    })()
    loading.set(scale, p)
  }
  return p
}

/** The best map there is for `scale` right now, asking for the wanted one if it is not here yet. */
export function basemapAt(scale: BasemapScale): Basemap {
  const want = loaded.get(scale)
  if (want) return want
  if (scale !== '110m') void loadBasemap(scale)
  return (scale === '10m' ? loaded.get('50m') : undefined) ?? coarsestBasemap()
}

/** Called whenever a finer scale arrives (so a map drawn from a coarser one can redraw). */
export function onBasemapLoaded(f: () => void): () => void {
  listeners.add(f)
  return () => {
    listeners.delete(f)
  }
}

// ── What is in view ────────────────────────────────────────────────────────────────────────────

/** The part of the world a map shows: a lon/lat rectangle (flat map) or a cap of `radius` degrees
 *  around a centre (globe, beam map). Tiles outside it are never drawn. */
export type Region =
  | { kind: 'rect'; west: number; south: number; east: number; north: number }
  | { kind: 'cap'; lon: number; lat: number; radius: number }

const RAD = Math.PI / 180

/** Angular distance in degrees. */
function arcDeg(lon1: number, lat1: number, lon2: number, lat2: number): number {
  const s =
    Math.sin(lat1 * RAD) * Math.sin(lat2 * RAD) + Math.cos(lat1 * RAD) * Math.cos(lat2 * RAD) * Math.cos((lon2 - lon1) * RAD)
  return Math.acos(Math.max(-1, Math.min(1, s))) / RAD
}

/** What a map of kind `kind` drawn with `proj` on a `width` × `height` canvas can show. */
export function visibleRegion(kind: 'globe' | 'aeqd' | 'world', proj: GeoProjection, width: number, height: number): Region {
  if (kind === 'world') {
    const corners = [[0, 0], [width, 0], [0, height], [width, height]].map((p) => proj.invert?.(p as [number, number]) ?? null)
    if (corners.some((c) => !c)) return { kind: 'rect', west: -180, south: -90, east: 180, north: 90 }
    const lons = corners.map((c) => c![0])
    const lats = corners.map((c) => c![1])
    return {
      kind: 'rect',
      west: Math.max(-180, Math.min(...lons)),
      east: Math.min(180, Math.max(...lons)),
      south: Math.max(-90, Math.min(...lats)),
      north: Math.min(90, Math.max(...lats)),
    }
  }
  const [cx, cy] = proj.translate()
  const reach = Math.max(...[[0, 0], [width, 0], [0, height], [width, height]].map(([x, y]) => Math.hypot(x - cx, y - cy)))
  const s = proj.scale()
  const [lambda, phi] = proj.rotate()
  // Orthographic: a screen distance d from the centre is asin(d / scale) of arc, and the globe never
  // shows more than its near hemisphere. Azimuthal equidistant: d / scale radians, up to the antipode.
  const radius = kind === 'globe' ? (reach >= s ? 90 : Math.asin(reach / s) / RAD) : Math.min(180, reach / s / RAD)
  return { kind: 'cap', lon: -lambda, lat: -phi, radius: radius + 0.5 }
}

const tileReach = new WeakMap<BasemapTile, { lon: number; lat: number; r: number }>()
function reachOf(t: BasemapTile): { lon: number; lat: number; r: number } {
  let c = tileReach.get(t)
  if (!c) {
    const lon = (t.west + t.east) / 2
    const lat = (t.south + t.north) / 2
    const edge = [[t.west, t.south], [t.east, t.south], [t.west, t.north], [t.east, t.north], [lon, t.south], [lon, t.north], [t.west, lat], [t.east, lat]]
    c = { lon, lat, r: Math.max(...edge.map(([x, y]) => arcDeg(lon, lat, x, y))) + 0.5 }
    tileReach.set(t, c)
  }
  return c
}

/** Does any of the tile fall inside the region? Conservative: a tile it cannot rule out is drawn. */
export function tileInRegion(t: BasemapTile, region: Region): boolean {
  if (region.kind === 'rect') {
    return t.east >= region.west && t.west <= region.east && t.north >= region.south && t.south <= region.north
  }
  if (region.radius >= 180) return true
  const c = reachOf(t)
  return arcDeg(region.lon, region.lat, c.lon, c.lat) <= region.radius + c.r
}

// ── Tracing geometry ───────────────────────────────────────────────────────────────────────────

/** Anything with the canvas path verbs: a 2-D context or a Path2D. */
type PathSink = Pick<CanvasPath, 'moveTo' | 'lineTo' | 'closePath'>

function tracePolygons(t: BasemapTile, out: PathSink): void {
  const { coords, parts } = t
  for (let r = 0; r + 1 < parts.length; r++) {
    const s = parts[r]
    out.moveTo(coords[2 * s], coords[2 * s + 1])
    for (let i = s + 1; i < parts[r + 1]; i++) out.lineTo(coords[2 * i], coords[2 * i + 1])
    out.closePath()
  }
}
function traceLines(t: BasemapTile, out: PathSink, maxRank: number): void {
  const { coords, parts, ranks } = t
  for (let l = 0; l + 1 < parts.length; l++) {
    if (ranks && ranks[l] > maxRank) continue
    const s = parts[l]
    out.moveTo(coords[2 * s], coords[2 * s + 1])
    for (let i = s + 1; i < parts[l + 1]; i++) out.lineTo(coords[2 * i], coords[2 * i + 1])
  }
}
function traceShore(t: BasemapTile, out: PathSink): void {
  const { coords, shore } = t
  if (!shore) return
  for (let k = 0; k < shore.length; k += 3) {
    const s = shore[k]
    out.moveTo(coords[2 * s], coords[2 * s + 1])
    for (let i = s + 1; i < s + shore[k + 1]; i++) out.lineTo(coords[2 * i], coords[2 * i + 1])
    if (shore[k + 2]) out.closePath()
  }
}

type Trace = 'fill' | 'shore' | `lines:${number}`
const pathCache = new WeakMap<BasemapTile, Map<Trace, Path2D>>()
function trace(t: BasemapTile, what: Trace, out: PathSink): void {
  if (what === 'fill') tracePolygons(t, out)
  else if (what === 'shore') traceShore(t, out)
  else traceLines(t, out, Number(what.slice(6)))
}
/** The tiles' geometry as one path in lon/lat: a combined Path2D from per-tile cached ones, or —
 *  where there is no Path2D (a test's DOM) — traced straight into the context. */
function addTiles(ctx: CanvasRenderingContext2D, tiles: BasemapTile[], what: Trace): Path2D | null {
  if (typeof Path2D !== 'function') {
    ctx.beginPath()
    for (const t of tiles) trace(t, what, ctx)
    return null
  }
  const all = new Path2D()
  for (const t of tiles) {
    let m = pathCache.get(t)
    if (!m) pathCache.set(t, (m = new Map()))
    let p = m.get(what)
    if (!p) {
      p = new Path2D()
      trace(t, what, p)
      m.set(what, p)
    }
    all.addPath(p)
  }
  return all
}

/** A d3 stream that writes into a canvas path, exactly as d3's own path context does. */
function contextStream(ctx: PathSink): GeoStream {
  let first = true
  let polygon = false
  return {
    point(x: number, y: number) {
      if (first) {
        ctx.moveTo(x, y)
        first = false
      } else ctx.lineTo(x, y)
    },
    lineStart() {
      first = true
    },
    lineEnd() {
      if (polygon) ctx.closePath()
    },
    polygonStart() {
      polygon = true
    },
    polygonEnd() {
      polygon = false
    },
    sphere() {},
  }
}

/** A cap region ready for per-piece tests: its centre as a unit vector and its radius's cosine
 *  and sine. A piece with centre U and radius r is in view when the angle between U and the
 *  centre is at most radius + r, i.e. U·C ≥ cos(radius + r). */
interface CapTest {
  x: number
  y: number
  z: number
  cos: number
  sin: number
}
function capTest(region: Region): CapTest | null {
  // Past 91° the sum below could pass the antipode, where the cosine test turns back on itself;
  // a view that wide is most of the sphere anyway, so it is not culled.
  if (region.kind !== 'cap' || region.radius >= 91) return null
  const lat = region.lat * RAD
  const lon = region.lon * RAD
  const r = region.radius * RAD
  return { x: Math.cos(lat) * Math.cos(lon), y: Math.cos(lat) * Math.sin(lon), z: Math.sin(lat), cos: Math.cos(r), sin: Math.sin(r) }
}
function pieceInView(caps: Float32Array | undefined, i: number, c: CapTest | null): boolean {
  if (!c || !caps) return true
  const k = 5 * i
  // A piece 90° or more across (a continent at 1:110m) is never culled; below that, with the view
  // under 91°, radius + r stays short of the antipode and cos(radius + r) = cos·cos r − sin·sin r.
  if (caps[k + 3] <= 0) return true
  return caps[k] * c.x + caps[k + 1] * c.y + caps[k + 2] * c.z >= c.cos * caps[k + 3] - c.sin * caps[k + 4]
}

/** Points of [a, a + n) that survive the sub-pixel filter: the first always (and, for an open line,
 *  the last), then each one more than `tol` degrees in lon or lat from the last kept one. */
function eachKept(coords: Float32Array, a: number, n: number, tol: number, open: boolean, f: (x: number, y: number) => void): number {
  let kept = 0
  let lx = 0
  let ly = 0
  for (let i = a; i < a + n; i++) {
    const x = coords[2 * i]
    const y = coords[2 * i + 1]
    if (kept > 0 && !(open && i === a + n - 1) && Math.abs(x - lx) <= tol && Math.abs(y - ly) <= tol) continue
    lx = x
    ly = y
    kept++
    f(x, y)
  }
  return kept
}
const noop = () => {}

/** Stream the tiles through a projection stream, as d3 streams GeoJSON (rings without their closing
 *  point; a closed coast run repeats its first point, as a closed LineString does). With `region`,
 *  pieces wholly out of view are skipped; with `tol` (degrees), points that would land within a
 *  fraction of a pixel of the previous one are dropped before projection — the expensive part —
 *  and a ring that shrinks below a triangle is dropped with them. */
export function streamTiles(s: GeoStream, tiles: BasemapTile[], what: Trace, region?: Region, tol = 0): void {
  const c = region ? capTest(region) : null
  for (const t of tiles) {
    const { coords, parts } = t
    if (what === 'fill') {
      const polys = t.polys!
      for (let p = 0; p + 1 < polys.length; p++) {
        if (!pieceInView(t.fillCaps, p, c)) continue
        const r0 = polys[p]
        if (eachKept(coords, parts[r0], parts[r0 + 1] - parts[r0], tol, false, noop) < 3) continue
        s.polygonStart()
        for (let r = r0; r < polys[p + 1]; r++) {
          const a = parts[r]
          const n = parts[r + 1] - a
          if (r > r0 && eachKept(coords, a, n, tol, false, noop) < 3) continue
          s.lineStart()
          eachKept(coords, a, n, tol, false, (x, y) => s.point(x, y))
          s.lineEnd()
        }
        s.polygonEnd()
      }
    } else if (what === 'shore') {
      const shore = t.shore!
      for (let k = 0; k < shore.length; k += 3) {
        if (!pieceInView(t.shoreCaps, k / 3, c)) continue
        const a = shore[k]
        s.lineStart()
        eachKept(coords, a, shore[k + 1], tol, !shore[k + 2], (x, y) => s.point(x, y))
        if (shore[k + 2]) s.point(coords[2 * a], coords[2 * a + 1])
        s.lineEnd()
      }
    } else {
      const maxRank = Number(what.slice(6))
      for (let l = 0; l + 1 < parts.length; l++) {
        if ((t.ranks && t.ranks[l] > maxRank) || !pieceInView(t.lineCaps, l, c)) continue
        s.lineStart()
        eachKept(coords, parts[l], parts[l + 1] - parts[l], tol, true, (x, y) => s.point(x, y))
        s.lineEnd()
      }
    }
  }
}

/** How many points of `tiles` are in view (before the sub-pixel filter): the cost of a projected
 *  draw, so the map can tell when 1:10m would be too much for the area on screen. */
function pointsInView(tiles: BasemapTile[], region: Region): number {
  const c = capTest(region)
  let n = 0
  for (const t of tiles) {
    if (t.polys) {
      for (let p = 0; p + 1 < t.polys.length; p++) {
        if (pieceInView(t.fillCaps, p, c)) n += 2 * (t.parts[t.polys[p + 1]] - t.parts[t.polys[p]]) // filled and stroked
      }
    } else {
      for (let l = 0; l + 1 < t.parts.length; l++) if (pieceInView(t.lineCaps, l, c)) n += t.parts[l + 1] - t.parts[l]
    }
  }
  return n
}

// ── Painting ───────────────────────────────────────────────────────────────────────────────────

/** The basemap's colours (MapView reads them from the theme's --map-* tokens). */
export interface BasemapInks {
  land: string
  /** The sea's colour: lakes are filled with it. */
  water: string
  /** Rivers: a step from the sea's colour, so a line one pixel wide still reads against the land. */
  river: string
  coast: string
  state: string
}

export interface BasemapPaint {
  map: Basemap
  region: Region
  inks: BasemapInks
  /** Layout px per degree at the map's centre — line weights and which rivers show depend on it. */
  pxPerDeg: number
  /** Coastlines and country borders (the map's `coast` layer) at this opacity; 0 = off. */
  coast: number
  /** US state lines (the `states` layer) at this opacity; 0 = off. */
  states: number
}

/** Rivers are ranked 0 (the Amazon) to 9: show more of them as the map zooms in. */
export function riverRankFor(pxPerDeg: number): number {
  return pxPerDeg < 4 ? 1 : pxPerDeg < 8 ? 3 : pxPerDeg < 16 ? 5 : pxPerDeg < 40 ? 6 : 9
}

/** Lines thicken a little as the map zooms in, so a close view does not read as hairlines. */
function weight(base: number, pxPerDeg: number): number {
  return base * Math.min(1.6, Math.max(1, Math.log2(pxPerDeg / 8) * 0.25 + 1))
}

function inRegion(layer: BasemapLayer, region: Region): BasemapTile[] {
  return layer.tiles.filter((t) => tileInRegion(t, region))
}

/** The flat map: equirectangular, so lon/lat go to the canvas through
 *  one transform — x = tx + k·lon, y = ty − k·lat — and the cached tile paths are drawn as they are.
 *  `relief`, when given, is drawn over the land and under every line (see `paintRelief`). */
export function paintEquirect(
  ctx: CanvasRenderingContext2D,
  k: number,
  tx: number,
  ty: number,
  p: BasemapPaint,
  relief?: (ctx: CanvasRenderingContext2D) => void,
): void {
  const { map, region, inks } = p
  const land = inRegion(map.land, region)
  const lakes = inRegion(map.lakes, region)
  const draw = (fill: boolean, tiles: BasemapTile[], what: Trace, ink: string, widthPx: number, alpha: number) => {
    if (!tiles.length || alpha <= 0) return
    ctx.save()
    ctx.transform(k, 0, 0, -k, tx, ty)
    ctx.globalAlpha = alpha
    const path = addTiles(ctx, tiles, what)
    if (fill) {
      ctx.fillStyle = ink
      if (path) ctx.fill(path)
      else ctx.fill()
    } else {
      ctx.strokeStyle = ink
      ctx.lineWidth = widthPx / k
      ctx.lineJoin = 'round'
      if (path) ctx.stroke(path)
      else ctx.stroke()
    }
    ctx.restore()
  }
  draw(true, land, 'fill', inks.land, 0, 1)
  relief?.(ctx)
  draw(true, lakes, 'fill', inks.water, 0, 1)
  paintLines((tiles, what, ink, w, a) => draw(false, tiles, what, ink, w, a), p, land, lakes)
}

/** The most points a projected base map may stream per draw. d3 projects about five million points
 *  a second here, and the base map is redrawn on every frame of a drag, so this keeps it to a few
 *  milliseconds; past it, a 1:10m view falls back to 1:50m (the beam map's wide view when zoomed). */
export const PROJECTED_BUDGET = 160_000

/** The 2-D globe and the beam map: the visible tiles streamed through the projection (d3 clips at
 *  the horizon) into the canvas. Pieces out of view are skipped and sub-pixel steps dropped before
 *  projection, and the projection's adaptive resampling is off while the base map streams: Natural
 *  Earth's edges are already short at the scale each file is drawn at. */
export function paintProjected(ctx: CanvasRenderingContext2D, proj: GeoProjection, p: BasemapPaint, landInk = p.inks.land): void {
  const { region, inks, pxPerDeg } = p
  let map = p.map
  if (map.scale === '10m') {
    const tiles = [map.land, map.lakes, map.rivers, map.borders, map.states].flatMap((l) => inRegion(l, region))
    if (pointsInView(tiles, region) > PROJECTED_BUDGET) map = basemapAt('50m')
  }
  const land = inRegion(map.land, region)
  const lakes = inRegion(map.lakes, region)
  const tol = 0.6 / Math.max(pxPerDeg, 1e-6)
  const precision = proj.precision()
  proj.precision(0)
  const draw = (fill: boolean, tiles: BasemapTile[], what: Trace, ink: string, widthPx: number, alpha: number) => {
    if (!tiles.length || alpha <= 0) return
    ctx.globalAlpha = alpha
    ctx.beginPath()
    streamTiles(proj.stream(contextStream(ctx)), tiles, what, region, tol)
    if (fill) {
      ctx.fillStyle = ink
      ctx.fill()
    } else {
      ctx.strokeStyle = ink
      ctx.lineWidth = widthPx
      ctx.lineJoin = 'round'
      ctx.stroke()
    }
    ctx.globalAlpha = 1
  }
  try {
    draw(true, land, 'fill', landInk, 0, 1)
    draw(true, lakes, 'fill', inks.water, 0, 1)
    paintLines((tiles, what, ink, w, a) => draw(false, tiles, what, ink, w, a), { ...p, map }, land, lakes)
  } finally {
    proj.precision(precision)
  }
}

/** How strongly the shaded relief is laid on at `pxPerDeg`. The raster has about 11 px per degree, so
 *  past twice that it is being magnified; it fades to half by then and stays there, so a close view
 *  keeps a hint of terrain without a soft raster competing with the sharp lines. */
export function reliefAlphaFor(pxPerDeg: number): number {
  const RELIEF = 0.7
  return RELIEF * (pxPerDeg <= 12 ? 1 : pxPerDeg >= 24 ? 0.5 : 1 - ((pxPerDeg - 12) / 12) * 0.5)
}

/** The shaded relief (assets/earth-relief.webp: a world hillshade, scripts/gen-relief.py) laid over
 *  what is already drawn with 'hard-light' at `alpha`: its grey 128 — flat ground and all water —
 *  leaves the theme's colours exactly as they are, darker greys shade a slope away from the light
 *  and lighter ones lift a slope towards it. (x0, y0)–(x1, y1) is where the WHOLE world falls on
 *  the canvas; only the part inside cw × ch is drawn. */
export function paintRelief(
  ctx: CanvasRenderingContext2D,
  img: CanvasImageSource & { width: number; height: number },
  x0: number,
  y0: number,
  x1: number,
  y1: number,
  cw: number,
  ch: number,
  alpha: number,
): void {
  const vx0 = Math.max(0, x0)
  const vy0 = Math.max(0, y0)
  const vx1 = Math.min(cw, x1)
  const vy1 = Math.min(ch, y1)
  if (!(vx1 > vx0 && vy1 > vy0 && alpha > 0 && img.width > 0)) return
  const fx = img.width / (x1 - x0)
  const fy = img.height / (y1 - y0)
  ctx.save()
  ctx.globalCompositeOperation = 'hard-light'
  ctx.globalAlpha = alpha
  ctx.drawImage(img, (vx0 - x0) * fx, (vy0 - y0) * fy, (vx1 - vx0) * fx, (vy1 - vy0) * fy, vx0, vy0, vx1 - vx0, vy1 - vy0)
  ctx.restore()
}

/** The lines, in the same order and weights whichever way they are drawn: rivers, then US states,
 *  then country borders, then the coast over everything. */
function paintLines(
  stroke: (tiles: BasemapTile[], what: Trace, ink: string, widthPx: number, alpha: number) => void,
  p: BasemapPaint,
  land: BasemapTile[],
  lakes: BasemapTile[],
): void {
  const { map, region, inks, pxPerDeg } = p
  stroke(inRegion(map.rivers, region), `lines:${riverRankFor(pxPerDeg)}`, inks.river, weight(0.7, pxPerDeg), 0.9)
  stroke(inRegion(map.states, region), 'lines:255', inks.state, weight(0.6, pxPerDeg), p.states)
  stroke(inRegion(map.borders, region), 'lines:255', inks.coast, weight(0.6, pxPerDeg), p.coast * 0.75)
  stroke(land, 'shore', inks.coast, weight(0.8, pxPerDeg), p.coast)
  stroke(lakes, 'shore', inks.coast, weight(0.6, pxPerDeg), p.coast * 0.8)
}
