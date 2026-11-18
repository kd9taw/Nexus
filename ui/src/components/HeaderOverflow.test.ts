// WHICH WAY THE HEADER'S ⋯ GROUP OPENS (a beta tester's report: the Memories group opened off the window's
// left edge, −441 px at 1920x1080 with the Dashboard rail, whenever the header wrapped and put ⋯ at the
// start of its second line). The choice is a pure function of on-screen boxes: rightward when it fits,
// else leftward, else slid back inside from its start. Whether the real app lands it on screen is the
// real-browser probe's job (probe/overflow-probe.mjs); this pins the boundaries.
import { describe, expect, it } from 'vitest'
import { placeOverflow } from './HeaderOverflow'

const BOUND = { left: 88, right: 1920 }
const W = 616

describe('placeOverflow', () => {
  it('opens rightward from a button at the start of a wrapped header line (the reported case)', () => {
    expect(placeOverflow({ left: 145, right: 175 }, W, BOUND)).toEqual({ side: 'start', offset: 0 })
  })

  it('opens leftward from a button at the end of a one-row header, as before', () => {
    expect(placeOverflow({ left: 1314, right: 1343 }, W, BOUND)).toEqual({ side: 'end', offset: 0 })
  })

  it('rightward exactly to the bound fits; one px more goes leftward', () => {
    expect(placeOverflow({ left: 1920 - W, right: 1920 - W + 30 }, W, BOUND).side).toBe('start')
    expect(placeOverflow({ left: 1920 - W + 1, right: 1920 - W + 31 }, W, BOUND).side).toBe('end')
  })

  it('leftward exactly to the bound fits; one px short of it slides back from the start', () => {
    const narrow = { left: 88, right: 710 }
    expect(placeOverflow({ left: 674, right: 704 }, W, narrow)).toEqual({ side: 'end', offset: 0 })
    const p = placeOverflow({ left: 400, right: 703 }, W, narrow)
    expect(p.side).toBe('start')
    // Its far edge meets the bound's right: 400 + offset + 616 = 710, and its start stays inside.
    expect(400 + p.offset + W).toBe(710)
    expect(400 + p.offset).toBeGreaterThanOrEqual(88)
  })

  it('a group wider than the bound itself keeps its start on the bound’s start, never off it', () => {
    const p = placeOverflow({ left: 300, right: 330 }, W, { left: 88, right: 600 })
    expect(300 + p.offset).toBe(88)
  })

  // The slide is written as a CSS `left` in the group's own px, under `--ui-zoom`: at 150 % one of its px is
  // 1.5 on screen, so the exact clamp is the screen distance divided by that scale, no more (a laid-out
  // width that counted the border-box padding and border twice slid it ~4 % too far, under the mode bar).
  it('the slide lands exactly on the clamp under the app’s zoom: screen px divided by the group’s scale', () => {
    const scale = 1.5
    const wOnScreen = W * scale // 924
    const bound = { left: 100, right: 1100 }
    const anchor = { left: 500, right: 545 }
    const p = placeOverflow(anchor, wOnScreen, bound, scale)
    expect(p.side).toBe('start')
    // Far edge on the bound: 500 + offset × 1.5 + 924 = 1100 → offset = −324 / 1.5 = −216 of the group's px.
    expect(p.offset).toBe(-216)
    expect(anchor.left + p.offset * scale + wOnScreen).toBe(bound.right)
    // Wider than the bound: its start on the bound's start, exactly (500 + offset × 1.5 = 132).
    const q = placeOverflow(anchor, wOnScreen, { left: 132, right: 900 }, scale)
    expect(q.offset).toBeCloseTo(-368 / 1.5, 9)
    expect(anchor.left + q.offset * scale).toBeCloseTo(132, 9)
  })

  it('no slide, no offset, whatever the scale', () => {
    expect(placeOverflow({ left: 145, right: 175 }, W * 1.5, BOUND, 1.5)).toEqual({ side: 'start', offset: 0 })
    expect(placeOverflow({ left: 1314, right: 1343 }, W * 1.5, BOUND, 1.5)).toEqual({ side: 'end', offset: 0 })
  })
})
