// 3DSS — 3D stacked-spectrum ("alternate waterfall") surface, for both renderers.
//
// Ported from AetherSDR's DssRenderer (src/gui/DssRenderer.cpp, GPLv3) — a perspective
// stacked-trace surface: a rolling history of spectrum rows drawn back-to-front (painter's
// algorithm) as a receding trapezoid. The newest trace spans the full width across the
// front; older traces recede into a narrower, higher trapezoid. Each ridge fills to the
// floor so nearer traces occlude farther ones; fill colour follows amplitude via the same
// baked LUT the 2D waterfall uses, dimmed with depth (atmospheric haze), lit by local slope,
// with a bright per-amplitude rim on each crest. See NOTICE.
//
// The canvas-2D drawing below is `../dss.ts` moved onto the shared ring (spectrum/ring.ts), and
// keeps that file's deliberate differences from the C++ original: colour from the shared
// 256-entry LUT, not a Qt palette; the zCurve pow lifts the floor as their shader does.
//
// Changes from `../dss.ts` and from the C++ original:
//  * Rows are read from the ring through each row's OWN span and display range. Ridge height is
//    anchored to the noise floor as AetherSDR anchors it, strength = clamp((v − floor) / range):
//    the floor is the host's noise estimate the row was committed with, so the noise sits on the
//    baseline and signals rise from it.
//  * Temporal median-of-3 impulse rejection, after DssRenderer.cpp's `smoothDssRow`/`median3`: a
//    column's strength is the median of that frequency in this row and the two before it, so a
//    one-row broadband burst does not throw up a wall. Taken at the same FREQUENCY through each
//    row's own span, not the same column, so it holds across a retune. Their spatial 1-2-1
//    smoothing and temporal IIR are not ported.
//  * The scrollback offset is honoured: the stack ends at the row the 2D view is showing.
//  * WebGL2 draws the same surface on the GPU (GLSL below): a fragment pass derives the anchored,
//    median-of-3 strength of every (column, row) from the texture ring each frame, then ONE
//    instanced draw displaces the mesh from it, back to front, each row's fill then its rim. The
//    geometry constants and the vertex-displacement idea are AetherSDR's
//    (resources/shaders/dss_mesh.vert). The shading follows the canvas path below rather than
//    dss_mesh.frag's vertical gradient, so both renderers draw the same surface; the GPU draws one
//    column per device pixel where canvas-2D keeps 256.

import { aggregateRow, GLSL_AGGREGATE, strengthIn } from './aggregate'
import { M_CEIL, M_DB_PER_UNIT, M_FLOOR, M_HI, M_LO, M_N, META, type SpectrumRing } from './ring'
import type { Detector } from './types'

// Perspective geometry (shared with AetherSDR's dss_mesh.vert constants).
const K_BACK_WIDTH = 0.6 // back row width / front
const K_DEPTH_SPAN = 0.58 // baseline rise to the back
const K_FRONT_MAX_RIDGE = 0.46 // front ridge height / plot H
const K_HAZE = 0.16 // fade toward bg with depth
const K_SLOPE_GAIN = 0.55 // slope shading strength
const K_SHADE_LO = 0.68
const K_SHADE_HI = 1.32
const K_MIN_DIM = 0.5 // depth dimming never falls below this
const Z_CURVE = 0.55 // floor-lift exponent (their default)

/** History depth shown in the 3D stack (front → back). */
export const DSS_ROWS = 96
/** Columns the canvas-2D surface is resampled to — fewer than the 2D view (the receding rows are
 * narrow anyway) so the per-column trapezoid fill stays cheap on canvas-2D. */
export const DSS_COLS = 256
/** The GPU surface's column cap (one per device pixel up to this). */
export const DSS_MAX_GL_COLS = 4096

function clamp(x: number, lo: number, hi: number): number {
  return x < lo ? lo : x > hi ? hi : x
}

/** The median of three, as DssRenderer.cpp writes it. */
export function median3(a: number, b: number, c: number): number {
  return Math.max(Math.min(a, b), Math.min(Math.max(a, b), c))
}

