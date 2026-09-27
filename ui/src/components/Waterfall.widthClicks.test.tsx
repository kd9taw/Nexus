// @vitest-environment jsdom
//
// THE MARKER WIDTH IS PAINT: EVERY WATERFALL CLICK LANDS EXACTLY WHERE IT DID (2026-09-27).
//
// Operator, asking for the width: "change nothing on the right and left mouse clicks as I want to
// be able to set a split, etc the same way". So the same clicks at the same pixels, with every
// gesture the wide graph knows, must hand `onTune` the same frequency and the same marker whether
// the markers are single lines or brackets, and those are pinned to the numbers the waterfall has
// always produced. A click inside a bracket tunes to the clicked pixel; nothing snaps to an edge.
//
// Same observation technique as Waterfall.zoom.test.tsx: jsdom lays nothing out, so the one box the
// click handler measures is stubbed.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, fireEvent, cleanup } from '@testing-library/react'
import { Waterfall } from './Waterfall'

vi.mock('../api', () => ({
  getSpectrumRow: () => Promise.resolve({ row: [], loHz: 200, hiHz: 4000 }),
}))

/** CSS width the click mapping is stubbed at. */
const W = 400
const TX_HZ = 1000
const RX_HZ = 1500

/** Every gesture `tuneTarget` answers, and the two it must refuse. */
const GESTURES = [
  { name: 'left', init: { button: 0 }, target: 'rx' },
  { name: 'Shift+left', init: { button: 0, shiftKey: true }, target: 'tx' },
  { name: 'right', init: { button: 2 }, target: 'tx' },
  { name: 'Ctrl+left', init: { button: 0, ctrlKey: true }, target: 'both' },
  { name: 'Ctrl+right (the Mac Ctrl-click)', init: { button: 2, ctrlKey: true }, target: 'both' },
  { name: '⌘+left', init: { button: 0, metaKey: true }, target: 'both' },
  { name: 'middle', init: { button: 1 }, target: null },
  { name: 'back', init: { button: 3 }, target: null },
] as const

// Both edges, open band, and pixels INSIDE the TX and RX brackets at every width below (in the Std
// view 1 px = 7 Hz, so TX at 1000 Hz is x 114.3 and RX at 1500 Hz is x 185.7).
const PIXELS = [0, 1, 57, 100, 114, 115, 116, 118, 120, 143, 185, 186, 187, 190, 200, 256.5, 399, 400]
/** No width, FT8, FT4, Q65-60A and JT65C: the narrowest and the widest brackets Nexus draws. */
const WIDTHS = [43.75, 62.5, 108.33, 699.83]

beforeEach(() => {
  localStorage.clear()
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(() => {
  cleanup()
  localStorage.clear()
})

type Seen = { gesture: string; px: number; tuned: unknown[] | null; prevented: boolean }

/** Click every gesture at every pixel; record what reached `onTune` and whether the browser's
 *  default was cancelled. */
function clickAll(markerWidthHz?: number | null): { seen: Seen[]; menuPrevented: boolean } {
  const onTune = vi.fn()
  const { container, unmount } = render(
    <Waterfall
      transmitting={false}
      rxOffsetHz={RX_HZ}
      txOffsetHz={TX_HZ}
      theme="dark"
      onTune={onTune}
      markerWidthHz={markerWidthHz}
    />,
  )
  const canvas = container.querySelector('canvas.waterfall-canvas') as HTMLCanvasElement
  canvas.getBoundingClientRect = () =>
    ({ left: 0, top: 0, right: W, bottom: 200, width: W, height: 200, x: 0, y: 0 }) as DOMRect
  const seen: Seen[] = []
  for (const g of GESTURES) {
    for (const px of PIXELS) {
      onTune.mockClear()
      const notCancelled = fireEvent.mouseDown(canvas, { ...g.init, clientX: px })
      expect(onTune.mock.calls.length, `${g.name} at ${px}: one click, at most one tune`).toBeLessThanOrEqual(1)
      seen.push({ gesture: g.name, px, tuned: onTune.mock.calls[0] ?? null, prevented: !notCancelled })
    }
  }
  const menuNotCancelled = fireEvent.contextMenu(canvas)
  unmount()
  return { seen, menuPrevented: !menuNotCancelled }
}

describe('waterfall clicks with the markers drawn as the signal width', () => {
  it('the plain single-line waterfall tunes where it always has (pinned)', () => {
    // The Std view maps 200–3000 Hz across the canvas and rounds to the hertz — the arithmetic the
    // click handler has used since #115.
    const { seen, menuPrevented } = clickAll()
    for (const s of seen) {
      const g = GESTURES.find((x) => x.name === s.gesture)!
      const want = g.target ? [Math.round(200 + (s.px / W) * 2800), g.target] : null
      expect(s.tuned, `${s.gesture} at ${s.px}`).toEqual(want)
      expect(s.prevented, `${s.gesture} at ${s.px}: default cancelled`).toBe(g.target !== null)
    }
    expect(menuPrevented, 'the right-click split gesture must never open a menu').toBe(true)
  })

  it('with a width at every size, every gesture at every pixel tunes exactly as the single line does', () => {
    const before = clickAll()
    for (const bw of WIDTHS) {
      const after = clickAll(bw)
      expect(after.seen, `markerWidthHz ${bw}`).toEqual(before.seen)
      expect(after.menuPrevented, `markerWidthHz ${bw}: context menu`).toBe(before.menuPrevented)
    }
  })

  it('a split set by clicking inside the TX bracket lands on the clicked hertz, not on the bracket', () => {
    // 118 px is inside FT8's TX bracket (1000–1043.75 Hz): a right-click there is a split to 1026 Hz.
    const { seen } = clickAll(43.75)
    const inside = seen.find((s) => s.gesture === 'right' && s.px === 118)
    expect(inside?.tuned).toEqual([1026, 'tx'])
    // …and a left-click in the RX bracket moves RX to the pixel, leaving TX to the caller.
    expect(seen.find((s) => s.gesture === 'left' && s.px === 187)?.tuned).toEqual([1509, 'rx'])
  })

  it('zoomed (a 600 Hz window around RX), a width still changes no click', () => {
    localStorage.setItem('nexus.waterfall.zoom', '600')
    const before = clickAll()
    // Pinned: the window is 1200–1800 Hz (zoomWindow around RX 1500), so x maps 1.5 Hz per px.
    const left57 = before.seen.find((s) => s.gesture === 'left' && s.px === 57)
    expect(left57?.tuned).toEqual([Math.round(1200 + (57 / W) * 600), 'rx'])
    for (const bw of WIDTHS) expect(clickAll(bw).seen, `zoomed, markerWidthHz ${bw}`).toEqual(before.seen)
  })
})
