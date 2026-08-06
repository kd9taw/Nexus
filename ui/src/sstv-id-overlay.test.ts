// THE ID OVERLAY MIRROR — `ui/src/sstvIdOverlay.ts` against `crates/tempo-sstv/src/idcard.rs`,
// which is the arbiter.
//
// The station ID is proved legible in Rust: `crates/tempo-sstv/tests/id_legibility.rs`
// renders the plate, runs it through the production encoder, decodes it with the real
// decoder and reads the callsign back out of the pixels, for all 15 modes at three SNRs.
// That proof covers the glyphs and geometry IN RUST. The webview draws the operator's
// preview with its own copy, and a copy that drifts is a preview that lies about what
// went on the air.
//
// So nothing below hardcodes a glyph. Each guard parses BOTH sides and compares them, the
// same discipline `band-tables.test.ts` uses for the band plan: "the Rust source is the
// fixture only in the sense that it is the side with the citation." When this goes red,
// change the Rust side first and re-run the legibility test — a glyph that differs here
// is a callsign the far end reads differently from the one we proved.
//
// ⚠️ WHAT THIS DOES NOT PROVE, written down rather than implied: that the two RASTERIZE
// identically. It proves they agree on the font, the constants and the plate rectangle;
// identical output then follows from identical inputs to the same integer block layout.
// The reason it does not need to prove more is that the webview's copy is not what goes
// on the air — `sstv_send` burns the plate in Rust before encoding, so the Rust proof
// covers the transmitted bytes and this file covers the preview matching them.

import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import {
  GLYPHS,
  GLYPH_H,
  GLYPH_W,
  ID_GAP_CELLS,
  ID_MIN_SX,
  ID_PAD_CELLS,
  ID_STROKE_FRACTION,
  normalizeCall,
  plateFor,
} from './sstvIdOverlay'

const IDCARD_RS = readFileSync(
  fileURLToPath(new URL('../../crates/tempo-sstv/src/idcard.rs', import.meta.url)),
  'utf8',
)

/** A `pub const NAME: type = value;` from the Rust source. */
function rustConst(name: string): number {
  const m = new RegExp(`pub const ${name}\\s*:\\s*\\w+\\s*=\\s*([0-9._]+)\\s*;`).exec(IDCARD_RS)
  expect(m, `idcard.rs declares ${name}`).not.toBeNull()
  return Number(m![1].replace(/_/g, ''))
}

/** Every `('X', [0b…, …])` row of the Rust `GLYPHS` table, in order.
 *
 * ⚠️ EVERY `\s*` HERE IS LOAD-BEARING, and this is the second time the repo has learned
 * it (see the identical warning in `band-tables.test.ts`). The table was written one
 * glyph per line and `cargo fmt` promptly exploded all 39 rows onto six lines each — a
 * 39-element array is past rustfmt's width heuristics — at which point a regex expecting
 * `('A', [` on one line matched nothing at all, and the guard failed claiming Rust had no
 * glyphs. That is a confusing way to learn you ran the formatter. This guard exists to
 * compare DATA, so it must not be sensitive to layout. */
function rustGlyphs(): { ch: string; rows: number[] }[] {
  const decl = IDCARD_RS.indexOf('pub const GLYPHS')
  expect(decl, 'idcard.rs declares GLYPHS').toBeGreaterThan(-1)
  const end = IDCARD_RS.indexOf('\n];', decl)
  const body = IDCARD_RS.slice(decl, end)
  return [...body.matchAll(/\(\s*'(.)'\s*,\s*\[([^\]]*)\]\s*,?\s*\)/g)].map((m) => ({
    ch: m[1],
    rows: m[2]
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean)
      .map((s) => Number.parseInt(s.replace(/^0b/, ''), 2)),
  }))
}

describe('the overlay constants mirror idcard.rs', () => {
  it('parsed both sides', () => {
    expect(rustGlyphs().length).toBeGreaterThan(30)
    expect(Object.keys(GLYPHS).length).toBeGreaterThan(30)
  })

  it('every geometry constant matches the Rust arbiter', () => {
    expect(GLYPH_W).toBe(rustConst('GLYPH_W'))
    expect(GLYPH_H).toBe(rustConst('GLYPH_H'))
    expect(ID_STROKE_FRACTION).toBe(rustConst('ID_STROKE_FRACTION'))
    expect(ID_MIN_SX).toBe(rustConst('ID_MIN_SX'))
    expect(ID_GAP_CELLS).toBe(rustConst('ID_GAP_CELLS'))
    expect(ID_PAD_CELLS).toBe(rustConst('ID_PAD_CELLS'))
  })
})

