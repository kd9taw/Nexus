// @vitest-environment jsdom
//
// THE FT WATERFALL'S MARKERS SHOW THE SIGNAL'S WIDTH, IN WSJT-X'S SHAPE (2026-09-27).
//
// Operator: "in wsjtx, when working in the waterfall, it has a width of the signal instead of a
// single line like today. I would like to implement the width lines, but change nothing on the
// right and left mouse clicks". WSJT-X 3.x draws each marker as two full-height lines, one at the
// marker's frequency (the signal's lowest tone) and one at its top tone (its "bars",
// plotter.cpp:656-685), and brackets the pair in the scale: TX red with the bar on top, RX green
// with the bar underneath (its "goal posts", plotter.cpp:636-643 and 687-705). Nexus draws the same
// two lines through the waterfall and closes each bracket at the waterfall's own edge: TX's at the
// top, RX's at the bottom, where their labels already sit.
//
// The recording-canvas technique is Waterfall.markers.test.tsx's, and it is faithful for the same
// reason: the overlay only WRITES to its context, so the component's own effect, rAF loop and
// drawOverlay run as shipped. The one addition is a known canvas box (jsdom lays nothing out), so a
// marker's pixels can be computed: a 1000 × 200 box at text scale 1 has an 18 px axis strip, so the
// waterfall is 182 px tall and the default Std view maps 200–3000 Hz across 1000 px.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { Waterfall } from './Waterfall'

// THE BUDGET (2026-10-09). The slowest case here, "zoomed: a marker half out of the window draws the half…", takes
// 0.39 s and 0.38 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', () => ({
  getSpectrumRow: () => new Promise(() => {}),
}))

type Call = { op: string; args: number[]; fillStyle: string; alpha: number }

function recordingCtx() {
  const calls: Call[] = []
  const state: Record<string, unknown> = {
    fillStyle: '#000000',
    strokeStyle: '#000000',
    globalAlpha: 1,
    font: '',
    lineWidth: 1,
    textAlign: 'left',
    textBaseline: 'alphabetic',
  }
  const ctx = new Proxy(state, {
    get(target, prop) {
      if (typeof prop !== 'string') return undefined
      if (prop in target) return target[prop]
      if (prop === 'measureText') return () => ({ width: 10 })
      if (prop === 'createLinearGradient') return () => ({ addColorStop() {} })
      return (...args: unknown[]) => {
        calls.push({ op: prop, args: args as number[], fillStyle: String(target.fillStyle), alpha: Number(target.globalAlpha) })
      }
    },
    set(target, prop, value) {
      if (typeof prop === 'string') target[prop] = value
      return true
    },
  })
  return { ctx, calls }
}

const TX = '#a1b2c3'
const RX = '#c3b2a1'
/** `--well-bg`: the axis strip's ground, and the keyline under each bracket's bar. */
const GROUND = '#010203'
/** The canvas box every test lays out: CSS px. */
const W = 1000
const H = 200
/** At text scale 1 the axis strip is 18 px (`axisHFor`), so the waterfall above it is 182. */
const WF_H = 182
/** FT8 and FT4, WSJT-X's widths (waterfall.markerwidth.test.ts carries the arithmetic). */
const FT8 = 43.75
const FT4 = 62.5

let overlay: ReturnType<typeof recordingCtx>
let realRaf: typeof requestAnimationFrame
let realCaf: typeof cancelAnimationFrame

beforeEach(() => {
  localStorage.clear()
  overlay = recordingCtx()
  // The overlay only. The picture is the spectrum renderer's, on canvases of its own, and they get
  // what jsdom gives every canvas (no context), so the renderer stands inert, as it does in jsdom.
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(function (this: HTMLCanvasElement) {
    return (this.classList.contains('waterfall-overlay') ? overlay.ctx : null) as unknown as CanvasRenderingContext2D
  } as unknown as typeof HTMLCanvasElement.prototype.getContext)
  // The one box the component measures (`resize` reads the waterfall canvas).
  vi.spyOn(HTMLCanvasElement.prototype, 'getBoundingClientRect').mockImplementation(
    () => ({ left: 0, top: 0, right: W, bottom: H, width: W, height: H, x: 0, y: 0, toJSON() {} }) as DOMRect,
  )
  window.matchMedia = ((q: string) =>
    ({ matches: false, media: q, addEventListener: () => {}, removeEventListener: () => {} }) as unknown as MediaQueryList) as typeof window.matchMedia
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  realRaf = globalThis.requestAnimationFrame
  realCaf = globalThis.cancelAnimationFrame
  globalThis.requestAnimationFrame = ((cb: FrameRequestCallback) =>
    setTimeout(() => cb(performance.now()), 16) as unknown as number) as typeof requestAnimationFrame
  globalThis.cancelAnimationFrame = ((id: number) => clearTimeout(id as unknown as NodeJS.Timeout)) as typeof cancelAnimationFrame
  for (const [k, v] of Object.entries({ '--tx': TX, '--rx': RX, '--well-bg': GROUND, '--well-ink': '#fefdfc' }))
    document.documentElement.style.setProperty(k, v)
})

