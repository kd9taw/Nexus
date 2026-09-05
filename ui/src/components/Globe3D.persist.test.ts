// @vitest-environment jsdom
// THE 3-D GLOBE REMEMBERS ITS LAYER PICKS (#211).
//
// The 2-D map persisted its layers (#199); the 3-D globe held them in a plain useState and reset
// to defaults on every mount, so a Connect operator working on the globe lost their layer choices
// each time. The fix reuses the 2-D map's per-surface store. This pins the parser that guards the
// restore — a malformed or foreign store must never poison the defaults it merges onto.
import { describe, it, expect } from 'vitest'
import { globeLayersFromStored } from './Globe3D'

describe('globeLayersFromStored', () => {
  it('returns nothing for an empty or unusable store', () => {
    expect(globeLayersFromStored(null)).toEqual({})
    expect(globeLayersFromStored('')).toEqual({})
    expect(globeLayersFromStored('not json')).toEqual({})
    expect(globeLayersFromStored('42')).toEqual({}) // valid JSON, not an object
    expect(globeLayersFromStored('null')).toEqual({})
  })

  it('keeps the known boolean toggles a real save wrote', () => {
    const got = globeLayersFromStored(JSON.stringify({ aurora: true, sats: true, spots: false }))
    expect(got).toEqual({ aurora: true, sats: true, spots: false })
  })

  it('drops unknown keys and non-boolean values, so a foreign store cannot inject junk', () => {
    const got = globeLayersFromStored(
      JSON.stringify({ aurora: true, bogus: true, muf: 'yes', grid: 1 }),
    )
    expect(got).toEqual({ aurora: true }) // bogus unknown; muf/grid not booleans
  })

  it('round-trips a full saved layer set', () => {
    const saved = {
      spots: false,
      arcs: false,
      states: true,
      lights: false,
      flare: false,
      aurora: true,
      muf: false,
      pca: false,
      heat: false,
      openings: false,
      grid: true,
      sats: true,
      pass: false,
      rings: false,
      cqzones: true,
      coverage: true,
      decodes: false,
      dxped: true,
      greyline: false,
    }
    expect(globeLayersFromStored(JSON.stringify(saved))).toEqual(saved)
  })
})
