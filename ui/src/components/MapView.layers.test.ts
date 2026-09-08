// @vitest-environment jsdom
//
// Layer persistence (#199): "Save selected layers within Connect". The layer set lived in
// un-persisted React state while every sibling control (projection, intent, 2D/3D) already
// persisted per-surface — so each launch reset the map to defaults plus the intent preset.
//
// The seam under test is the pure store→state parser: everything it accepts is CLAMPED
// against the current layer table (unknown keys dropped, missing keys defaulted, opacity
// bounded), because a persisted blob from an older build is exactly the input it will meet.
import { describe, it, expect } from 'vitest'
import { layersFromStored, DEFAULT_LAYERS } from './MapView'

describe('layersFromStored', () => {
  it('round-trips a stored pick and defaults what the blob does not carry', () => {
    const stored = JSON.stringify({
      ota: { visible: true, opacity: 0.9 },
      muf: { visible: false },
    })
    const out = layersFromStored(stored)!
    expect(out.ota).toEqual({ visible: true, opacity: 0.9 })
    expect(out.muf.visible).toBe(false)
    expect(out.muf.opacity).toBe(DEFAULT_LAYERS.muf.opacity)
    // Untouched layers keep their defaults wholesale.
    expect(out.coast).toEqual(DEFAULT_LAYERS.coast)
  })

  it('drops unknown keys, clamps opacity, and refuses garbage outright', () => {
    const out = layersFromStored(
      JSON.stringify({ notALayer: { visible: true }, aurora: { visible: true, opacity: 7 } }),
    )!
    expect('notALayer' in out).toBe(false)
    expect(out.aurora.visible).toBe(true)
    expect(out.aurora.opacity).toBe(DEFAULT_LAYERS.aurora.opacity)
    expect(layersFromStored(null)).toBeNull()
    expect(layersFromStored('not json')).toBeNull()
    expect(layersFromStored('"a string"')).toBeNull()
  })

  // The TX/RX path-line defaults are a PRODUCT decision, not an implementation detail:
  // "who heard me" is a handful of paths and worth showing unasked, while the decode roster
  // on a busy FT8 band is 100+ stations — defaulting THAT on would hand every operator a
  // spider's web on upgrade. A silent flip is exactly the change nobody would notice in a
  // diff, so it is pinned here rather than left to a constant.
  it('ships TX path lines on and RX path lines off', () => {
    expect(DEFAULT_LAYERS.txPaths.visible).toBe(true)
    expect(DEFAULT_LAYERS.rxPaths.visible).toBe(false)
  })

  it('gives an older build’s stored layers the new path rows at their defaults', () => {
    // The blob a 1.11 install already has on disk names neither key.
    const out = layersFromStored(JSON.stringify({ coast: { visible: false } }))!
    expect(out.txPaths).toEqual(DEFAULT_LAYERS.txPaths)
    expect(out.rxPaths).toEqual(DEFAULT_LAYERS.rxPaths)
  })
})
