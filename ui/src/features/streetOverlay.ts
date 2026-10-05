// STREET OVERLAYS: how Nexus's own map layers sit on the street map (components/StreetMap.tsx).
//
// THE STACK. MapLibre's canvas draws the base map (land, water, streets and their names), the 2-D
// map's overlay canvas sits on it with `pointer-events: none`, and the map's DOM chrome sits on that.
// MapView mounts all three. MapLibre owns drag and zoom; MapView's hit test runs from MapLibre's
// pointer events.
//
// THE PROJECTION LOCK. Every overlay is drawn through a d3 projection, and on Street that is a
// `geoMercator()` locked to MapLibre's camera. MapLibre's world is 512 · 2^zoom CSS px across, so the
// scale is 512 · 2^zoom / 2π, and the translate puts the camera's centre on the pixel where MapLibre's
// own `project` put it. Rotation and pitch are off and the world is drawn once (StreetMap), so the two
// Mercators are one function: a position lands on the same pixel through both (streetOverlay.test.ts
// checks it against MapLibre's formula; the real-browser check runs MapLibre itself).
//
// STREET SCALE starts at zoom 8, where a 4-character square is about 700 px across. From there a
// position known only by its grid square is drawn as that square's outline, never as a pin claiming a
// street it does not know, and the world-scale fields are dimmed in the Layers panel.
import { geoMercator, type GeoProjection } from 'd3-geo'
import type { StreetCamera } from '../components/StreetMap'
import type { StreetPack } from './streetPack'

/** MapLibre's world at zoom 0, in CSS px. */
export const WORLD_PX = 512

/** Where street scale begins (MapLibre zoom). */
export const STREET_SCALE_ZOOM = 8

/** How much larger than it was drawn a picture held through a zoom-in may show (MapView). Holding a
 *  picture magnifies it, and the offset of its lines from the map with it; past this it is redrawn,
 *  so through a zoom-in the overlays stay within 5% of their offset at rest (operator ruling
 *  2026-10-05). */
export const HELD_ZOOM_IN_LIMIT = 1.05

/** Paths are clipped this far outside the map, so the cut edge of a clipped area is never stroked
 *  where it can be seen. */
const CLIP_MARGIN_PX = 16

/** A camera as a value. MapLibre's `project` is live, so the centre's pixel is read once, here: a view
 *  kept for later still says where the map WAS. */
export interface StreetView {
  /** [longitude, latitude] in degrees. */
  center: [number, number]
  zoom: number
  /** Where MapLibre drew `center`, in the map's CSS px, when the view was read. */
  at: [number, number]
}

export function streetViewOf(cam: StreetCamera): StreetView {
  return { center: cam.center, zoom: cam.zoom, at: cam.project(cam.center) }
}

export function sameStreetView(a: StreetView | null, b: StreetView | null): boolean {
  return (
    a === b ||
    (a != null &&
      b != null &&
      a.zoom === b.zoom &&
      a.center[0] === b.center[0] &&
      a.center[1] === b.center[1] &&
      a.at[0] === b.at[0] &&
      a.at[1] === b.at[1])
  )
}

/** The d3 projection every overlay is drawn and hit-tested through on Street, for a map `w` × `h`. */
export function streetProjection(v: StreetView, w: number, h: number): GeoProjection {
  const p = geoMercator()
    .scale((WORLD_PX * 2 ** v.zoom) / (2 * Math.PI))
    .translate([0, 0])
  const [x, y] = p(v.center) ?? [0, 0]
  return p.translate([v.at[0] - x, v.at[1] - y]).clipExtent([
    [-CLIP_MARGIN_PX, -CLIP_MARGIN_PX],
    [w + CLIP_MARGIN_PX, h + CLIP_MARGIN_PX],
  ])
}

/** Web Mercator's latitude limit: the world is a square. */
const MAX_LAT = 85.0511287798

/** What a street view of `w` × `h` shows, in degrees, inside the one world MapLibre draws. Read
 *  off the Mercator's own scale and translate rather than `invert`, which wraps a longitude past
 *  ±180° back into range: a zoomed-out map wider than the world would come back inside out. */