/**
 * The strength field the stack is drawn from: `rows` × `cols`, row 0 = the front (the row
 * `offsetRows` back), each value 0..1. `present[r]` is 0 where that row does not exist (not yet
 * committed, or a row with no span), and such a row is not drawn. Columns outside a row's span
 * have strength 0.
 */
export function dssStrengths(
  ring: SpectrumRing,
  cols: number,
  view: { loHz: number; hiHz: number },
  offsetRows: number,
  detector: Detector,
  out: Float32Array,
  present: Uint8Array,
): number {
  const rows = Math.max(0, Math.min(DSS_ROWS, ring.count - offsetRows))
  // Two rows past the back one feed its median.
  const span = rows + 2
  const raw = new Float32Array(span * cols)
  const have = new Uint8Array(span)
  const px = new Float32Array(cols)
  for (let r = 0; r < span; r++) {
    const idx = ring.indexAt(offsetRows + r)
    if (idx < 0) continue
    const m = idx * META
    const n = ring.meta[m + M_N]
    const lo = ring.meta[m + M_LO]
    const hi = ring.meta[m + M_HI]
    if (!(n > 0) || !(hi > lo)) continue
    have[r] = 1
    const dbPerUnit = ring.meta[m + M_DB_PER_UNIT]
    aggregateRow(ring.values, idx * ring.cols, n, lo, hi, view.loHz, view.hiHz, px, detector, dbPerUnit)
    const floor = ring.meta[m + M_FLOOR]
    const ceil = ring.meta[m + M_CEIL]
    for (let c = 0; c < cols; c++) {
      const v = px[c]
      raw[r * cols + c] = Number.isNaN(v) ? 0 : strengthIn(v, floor, ceil)
    }
  }
  for (let r = 0; r < rows; r++) {
    present[r] = have[r]
    if (!have[r]) continue
    const o = r * cols
    if (have[r + 1] && have[r + 2]) {
      for (let c = 0; c < cols; c++) out[o + c] = median3(raw[o + c], raw[o + cols + c], raw[o + 2 * cols + c])
    } else {
      out.set(raw.subarray(o, o + cols), o)
    }
  }
  return rows
}

/**
 * Render the 3DSS surface into `ctx` over a `w`×`h` device-pixel region from a strength field
 * (`dssStrengths`, `cols` wide, `rows` deep). `lut` is the baked 256×RGBA colormap; the background
 * the haze fades toward is its floor, so an empty stack reads as a quiet band.
 */
