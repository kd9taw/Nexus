// ⭐ THE BURNED-IN STATION ID — the webview's mirror of the Rust arbiter.
//
// ⚠️ COMPLIANCE FEATURE, NOT DECORATION. Before this, an SSTV over carried NO station
// identification of any kind: no burned-in overlay (this file's job — `SstvView` was
// `drawImage` only, with no `fillText` anywhere), no CW ident (`cw_id_after_73` reaches
// `pending_cw_id` only through the FT/digital QSO state machine; `Engine::sstv_send`
// never touched it), and the FSK ID is decode-only — no encoder exists. One over is a
// single continuous PTT hold of up to ~290 s of picture-only audio.
//
// ⚠️ THE RUST SIDE IS THE AUTHORITY, NOT THIS FILE. `crates/tempo-sstv/src/idcard.rs`
// carries the glyph table, the geometry and the drawing, and `sstv_send` burns the plate
// into the pixels it is handed BEFORE encoding — so no webview bug, stale buffer or
// third-party caller can put an unidentified picture on the air. This file exists so the
// operator's PREVIEW is what actually goes out. The two draw identical pixels, so the
// double draw is a no-op, and `sstv-id-overlay.test.ts` parses the Rust source and fails
// if this mirror drifts from it by a single bit.
//
// The design rationale (why a bitmap font and not `fillText`, why white-on-black, why the
// glyphs are scaled anisotropically, why top-left) lives in `idcard.rs`'s module header
// and is not duplicated here — one copy, in the file that is the arbiter.

/** Glyph cell width in font pixels; each row's low 5 bits are the columns, bit 4 left. */
export const GLYPH_W = 5
/** Glyph cell height in font pixels. */
export const GLYPH_H = 7
/** Horizontal scale as a fraction of the mode's line_pixels — mirrors ID_STROKE_FRACTION. */
export const ID_STROKE_FRACTION = 0.015
/** Floor on the horizontal scale — mirrors ID_MIN_SX. */
export const ID_MIN_SX = 3
/** Gap between glyph cells, in units of the horizontal scale — mirrors ID_GAP_CELLS. */
export const ID_GAP_CELLS = 1
/** Plate padding and the plate's inset from the corner, in units of the horizontal
 *  scale — mirrors ID_PAD_CELLS. */
export const ID_PAD_CELLS = 1

