// @vitest-environment jsdom
//
// JS8'S WATERFALL DARKENS THE AUDIO OUTSIDE ITS DECODE FILTER, AS JS8CALL'S DOES (2026-10-16), AND NO
// OTHER MODE'S WATERFALL CHANGES.
//
// JS8Call 3.0.3 paints its filter over the whole wide graph: black at `m_filterOpacity` on each side
// of centre ± width/2, and a yellow line at the edge (`CPlotter::drawFilter`, CPlotter.cpp:468-503,
// painted last, :150-154). The strength is the "Filter Opacity" spin box's 50 %, turned into 127 of
// 255 (`int(0.5 * 255)`, WideGraph.cpp:239-240 and :616-618; CPlotter.h:207 starts at 127).
//
// The recording-canvas technique is Waterfall.markers.test.tsx's (the overlay only WRITES to its
// context, so the component's own effect, rAF loop and drawOverlay run as shipped), with a known box:
// 1000 × 200 CSS px at text scale 1, an 18 px axis strip, and the default view 200–3000 Hz.
//
// THE OTHER MODES: `__fixtures__/waterfall-overlay-without-shade.json` is every overlay call FT8's,
// FT4's (keyed), a single-line and RTTY's waterfall drew on the tree BEFORE the shade existed,
// recorded by this harness. Each must still draw exactly that, call for call. `clearRect` keeps no
// paint state, and a font is recorded only where text is drawn: the context's leftover state from an
// earlier frame is not part of what a frame draws.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { Waterfall } from './Waterfall'
import before from './__fixtures__/waterfall-overlay-without-shade.json'

// The house budget for a canvas test under a loaded full suite (Waterfall.width.test.tsx).
vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', () => ({
  getSpectrumRow: () => new Promise(() => {}),
}))

type Call = { op: string; args: unknown[]; fillStyle: string; alpha: number; font: string }

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
        calls.push({ op: prop, args, fillStyle: String(target.fillStyle), alpha: Number(target.globalAlpha), font: String(target.font) })
      }
    },
    set(target, prop, value) {
      if (typeof prop === 'string') target[prop] = value
      return true
    },
  })
  return { ctx, calls }
}

const W = 1000
const H = 200
/** JS8Call's 50 %: 127 of 255. */
const STRENGTH = 127 / 255
const YELLOW = '#ffff00'

let overlay: ReturnType<typeof recordingCtx>
let realRaf: typeof requestAnimationFrame
let realCaf: typeof cancelAnimationFrame

beforeEach(() => {
  localStorage.clear()
  overlay = recordingCtx()
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(function (this: HTMLCanvasElement) {
    return (this.classList.contains('waterfall-overlay') ? overlay.ctx : null) as unknown as CanvasRenderingContext2D
  } as unknown as typeof HTMLCanvasElement.prototype.getContext)
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
  for (const [k, v] of Object.entries({ '--tx': '#a1b2c3', '--rx': '#c3b2a1', '--well-bg': '#010203', '--well-ink': '#fefdfc', '--mark': '#123456' }))
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

async function paint(props: Partial<React.ComponentProps<typeof Waterfall>> = {}) {
  overlay.calls.length = 0
  const r = render(<Waterfall transmitting={false} rxOffsetHz={1500} txOffsetHz={1000} theme="dark" {...props} />)
  await frames(120)
  return r
}

/** Where the default view (200–3000 Hz across W) puts a frequency. */
const x = (hz: number) => ((hz - 200) / 2800) * W
/** The shade's rectangles: [x, y, w, h]. */
const shades = (f: Call[]) =>
  f.filter((c) => c.op === 'fillRect' && c.fillStyle === '#000000' && c.alpha === STRENGTH).map((c) => c.args as number[])
/** The edge lines' left x, full height and 1 px wide. */
const edges = (f: Call[]) =>
  f.filter((c) => c.op === 'fillRect' && c.fillStyle === YELLOW).map((c) => c.args as number[])

describe('JS8: the audio outside the decode filter is darkened, at JS8Call’s strength', () => {
  it('JS8Call’s default filter (1500 Hz, 2000 wide): both sides darkened at 127 of 255, top to bottom, a yellow line at each edge', async () => {
    await paint({ shade: { loHz: 500, hiHz: 2500 } })
    const f = lastFrame()
    expect(shades(f)).toEqual([
      [0, 0, x(500), H],
      [x(2500), 0, W - x(2500), H],
    ])
    expect(edges(f)).toEqual([
      [x(500), 0, 1, H],
      [x(2500), 0, 1, H],
    ])
    // Over everything else, as JS8Call paints its filter last: the RX/TX markers and the axis first.
    const lastMarker = f.map((c) => c.op === 'fillText' && ['TX', 'RX'].includes(String(c.args[0]))).lastIndexOf(true)
    const firstShade = f.findIndex((c) => c.op === 'fillRect' && c.alpha === STRENGTH)
    expect(lastMarker, 'the markers are drawn').toBeGreaterThan(0)
    expect(firstShade, 'the shade comes after them').toBeGreaterThan(lastMarker)
  })

  it('an edge outside the view darkens nothing on that side, and draws no line', async () => {
    await paint({ shade: { loHz: 100, hiHz: 2900 } })
    const f = lastFrame()
    expect(shades(f)).toEqual([[x(2900), 0, W - x(2900), H]])
    expect(edges(f)).toEqual([[x(2900), 0, 1, H]])
    cleanup()
    await paint({ shade: { loHz: 600, hiHz: 3500 } })
    const g = lastFrame()
    expect(shades(g)).toEqual([[0, 0, x(600), H]])
    expect(edges(g)).toEqual([[x(600), 0, 1, H]])
    cleanup()
    // A filter wholly above the view leaves nothing in view that is decoded: all of it darkened.
    await paint({ shade: { loHz: 3200, hiHz: 3800 } })
    expect(shades(lastFrame())).toEqual([[0, 0, W, H]])
    expect(edges(lastFrame())).toEqual([])
  })

  it('with the filter off (no shade) nothing is darkened', async () => {
    await paint({ shade: null })
    expect(shades(lastFrame())).toEqual([])
    expect(edges(lastFrame())).toEqual([])
  })
})

describe('every other mode’s waterfall draws exactly as it did before', () => {
  /** A frame as the fixture holds it. */
  const asRecorded = (f: Call[]) =>
    f.map((c) => (c.op === 'clearRect' ? [c.op, c.args, null, null, null] : [c.op, c.args, c.fillStyle, c.alpha, c.op === 'fillText' ? c.font : null]))
  const cases: [keyof typeof before, Partial<React.ComponentProps<typeof Waterfall>>][] = [
    ['ft8', { markerWidthHz: 43.75 }],
    ['ft4Transmitting', { markerWidthHz: 62.5, transmitting: true }],
    ['singleLine', {}],
    [
      'rtty',
      {
        cursors: [
          { hz: 915, color: 'var(--mark)', label: 'M' },
          { hz: 1085, color: '#00ff00', label: 'S' },
        ],
      },
    ],
  ]
  it.each(cases)('%s: call for call', async (name, props) => {
    await paint(props)
    const f = lastFrame()
    expect(f.length, 'CONTROL: a frame was drawn').toBeGreaterThan(10)
    expect(asRecorded(f)).toEqual(before[name])
  })
})