export function drawDss2d(
  ctx: CanvasRenderingContext2D,
  w: number,
  h: number,
  strengths: Float32Array,
  present: Uint8Array,
  rows: number,
  cols: number,
  lut: Uint8ClampedArray,
): void {
  if (w <= 0 || h <= 0) return
  const H = h
  const bgR = lut[0]
  const bgG = lut[1]
  const bgB = lut[2]
  ctx.save()
  ctx.fillStyle = `rgb(${bgR},${bgG},${bgB})`
  ctx.fillRect(0, 0, w, H)

  const bottomY = H
  const depthSpan = H * K_DEPTH_SPAN
  const frontMaxRidge = H * K_FRONT_MAX_RIDGE
  const denom = DSS_ROWS

  // Reused per-row buffers (no per-frame garbage): x/y points + fill colours.
  const xs = new Float64Array(cols)
  const ys = new Float64Array(cols)

  // Back (oldest) → front (newest): painter's algorithm. Nearer traces are wider, sit lower,
  // and fill to the floor, so they occlude farther ones.
  for (let row = rows - 1; row >= 0; row--) {
    if (!present[row]) continue
    const inten = strengths.subarray(row * cols, (row + 1) * cols)
    const depthFrac = row / denom
    const rowWidthFrac = 1 - depthFrac * (1 - K_BACK_WIDTH)
    const inset = w * (1 - rowWidthFrac) * 0.5
    const rowW = w - 2 * inset
    const baselineY = bottomY - depthFrac * depthSpan
    const maxRidge = frontMaxRidge * rowWidthFrac
    const dim = K_MIN_DIM + (1 - K_MIN_DIM) * (1 - depthFrac)
    const haze = depthFrac * K_HAZE

    // Pass 1: geometry — floor-anchored ridge heights with the pow(strength, zCurve) lift.
    for (let c = 0; c < cols; c++) {
      const x = inset + (cols > 1 ? c / (cols - 1) : 0) * rowW
      xs[c] = x
      ys[c] = baselineY - Math.pow(inten[c], Z_CURVE) * maxRidge
    }

    // Pass 2: per-column trapezoid fill toward the floor (palette by amplitude, hazed by depth,
    // lit by local slope). AA off so adjacent columns tile without seams.
    //
    // The fill stops at the next nearer row's baseline when that row is drawn: everything below it
    // is painted over by that row, which is wider and fills from its own ridge down, so the picture
    // is the same and 96 rows no longer each paint to the plot floor (about 38 screens of overdraw).
    const fillTo = row > 0 && present[row - 1] ? bottomY - ((row - 1) / denom) * depthSpan : bottomY
    const slopeScale = maxRidge > 1 ? maxRidge : 1
    ctx.imageSmoothingEnabled = false
    for (let c = 0; c < cols - 1; c++) {
      const cl = c > 0 ? c - 1 : 0
      const cr = c < cols - 1 ? c + 1 : cols - 1
      const slope = (ys[cl] - ys[cr]) / slopeScale
      const shade = clamp(1 + K_SLOPE_GAIN * slope, K_SHADE_LO, K_SHADE_HI)
      const li = ((inten[c] * 255) | 0) * 4
      // base colour → haze toward bg → scale by dim×shade
      const f = dim * shade
      const r = clamp((lut[li] + (bgR - lut[li]) * haze) * f, 0, 255) | 0
      const g = clamp((lut[li + 1] + (bgG - lut[li + 1]) * haze) * f, 0, 255) | 0
      const b = clamp((lut[li + 2] + (bgB - lut[li + 2]) * haze) * f, 0, 255) | 0
      ctx.fillStyle = `rgb(${r},${g},${b})`
      ctx.beginPath()
      ctx.moveTo(xs[c], ys[c])
      ctx.lineTo(xs[c + 1], ys[c + 1])
      ctx.lineTo(xs[c + 1], fillTo)
      ctx.lineTo(xs[c], fillTo)
      ctx.closePath()
      ctx.fill()
    }

    // Ridge line — bright per-amplitude rim on the crest (front row a touch bolder).
    ctx.lineWidth = row === 0 ? 1.6 : 1
    ctx.lineJoin = 'round'
    ctx.lineCap = 'round'
    for (let c = 0; c < cols - 1; c++) {
      const li = ((inten[c] * 255) | 0) * 4
      // lighten ~1.55× (approx Qt lighter(165)), haze, then dim.
      const r = clamp((Math.min(255, lut[li] * 1.55) + (bgR - Math.min(255, lut[li] * 1.55)) * haze) * dim, 0, 255) | 0
      const g = clamp((Math.min(255, lut[li + 1] * 1.55) + (bgG - Math.min(255, lut[li + 1] * 1.55)) * haze) * dim, 0, 255) | 0
      const b = clamp((Math.min(255, lut[li + 2] * 1.55) + (bgB - Math.min(255, lut[li + 2] * 1.55)) * haze) * dim, 0, 255) | 0
      ctx.strokeStyle = `rgb(${r},${g},${b})`
      ctx.beginPath()
      ctx.moveTo(xs[c], ys[c])
      ctx.lineTo(xs[c + 1], ys[c + 1])
      ctx.stroke()
    }
  }
  ctx.restore()
}

// ---------------------------------------------------------------------------------------------
// WebGL2. Both passes read the same ring textures the 2D waterfall does (webgl2.ts binds them).

const GLSL_HEAD = `#version 300 es
precision highp float;
precision highp int;
precision highp sampler2D;
`

/** Unpack a strength the derive pass stored as 16 bits in an RGBA8 texel's R and G. */
const GLSL_DSS_AT = `
float dssAt(sampler2D field, int c, int r) {
  vec4 t = texelFetch(field, ivec2(c, r), 0);
  return (floor(t.r * 255.0 + 0.5) * 256.0 + floor(t.g * 255.0 + 0.5)) / 65535.0;
}
`

