// Operator text overlays for the SSTV composer — the pure half.
//
// Geometry, hit-testing and drawing for the text items an operator lays over the
// transmit picture ("CQ SSTV DE KD9TAW", a report, a 73). The component owns state and
// gestures; everything here is a pure function so it runs under jsdom (which has no
// canvas at all — the one impure need, measuring banner text, is INJECTED so tests can
// stub it and the component can hand over `ctx.measureText`).
//
// TWO STYLES, one physics argument (idcard.rs is the authority):
//  - `crisp`: the ID plate's bitmap font, integer-scaled, fillRect only. Survives the
//    demod at any size the controls allow, exactly like the ident does.
//  - `banner`: bold system font with a thick contrasting outline or a solid plate —
//    what MMSSTV draws. Legible on the wire because the treatment keeps a LUMINANCE
//    edge; the size floor keeps the antialiased fringe a small fraction of the stroke.
//    Mixed case allowed: at banner sizes the x-height detail survives.
//
// POSITIONS ARE NORMALISED (cx, cy in 0..1 of the raster) so an item keeps its place
// when the operator flips between the five rasters, exactly as the crop centre does.
// Colour is an 8-swatch palette; every item carries a treatment (plate or outline) —
// untreated colored text is not offered, because chroma subsampling smears it.

import { OVERLAY_GLYPHS, GLYPH_W, GLYPH_H, normalizeOverlayText } from './sstvOverlayFont'
import { ID_STROKE_FRACTION, ID_MIN_SX, ID_GAP_CELLS, ID_PAD_CELLS } from './sstvIdOverlay'

export type OverlayStyle = 'crisp' | 'banner'
export type OverlayTreatment = 'plate' | 'outline'

export interface OverlayItem {
  id: string
  /** Raw operator text — folded per style at draw time, never destructively stored. */
  text: string
  /** Item centre, normalised to the raster (0..1 each axis). */
  cx: number
  cy: number
  /** Size step 1–4 — a multiplier of the style's raster-derived base size. */
  size: 1 | 2 | 3 | 4
  style: OverlayStyle
  /** Palette id from OVERLAY_COLORS. */
  color: string
  treatment: OverlayTreatment
}

/** The 8-swatch palette. `rgb` feeds the canvas; luminance decides the contrast side. */
export const OVERLAY_COLORS: ReadonlyArray<{ id: string; css: string; lum: number }> = [
  { id: 'white', css: '#ffffff', lum: 1.0 },
  { id: 'black', css: '#000000', lum: 0.0 },
  { id: 'yellow', css: '#ffe14d', lum: 0.84 },
  { id: 'orange', css: '#ff9d2e', lum: 0.68 },
  { id: 'red', css: '#f2453d', lum: 0.42 },
  { id: 'green', css: '#3ddc60', lum: 0.72 },
  { id: 'cyan', css: '#3fd8e8', lum: 0.75 },
  { id: 'blue', css: '#3d7bf2', lum: 0.48 },
]

export function colorCss(id: string): string {
  return OVERLAY_COLORS.find((c) => c.id === id)?.css ?? '#ffffff'
}

/** The contrasting side for a swatch — the plate behind it / the outline around it.
 *  Light text gets black backing, dark text gets white, so the pair always spans the
 *  luminance range the demod actually transmits (black→1500 Hz … white→2300 Hz). */
export function contrastCss(id: string): string {
  const lum = OVERLAY_COLORS.find((c) => c.id === id)?.lum ?? 1
  return lum >= 0.5 ? '#000000' : '#ffffff'
}

/** Crisp horizontal glyph scale for a raster: the plate's own derivation × the size
 *  step, so size 1 IS ident-weight and every step up stays integer (no antialiasing). */
export function crispSx(width: number, size: number): number {
  return Math.max(ID_MIN_SX, Math.ceil(ID_STROKE_FRACTION * width)) * size
}

/** Crisp run width for `n` glyphs at scale `sx` — glyphs, gaps, plate padding. */
function crispW(sx: number, n: number): number {
  return n <= 0 ? 0 : sx * (GLYPH_W * n + ID_GAP_CELLS * (n - 1) + 2 * ID_PAD_CELLS)
}

/** The scale actually drawn: the requested step, SHRUNK until the run fits the raster —
 *  the plate's own shrink-before-truncate rule. Size is a request, not a promise: a
 *  15-glyph CQ at step 2 physically cannot fit a 320 px raster at sx 10, and silently
 *  overflowing the transmit buffer would be worse than honest smaller text. Floor 1. */
