// #445: the pop-up column's room in its corner, clear of every transmit surface (toastRoom.ts).
import { describe, it, expect } from 'vitest'
import { scrollReach, toastPlace, toastRoom, TOAST_MIN_ROOM, type Box, type Scroller } from './toastRoom'

const VIEW = { w: 1024, h: 768 }
const W = 340
const GAP = 16
const box = (left: number, top: number, right: number, bottom: number): Box => ({ left, top, right, bottom })
/** The column's box for a room in `corner`. */
function column(corner: 'bottom-right' | 'bottom-left' | 'top-right' | 'top-left', room: { inset: number; maxHeight: number }): Box {
  const left = corner.endsWith('left') ? GAP : VIEW.w - GAP - W
  const top = corner.startsWith('top') ? room.inset : VIEW.h - room.inset - room.maxHeight
  return box(left, top, left + W, top + room.maxHeight)
}
const overlaps = (a: Box, b: Box) => a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top

describe('toastRoom (#445)', () => {
  it('with nothing to avoid, the corner as it always was', () => {
    expect(toastRoom('bottom-right', VIEW, W, [], GAP)).toEqual({ inset: 16, maxHeight: 768 - 32 })
    expect(toastRoom('top-left', VIEW, W, [], GAP)).toEqual({ inset: 16, maxHeight: 768 - 32 })
  })

  it('a bottom corner stands above the TX dock (Phone: PTT at the bottom left)', () => {
    const dock = box(0, 700, 1024, 768)
    const strip = box(0, 260, 1024, 300)
    for (const corner of ['bottom-left', 'bottom-right'] as const) {
      const room = toastRoom(corner, VIEW, W, [dock, strip], GAP)!
      expect(room.inset, corner).toBe(768 - (700 - GAP))
      expect(room.maxHeight).toBe(700 - GAP - (300 + GAP))
      expect(overlaps(column(corner, room), dock)).toBe(false)
      expect(overlaps(column(corner, room), strip), 'a burst grew up over the TX strip').toBe(false)
    }
  })

  it('a top corner stands below a TX cluster at the top, and stops above the TX strip', () => {
    const cluster = box(700, 4, 1020, 40) // the top bar's TX cluster, right half only
    const strip = box(0, 260, 1024, 300)
    const tr = toastRoom('top-right', VIEW, W, [cluster, strip], GAP)!
    expect(tr).toEqual({ inset: 40 + GAP, maxHeight: 260 - GAP - (40 + GAP) })
    // The left column does not cross the cluster: it keeps its 16 px.
    const tl = toastRoom('top-left', VIEW, W, [cluster, strip], GAP)!
    expect(tl).toEqual({ inset: GAP, maxHeight: 260 - GAP - GAP })
    for (const [c, r] of [['top-right', tr], ['top-left', tl]] as const)
      for (const b of [cluster, strip]) expect(overlaps(column(c, r), b), `${c} over a TX surface`).toBe(false)
  })

  it('skips a stretch too short for a toast, toward the next one', () => {
    // FT's strip parked near the top: the 30 px above it is no room for the top corner.
    const strip = box(0, 62, 1024, 140)
    const room = toastRoom('top-right', VIEW, W, [strip], GAP)!
    expect(room.inset).toBe(140 + GAP)
    expect(62 - GAP - GAP).toBeLessThan(TOAST_MIN_ROOM)
  })

  it('a surface only on the other side of the window does not move the column', () => {
    const leftDock = box(0, 700, 400, 768)
    expect(toastRoom('bottom-right', VIEW, W, [leftDock], GAP)).toEqual({ inset: 16, maxHeight: 768 - 32 })
  })

  it('with no stretch tall enough, takes the tallest, and none at all is null', () => {
    const a = box(0, 40, 1024, 400)
    const b = box(0, 440, 1024, 740)
    const room = toastRoom('bottom-left', VIEW, W, [a, b], GAP)!
    // Stretches: 16–24 (8), 416–424 (8), 756–752 (none). The tallest of what is left.
    expect(room.maxHeight).toBeLessThan(TOAST_MIN_ROOM)
    expect(toastRoom('bottom-left', VIEW, W, [box(0, 0, 1024, 768)], GAP)).toBeNull()
  })
})

