// @vitest-environment jsdom
//
// Connect's rail widths: the pure clamp and the per-surface record behind the two
// draggable rail separators. The layout contract requires anything persisted that
// encodes a size to be CLAMPED ON LOAD against the current window, not only at drag time —
// a width dragged on a 3440-wide monitor must not open a 1024 pop-out with the map gone.
//
// THE INVARIANTS (verified in a real browser, 2026-09-13, after two defects shipped past the
// first version of this file — it only ever fitted 600/600 into 900 and stepped from nothing
// stored, so neither the floor overshoot nor the step-restores-the-raw-width bug could show):
//   · after ANY fit or move, every open rail is within [RAIL_MIN, RAIL_MAX] and the pair sums to
//     no more than the room — i.e. the map keeps MAP_MIN whenever two floored rails leave it;
//   · moving one rail never changes the other rail's DISPLAYED width;
//   · the stored preference survives a re-clamp, so a bigger window gets the width back.
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
  resetRail,
  saveRailWidths,
  stepRail,
  type RailFitContext,
  type RailWidths,
} from './connectRails'

beforeEach(() => localStorage.clear())

const both = { left: true, right: true }
/** The measured `.connect` rooms the browser defects were found in: 1024×768 at 100% (grid box
 *  1002.9) and at 125% (798.1), both on the `sm` tier whose default rail is 248. */
const ROOM_1024_Z1 = railRoom(1002.9, 12, 12, 3)
const ROOM_1024_Z125 = railRoom(798.1, 12, 12, 3)
const sm = (room: number): RailFitContext => ({ room, present: both, defaultPx: 248 })

/** What the grid renders for a side: a fitted width, or the tier default for null. */
const shown = (w: RailWidths, ctx: RailFitContext) => ({
  left: w.left ?? ctx.defaultPx,
  right: w.right ?? ctx.defaultPx,
})

/** The invariant every result must hold. */
function expectFits(w: RailWidths, ctx: RailFitContext) {
  const s = shown(w, ctx)
  for (const side of ['left', 'right'] as const) {
    expect(s[side], `${side} below the floor`).toBeGreaterThanOrEqual(RAIL_MIN)
    expect(s[side], `${side} above the cap`).toBeLessThanOrEqual(RAIL_MAX)
  }
  if (ctx.room >= 2 * RAIL_MIN) {
    expect(s.left + s.right, `rails ${s.left}+${s.right} overrun the room ${ctx.room} — the map drops below ${MAP_MIN}`).toBeLessThanOrEqual(ctx.room)
  }
}

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
  it('never takes the room the other rail and the map floor need — rounding DOWN, never over', () => {
    expect(clampRail(900, 300, 900)).toBe(600)
    // 674.9 − 306 = 368.9: rounding to 369 would leave the map 0.1 px short of its floor.
    expect(clampRail(9999, 306, 674.9)).toBe(368)
  })
  it('keeps the floor even when there is no room at all (the operator can still grab it)', () => {
    expect(clampRail(400, 300, 350)).toBe(RAIL_MIN)
  })
})