afterEach(() => {
  cleanup()
  globalThis.requestAnimationFrame = realRaf
  globalThis.cancelAnimationFrame = realCaf
  document.documentElement.removeAttribute('style')
  localStorage.clear()
  vi.restoreAllMocks()
})

const frames = (ms: number) => act(() => new Promise((r) => setTimeout(r, ms)))

function lastFrame(): Call[] {
  const at = overlay.calls.map((c) => c.op).lastIndexOf('clearRect')
  return overlay.calls.slice(at)
}
const rects = (f: Call[], ink: string) => f.filter((c) => c.op === 'fillRect' && c.fillStyle === ink)
/** Full-height 2 px lines, left edge x. */
const lines = (f: Call[], ink: string, wfH = WF_H) => rects(f, ink).filter((c) => c.args[2] === 2 && c.args[3] === wfH).map((c) => c.args[0])
/** 2 px bars: [x, y, w]. */
const bars = (f: Call[], ink: string) => rects(f, ink).filter((c) => c.args[3] === 2).map((c) => [c.args[0], c.args[1], c.args[2]])
/** 1 px keylines in the well's ground: [x, y, w]. (The strip's own ground is AXIS_H tall.) */
const keys = (f: Call[]) => rects(f, GROUND).filter((c) => c.args[3] === 1).map((c) => [c.args[0], c.args[1], c.args[2]])
const label = (f: Call[], text: string) => f.find((c) => c.op === 'fillText' && (c.args as unknown[])[0] === text)

/** Where the Std view (200–3000 Hz across W) puts a frequency. */
const x = (hz: number, lo = 200, hi = 3000) => ((hz - lo) / (hi - lo)) * W

async function paint(props: Partial<React.ComponentProps<typeof Waterfall>> = {}) {
  const r = render(<Waterfall transmitting={false} rxOffsetHz={1500} txOffsetHz={1000} theme="dark" {...props} />)
  await frames(120)
  return r
}