describe('toastRoom at a zoom (#445)', () => {
  it('the least room is a toast\'s height at the window\'s zoom: at 175 % a 72 px stretch is too short', () => {
    // A 72 px stretch between the strip and the dock: room for a toast at 100 %, not at 175 %, where
    // the column goes up to the stretch above the strip instead.
    const strip = box(0, 300, 1024, 600)
    const dock = box(0, 704, 1024, 768)
    const at100 = toastRoom('bottom-right', VIEW, W, [strip, dock], GAP)!
    expect(at100).toEqual({ inset: 768 - (704 - GAP), maxHeight: 704 - GAP - (600 + GAP) })
    const at175 = toastRoom('bottom-right', VIEW, W, [strip, dock], GAP, TOAST_MIN_ROOM * 1.75)!
    expect(at175, 'a toast at 175 % was clipped into the 72 px stretch').toEqual({ inset: 768 - (300 - GAP), maxHeight: 300 - GAP - GAP })
  })

  it('CW at 1920×1080, 175 %: the bottom corners skip the 72 px sliver above the dock that clipped one toast', () => {
    // Measured in real Chrome (toastcorner probe): the TX strip and the dock leave a 72 px stretch
    // between them on the column's side, and a toast there is 76 px tall.
    const view = { w: 1920, h: 1080 }
    const zoom = 1.75
    const gap = 16 * zoom
    const strip = box(0, 664, 1920, 731)
    const dock = box(0, 859, 1920, 1080)
    const unscaled = toastRoom('bottom-right', view, 595, [strip, dock], gap)!
    expect(unscaled, 'the sliver the probe saw').toEqual({ inset: 1080 - 831, maxHeight: 72 })
    const scaled = toastRoom('bottom-right', view, 595, [strip, dock], gap, TOAST_MIN_ROOM * zoom)!
    expect(scaled).toEqual({ inset: 1080 - 636, maxHeight: 636 - 28 })
    expect(scaled.maxHeight).toBeGreaterThanOrEqual(76)
    expect(toastRoom('bottom-left', view, 595, [strip, dock], gap, TOAST_MIN_ROOM * zoom)!.maxHeight).toBe(636 - 28)
  })
})

// What the operator is using comes second only to the transmit surfaces: kept off where there is room, stepped aside for
// where there is not, and never paid for with a transmit surface.
describe('toastPlace: what the operator is using', () => {
  const keep = box(0, 690, 1024, 768) // a folded dock's keep row (Send, Esc/Stop) at the foot
  const strip = box(0, 40, 1024, 100) // the TX strip, Stop TX in it, near the top
  /** The bottom-right column as tall as its toasts (`h`), at the foot of its room. */
  const col = (room: { inset: number; maxHeight: number }, h: number) => {
    const left = VIEW.w - GAP - W
    return box(left, VIEW.h - room.inset - h, left + W, VIEW.h - room.inset)
  }

  it('with nothing in use, exactly toastRoom', () => {
    expect(toastPlace('bottom-right', VIEW, W, [keep, strip], [], GAP, 120)).toEqual({ room: toastRoom('bottom-right', VIEW, W, [keep, strip], GAP), stepAside: false })
  })

  it('keeps off an open menu where a stretch clear of both is tall enough (CW: the menu hangs from the header)', () => {
    const menu = box(500, 200, 1000, 520)
    const at = toastPlace('bottom-right', VIEW, W, [keep, strip], [menu], GAP, 120)
    expect(at.stepAside).toBe(false)
    expect(at.room).toEqual({ inset: 768 - (690 - GAP), maxHeight: 690 - GAP - (520 + GAP) })
    for (const b of [keep, strip, menu]) expect(overlaps(column('bottom-right', at.room!), b)).toBe(false)
  })

  it('keeps off the focused macro key, which Tab brought between the strip and the keep row', () => {
    const key = box(700, 600, 820, 640)
    const at = toastPlace('bottom-right', VIEW, W, [keep, strip], [key], GAP, 120)
    expect(at.stepAside).toBe(false)
    expect(overlaps(column('bottom-right', at.room!), key), 'the column stands over the focused key').toBe(false)
    expect(overlaps(column('bottom-right', at.room!), keep)).toBe(false)
  })

  it('where nothing clear of both is tall enough, keeps its transmit-safe room and steps aside while it meets it', () => {
    // A menu filling the whole stretch between the strip and the keep row on the column's side.
    const menu = box(600, 170, 1000, 680)
    const at = toastPlace('bottom-right', VIEW, W, [keep, strip], [menu], GAP, 120)
    expect(at.room, 'it moved somewhere the transmit rule does not allow').toEqual(toastRoom('bottom-right', VIEW, W, [keep, strip], GAP))
    expect(at.stepAside).toBe(true)
    for (const b of [keep, strip]) expect(overlaps(column('bottom-right', at.room!), b)).toBe(false)
  })

  it('does not step aside for something the column, as tall as its toasts, does not reach', () => {
    // A lone short toast sits at the foot of its room; the menu is higher up.
    const menu = box(600, 170, 1000, 560)
    const room = toastRoom('bottom-right', VIEW, W, [keep, strip], GAP)!
    const h = 80
    expect(overlaps(col(room, h), menu)).toBe(false)
    // The clear stretch below the menu (560+16 to 690−16) is 98 px: at 175 % a toast needs more.
    const at = toastPlace('bottom-right', VIEW, W, [keep, strip], [menu], GAP, h, TOAST_MIN_ROOM * 1.75)
    expect(at).toEqual({ room, stepAside: false })
  })

  it('with no transmit-safe room at all, steps aside rather than stand in the bare corner', () => {
    const at = toastPlace('bottom-right', VIEW, W, [box(0, 0, 1024, 768)], [], GAP, 120)
    expect(at).toEqual({ room: null, stepAside: true })
  })
})