describe('fitRails — preferences into THIS box (load and window resize)', () => {
  it('leaves an unstored side that fits alone: the default is the stylesheet’s, not ours', () => {
    expect(fitRails({ left: null, right: null }, sm(1000))).toEqual({ left: null, right: null })
    expect(fitRails({ left: null, right: null }, sm(ROOM_1024_Z1)), '1024×768 at 100%: today’s layout').toEqual({
      left: null,
      right: null,
    })
  })

  it('shrinks the DEFAULT rails too when they leave the map short (1024×768 at 125%)', () => {
    // D2: two 248 rails in a 470.1 room left the map 254 px and clipped its Conditions card.
    // Neither side was set by the operator, so both give way equally.
    const out = fitRails({ left: null, right: null }, sm(ROOM_1024_Z125))
    expect(out).toEqual({ left: 235, right: 235 })
    expectFits(out, sm(ROOM_1024_Z125))
  })

  it('fits a both-stored oversized pair proportionally, rounding down (600/500 into 1024 at 100%)', () => {
    const ctx = sm(ROOM_1024_Z1)
    const out = fitRails({ left: 600, right: 500 }, ctx)
    expect(out).toEqual({ left: 368, right: 306 })
    expectFits(out, ctx)
  })

  it('never re-floors a proportional share past the room (360/260 at 125%)', () => {
    // D1b: 260 scaled to 197 and was re-floored to 200 while the left kept its 273 share, so the
    // pair overran the room by 3 px and the map loaded at 277.1. The floored side's shortfall
    // has to come out of the other side.
    const ctx = sm(ROOM_1024_Z125)
    const out = fitRails({ left: 360, right: 260 }, ctx)
    expect(out).toEqual({ left: 270, right: 200 })
    expectFits(out, ctx)
  })

  it('keeps the rail the operator set LAST at its width; the other one gives way', () => {
    // What a reload shows after a move in a box that could not hold both preferences: the moved
    // rail exactly as moved, the other one grown back toward its preference as far as it can.
    const ctx = sm(ROOM_1024_Z1)
    const out = fitRails({ left: 352, right: 500, last: 'left' }, ctx)
    expect(out).toEqual({ left: 352, right: 322 })
    expectFits(out, ctx)
  })

  it('treats the only stored side as the one the operator set (an older record with no `last`)', () => {
    const ctx: RailFitContext = { room: railRoom(1024, 12, 12, 3), present: both, defaultPx: 300 }
    const out = fitRails({ left: 5000, right: null }, ctx)
    expect(out).toEqual({ left: ctx.room - RAIL_MIN, right: RAIL_MIN })
    expectFits(out, ctx)
  })

  it('scales two stored rails down together rather than starving the second one', () => {
    const out = fitRails({ left: 600, right: 600 }, { room: 900, present: both, defaultPx: 300 })
    expect(out).toEqual({ left: 450, right: 450 })
  })

  it('gives a lone rail the whole room when the other rail is closed', () => {
    const out = fitRails({ left: 700, right: null }, { room: 650, present: { left: true, right: false }, defaultPx: 300 })
    expect(out.left).toBe(650)
  })

  it('holds both rails at the floor when even two floors do not leave the map its room', () => {
    // Below the supported window floor: the map gives, the rails stay grabbable.
    const tiny = fitRails({ left: 600, right: 600 }, { room: 100, present: both, defaultPx: 300 })
    expect(tiny).toEqual({ left: RAIL_MIN, right: RAIL_MIN })
    const defaults = fitRails({ left: null, right: null }, { room: 300, present: both, defaultPx: 248 })
    expect(defaults).toEqual({ left: RAIL_MIN, right: RAIL_MIN })
  })

  it('caps at RAIL_MAX in an unbounded box', () => {
    const huge = fitRails({ left: 9000, right: null }, { room: Infinity, present: both, defaultPx: 300 })
    expect(huge).toEqual({ left: RAIL_MAX, right: null })
  })
})

