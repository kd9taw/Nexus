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
})
