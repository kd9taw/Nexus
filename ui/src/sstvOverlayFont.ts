// The OVERLAY glyph set — the ID plate's bitmap font, widened for operator text.
//
// The station-ID font in `sstvIdOverlay.ts` is a bit-exact mirror of the Rust arbiter
// (`idcard.rs`) and is guarded by a test that parses the Rust source: it must never grow
// a glyph the arbiter doesn't have. Operator text overlays want punctuation the ident
// never needs ("CQ CQ?", "5-9-9!", "73."), so the extensions live HERE, UI-only, layered
// over an IMPORT of the mirrored table — never a copy of it, so the mirror test keeps
// guarding one table and this file cannot drift it.
//
// `sstv-overlay-font.test.ts` holds the other half of the contract: no extension may
// shadow a mirrored key, and every extension is a well-formed 5×7 bitmap.

import { GLYPHS, GLYPH_W, GLYPH_H } from './sstvIdOverlay'

/** UI-only punctuation, same 5×7 cell format as the mirrored table (low 5 bits, bit 4
 *  left). Uppercase-only remains deliberate — at these stroke widths lowercase loses its
 *  x-height detail on the wire, which is why the ident font never had it either. */
export const EXTRA_GLYPHS: Record<string, number[]> = {
  '.': [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100],
  ',': [0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b00100, 0b01000],
  '?': [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b00000, 0b00100],
  '!': [0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00000, 0b00100],
  ':': [0b00000, 0b01100, 0b01100, 0b00000, 0b01100, 0b01100, 0b00000],
  "'": [0b00100, 0b00100, 0b01000, 0b00000, 0b00000, 0b00000, 0b00000],
  '(': [0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010],
  ')': [0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000],
  '+': [0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000],
  '=': [0b00000, 0b00000, 0b11111, 0b00000, 0b11111, 0b00000, 0b00000],
  '@': [0b01110, 0b10001, 0b10111, 0b10101, 0b10111, 0b10000, 0b01110],
  '#': [0b01010, 0b01010, 0b11111, 0b01010, 0b11111, 0b01010, 0b01010],
}

/** The full table the overlay renderer draws from: mirror first, extensions layered on.
 *  Spread order means a shadowing key WOULD win — which is why the guard test forbids
 *  the intersection outright rather than trusting the order. */
export const OVERLAY_GLYPHS: Record<string, number[]> = { ...GLYPHS, ...EXTRA_GLYPHS }

export { GLYPH_W, GLYPH_H }

/** Fold operator text to what the crisp font can draw: uppercase, drop what has no
 *  glyph (dropped, not substituted — a `?` box mid-word is worse than a tighter word),
 *  cap the length so a paste can't build a plate wider than any raster. */
export function normalizeOverlayText(s: string, maxChars = 64): string {
  return (s ?? '')
    .toUpperCase()
    .split('')
    .filter((c) => OVERLAY_GLYPHS[c] !== undefined)
    .slice(0, maxChars)
    .join('')
}