/** Pass 1, one fragment per (column, depth row) of an RGBA8 target `cols` × `DSS_ROWS`: the
 *  anchored, median-of-3 strength, packed into R and G; B = 1 where the row exists. */
export const GLSL_DSS_DERIVE_FS = `${GLSL_HEAD}
uniform sampler2D uRing;
uniform sampler2D uMeta;
uniform int uHead;
uniform int uCount;
uniform int uDepth;
uniform int uOffset;
uniform int uCols;
uniform int uDetector;
uniform float uViewLo;
uniform float uViewHi;
out vec4 outColor;
${GLSL_AGGREGATE}
bool rowStrength(int age, float x, out float s) {
  s = 0.0;
  if (age >= uCount) return false;
  int r = uHead - age;
  if (r < 0) r += uDepth;
  vec4 m0 = texelFetch(uMeta, ivec2(0, r), 0);
  if (m0.z < 0.5 || !(m0.y > m0.x)) return false;
  vec4 m1 = texelFetch(uMeta, ivec2(1, r), 0);
  float v;
  if (aggregateRow(uRing, r, int(m0.z), m0.x, m0.y, uViewLo, (uViewHi - uViewLo) / float(uCols), x,
                   uDetector, m0.w, v)) s = strengthIn(v, m1.x, m1.y);
  return true;
}
void main() {
  int c = int(gl_FragCoord.x);
  int r = int(gl_FragCoord.y);
  float x = float(c);
  float s0;
  if (!rowStrength(uOffset + r, x, s0)) {
    outColor = vec4(0.0);
    return;
  }
  float s1;
  float s2;
  if (rowStrength(uOffset + r + 1, x, s1) && rowStrength(uOffset + r + 2, x, s2)) {
    s0 = max(min(s0, s1), min(max(s0, s1), s2));
  }
  float u = floor(s0 * 65535.0 + 0.5);
  outColor = vec4(floor(u / 256.0) / 255.0, mod(u, 256.0) / 255.0, 1.0, 1.0);
}
`

/** Pass 2, `drawArraysInstanced(TRIANGLES, 0, (cols − 1) × 12, rows)`: instance 0 is the back row.
 *  Within an instance the fill segments come first, then the rim, so a row's rim lies over its own
 *  fill and under the next row's, as the canvas path paints them. */