describe('stepRail — one rail moves, the other stays exactly where it is on screen', () => {
  it('a step on a both-stored oversized pair never writes the other rail’s raw preference back', () => {
    // D1: from 368/306 (fitted from 600/500), one ArrowRight on the left rail used to restore the
    // right rail's stored 500 and drop the map to 86.9 px.
    const ctx = sm(ROOM_1024_Z1)
    const pref = { left: 600, right: 500 }
    const applied = fitRails(pref, ctx)

    const grow = stepRail({ pref, applied }, 'left', 368 + 16, ctx)
    expect(grow.applied, 'no room to grow: the left rail holds, the right rail is untouched').toEqual({ left: 368, right: 306 })
    expect(grow.pref, 'the right rail keeps its preference for a bigger window').toEqual({ left: 368, right: 500, last: 'left' })
    expectFits(grow.applied, ctx)

    const shrink = stepRail({ pref, applied }, 'left', 368 - 16, ctx)
    expect(shrink.applied).toEqual({ left: 352, right: 306 })
    expectFits(shrink.applied, ctx)
  })

  it('the same at 125% with 360/260 stored', () => {
    const ctx = sm(ROOM_1024_Z125)
    const pref = { left: 360, right: 260 }
    const applied = fitRails(pref, ctx)
    expect(applied).toEqual({ left: 270, right: 200 })

    const grow = stepRail({ pref, applied }, 'left', 286, ctx)
    expect(grow.applied).toEqual({ left: 270, right: 200 })
    const shrink = stepRail({ pref, applied }, 'left', 254, ctx)
    expect(shrink.applied).toEqual({ left: 254, right: 200 })
    expect(shrink.pref.right, 'preference kept').toBe(260)
    expectFits(shrink.applied, ctx)
  })

  it('a reload after the step shows the moved rail exactly as moved (and never shrinks either rail)', () => {
    const ctx = sm(ROOM_1024_Z1)
    const pref = { left: 600, right: 500 }
    const moved = stepRail({ pref, applied: fitRails(pref, ctx) }, 'left', 352, ctx)
    const reloaded = fitRails(parseRailWidths(JSON.stringify(moved.pref)), ctx)
    expect(reloaded.left).toBe(moved.applied.left)
    expect(reloaded.right!).toBeGreaterThanOrEqual(moved.applied.right!)
    expectFits(reloaded, ctx)
  })

  it('stepping a DEFAULT rail against a shrunk default neighbour keeps the neighbour shrunk', () => {
    const ctx = sm(ROOM_1024_Z125)
    const pref = { left: null, right: null }
    const applied = fitRails(pref, ctx) // 235/235
    const out = stepRail({ pref, applied }, 'right', 400, ctx)
    expect(out.applied).toEqual({ left: 235, right: 235 })
    expectFits(out.applied, ctx)
  })
})

describe('resetRail — a double-click puts ONE rail back to its default, as far as the box allows', () => {
  it('returns to the stylesheet default when it fits, holding the other rail', () => {
    const ctx = sm(ROOM_1024_Z1)
    const out = resetRail({ pref: { left: 352, right: 500, last: 'left' }, applied: { left: 352, right: 306 } }, 'left', ctx)
    expect(out.applied).toEqual({ left: null, right: 306 })
    expect(out.pref.left).toBeNull()
    expect(out.pref.right).toBe(500)
    expectFits(out.applied, ctx)
  })

  it('is clamped when the default no longer fits beside the other rail', () => {
    const ctx = sm(ROOM_1024_Z125)
    const out = resetRail({ pref: { left: 300, right: 260 }, applied: { left: 200, right: 270 } }, 'left', ctx)
    expect(out.applied).toEqual({ left: 200, right: 270 })
    expectFits(out.applied, ctx)
  })
})

describe('the stored record', () => {
  it('parses junk to "nothing stored" instead of throwing', () => {
    for (const raw of [null, '', 'x', '[]', '{"left":-5,"right":"a"}', '{"left":null}', '{"left":1e999}', '{"last":"up"}']) {
      expect(parseRailWidths(raw)).toEqual({ left: null, right: null })
    }
    expect(parseRailWidths('{"left":320.4}')).toEqual({ left: 320, right: null })
    expect(parseRailWidths('{"left":320,"last":"left"}')).toEqual({ left: 320, right: null, last: 'left' })
  })

  it('round-trips, and a side reset to default leaves the record', () => {
    saveRailWidths({ left: 360, right: null })
    expect(localStorage.getItem('nexus.connect.railWidths')).toBe('{"left":360}')
    expect(loadRailWidths()).toEqual({ left: 360, right: null })
    saveRailWidths({ left: 360, right: 280, last: 'right' })
    expect(loadRailWidths()).toEqual({ left: 360, right: 280, last: 'right' })
    saveRailWidths({ left: null, right: null })
    expect(loadRailWidths()).toEqual({ left: null, right: null })
  })
})