export function fitCrispSx(width: number, size: number, n: number): number {
  let sx = crispSx(width, size)
  while (sx > 1 && crispW(sx, n) > width) sx--
  return sx
}

/** Banner font height in raster pixels. The 24 px floor is the legibility gate: below
 *  it the antialiased fringe of a system font is a large fraction of the stroke and the
 *  idcard.rs smear argument applies in full. */
export const BANNER_MIN_PX = 24
export function bannerPx(height: number, size: number): number {
  const px = Math.round(height * 0.055 * size)
  return Math.min(Math.max(px, BANNER_MIN_PX), Math.floor(height / 2))
}

/** Measure function for banner text width — injected. The component passes a bound
 *  `ctx.measureText`; tests pass an approximation. Crisp never needs it. */
export type BannerMeasure = (text: string, px: number) => number

export interface OverlayRect {
  x: number
  y: number
  w: number
  h: number
}

/** The item's bounding rect in raster pixels (plate/outline included). */
export function overlayRect(
  item: OverlayItem,
  width: number,
  height: number,
  measure: BannerMeasure,
): OverlayRect {
  if (item.style === 'crisp') {
    const text = normalizeOverlayText(item.text)
    const n = Math.max(1, text.length)
    const sx = fitCrispSx(width, item.size, n)
    const sy = Math.ceil(sx / 2)
    const w = crispW(sx, n)
    const h = GLYPH_H * sy + 2 * ID_PAD_CELLS * sx
    return { x: Math.round(item.cx * width - w / 2), y: Math.round(item.cy * height - h / 2), w, h }
  }
  const px = bannerPx(height, item.size)
  const pad = Math.max(2, Math.round(px / 5))
  const w = Math.ceil(measure(item.text, px)) + 2 * pad
  const h = px + 2 * pad
  return { x: Math.round(item.cx * width - w / 2), y: Math.round(item.cy * height - h / 2), w, h }
}

/** Topmost item under a raster-space point (last in the array draws last = wins). */
export function hitTest(
  items: OverlayItem[],
  x: number,
  y: number,
  width: number,
  height: number,
  measure: BannerMeasure,
): OverlayItem | null {
  for (let i = items.length - 1; i >= 0; i--) {
    const r = overlayRect(items[i], width, height, measure)
    if (x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h) return items[i]
  }
  return null
}

/** Move an item by raster-pixel deltas, keeping its CENTRE inside the raster. The
 *  centre clamp (not the rect) is deliberate: a wide banner may legitimately bleed off
 *  an edge, but its handle must never leave reach — same reasoning as window managers
 *  keeping a title bar on screen. */
export function moveItem(item: OverlayItem, dx: number, dy: number, width: number, height: number): OverlayItem {
  const cx = Math.min(0.98, Math.max(0.02, item.cx + dx / width))
  const cy = Math.min(0.98, Math.max(0.02, item.cy + dy / height))
  return { ...item, cx, cy }
}

let seq = 0
export function newOverlayId(): string {
  return `ov-${Date.now().toString(36)}-${(seq++).toString(36)}`
}

/** One-click presets. Positions chosen to clear the top-left ID plate at every raster
 *  (the plate is left-anchored; these centre, and sit below/above its band) — pinned by
 *  a test against `plateFor`'s actual geometry, not by hope. */
export function presetCq(mycall: string): OverlayItem {
  return {
    id: newOverlayId(),
    text: `CQ CQ DE ${mycall}`.trim(),
    cx: 0.5,
    cy: 0.24,
    size: 2,
    style: 'crisp',
    color: 'white',
    treatment: 'plate',
  }
}
export function preset73(mycall: string): OverlayItem {
  return {
    id: newOverlayId(),
    text: `73 DE ${mycall}`.trim(),
    cx: 0.5,
    cy: 0.86,
    size: 2,
    style: 'crisp',
    color: 'yellow',
    treatment: 'plate',
  }
}
export function presetReply(hiscall: string, mycall: string): OverlayItem {
  return {
    id: newOverlayId(),
    text: `${hiscall} DE ${mycall} 599`.trim(),
    cx: 0.5,
    cy: 0.5,
    size: 2,
    style: 'crisp',
    color: 'white',
    treatment: 'plate',
  }
}

