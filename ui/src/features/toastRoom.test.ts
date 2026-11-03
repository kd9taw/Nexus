// #445: the pop-up column's room in its corner, clear of every transmit surface (toastRoom.ts).
import { describe, it, expect } from 'vitest'
import { toastRoom, TOAST_MIN_ROOM, type Box } from './toastRoom'

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