/** 5×7 uppercase bitmap font — a byte-for-byte mirror of `idcard.rs`'s `GLYPHS`. */
export const GLYPHS: Record<string, number[]> = {
  A: [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
  B: [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
  C: [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110],
  D: [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110],
  E: [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
  F: [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
  G: [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111],
  H: [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
  I: [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111],
  J: [0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100],
  K: [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
  L: [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
  M: [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
  N: [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001],
  O: [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
  P: [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
  Q: [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101],
  R: [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
  S: [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
  T: [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
  U: [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
  V: [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
  W: [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001],
  X: [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
  Y: [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100],
  Z: [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
  '0': [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
  '1': [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
  '2': [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
  '3': [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110],
  '4': [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
  '5': [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
  '6': [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
  '7': [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
  '8': [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
  '9': [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
  '/': [0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000],
  '-': [0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000],
  ' ': [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000],
}

/** The operator's callsign reduced to what the font can draw — mirrors `normalize_call`.
 *  Characters are DROPPED rather than substituted: a `?` box in a callsign is worse than
 *  a shorter call, and everything a real call can contain (including a portable `/`) is
 *  in the font. */
export function normalizeCall(call: string, maxChars = 32): string {
  return (call ?? '')
    .trim()
    .toUpperCase()
    .split('')
    .filter((c) => /[A-Z0-9/-]/.test(c))
    .slice(0, maxChars)
    .join('')
}

/** Where the plate sits, in destination-raster pixels. Mirrors Rust's `IdPlate`. */
export interface IdPlate {
  sx: number
  sy: number
  x: number
  y: number
  w: number
  h: number
  call: string
}

/** Plate width for `n` glyphs at scale `sx` — the run plus padding both sides. */
function plateW(sx: number, n: number): number {
  return n <= 0 ? 0 : sx * (GLYPH_W * n + ID_GAP_CELLS * (n - 1) + 2 * ID_PAD_CELLS)
}

/** How many glyphs fit `width` at scale `sx`, keeping the plate's inset clear both sides. */
function maxChars(sx: number, width: number): number {
  const avail = Math.max(0, width - 2 * ID_PAD_CELLS * sx)
  const cells = Math.floor(avail / sx)
  if (cells < GLYPH_W + 2 * ID_PAD_CELLS) return 0
  return Math.floor((cells + ID_GAP_CELLS - 2 * ID_PAD_CELLS) / (GLYPH_W + ID_GAP_CELLS))
}

/** Plate geometry for a `width`×`height` raster carrying `call` — mirrors `plate_for`.
 *  Null when there is nothing to draw or nothing will fit. */
export function plateFor(width: number, height: number, call: string): IdPlate | null {
  const full = normalizeCall(call)
  if (!full || width <= 0 || height <= 0) return null
  let sx = Math.max(ID_MIN_SX, Math.ceil(ID_STROKE_FRACTION * width))
  // Shrink to fit before dropping characters — the call is the point.
  while (sx > 2 && plateW(sx, full.length) + 2 * ID_PAD_CELLS * sx > width) sx--
  const nMax = maxChars(sx, width)
  if (nMax === 0) return null
  const text = full.slice(0, nMax)
  const sy = Math.ceil(sx / 2)
  const w = plateW(sx, text.length)
  const h = GLYPH_H * sy + 2 * ID_PAD_CELLS * sx
  if (ID_PAD_CELLS * sx + h > height) return null
  return { sx, sy, x: ID_PAD_CELLS * sx, y: ID_PAD_CELLS * sx, w, h, call: text }
}

/**
 * Burn the ID plate onto a canvas context, in DESTINATION-raster coordinates.
 *
 * ⚠️ **Call this AFTER the crop and AFTER the final resample, on the target-raster
 * canvas.** Two reasons, and both are the difference between an ident and a smudge:
 *
 *  - The operator's drag box moves the SOURCE behind a fixed frame. Drawing here, in
 *    destination coordinates, puts the plate where no crop rectangle can reach it. (Do
 *    not "simplify" this by drawing into the source and letting the crop carry it — that
 *    is precisely the version the operator can crop their own callsign out of.)
 *  - Text drawn at 4032 px and then downscaled 12.6× goes through the same box filter as
 *    everything else: a 3 px stroke becomes a 0.24 px grey smear. Present in the bitmap,
 *    and not an identification.
 *
 * `fillRect` only — no `fillText`, no antialiasing, integer blocks throughout, so the
 * stroke width on the wire is the exact number the legibility test measured.
 */
export function drawIdPlate(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  call: string,
): IdPlate | null {
  const p = plateFor(width, height, call)
  if (!p) return null
  ctx.save()
  // Defensive: the composer sets these, but a plate drawn through a stale transform or a
  // leftover alpha would be a silently weakened ident.
  ctx.setTransform(1, 0, 0, 1, 0, 0)
  ctx.globalAlpha = 1
  ctx.imageSmoothingEnabled = false
  ctx.fillStyle = '#000'
  ctx.fillRect(p.x, p.y, p.w, p.h)
  ctx.fillStyle = '#fff'
  const gx0 = p.x + ID_PAD_CELLS * p.sx
  const gy0 = p.y + ID_PAD_CELLS * p.sx
  for (let i = 0; i < p.call.length; i++) {
    const bits = GLYPHS[p.call[i]]
    if (!bits) continue
    const cellX = gx0 + i * (GLYPH_W + ID_GAP_CELLS) * p.sx
    for (let r = 0; r < GLYPH_H; r++) {
      for (let c = 0; c < GLYPH_W; c++) {
        if ((bits[r] & (1 << (GLYPH_W - 1 - c))) === 0) continue
        ctx.fillRect(cellX + c * p.sx, gy0 + r * p.sy, p.sx, p.sy)
      }
    }
  }
  ctx.restore()
  return p
}
