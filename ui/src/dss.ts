// 3DSS — 3D stacked-spectrum ("alternate waterfall") renderer.
//
// Ported from AetherSDR's DssRenderer (src/gui/DssRenderer.cpp, GPLv3) — a perspective
// stacked-trace surface: a rolling history of spectrum rows drawn back-to-front (painter's
// algorithm) as a receding trapezoid. The newest trace spans the full width across the
// front; older traces recede into a narrower, higher trapezoid. Each ridge fills to the
// floor so nearer traces occlude farther ones; fill colour follows amplitude via the same
// baked LUT the 2D waterfall uses, dimmed with depth (atmospheric haze), lit by local slope,
// with a bright per-amplitude rim on each crest.
//
// Differences from the C++ original (deliberate): our history rows are already NORMALIZED to
// the visual-AGC window (0..255) at push time, so there is no dBm floor/range — the stored
// value IS the strength (a zCurve pow still lifts the floor, matching their shader). Colour
// comes from the shared 256-entry LUT, not a Qt palette. Rendered on canvas-2D, fed from the
// same WaterfallHistory ring as the 2D view — only on a new row (8–20 Hz), so the per-column
// trapezoid fill stays cheap.

import type { WaterfallHistory } from './waterfallHistory'

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
/** Columns the surface is resampled to — fewer than the 2D view (the receding rows are
 * narrow anyway) so the per-column trapezoid fill stays cheap on canvas-2D. */
export const DSS_COLS = 256

function clamp(x: number, lo: number, hi: number): number {
  return x < lo ? lo : x > hi ? hi : x
}

/**
 * Render the 3DSS surface into `ctx` over a `w`×`h` device-pixel region. `hist` supplies the
 * rows (newest = front) with their own frequency frames; `lut` is the baked 256×RGBA colormap;
 * `bg` is the background the haze fades toward (the palette floor, so an empty stack reads as a
 * quiet band). `view` maps output columns to frequency so the stack tracks zoom/retune like the
 * 2D view. `scaleStripPx` reserves the bottom strip (transparent) for a host-drawn axis.
 */
