#!/usr/bin/env node
// Generate the map's base geography: ui/src/data/basemap-110m.ts, basemap-50m.bin, basemap-10m.bin.
//
// SOURCE. Natural Earth vectors (public domain), at the three scales Natural Earth publishes:
// 1:110m, 1:50m and 1:10m. Each file is pinned to the v5.1.2 release tag of Natural Earth's own
// repository, so the URL can never serve different bytes, and the SHA-256 of every download is
// checked against the table below before anything is read. Per-dataset versions (the
// `.VERSION.txt` beside each shapefile at that tag) are in NOTICE.
//
// WHAT EACH FILE HOLDS — the layers the map draws, finest file used only when the map is zoomed:
//   land     polygons  the land, filled            (ne_*_land; the 1 m "Null Island" debug square dropped)
//   lakes    polygons  lakes and reservoirs, filled in the sea's colour (ne_*_lakes, by scalerank)
//   rivers   lines     rivers, each with its Natural Earth scalerank so the map can show only the
//                      major ones at a given zoom (ne_*_rivers_lake_centerlines; the "Lake
//                      Centerline" pieces are dropped because the lakes themselves are drawn)
//   borders  lines     land borders between countries (ne_*_admin_0_boundary_lines_land, without
//                      the lease and overlay limits, which are not borders)
//   states   lines     US state lines (ne_*_admin_1_states_provinces_lines, USA only: the map's
//                      states layer is the US states, the only first-level lines it shows)
//
// THE COASTLINE IS NOT STORED. It is the outline of the land and lake polygons, less the edges the
// data had to invent: Natural Earth cuts land at the antimeridian and closes Antarctica along the
// pole, and this script cuts it again into tiles (below). Every invented edge lies ON a tile
// boundary, and the reader (ui/src/basemap.ts) drops exactly the edges whose two ends sit on the
// same boundary line. So the coastline and the land fill are the same points and can never drift
// apart at any zoom, and the 1:10m file carries the coast once instead of twice.
//
// TILES. The 1:10m and 1:50m files are cut into 10° and 30° tiles (polygons by Sutherland–Hodgman,
// lines by segment), so a zoomed map projects only the tiles it can see. A tile's polygon pieces
// share their cut edges exactly, and the land of all visible tiles is filled as ONE path, so the
// cuts never show. 1:110m is one tile.
//
// THE FORMAT (the reader is ui/src/basemap.ts — keep the two in step). Integers on a grid of
// `units` per degree, x = (lon + 180) * units, y = (lat + 90) * units, so every tile boundary is an
// exact integer. Then, as unsigned LEB128 varints:
//   "NXBM" (4 bytes), version byte (1), units, tileDeg, layerCount,
//   per layer: nameLength, name (ASCII), kind (0 polygons, 1 lines, 2 lines with a rank), tileCount,
//     per tile: tileIndex (row * (360 / tileDeg) + col, from lon -180 and lat -90), partCount,
//       kind 0: per polygon ringCount, per ring pointCount (the first ring is the exterior)
//       kind 1: per line pointCount        kind 2: per line rank, pointCount
//       then every point of the tile as zigzag deltas (dx, dy), the first from the tile's
//       south-west corner and each later one from the point before it.
// Rings are stored without their closing point. Exterior rings run clockwise and holes
// anticlockwise (lon right, lat up), which is d3-geo's convention for spherical polygons.
//
// Run:   node scripts/gen-basemap.mjs [--cache DIR]
// It downloads what it needs into the cache (default: $TMPDIR/natural-earth-v5.1.2) and writes the
// three files (1:110m as a module holding the same bytes in base64, so the app has it without a
// fetch). Deterministic: the same inputs give the same bytes.