export function streetBounds(proj: GeoProjection, w: number, h: number): Bounds {
  const k = proj.scale()
  const [tx, ty] = proj.translate()
  const lon = (x: number) => ((x - tx) / k) * (180 / Math.PI)
  const lat = (y: number) => (2 * Math.atan(Math.exp((ty - y) / k)) - Math.PI / 2) * (180 / Math.PI)
  return {
    w: Math.max(-180, lon(0)),
    s: Math.max(-MAX_LAT, lat(h)),
    e: Math.min(180, lon(w)),
    n: Math.min(MAX_LAT, lat(0)),
  }
}

/** Web Mercator in world units (0 to 1 across the world): MapLibre's own formula. */
function mercatorUnit([lon, lat]: [number, number]): [number, number] {
  return [(180 + lon) / 360, (180 - (180 / Math.PI) * Math.log(Math.tan(Math.PI / 4 + (lat * Math.PI) / 360))) / 360]
}

/** How a picture drawn at `drawn` moves to sit on the map at `live`: a point drawn at p is now at
 *  `s · p + (x, y)`. Exact, because two views of one Mercator differ only by a scale and a shift. */
export function streetShift(drawn: StreetView, live: StreetView): { s: number; x: number; y: number } {
  const s = 2 ** (live.zoom - drawn.zoom)
  const size = WORLD_PX * 2 ** live.zoom
  const [u0, v0] = mercatorUnit(drawn.center)
  const [u1, v1] = mercatorUnit(live.center)
  return {
    s,
    x: size * (u0 - u1) + live.at[0] - s * drawn.at[0],
    y: size * (v0 - v1) + live.at[1] - s * drawn.at[1],
  }
}

/** Does the pack's square hold this position? Its bbox runs west > east across the 180° meridian. */
export function packCovers({ bbox: [w, s, e, n] }: StreetPack, lat: number, lon: number): boolean {
  if (lat < s || lat > n) return false
  return w <= e ? lon >= w && lon <= e : lon >= w || lon <= e
}

/** A grid square's edges, in degrees. */
export interface Bounds {
  w: number
  s: number
  e: number
  n: number
}

const A = 65

/** The square a 4-, 6- or 8-character locator names; null for anything else. */
export function locatorBounds(grid: string): Bounds | null {
  const g = grid.trim().toUpperCase()
  if (!/^[A-R]{2}[0-9]{2}([A-X]{2}([0-9]{2})?)?$/.test(g)) return null
  let dw = 2
  let ds = 1
  let w = (g.charCodeAt(0) - A) * 20 - 180 + Number(g[2]) * dw
  let s = (g.charCodeAt(1) - A) * 10 - 90 + Number(g[3]) * ds
  if (g.length >= 6) {
    dw = 5 / 60
    ds = 2.5 / 60
    w += (g.charCodeAt(4) - A) * dw
    s += (g.charCodeAt(5) - A) * ds
  }
  if (g.length === 8) {
    dw /= 10
    ds /= 10
    w += Number(g[6]) * dw
    s += Number(g[7]) * ds
  }
  return { w, s, e: w + dw, n: s + ds }
}

/** Is `v` within a hair of `off` plus a whole number of `step`s? */
function onLattice(v: number, step: number, off: number): boolean {
  const r = (((v - off) % step) + step) % step
  return r < 1e-7 || step - r < 1e-7
}

/** The square whose centre this position is, or null. The backend places a spot, and a park it has
 *  no coordinates for, at the centre of its 4- or 6-character square, and the two lattices of centres
 *  never meet (a 4-character centre sits 60′ into its square, a 6-character one 2.5′ plus a whole
 *  number of 5′), so the centre names its square exactly. */
