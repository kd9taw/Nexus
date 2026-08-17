// The overlay font's contract with the ident font: LAYERED, never drifted.
//
// The ID plate's glyph table is a bit-exact mirror of the Rust arbiter, guarded by a
// test that parses idcard.rs. The overlay extensions ride on top of an IMPORT of that
// table — so the one way this file could corrupt the ident is an extension that SHADOWS
// a mirrored key (spread order would let it win). That is what the first test forbids.
import { describe, it, expect } from 'vitest'
import { GLYPHS, GLYPH_W, GLYPH_H } from './sstvIdOverlay'
import { EXTRA_GLYPHS, OVERLAY_GLYPHS, normalizeOverlayText } from './sstvOverlayFont'

describe('overlay font extensions', () => {
  it('no extension shadows a mirrored ident glyph', () => {
    const shadowed = Object.keys(EXTRA_GLYPHS).filter((k) => GLYPHS[k] !== undefined)
    expect(shadowed, 'extensions must extend, never redefine, the mirrored table').toEqual([])
  })

  it('positive control: the guard actually sees a shadow', () => {
    // The same check run over a table that DOES shadow 'A' must trip — otherwise the
    // test above is a filter that can never match, not a guard.
    const bad = { ...EXTRA_GLYPHS, A: EXTRA_GLYPHS['?'] }
    const shadowed = Object.keys(bad).filter((k) => GLYPHS[k] !== undefined)
    expect(shadowed).toEqual(['A'])
  })

  it('every extension is a well-formed 5×7 bitmap', () => {
    for (const [ch, rows] of Object.entries(EXTRA_GLYPHS)) {
      expect(rows.length, `'${ch}' row count`).toBe(GLYPH_H)
      for (const row of rows) {
        expect(row, `'${ch}' row fits ${GLYPH_W} bits`).toBeGreaterThanOrEqual(0)
        expect(row, `'${ch}' row fits ${GLYPH_W} bits`).toBeLessThan(1 << GLYPH_W)
      }
    }
  })

  it('the combined table is the union', () => {
    expect(Object.keys(OVERLAY_GLYPHS).length).toBe(
      Object.keys(GLYPHS).length + Object.keys(EXTRA_GLYPHS).length,
    )
  })
})

describe('normalizeOverlayText', () => {
  it('uppercases and keeps what the font can draw, punctuation included', () => {
    expect(normalizeOverlayText('cq cq de kd9taw?')).toBe('CQ CQ DE KD9TAW?')
    expect(normalizeOverlayText('5-9-9!')).toBe('5-9-9!')
  })
  it('drops unknown characters rather than substituting boxes', () => {
    expect(normalizeOverlayText('73 ~ de € kd9taw')).toBe('73  DE  KD9TAW')
  })
  it('caps runaway length', () => {
    expect(normalizeOverlayText('A'.repeat(500)).length).toBe(64)
  })
})