import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const TAG = 'v5.1.2'
const BASE = `https://raw.githubusercontent.com/nvkelso/natural-earth-vector/${TAG}/geojson`
/** SHA-256 of each source file at the tag, as downloaded on 2026-10-04. */
const SHA256 = {
  'ne_10m_admin_0_boundary_lines_land.geojson': '74d9c16229c095fde65943a9919e337682f044bcebccb120764f38edf3b70f4a',
  'ne_10m_admin_1_states_provinces_lines.geojson': '1a1f30ccaaf4cc9c4bde34266f0b8cbb955d3a4cf254b756912255f2ec7c75b6',
  'ne_10m_lakes.geojson': '2d036f53dedec578001c5c30c2959ee7d4eebc1306900fa4367c49929ec8f2d9',
  'ne_10m_land.geojson': '1ac90796408bc6ad6911d69448485d3c4dbf2190370080368a09976e1c9f7416',
  'ne_10m_rivers_lake_centerlines.geojson': 'bb854a900ecbd3b408df46d5e16e3e0f974ba55993f9d8b5c26e855273c0905a',
  'ne_50m_admin_0_boundary_lines_land.geojson': '2faac4f6b34386f3d21b6e018cf151f241f00e5c936d44dd17d7d9bfb147fa48',
  'ne_50m_admin_1_states_provinces_lines.geojson': '72cca93c850d412628a5da4bc5ebfe21ba4d376eb34611bde6b623ee73f0fdcf',
  'ne_50m_lakes.geojson': 'd350b75978b26fe839b797c2c529b2fb8f47fb3983c03f4964e36d5df9378a52',
  'ne_50m_land.geojson': 'e874b27a51d146452be360cafb3cc50c86001074a67d534113e6534682f9826b',
  'ne_50m_rivers_lake_centerlines.geojson': 'f286e0ce978fde999ca2d7a78c764be08542e19b63cded52b05c12d5173ccc51',
  'ne_110m_admin_0_boundary_lines_land.geojson': 'd42479fd79552cca4eec7f85fcdca717a790d29ff06be7676f1af0568c6d3f7c',
  'ne_110m_admin_1_states_provinces_lines.geojson': 'f204e94d5c4d16c6ce4b59ecb50e264bd95c22b9138d8a994c010c222e186aad',
  'ne_110m_lakes.geojson': 'eb02ecc86c82004fccbf979058bfabbbd6c2d07968c7844d38eb1c9152d2ffc9',
  'ne_110m_land.geojson': '9e0729ee253ca7d7a5c4ae9395fb1902264c5377c52e224d13dd85010e2835d9',
  'ne_110m_rivers_lake_centerlines.geojson': '55aa4497405afc07cdc931b7fbe062c4d6693ba2a550c0d24899953f5d507c8d',
}

/** Per scale: the grid, the tile size, and which lakes and rivers are kept (scalerank, lower is
 *  more important). 1:10m keeps "major" rivers only — rank 7 and above are thousands of streams. */
const LEVELS = [
  { scale: '110m', units: 100, tileDeg: 360, lakeRank: 9, riverRank: 9 },
  { scale: '50m', units: 100, tileDeg: 30, lakeRank: 9, riverRank: 9 },
  { scale: '10m', units: 1000, tileDeg: 10, lakeRank: 7, riverRank: 6 },
]

const argv = process.argv.slice(2)
const cacheArg = argv.indexOf('--cache')
const CACHE = cacheArg >= 0 ? argv[cacheArg + 1] : join(tmpdir(), `natural-earth-${TAG}`)
const OUT = new URL('../ui/src/data/', import.meta.url)

async function source(name) {
  mkdirSync(CACHE, { recursive: true })
  const file = join(CACHE, name)
  if (!existsSync(file)) {
    const res = await fetch(`${BASE}/${name}`)
    if (!res.ok) throw new Error(`${name}: HTTP ${res.status}`)
    writeFileSync(file, Buffer.from(await res.arrayBuffer()))
  }
  const bytes = readFileSync(file)
  const sum = createHash('sha256').update(bytes).digest('hex')
  if (SHA256[name] !== sum) throw new Error(`${name}: SHA-256 ${sum}, expected ${SHA256[name] ?? '(not pinned)'}`)
  return JSON.parse(bytes.toString('utf8')).features
}

// ── Geometry on the integer grid ────────────────────────────────────────────────────────────

const quantize = (units) => ([lon, lat]) => [Math.round((lon + 180) * units), Math.round((lat + 90) * units)]

