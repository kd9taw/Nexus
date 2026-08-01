import { describe, it, expect } from 'vitest'
import { decollideLabels } from './mapLabels'

// Row height the map's satellite labels use (10 px font + breathing room).
const H = 12
const TOP = 6
const BOTTOM = 494

describe('decollideLabels (band-map push-down/compress-up, adapted to 2-D)', () => {
  it('non-colliding labels keep their anchor y (x-disjoint neighbours included)', () => {
    const boxes = [
      { x: 10, w: 30, y: 50 },
      { x: 200, w: 30, y: 52 }, // same height but x-disjoint — no collision
      { x: 10, w: 30, y: 100 }, // same column, far apart vertically
    ]
    expect(decollideLabels(boxes, H, TOP, BOTTOM)).toEqual([50, 52, 100])
  })

  it('an x-overlapping pair pushes the lower label down clear of the upper', () => {
    const boxes = [
      { x: 10, w: 40, y: 50 },
      { x: 30, w: 40, y: 54 },
    ]
    expect(decollideLabels(boxes, H, TOP, BOTTOM)).toEqual([50, 62])
  })

  it('a three-label pile becomes a chain, returned in INPUT order', () => {
    const boxes = [
      { x: 12, w: 40, y: 51 },
      { x: 10, w: 40, y: 50 },
      { x: 14, w: 40, y: 52 },
    ]
    expect(decollideLabels(boxes, H, TOP, BOTTOM)).toEqual([62, 50, 74])
  })

  it('a pile at the bottom edge clamps and compresses UP (band-map pass 2)', () => {
    const boxes = [
      { x: 10, w: 40, y: 490 },
      { x: 12, w: 40, y: 491 },
    ]
    // Forward pass → [490, 502]; 502 breaches the bottom bound → clamp to 494
    // and compress the chain above it.
    expect(decollideLabels(boxes, H, TOP, BOTTOM)).toEqual([482, 494])
  })
})