export function drawDss(
  ctx: CanvasRenderingContext2D,
  w: number,
  h: number,
  hist: WaterfallHistory,
  lut: Uint8ClampedArray,
  bg: readonly [number, number, number],
  view: { loHz: number; hiHz: number },
  scaleStripPx = 0,
): void {
  if (w <= 0 || h <= 0) return
  const H = Math.max(1, h - Math.max(0, scaleStripPx))
  const [bgR, bgG, bgB] = bg
  ctx.save()
  ctx.fillStyle = `rgb(${bgR},${bgG},${bgB})`
  ctx.fillRect(0, 0, w, H)

  const count = Math.min(DSS_ROWS, hist.length)
  if (count <= 0) {
    ctx.restore()
    return
  }
  const span = view.hiHz - view.loHz
  if (!(span > 0)) {
    ctx.restore()
    return
  }

  const bottomY = H
  const depthSpan = H * K_DEPTH_SPAN
  const frontMaxRidge = H * K_FRONT_MAX_RIDGE
  const denom = DSS_ROWS

  // Reused per-row buffers (no per-frame garbage): x/y points + fill colours.
  const xs = new Float64Array(DSS_COLS)
  const ys = new Float64Array(DSS_COLS)
  const inten = new Float32Array(DSS_COLS) // this row's per-column intensity (0..255)

  // Resample one history row to DSS_COLS over the view window, through the row's OWN frame
  // (so the stack is frequency-honest across retunes), peak-preserving.
  const sampleRow = (age: number): boolean => {
    const fr = hist.frameAt(age)
    if (!fr || !(fr.hiHz > fr.loHz)) return false
    const rowSpan = fr.hiHz - fr.loHz
    const cols = hist.columns
    for (let c = 0; c < DSS_COLS; c++) {
      const hz = view.loHz + (span * (c + 0.5)) / DSS_COLS
      if (hz < fr.loHz || hz > fr.hiHz) {
        inten[c] = 0
        continue
      }
      // Max-pool the source bins covering this output cell (a narrow carrier must not vanish).
      const s0 = clamp(Math.floor(((hz - rowSpan / (2 * DSS_COLS) - fr.loHz) / rowSpan) * cols), 0, cols - 1)
      const s1 = clamp(Math.ceil(((hz + rowSpan / (2 * DSS_COLS) - fr.loHz) / rowSpan) * cols), 0, cols - 1)
      let m = 0
      for (let s = s0; s <= s1; s++) {
        const v = hist.intensityAt(age, s)
        if (v > m) m = v
      }
      inten[c] = m
    }
    return true
  }

  // Back (oldest) → front (newest): painter's algorithm. Nearer traces are wider, sit lower,
  // and fill to the floor, so they occlude farther ones.
  for (let age = count - 1; age >= 0; age--) {
    if (!sampleRow(age)) continue
    const depthFrac = age / denom
    const rowWidthFrac = 1 - depthFrac * (1 - K_BACK_WIDTH)
    const inset = w * (1 - rowWidthFrac) * 0.5
    const rowW = w - 2 * inset
    const baselineY = bottomY - depthFrac * depthSpan
    const maxRidge = frontMaxRidge * rowWidthFrac
    const dim = K_MIN_DIM + (1 - K_MIN_DIM) * (1 - depthFrac)
    const haze = depthFrac * K_HAZE

    // Pass 1: geometry — floor-anchored ridge heights with the pow(strength, zCurve) lift.
    for (let c = 0; c < DSS_COLS; c++) {
      const x = inset + (DSS_COLS > 1 ? c / (DSS_COLS - 1) : 0) * rowW
      const s = Math.pow(inten[c] / 255, Z_CURVE)
      xs[c] = x
      ys[c] = baselineY - s * maxRidge
    }

    // Pass 2: per-column trapezoid fill to the floor (palette by amplitude, hazed by depth,
    // lit by local slope). AA off so adjacent columns tile without seams.
    const slopeScale = maxRidge > 1 ? maxRidge : 1
    ctx.imageSmoothingEnabled = false
    for (let c = 0; c < DSS_COLS - 1; c++) {
      const cl = c > 0 ? c - 1 : 0
      const cr = c < DSS_COLS - 1 ? c + 1 : DSS_COLS - 1
      const slope = (ys[cl] - ys[cr]) / slopeScale
      const shade = clamp(1 + K_SLOPE_GAIN * slope, K_SHADE_LO, K_SHADE_HI)
      const li = (inten[c] | 0) * 4
      // base colour → haze toward bg → scale by dim×shade
      const f = dim * shade
      const r = clamp((lut[li] + (bgR - lut[li]) * haze) * f, 0, 255) | 0
      const g = clamp((lut[li + 1] + (bgG - lut[li + 1]) * haze) * f, 0, 255) | 0
      const b = clamp((lut[li + 2] + (bgB - lut[li + 2]) * haze) * f, 0, 255) | 0
      ctx.fillStyle = `rgb(${r},${g},${b})`
      ctx.beginPath()
      ctx.moveTo(xs[c], ys[c])
      ctx.lineTo(xs[c + 1], ys[c + 1])
      ctx.lineTo(xs[c + 1], bottomY)
      ctx.lineTo(xs[c], bottomY)
      ctx.closePath()
      ctx.fill()
    }

    // Ridge line — bright per-amplitude rim on the crest (front row a touch bolder).
    ctx.lineWidth = age === 0 ? 1.6 : 1
    ctx.lineJoin = 'round'
    ctx.lineCap = 'round'
    for (let c = 0; c < DSS_COLS - 1; c++) {
      const li = (inten[c] | 0) * 4
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
