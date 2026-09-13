// @vitest-environment jsdom
//
// Connect's rail widths: the pure clamp and the per-surface record behind the two
// draggable rail separators. The layout contract requires anything persisted that
// encodes a size to be CLAMPED ON LOAD against the current window, not only at drag time —
// a width dragged on a 3440-wide monitor must not open a 1024 pop-out with the map gone.
import { describe, it, expect, beforeEach } from 'vitest'
import {
  MAP_MIN,
  RAIL_MAX,
  RAIL_MIN,
  clampRail,
  fitRails,
  loadRailWidths,
  parseRailWidths,
  railRoom,
  saveRailWidths,
} from './connectRails'

beforeEach(() => localStorage.clear())

describe('railRoom — what the rails may share of the grid', () => {
  it('is the box minus padding, the gaps between visible columns, and the map floor', () => {
    // 3 columns ⇒ 2 gaps.
    expect(railRoom(1256, 12, 12, 3)).toBe(1256 - 24 - 24 - MAP_MIN)
    // 2 columns (one rail closed) ⇒ 1 gap.
    expect(railRoom(1256, 12, 12, 2)).toBe(1256 - 24 - 12 - MAP_MIN)
  })

  it('treats an unmeasurable box as unbounded, never as zero room', () => {
    // A 0×0 box is a hidden keep-alive host (or jsdom): clamping against it would shrink
    // every rail to the floor and persist nothing useful.
    expect(railRoom(0, 12, 12, 3)).toBe(Infinity)
    expect(railRoom(NaN, NaN, NaN, 3)).toBe(Infinity)
  })
})

describe('clampRail — one rail against the room the other leaves', () => {
  it('passes a sane width through', () => {
    expect(clampRail(320, 300, 1000)).toBe(320)
  })
  it('floors at RAIL_MIN and caps at RAIL_MAX', () => {
    expect(clampRail(10, 300, 1000)).toBe(RAIL_MIN)
    expect(clampRail(5000, 0, 100000)).toBe(RAIL_MAX)
  })
  it('never takes the room the other rail and the map floor need', () => {
    expect(clampRail(900, 300, 900)).toBe(600)
  })
  it('keeps the floor even when there is no room at all (the operator can still grab it)', () => {
    expect(clampRail(400, 300, 350)).toBe(RAIL_MIN)
  })
})

describe('fitRails — stored preferences into THIS box', () => {
  const both = { left: true, right: true }

  it('leaves an unstored side alone: the default is the stylesheet’s, not ours', () => {
    expect(fitRails({ left: null, right: null }, { room: 200, present: both, defaultPx: 300 })).toEqual({
      left: null,
      right: null,
    })
  })

  it('clamps a saved oversized width against the current box (the load path)', () => {
    // A 1024 box: room 1024 − 24 − 24 − MAP_MIN, and the right rail keeps its 300 default.
    const room = railRoom(1024, 12, 12, 3)
    const out = fitRails({ left: 5000, right: null }, { room, present: both, defaultPx: 300 })
    expect(out.left).toBe(room - 300)
    expect(out.right).toBeNull()
  })

  it('scales two stored rails down together rather than starving the second one', () => {
    const out = fitRails({ left: 600, right: 600 }, { room: 900, present: both, defaultPx: 300 })
    expect(out).toEqual({ left: 450, right: 450 })
  })

  it('gives a lone rail the whole room when the other rail is closed', () => {
    const out = fitRails({ left: 700, right: null }, { room: 650, present: { left: true, right: false }, defaultPx: 300 })
    expect(out.left).toBe(650)
  })

  it('never returns a width below the floor or above the cap', () => {
    const tiny = fitRails({ left: 600, right: 600 }, { room: 100, present: both, defaultPx: 300 })
    expect(tiny.left).toBe(RAIL_MIN)
    expect(tiny.right).toBe(RAIL_MIN)
    const huge = fitRails({ left: 9000, right: null }, { room: Infinity, present: both, defaultPx: 300 })
    expect(huge.left).toBe(RAIL_MAX)
  })
})

describe('the stored record', () => {
  it('parses junk to "nothing stored" instead of throwing', () => {
    for (const raw of [null, '', 'x', '[]', '{"left":-5,"right":"a"}', '{"left":null}', '{"left":1e999}']) {
      expect(parseRailWidths(raw)).toEqual({ left: null, right: null })
    }
    expect(parseRailWidths('{"left":320.4}')).toEqual({ left: 320, right: null })
  })

  it('round-trips, and a side reset to default leaves the record', () => {
    saveRailWidths({ left: 360, right: null })
    expect(localStorage.getItem('nexus.connect.railWidths')).toBe('{"left":360}')
    expect(loadRailWidths()).toEqual({ left: 360, right: null })
    saveRailWidths({ left: null, right: null })
    expect(loadRailWidths()).toEqual({ left: null, right: null })
  })
})