/** Consecutive duplicates removed; for a ring, the closing duplicate too. */
function clean(pts, ring) {
  const out = []
  for (const p of pts) {
    const q = out[out.length - 1]
    if (!q || q[0] !== p[0] || q[1] !== p[1]) out.push(p)
  }
  if (ring) while (out.length > 1 && out[0][0] === out[out.length - 1][0] && out[0][1] === out[out.length - 1][1]) out.pop()
  return out
}

/** Twice the signed area, lat up: positive = anticlockwise. */
function area2(ring) {
  let a = 0
  for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) a += ring[j][0] * ring[i][1] - ring[i][0] * ring[j][1]
  return a
}

/** Sutherland–Hodgman against one axis-aligned half-plane. `axis` 0 = x, 1 = y; keep v >= at
 *  (sign 1) or v <= at (sign -1). Intersections land exactly on the line (that coordinate is `at`). */
function clipHalf(ring, axis, at, sign) {
  const out = []
  const inside = (p) => sign * (p[axis] - at) >= 0
  for (let i = 0; i < ring.length; i++) {
    const a = ring[i]
    const b = ring[(i + 1) % ring.length]
    const ia = inside(a)
    const ib = inside(b)
    if (ia) out.push(a)
    if (ia !== ib) {
      const t = (at - a[axis]) / (b[axis] - a[axis])
      const o = 1 - axis
      const p = []
      p[axis] = at
      p[o] = Math.round(a[o] + t * (b[o] - a[o]))
      out.push(p)
    }
  }
  return out
}

function clipRing(ring, x0, y0, x1, y1) {
  let r = ring
  r = clipHalf(r, 0, x0, 1)
  if (r.length) r = clipHalf(r, 0, x1, -1)
  if (r.length) r = clipHalf(r, 1, y0, 1)
  if (r.length) r = clipHalf(r, 1, y1, -1)
  r = clean(r, true)
  return r.length >= 3 && area2(r) !== 0 ? r : null
}

/** A polyline clipped to a rectangle, as the pieces that lie inside it (Liang–Barsky per segment). */
function clipLine(line, x0, y0, x1, y1) {
  const pieces = []
  let cur = null
  for (let i = 0; i + 1 < line.length; i++) {
    const [ax, ay] = line[i]
    const [bx, by] = line[i + 1]
    const dx = bx - ax
    const dy = by - ay
    let t0 = 0
    let t1 = 1
    let ok = true
    for (const [p, q] of [[-dx, ax - x0], [dx, x1 - ax], [-dy, ay - y0], [dy, y1 - ay]]) {
      if (p === 0) {
        if (q < 0) ok = false
      } else {
        const r = q / p
        if (p < 0) {
          if (r > t1) ok = false
          else if (r > t0) t0 = r
        } else if (r < t0) ok = false
        else if (r < t1) t1 = r
      }
      if (!ok) break
    }
    if (!ok) {
      cur = null
      continue
    }
    const s = t0 === 0 ? [ax, ay] : [Math.round(ax + t0 * dx), Math.round(ay + t0 * dy)]
    const e = t1 === 1 ? [bx, by] : [Math.round(ax + t1 * dx), Math.round(ay + t1 * dy)]
    if (!cur || cur[cur.length - 1][0] !== s[0] || cur[cur.length - 1][1] !== s[1]) {
      cur = [s]
      pieces.push(cur)
    }
    cur.push(e)
    if (t1 !== 1) cur = null
  }
  return pieces.map((p) => clean(p, false)).filter((p) => p.length >= 2)
}

/** Is the edge a→b one the data invented: both ends on the same tile boundary line? */
function cutEdge(a, b, step) {
  return (a[0] === b[0] && a[0] % step === 0) || (a[1] === b[1] && a[1] % step === 0)
}

/** Start the ring just after an invented edge, if it has one, so a coastline run never wraps. */
function rotateAfterCut(ring, step) {
  const n = ring.length
  for (let i = 0; i < n; i++) {
    if (cutEdge(ring[i], ring[(i + 1) % n], step)) return [...ring.slice(i + 1), ...ring.slice(0, i + 1)]
  }
  return ring
}

// ── Encoding ─────────────────────────────────────────────────────────────────────────────────