export const GLSL_DSS_MESH_VS = `${GLSL_HEAD}
uniform sampler2D uField;
uniform sampler2D uLut;
uniform int uCols;
uniform int uRows;
uniform float uW;
uniform float uH;
uniform float uPlotH;
out vec4 vColor;
out float vAcross;
out float vHalf;
const float DSS_ROWS = ${DSS_ROWS}.0;
const float K_BACK_WIDTH = ${K_BACK_WIDTH};
const float K_DEPTH_SPAN = ${K_DEPTH_SPAN};
const float K_FRONT_MAX_RIDGE = ${K_FRONT_MAX_RIDGE};
const float K_HAZE = ${K_HAZE};
const float K_SLOPE_GAIN = ${K_SLOPE_GAIN};
const float K_SHADE_LO = ${K_SHADE_LO};
const float K_SHADE_HI = ${K_SHADE_HI};
const float K_MIN_DIM = ${K_MIN_DIM};
const float Z_CURVE = ${Z_CURVE};
${GLSL_DSS_AT}
void main() {
  int r = uRows - 1 - gl_InstanceID;
  int segs = uCols - 1;
  int v = gl_VertexID;
  bool rim = v >= segs * 6;
  if (rim) v -= segs * 6;
  int s = v / 6;
  int corner = v - s * 6;
  vHalf = 1.0e6;
  vAcross = 0.0;
  if (texelFetch(uField, ivec2(0, r), 0).b < 0.5) {
    // An absent row: put the vertex outside the clip volume so the triangle is dropped.
    vColor = vec4(0.0);
    gl_Position = vec4(0.0, 0.0, 2.0, 1.0);
    return;
  }
  float depthFrac = float(r) / DSS_ROWS;
  float rowWidthFrac = 1.0 - depthFrac * (1.0 - K_BACK_WIDTH);
  float inset = uW * (1.0 - rowWidthFrac) * 0.5;
  float rowW = uW - 2.0 * inset;
  float baselineY = uPlotH - depthFrac * uPlotH * K_DEPTH_SPAN;
  float maxRidge = uPlotH * K_FRONT_MAX_RIDGE * rowWidthFrac;
  float dim = K_MIN_DIM + (1.0 - K_MIN_DIM) * (1.0 - depthFrac);
  float haze = depthFrac * K_HAZE;
  float st0 = dssAt(uField, s, r);
  float st1 = dssAt(uField, s + 1, r);
  vec2 p0 = vec2(inset + float(s) / float(segs) * rowW, baselineY - pow(st0, Z_CURVE) * maxRidge);
  vec2 p1 = vec2(inset + float(s + 1) / float(segs) * rowW, baselineY - pow(st1, Z_CURVE) * maxRidge);
  int li = int(st0 * 255.0);
  vec3 base = texelFetch(uLut, ivec2(li, 0), 0).rgb * 255.0;
  vec3 bg = texelFetch(uLut, ivec2(0, 0), 0).rgb * 255.0;
  vec2 pos;
  if (!rim) {
    // Down to the next nearer row's baseline when that row is drawn (the canvas path's reason).
    float fillTo = r > 0 && texelFetch(uField, ivec2(0, r - 1), 0).b > 0.5
      ? uPlotH - float(r - 1) / DSS_ROWS * uPlotH * K_DEPTH_SPAN
      : uPlotH;
    float stl = dssAt(uField, max(s - 1, 0), r);
    float yl = baselineY - pow(stl, Z_CURVE) * maxRidge;
    float slope = (yl - p1.y) / (maxRidge > 1.0 ? maxRidge : 1.0);
    float shade = clamp(1.0 + K_SLOPE_GAIN * slope, K_SHADE_LO, K_SHADE_HI);
    vec3 c = clamp((base + (bg - base) * haze) * (dim * shade), 0.0, 255.0);
    vColor = vec4(floor(c) / 255.0, 1.0);
    if (corner == 0 || corner == 3) pos = p0;
    else if (corner == 1) pos = p1;
    else if (corner == 2 || corner == 4) pos = vec2(p1.x, fillTo);
    else pos = vec2(p0.x, fillTo);
  } else {
    vec3 lit = min(base * 1.55, vec3(255.0));
    vec3 c = clamp((lit + (bg - lit) * haze) * dim, 0.0, 255.0);
    vColor = vec4(floor(c) / 255.0, 1.0);
    // A ribbon around the segment, half a pixel wider each side for the anti-aliased edge and
    // extended past both ends (the canvas path's round caps).
    float lw = r == 0 ? 1.6 : 1.0;
    vec2 d = p1 - p0;
    float len = length(d);
    d = len > 0.0 ? d / len : vec2(1.0, 0.0);
    vec2 nrm = vec2(-d.y, d.x);
    float hw = lw * 0.5 + 0.5;
    float side = (corner == 0 || corner == 1 || corner == 3) ? 1.0 : -1.0;
    bool atEnd = corner == 1 || corner == 2 || corner == 4;
    pos = (atEnd ? p1 + d * hw : p0 - d * hw) + nrm * hw * side;
    vAcross = side * hw;
    vHalf = lw * 0.5;
  }
  gl_Position = vec4(pos.x / uW * 2.0 - 1.0, 1.0 - pos.y / uH * 2.0, 0.0, 1.0);
}
`

export const GLSL_DSS_MESH_FS = `${GLSL_HEAD}
in vec4 vColor;
in float vAcross;
in float vHalf;
out vec4 outColor;
void main() {
  float a = clamp(vHalf + 0.5 - abs(vAcross), 0.0, 1.0);
  outColor = vec4(vColor.rgb, vColor.a * a);
}
`

/** The full-screen triangle every fragment pass draws (`drawArrays(TRIANGLES, 0, 3)`). */
export const GLSL_FULLSCREEN_VS = `${GLSL_HEAD}
void main() {
  vec2 p = vec2(gl_VertexID == 1 ? 3.0 : -1.0, gl_VertexID == 2 ? 3.0 : -1.0);
  gl_Position = vec4(p, 0.0, 1.0);
}
`

export { GLSL_HEAD }