// The browser scrolls first and tells the page after, so a column placed against where Stop TX stood stood over it for a
// frame while the operator scrolled (CW, Phone and JS8 at 700×800, 175 %, measured in the shell probe, check 14). A surface
// in a scroller counts as everywhere the scroller can bring it.
describe('scrollReach: everywhere scrolling can bring a surface', () => {
  const shell = (top: number, maxTop: number, zoom = 1): Scroller => ({ box: box(0, 100, 700, 700), zoom, top, maxTop, left: 0, maxLeft: 0 })

  it('outside any scroller, where it stands', () => {
    expect(scrollReach(box(10, 20, 30, 40), [])).toEqual(box(10, 20, 30, 40))
  })

  it('grows up by what is left to scroll and down by what has been scrolled, cut to the scroller', () => {
    // Stop TX at 400–440, the shell scrolled 100 of 500: content can come down 100 and go up 400.
    expect(scrollReach(box(300, 400, 460, 440), [shell(100, 500)])).toEqual(box(300, 100, 460, 540))
    // At the top: it can only go up, and no further than the shell's own top edge.
    expect(scrollReach(box(300, 400, 460, 440), [shell(0, 50)])).toEqual(box(300, 350, 460, 440))
  })

  it('reads scroll px at the scroller\'s zoom', () => {
    expect(scrollReach(box(300, 400, 460, 440), [shell(20, 40, 1.75)])).toEqual(box(300, 400 - 20 * 1.75, 460, 440 + 20 * 1.75))
  })

  it('is the same wherever the shell stands: a column placed at any scroll position is clear at every other', () => {
    // A stop that scrolls with the content: at scroll s it stands at 600 − s.
    const at = (s: number) => scrollReach(box(300, 600 - s, 460, 640 - s), [shell(s, 300)])
    for (const s of [0, 75, 150, 300]) expect(at(s), `scrolled ${s}`).toEqual(at(0))
    // And every position the stop takes lies inside it.
    for (let s = 0; s <= 300; s += 25) {
      const r = at(0)
      expect(600 - s >= r.top && 640 - s <= r.bottom, `scrolled ${s}`).toBe(true)
    }
  })

  it('never less than where it stands, even outside its scroller\'s box (a fixed surface)', () => {
    const r = scrollReach(box(10, 5, 50, 30), [shell(100, 500)])
    expect(r.left <= 10 && r.top <= 5 && r.right >= 50 && r.bottom >= 30, JSON.stringify(r)).toBe(true)
  })
})