describe('each marker spans the signal, WSJT-X style', () => {
  it('FT8: two lines 43.75 Hz apart for TX and for RX, TX bracketed on top, RX underneath', async () => {
    await paint({ markerWidthHz: FT8 })
    const f = lastFrame()
    expect(lines(f, TX), 'TX lines: at the marker and at its top tone').toEqual([x(1000) - 1, x(1000 + FT8) - 1])
    expect(lines(f, RX), 'RX lines').toEqual([x(1500) - 1, x(1500 + FT8) - 1])
    // The bar spans the gap between the two lines, so no pixel is painted twice.
    expect(bars(f, TX), 'TX bar, along the top edge').toEqual([[x(1000) + 1, 0, x(1000 + FT8) - x(1000) - 2]])
    expect(bars(f, RX), 'RX bar, along the bottom edge').toEqual([[x(1500) + 1, WF_H - 2, x(1500 + FT8) - x(1500) - 2]])
    // Each bar has a 1 px keyline of the well's ground on its inner edge, so it reads over a signal.
    expect(keys(f), 'keylines: under the TX bar, over the RX bar').toEqual([
      [x(1000) + 1, 2, x(1000 + FT8) - x(1000) - 2],
      [x(1500) + 1, WF_H - 3, x(1500 + FT8) - x(1500) - 2],
    ])
  })

  it('the width in pixels at a known scale: FT8 15.6 px and FT4 22.3 px across a 2800 Hz view on 1000 px', async () => {
    for (const [bw, px] of [
      [FT8, 15.625],
      [FT4, 22.321],
    ] as const) {
      overlay.calls.length = 0
      await paint({ markerWidthHz: bw })
      const f = lastFrame()
      for (const ink of [TX, RX]) {
        const [a, b] = lines(f, ink)
        expect(b - a, `${bw} Hz ${ink === TX ? 'TX' : 'RX'}`).toBeCloseTo(px, 3)
      }
      cleanup()
    }
  })

  it('the marker frequency stays exactly where the single line was: the LEFT line', async () => {
    await paint()
    const before = lastFrame()
    const txBefore = lines(before, TX)
    const rxBefore = lines(before, RX)
    expect(txBefore, 'CONTROL: no width is one TX line, as it always was').toHaveLength(1)
    expect(rxBefore, 'CONTROL: no width is one RX line').toHaveLength(1)
    cleanup()
    overlay.calls.length = 0
    await paint({ markerWidthHz: FT4 })
    const after = lastFrame()
    expect(lines(after, TX)[0], 'TX anchor moved').toBe(txBefore[0])
    expect(lines(after, RX)[0], 'RX anchor moved').toBe(rxBefore[0])
    expect(txBefore[0]).toBe(x(1000) - 1)
  })

  it('zoomed: the width follows the view, so a 600 Hz window draws FT8 72.9 px wide', async () => {
    localStorage.setItem('nexus.waterfall.zoom', '600') // a 600 Hz window around RX: 1200–1800
    await paint({ rxOffsetHz: 1500, txOffsetHz: 1400, markerWidthHz: FT8 })
    const f = lastFrame()
    const zx = (hz: number) => x(hz, 1200, 1800)
    expect(lines(f, TX)).toEqual([zx(1400) - 1, zx(1400 + FT8) - 1])
    expect(lines(f, RX)).toEqual([zx(1500) - 1, zx(1500 + FT8) - 1])
    const [a, b] = lines(f, RX)
    expect(b - a).toBeCloseTo(72.917, 3)
  })

  it('zoomed: a marker half out of the window draws the half that is in, and never pins a line to the edge', async () => {
    localStorage.setItem('nexus.waterfall.zoom', '600')
    const zx = (hz: number) => x(hz, 1200, 1800)
    // Marker in view, top tone past the right edge: its line, then the bar runs off the edge.
    await paint({ rxOffsetHz: 1500, txOffsetHz: 1790, markerWidthHz: FT8 })
    let f = lastFrame()
    expect(lines(f, TX), 'only the marker line is in view').toEqual([zx(1790) - 1])
    expect(bars(f, TX)).toEqual([[zx(1790) + 1, 0, W - zx(1790) - 1]])
    expect(keys(f).filter((k) => k[1] === 2), 'the TX keyline follows its bar').toEqual([[zx(1790) + 1, 2, W - zx(1790) - 1]])
    cleanup()
    overlay.calls.length = 0
    // Marker just below the window, top tone inside it: the top line, and the bar from the edge.
    await paint({ rxOffsetHz: 1500, txOffsetHz: 1180, markerWidthHz: FT8 })
    f = lastFrame()
    expect(lines(f, TX), 'only the top-tone line is in view').toEqual([zx(1180 + FT8) - 1])
    expect(bars(f, TX)).toEqual([[0, 0, zx(1180 + FT8) - 1]])
    expect(label(f, 'TX'), 'a visible marker keeps its label').toBeDefined()
    cleanup()
    overlay.calls.length = 0
    // Wholly outside: nothing, exactly as a single line outside the window drew nothing.
    await paint({ rxOffsetHz: 1500, txOffsetHz: 1100, markerWidthHz: FT8 })
    f = lastFrame()
    expect(rects(f, TX), 'a marker out of view drew something').toEqual([])
    expect(label(f, 'TX')).toBeUndefined()
  })

  it('the TX marker still brightens while keyed, by alpha, lines and bar alike; RX keeps its alpha', async () => {
    const idle = await paint({ markerWidthHz: FT8 })
    const quiet = rects(lastFrame(), TX).map((c) => c.alpha)
    idle.rerender(<Waterfall transmitting rxOffsetHz={1500} txOffsetHz={1000} theme="dark" markerWidthHz={FT8} />)
    await frames(120)
    const keyed = rects(lastFrame(), TX).map((c) => c.alpha)
    expect(quiet).toHaveLength(3)
    expect(new Set(quiet), 'idle TX alpha').toEqual(new Set([0.7]))
    expect(new Set(keyed), 'keyed TX alpha').toEqual(new Set([0.95]))
    expect(new Set(rects(lastFrame(), RX).map((c) => c.alpha)), 'RX alpha').toEqual(new Set([0.9]))
  })

  it('labels sit just right of the marker: after its top-tone line, and unchanged with no width', async () => {
    await paint({ markerWidthHz: FT8 })
    let f = lastFrame()
    expect(label(f, 'TX')?.args.slice(1)).toEqual([x(1000 + FT8) + 3, 9])
    expect(label(f, 'RX')?.args.slice(1)).toEqual([x(1500 + FT8) + 3, WF_H - 6])
    cleanup()
    overlay.calls.length = 0
    await paint()
    f = lastFrame()
    expect(label(f, 'TX')?.args.slice(1), 'the single line keeps its old label spot').toEqual([x(1000) + 3, 9])
    expect(label(f, 'RX')?.args.slice(1)).toEqual([x(1500) + 3, WF_H - 6])
  })

  it('every UI scale and pixel density: the bracket is laid out in CSS px, only the axis strip grows', async () => {
    // UI zoom 1.25 on Chromium (rect 1000, layout 800: `overlayTextScale`) at 2 device px per px.
    vi.spyOn(HTMLElement.prototype, 'offsetWidth', 'get').mockImplementation(function (this: HTMLElement) {
      return this.classList.contains('waterfall-canvas') ? 800 : 0
    })
    const dpr = Object.getOwnPropertyDescriptor(window, 'devicePixelRatio')
    Object.defineProperty(window, 'devicePixelRatio', { configurable: true, value: 2 })
    try {
      await paint({ markerWidthHz: FT8 })
      const f = lastFrame()
      const wfH = H - Math.round(18 * 1.25) // the strip follows the text size: 23 px
      expect(overlay.calls.find((c) => c.op === 'setTransform')?.args, 'device px per CSS px').toEqual([2, 0, 0, 2, 0, 0])
      expect(lines(f, TX, wfH)).toEqual([x(1000) - 1, x(1000 + FT8) - 1])
      expect(lines(f, RX, wfH)).toEqual([x(1500) - 1, x(1500 + FT8) - 1])
      expect(bars(f, RX), 'the RX bar moves up with the taller strip').toEqual([[x(1500) + 1, wfH - 2, x(1500 + FT8) - x(1500) - 2]])
      expect(keys(f).map((k) => k[1]), 'keylines: under the TX bar, over the RX bar').toEqual([2, wfH - 3])
    } finally {
      if (dpr) Object.defineProperty(window, 'devicePixelRatio', dpr)
      else delete (window as { devicePixelRatio?: number }).devicePixelRatio
    }
  })
})