/**
 * Draw every overlay item onto the transmit canvas, in raster coordinates.
 *
 * Called from `renderTx` AFTER the resample and BEFORE the ID plate — items are drawn
 * from state on every render (the canvas is rebuilt from the source each frame, so
 * anything painted once would vanish on the next mode change or crop drag), and the
 * plate drawing last is what makes covering the ident structurally impossible.
 */
export function drawOverlays(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  items: OverlayItem[],
): void {
  if (items.length === 0) return
  ctx.save()
  ctx.setTransform(1, 0, 0, 1, 0, 0)
  ctx.globalAlpha = 1
  ctx.imageSmoothingEnabled = false
  for (const item of items) {
    if (item.style === 'crisp') drawCrisp(ctx, width, height, item)
    else drawBanner(ctx, width, height, item)
  }
  ctx.restore()
}

function drawCrisp(ctx: CanvasRenderingContext2D, width: number, height: number, item: OverlayItem): void {
  const text = normalizeOverlayText(item.text)
  if (!text) return
  const sx = fitCrispSx(width, item.size, text.length)
  const sy = Math.ceil(sx / 2)
  const r = overlayRect(item, width, height, () => 0)
  const fg = colorCss(item.color)
  const bg = contrastCss(item.color)
  const gx0 = r.x + ID_PAD_CELLS * sx
  const gy0 = r.y + ID_PAD_CELLS * sx
  const glyphAt = (i: number) => gx0 + i * (GLYPH_W + ID_GAP_CELLS) * sx

  if (item.treatment === 'plate') {
    ctx.fillStyle = bg
    ctx.fillRect(r.x, r.y, r.w, r.h)
  } else {
    // Outline: the glyph blocks re-drawn at 8 offsets in the contrast colour first.
    // Block offsets, not stroked paths — the whole point of this style is that every
    // painted pixel is a full-value rectangle the demod can carry.
    const o = Math.max(1, Math.round(sx / 2))
    ctx.fillStyle = bg
    for (const [ox, oy] of [
      [-o, -o], [0, -o], [o, -o],
      [-o, 0], [o, 0],
      [-o, o], [0, o], [o, o],
    ] as const) {
      drawGlyphRun(ctx, text, glyphAt, gy0, sx, sy, ox, oy)
    }
  }
  ctx.fillStyle = fg
  drawGlyphRun(ctx, text, glyphAt, gy0, sx, sy, 0, 0)
}

function drawGlyphRun(
  ctx: CanvasRenderingContext2D,
  text: string,
  glyphAt: (i: number) => number,
  gy0: number,
  sx: number,
  sy: number,
  ox: number,
  oy: number,
): void {
  for (let i = 0; i < text.length; i++) {
    const bits = OVERLAY_GLYPHS[text[i]]
    if (!bits) continue
    const cellX = glyphAt(i)
    for (let row = 0; row < GLYPH_H; row++) {
      for (let col = 0; col < GLYPH_W; col++) {
        if ((bits[row] & (1 << (GLYPH_W - 1 - col))) === 0) continue
        ctx.fillRect(cellX + col * sx + ox, gy0 + row * sy + oy, sx, sy)
      }
    }
  }
}

function drawBanner(ctx: CanvasRenderingContext2D, width: number, height: number, item: OverlayItem): void {
  const text = item.text.trim()
  if (!text) return
  const px = bannerPx(height, item.size)
  ctx.font = `bold ${px}px system-ui, sans-serif`
  ctx.textAlign = 'center'
  ctx.textBaseline = 'middle'
  const x = item.cx * width
  const y = item.cy * height
  const fg = colorCss(item.color)
  const bg = contrastCss(item.color)
  if (item.treatment === 'plate') {
    const r = overlayRect(item, width, height, (t, p) => {
      ctx.font = `bold ${p}px system-ui, sans-serif`
      return ctx.measureText(t).width
    })
    ctx.font = `bold ${px}px system-ui, sans-serif`
    ctx.fillStyle = bg
    ctx.fillRect(r.x, r.y, r.w, r.h)
  } else {
    // Thick outline relative to the glyph size — the treatment IS the legibility.
    ctx.lineWidth = Math.max(2, Math.round(px / 7))
    ctx.lineJoin = 'round'
    ctx.strokeStyle = bg
    ctx.strokeText(text, x, y)
  }
  ctx.fillStyle = fg
  ctx.fillText(text, x, y)
}