describe('the glyph table mirrors idcard.rs bit for bit', () => {
  const rust = rustGlyphs()

  it('covers exactly the same characters', () => {
    expect(rust.map((g) => g.ch).sort()).toEqual(Object.keys(GLYPHS).sort())
  })

  it('every glyph is identical row for row', () => {
    for (const { ch, rows } of rust) {
      expect(GLYPHS[ch], `no TS glyph for ${JSON.stringify(ch)}`).toBeDefined()
      expect(GLYPHS[ch], `glyph ${JSON.stringify(ch)}`).toEqual(rows)
    }
  })

  it('every glyph is 7 rows of 5 columns and fits the cell', () => {
    for (const [ch, rows] of Object.entries(GLYPHS)) {
      expect(rows.length, `${ch} row count`).toBe(GLYPH_H)
      for (const r of rows) expect(r, `${ch} row out of the 5-bit cell`).toBeLessThan(1 << GLYPH_W)
    }
  })

  it('covers every character a callsign can hold', () => {
    for (const ch of 'ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789/-') {
      expect(GLYPHS[ch], `no glyph for ${ch}`).toBeDefined()
    }
  })

  it('no two glyphs share a bitmap (a matched filter would just pick the other one)', () => {
    const seen = new Map<string, string>()
    for (const [ch, rows] of Object.entries(GLYPHS)) {
      const key = rows.join(',')
      expect(seen.get(key), `${ch} has the same bitmap as ${seen.get(key)}`).toBeUndefined()
      seen.set(key, ch)
    }
  })
})

describe('plate geometry', () => {
  // The five rasters the 15 shipped modes use, and the geometry each gets. Pinned as
  // VALUES on both sides — `idcard.rs`'s `geometry_matches_the_pinned_table` holds the
  // identical table — so a change to the formula has to be argued for in two places.
  const PINNED: [number, number, number, number, number][] = [
    // line_pixels, image_lines, sx, sy, plate height
    [320, 256, 5, 3, 31],
    [320, 240, 5, 3, 31],
    [512, 400, 8, 4, 44],
    [640, 496, 10, 5, 55],
    [800, 616, 12, 6, 66],
  ]

  it('matches the pinned table for every shipped raster', () => {
    for (const [w, h, sx, sy, ph] of PINNED) {
      const p = plateFor(w, h, 'KD9TAW')
      expect(p, `${w}×${h}`).not.toBeNull()
      expect([p!.sx, p!.sy, p!.h], `${w}×${h}`).toEqual([sx, sy, ph])
    }
  })

  it('agrees with the formula built from the PARSED Rust constants', () => {
    for (const [w, h] of PINNED) {
      const sx = Math.max(rustConst('ID_MIN_SX'), Math.ceil(rustConst('ID_STROKE_FRACTION') * w))
      const p = plateFor(w, h, 'KD9TAW')!
      expect(p.sx, `${w}×${h} horizontal scale`).toBe(sx)
      expect(p.sy, `${w}×${h} vertical scale`).toBe(Math.ceil(sx / 2))
    }
  })

  it('⭐ the plate is inside the picture in every shipped raster', () => {
    for (const [w, h] of PINNED) {
      const p = plateFor(w, h, 'KD9TAW')!
      expect(p.x + p.w, `${w}×${h} right edge`).toBeLessThanOrEqual(w)
      expect(p.y + p.h, `${w}×${h} bottom edge`).toBeLessThanOrEqual(h)
    }
  })

  it('the stroke is at least 1.25 % of picture width — the 20 dB demod smear', () => {
    for (const [w, h] of PINNED) {
      expect(plateFor(w, h, 'KD9TAW')!.sx / w, `${w}×${h}`).toBeGreaterThanOrEqual(0.0125)
    }
  })

  it('sits at the TOP — a truncated over loses the tail, and an ID down there goes with it', () => {
    const p = plateFor(320, 256, 'KD9TAW')!
    expect(p.y).toBeLessThan(256 / 4)
    expect(p.x).toBeLessThan(320 / 4)
    // Inset rather than flush: Robot 24/36's row 0 carries no Cb at all, and line 0 is
    // where a receiver's sync settling shows.
    expect(p.x).toBeGreaterThan(0)
    expect(p.y).toBeGreaterThan(0)
  })

  it('a long portable call shrinks before it truncates', () => {
    const p = plateFor(320, 256, 'VP2E/KD9TAW')!
    expect(p.call).toBe('VP2E/KD9TAW')
    expect(p.sx).toBeLessThan(5)
    expect(p.x + p.w).toBeLessThanOrEqual(320)
  })

  it('no callsign means no plate — and the Send gate is what makes that unreachable', () => {
    expect(plateFor(320, 256, '')).toBeNull()
    expect(plateFor(320, 256, '   ')).toBeNull()
    expect(plateFor(320, 256, '!!!')).toBeNull()
  })
})

describe('normalizeCall mirrors normalize_call', () => {
  it('uppercases, trims, and keeps only what the font can draw', () => {
    expect(normalizeCall(' kd9taw ')).toBe('KD9TAW')
    expect(normalizeCall('KD9TAW/P')).toBe('KD9TAW/P')
    expect(normalizeCall('K.D9-T!AW')).toBe('KD9-TAW')
    expect(normalizeCall('KD9TAW', 3)).toBe('KD9')
    expect(normalizeCall('')).toBe('')
  })
})