class Writer {
  constructor() {
    this.buf = new Uint8Array(1 << 20)
    this.n = 0
  }
  byte(b) {
    if (this.n === this.buf.length) {
      const g = new Uint8Array(this.buf.length * 2)
      g.set(this.buf)
      this.buf = g
    }
    this.buf[this.n++] = b
  }
  uint(v) {
    if (!(Number.isInteger(v) && v >= 0)) throw new Error(`not a uint: ${v}`)
    while (v >= 0x80) {
      this.byte((v & 0x7f) | 0x80)
      v = Math.floor(v / 128)
    }
    this.byte(v)
  }
  sint(v) {
    this.uint(v < 0 ? -2 * v - 1 : 2 * v)
  }
  ascii(s) {
    this.uint(s.length)
    for (const c of s) this.byte(c.charCodeAt(0))
  }
  bytes() {
    return this.buf.slice(0, this.n)
  }
}

// ── One scale ────────────────────────────────────────────────────────────────────────────────

async function level({ scale, units, tileDeg, lakeRank, riverRank }) {
  const q = quantize(units)
  const step = tileDeg * units
  const cols = 360 / tileDeg
  const rows = 180 / tileDeg
  const polygonsOf = (features, keep) => {
    const out = []
    for (const f of features) {
      if (!keep(f.properties ?? {})) continue
      const g = f.geometry
      if (!g) continue
      const polys = g.type === 'Polygon' ? [g.coordinates] : g.type === 'MultiPolygon' ? g.coordinates : []
      for (const p of polys) {
        const rings = []
        for (let k = 0; k < p.length; k++) {
          let r = clean(p[k].map(q), true)
          if (r.length < 3 || area2(r) === 0) {
            if (k === 0) break
            continue
          }
          // Exterior clockwise, holes anticlockwise (lat up).
          if ((k === 0) === area2(r) > 0) r = r.reverse()
          rings.push(r)
        }
        if (rings.length) out.push(rings)
      }
    }
    return out
  }
  const linesOf = (features, keep, rank) => {
    const out = []
    for (const f of features) {
      const pr = f.properties ?? {}
      if (!keep(pr)) continue
      const g = f.geometry
      if (!g) continue
      const lines = g.type === 'LineString' ? [g.coordinates] : g.type === 'MultiLineString' ? g.coordinates : []
      for (const l of lines) {
        const c = clean(l.map(q), false)
        if (c.length >= 2) out.push({ pts: c, rank: rank ? rank(pr) : 0 })
      }
    }
    return out
  }
  const prop = (pr, k) => pr[k] ?? pr[k.toUpperCase()] ?? pr[k.toLowerCase()]

  const land = polygonsOf(await source(`ne_${scale}_land.geojson`), (p) => prop(p, 'featurecla') !== 'Null island')
  const lakes = polygonsOf(await source(`ne_${scale}_lakes.geojson`), (p) => (prop(p, 'scalerank') ?? 0) <= lakeRank)
  const rivers = linesOf(
    await source(`ne_${scale}_rivers_lake_centerlines.geojson`),
    (p) => !String(prop(p, 'featurecla') ?? '').startsWith('Lake Centerline') && (prop(p, 'scalerank') ?? 0) <= riverRank,
    (p) => Math.max(0, Math.min(255, prop(p, 'scalerank') ?? 0)),
  )
  const borders = linesOf(
    await source(`ne_${scale}_admin_0_boundary_lines_land.geojson`),
    (p) => !['Lease limit', 'Overlay limit'].includes(prop(p, 'featurecla')),
  )
  const states = linesOf(await source(`ne_${scale}_admin_1_states_provinces_lines.geojson`), (p) => prop(p, 'adm0_a3') === 'USA')

  // Into tiles.
  const tilesOf = (bbox) => {
    const [minx, miny, maxx, maxy] = bbox
    const c0 = Math.max(0, Math.min(cols - 1, Math.floor(minx / step)))
    const c1 = Math.max(0, Math.min(cols - 1, Math.floor(maxx / step - 1e-9)))
    const r0 = Math.max(0, Math.min(rows - 1, Math.floor(miny / step)))
    const r1 = Math.max(0, Math.min(rows - 1, Math.floor(maxy / step - 1e-9)))
    const out = []
    for (let r = r0; r <= r1; r++) for (let c = c0; c <= c1; c++) out.push([r * cols + c, c * step, r * step, (c + 1) * step, (r + 1) * step])
    return out
  }
  const bboxOf = (pts) => {
    let a = Infinity, b = Infinity, c = -Infinity, d = -Infinity
    for (const [x, y] of pts) {
      if (x < a) a = x
      if (y < b) b = y
      if (x > c) c = x
      if (y > d) d = y
    }
    return [a, b, c, d]
  }
  const tilePolygons = (polys) => {
    const byTile = new Map()
    for (const rings of polys) {
      for (const [t, x0, y0, x1, y1] of tilesOf(bboxOf(rings[0]))) {
        const ext = tileDeg === 360 ? rings[0] : clipRing(rings[0], x0, y0, x1, y1)
        if (!ext) continue
        const piece = [rotateAfterCut(ext, step)]
        for (const h of rings.slice(1)) {
          const c = tileDeg === 360 ? h : clipRing(h, x0, y0, x1, y1)
          if (c) piece.push(rotateAfterCut(c, step))
        }
        if (!byTile.has(t)) byTile.set(t, [])
        byTile.get(t).push(piece)
      }
    }
    return byTile
  }
  const tileLines = (lines) => {
    const byTile = new Map()
    for (const { pts, rank } of lines) {
      for (const [t, x0, y0, x1, y1] of tilesOf(bboxOf(pts))) {
        const pieces = tileDeg === 360 ? [pts] : clipLine(pts, x0, y0, x1, y1)
        for (const p of pieces) {
          if (!byTile.has(t)) byTile.set(t, [])
          byTile.get(t).push({ pts: p, rank })
        }
      }
    }
    return byTile
  }

  const layers = [
    ['land', 0, tilePolygons(land)],
    ['lakes', 0, tilePolygons(lakes)],
    ['rivers', 2, tileLines(rivers)],
    ['borders', 1, tileLines(borders)],
    ['states', 1, tileLines(states)],
  ]
  const w = new Writer()
  for (const c of 'NXBM') w.byte(c.charCodeAt(0))
  w.byte(1)
  w.uint(units)
  w.uint(tileDeg)
  w.uint(layers.length)
  const counts = {}
  for (const [name, kind, byTile] of layers) {
    w.ascii(name)
    w.uint(kind)
    const tiles = [...byTile.keys()].sort((a, b) => a - b)
    w.uint(tiles.length)
    let points = 0
    for (const t of tiles) {
      const parts = byTile.get(t)
      w.uint(t)
      w.uint(parts.length)
      const flat = []
      for (const part of parts) {
        if (kind === 0) {
          w.uint(part.length)
          for (const ring of part) {
            w.uint(ring.length)
            flat.push(...ring)
          }
        } else {
          if (kind === 2) w.uint(part.rank)
          w.uint(part.pts.length)
          flat.push(...part.pts)
        }
      }
      let px = (t % cols) * step
      let py = Math.floor(t / cols) * step
      for (const [x, y] of flat) {
        w.sint(x - px)
        w.sint(y - py)
        px = x
        py = y
      }
      points += flat.length
    }
    counts[name] = points
  }
  const bytes = w.bytes()
  if (scale === '110m') {
    // The coarsest scale is bundled into the app as a module, so the first frame of every map
    // has land without waiting on a fetch; the finer ones are assets fetched when wanted.
    const b64 = Buffer.from(bytes).toString('base64')
    writeFileSync(
      new URL(`basemap-${scale}.ts`, OUT),
      `// Generated by scripts/gen-basemap.mjs from Natural Earth 1:110m (public domain). Do not edit.\n` +
        `// The basemap file (format: see that script), base64, ${bytes.length} bytes.\n` +
        `export default '${b64}'\n`,
    )
  } else writeFileSync(new URL(`basemap-${scale}.bin`, OUT), bytes)
  console.log(`basemap-${scale}  ${bytes.length} bytes  points ${JSON.stringify(counts)}`)
}

for (const l of LEVELS) await level(l)