describe('what does not change', () => {
  it('no width, null or 0 draws the single line and nothing else', async () => {
    for (const markerWidthHz of [undefined, null, 0]) {
      overlay.calls.length = 0
      await paint({ markerWidthHz })
      const f = lastFrame()
      expect(lines(f, TX), `${markerWidthHz}: TX`).toEqual([x(1000) - 1])
      expect(lines(f, RX), `${markerWidthHz}: RX`).toEqual([x(1500) - 1])
      expect([...bars(f, TX), ...bars(f, RX)], `${markerWidthHz}: a bar with no width`).toEqual([])
      expect(keys(f), `${markerWidthHz}: a keyline with no width`).toEqual([])
      cleanup()
    }
  })

  it('named cursors (RTTY mark/space) still replace the markers, each a single line, width or not', async () => {
    await paint({
      markerWidthHz: FT8,
      cursors: [
        { hz: 1500, color: 'var(--rx)', label: 'M' },
        { hz: 1670, color: 'var(--tx)', label: 'S' },
      ],
    })
    const f = lastFrame()
    expect(lines(f, RX)).toEqual([x(1500) - 1])
    expect(lines(f, TX)).toEqual([x(1670) - 1])
    expect([...bars(f, TX), ...bars(f, RX)]).toEqual([])
    expect(keys(f)).toEqual([])
    expect(label(f, 'TX'), 'no TX marker behind the cursors').toBeUndefined()
  })
})
