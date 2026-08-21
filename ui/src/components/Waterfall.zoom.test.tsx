// @vitest-environment jsdom
//
// The waterfall's zoom WINDOW follows the RX marker — issue #115 (akhepcat), "waterfall
// X-axis labels incorrect when switching bandpass".
//
// THE MECHANISM. A zoom pick is a span, not a window: `zoomRange(rxOffsetHz, span)` returns a
// span-wide window CENTRED ON THE RX MARKER, and the axis labels are step-aligned multiples
// inside those edges. So the labels were never computed wrongly — the WINDOW was stale. It
// used to be assigned in exactly two places, the zoom <select>'s onChange and a `useState`
// INITIALISER, and nothing in between: a persisted zoom therefore centred on whatever
// `rxOffsetHz` was at FIRST RENDER — 0 before the first snapshot lands, or a cockpit's
// not-yet-netted centre — and stayed pinned there for the life of the mount. The operator
// listening at 2500 Hz read an axis labelled 200–800.
//
// WHAT THIS FILE CAN AND CANNOT PROVE. jsdom has no 2D canvas — `getContext('2d')` returns
// null and the render effect bails before it draws a pixel — so the tick labels themselves
// are not observable here (the same limit `Waterfall.flow.test.tsx` documents). What IS
// observable is the window they are drawn from: the axis reads `viewLoRef`/`viewHiRef` and
// the click handler reads `view.lo`/`view.hi`, and BOTH are assigned from the same `view` in
// the render body — so a click mapped back through the canvas answers exactly the window the
// axis would have labelled. The tick arithmetic itself (`Math.ceil(lo/step)*step`) is
// deliberately not under test: it was never the defect.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, fireEvent, cleanup } from '@testing-library/react'
import { Waterfall } from './Waterfall'
import { WF_F_MIN, WF_F_MAX, WF_STD_HI } from '../waterfall'

vi.mock('../api', () => ({
  getSpectrumRow: () => Promise.resolve({ row: [], loHz: 200, hiHz: 4000 }),
}))

const ZOOM_KEY = 'nexus.waterfall.zoom'
/** CSS width the click mapping is stubbed at — a click at 0 reads `lo`, at W reads `hi`. */
const W = 400

beforeEach(() => {
  localStorage.clear()
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(() => cleanup())

function mount(rxOffsetHz: number) {
  const onTune = vi.fn()
  const view = (hz: number) => (
    <Waterfall
      transmitting={false}
      rxOffsetHz={hz}
      txOffsetHz={1500}
      theme="dark"
      onTune={onTune}
    />
  )
  const utils = render(view(rxOffsetHz))
  const canvas = utils.container.querySelector('canvas.waterfall-canvas') as HTMLCanvasElement
  // jsdom lays nothing out (every rect is zeros), and the click→Hz mapping divides by the
  // rect width. Stub the ONE element the handler measures; the stub outlives a re-render
  // because React keeps the same node.
  canvas.getBoundingClientRect = () =>
    ({ left: 0, top: 0, right: W, bottom: 200, width: W, height: 200, x: 0, y: 0 }) as DOMRect
  return { canvas, onTune, retune: (hz: number) => utils.rerender(view(hz)) }
}

/** The [lo, hi] the component is mapping RIGHT NOW, read back through a click at each edge
 *  of the canvas — i.e. the same `view` the axis labels its ticks from. */
function windowOf(canvas: HTMLCanvasElement, onTune: ReturnType<typeof vi.fn>) {
  onTune.mockClear()
  fireEvent.mouseDown(canvas, { button: 0, clientX: 0 }) // left edge  → view.lo
  fireEvent.mouseDown(canvas, { button: 0, clientX: W }) // right edge → view.hi
  return { lo: onTune.mock.calls[0][0] as number, hi: onTune.mock.calls[1][0] as number }
}

describe('waterfall zoom window (issue #115)', () => {
  it('a persisted zoom re-centres when the RX offset arrives, instead of freezing at mount', () => {
    localStorage.setItem(ZOOM_KEY, '600')
    // Mount the way App does before the first snapshot: no RX offset yet.
    const { canvas, onTune, retune } = mount(0)
    expect(windowOf(canvas, onTune)).toEqual({ lo: WF_F_MIN, hi: WF_F_MIN + 600 })

    // The snapshot lands and the operator is listening at 2500 Hz. THE DEFECT: the window
    // stayed at 200–800, so the axis labelled 200/400/600/800 under a picture of 2200–2800.
    retune(2500)
    expect(windowOf(canvas, onTune)).toEqual({ lo: 2200, hi: 2800 })
  })

  it('keeps the RX marker inside the window wherever it moves, at full span', () => {
    localStorage.setItem(ZOOM_KEY, '600')
    const { canvas, onTune, retune } = mount(1500)
    for (const rx of [1500, 2500, 250, 3950, 1000]) {
      retune(rx)
      const { lo, hi } = windowOf(canvas, onTune)
      expect(hi - lo, `span at ${rx}`).toBe(600) // clamped at an edge, never shrunk
      expect(lo, `lo at ${rx}`).toBeLessThanOrEqual(rx)
      expect(hi, `hi at ${rx}`).toBeGreaterThanOrEqual(rx)
    }
  })

  it('Std and Full are FIXED windows and do NOT chase the marker', () => {
    // The other direction of the guard: `zoomRange` treats 0/-1 as fixed windows, so
    // re-deriving on every RX move must be a no-op for the two views most operators run.
    for (const [span, lo, hi] of [
      ['0', WF_F_MIN, WF_STD_HI],
      ['-1', WF_F_MIN, WF_F_MAX],
    ] as const) {
      localStorage.setItem(ZOOM_KEY, span)
      const { canvas, onTune, retune } = mount(1500)
      expect(windowOf(canvas, onTune), `span ${span} at mount`).toEqual({ lo, hi })
      retune(3200)
      expect(windowOf(canvas, onTune), `span ${span} after retune`).toEqual({ lo, hi })
      cleanup()
    }
  })
})