export function squareOfCentre(lat: number, lon: number): Bounds | null {
  const x = lon + 180
  const y = lat + 90
  if (onLattice(x, 2, 1) && onLattice(y, 1, 0.5)) return { w: lon - 1, s: lat - 0.5, e: lon + 1, n: lat + 0.5 }
  const hw = 2.5 / 60
  const hs = 1.25 / 60
  if (onLattice(x, 5 / 60, hw) && onLattice(y, 2.5 / 60, hs)) return { w: lon - hw, s: lat - hs, e: lon + hw, n: lat + hs }
  return null
}

/** The Maidenhead ladder: field, square, subsquare and extended square (the 2-, 4-, 6- and
 *  8-character locators), each cell's width and height in degrees. */
export interface GridLevel {
  chars: 2 | 4 | 6 | 8
  lon: number
  lat: number
}
export const GRID_LEVELS: readonly GridLevel[] = [
  { chars: 2, lon: 20, lat: 10 },
  { chars: 4, lon: 2, lat: 1 },
  { chars: 6, lon: 5 / 60, lat: 2.5 / 60 },
  { chars: 8, lon: 0.5 / 60, lat: 0.25 / 60 },
]

/** Lines are drawn down to the finest level whose cells are this wide on screen… */
export const GRID_LINE_MIN_PX = 40
/** …and cells are named at the finest level that holds an 8-character name. */
export const GRID_LABEL_MIN_PX = 70

/** The finest level whose cells are at least `minPx` wide, or null when even a field is narrower. */
export function gridLevelFor(pxPerLonDeg: number, minPx: number): GridLevel | null {
  let best: GridLevel | null = null
  for (const l of GRID_LEVELS) if (l.lon * pxPerLonDeg >= minPx) best = l
  return best
}

/** The locator of the cell of `chars` characters holding (lat, lon). Subsquare letters are lower
 *  case, as an operator writes them (EN52xa). */
export function locatorAt(lat: number, lon: number, chars: GridLevel['chars']): string {
  let x = Math.min(359.9999999, Math.max(0, lon + 180))
  let y = Math.min(179.9999999, Math.max(0, lat + 90))
  let out = String.fromCharCode(A + Math.floor(x / 20)) + String.fromCharCode(A + Math.floor(y / 10))
  if (chars === 2) return out
  x %= 20
  y %= 10
  out += `${Math.floor(x / 2)}${Math.floor(y)}`
  if (chars === 4) return out
  x = (x % 2) * 12
  y = (y % 1) * 24
  out += String.fromCharCode(97 + Math.floor(x)) + String.fromCharCode(97 + Math.floor(y))
  if (chars === 6) return out
  return `${out}${Math.floor((x % 1) * 10)}${Math.floor((y % 1) * 10)}`
}

/** The edges of a level's cells inside [west, east] × [south, north], in degrees. */
export function gridEdges(l: GridLevel, west: number, south: number, east: number, north: number) {
  const lons: number[] = []
  const lats: number[] = []
  for (let k = Math.ceil((west + 180) / l.lon); -180 + k * l.lon <= east; k++) lons.push(-180 + k * l.lon)
  for (let k = Math.ceil((south + 90) / l.lat); -90 + k * l.lat <= north; k++) lats.push(-90 + k * l.lat)
  return { lons, lats }
}

/** Each level's cells inside [west, east] × [south, north]: name and centre. */
export function gridCells(l: GridLevel, west: number, south: number, east: number, north: number) {
  const out: Array<{ name: string; lat: number; lon: number }> = []
  for (let i = Math.floor((west + 180) / l.lon); -180 + i * l.lon < east; i++) {
    for (let j = Math.floor((south + 90) / l.lat); -90 + j * l.lat < north; j++) {
      const lon = -180 + (i + 0.5) * l.lon
      const lat = -90 + (j + 0.5) * l.lat
      out.push({ name: locatorAt(lat, lon, l.chars), lat, lon })
    }
  }
  return out
}

/** The fields drawn at a planet's scale (MUF, aurora, flare, polar-cap absorption, band heat). At
 *  street scale they show no detail, so their rows in the Layers panel are dimmed there. Their state
 *  is never touched. */
export const WORLD_SCALE_LAYERS: readonly string[] = ['muf', 'aurora', 'flare', 'pca', 'heat']
