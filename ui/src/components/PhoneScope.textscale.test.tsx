// @vitest-environment jsdom
//
// #215 — THE PHONE/CW SCOPE'S OVERLAY TEXT FOLLOWS THE UI SCALE.
//
// 1.13.0 made the Operate waterfall's axis digits and RX/TX labels grow with the UI scale
// (`overlayTextScale`, `waterfall.overlayscale.test.ts`). The Phone/CW scope draws the same
// class of text — the DIAL plates and the frequency scale an operator reads to see where a
// click will land — and was not swept: its font sizes are `N * scaleY`, and `scaleY` is the
// DEVICE-PIXEL ratio, not the zoom. On Chromium/WebView2 both the rect and the device-pixel
// box are measured post-zoom, so the zoom cancels out of `dH / cssH` and a `10 * scaleY`
// label stays the same physical size at 175 % as at 100 % while the scope around it grows.
// That is exactly the complaint on this issue ("all fonts bigger"; "the frequency digits get
// lost"), on the cockpit the reporter of the scope's own axis was using.
//
// WHY THIS TEST CAN EXIST: jsdom has no 2D canvas, so the draw path is reached only through a
// recording fake — the same device `PhoneScope.draw.test.tsx` uses and explains. What is
// measured is real: the component's own effect, its own rAF loop, its own draw.
//
// WHAT IT CANNOT SEE: nothing here is a pixel measurement. The claim "the digits are legible
// at 125 %" is geometry and jsdom never lays out; this asserts only that the font the draw
// path hands the canvas carries the zoom, which is the thing that was missing.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { PhoneScope } from './PhoneScope'

// THE BUDGET (2026-10-09). The slowest case here, "the DIAL plate is drawn 1.5× larger when the UI is…", takes 0.64 s
// and 0.64 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than one
// core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const ROW = Array.from({ length: 512 }, (_, i) => (i === 200 ? 0.9 : 0.1))

let rowsServed = 0
let rowShape: { loHz: number; hiHz: number; source: string } = { loHz: 0, hiHz: 4000, source: 'rx' }
vi.mock('../api', () => ({
  getScopeFrame: () => {
    rowsServed++
    const rf = rowShape.source === 'civ'
    return Promise.resolve({
      seq: rowsServed,
      tMs: Date.now(),
      bins: ROW,
      scale: rf ? { kind: 'relative' } : { kind: 'dbfs', loDb: -120, hiDb: 0 },
      ...rowShape,
    })
  },
}))

/** One recorded text draw: what was written, and the font it was written in. */
type TextOp = { text: string; font: string }
let texts: TextOp[]

function recordingCtx() {
  const ops: TextOp[] = []
  const ctx = {
    fillStyle: '',
    strokeStyle: '',
    lineWidth: 1,
    font: '',
    textAlign: 'left',
    textBaseline: 'alphabetic',
    createLinearGradient: () => ({ addColorStop: () => {} }),
    fillRect: () => {},
    clearRect: () => {},
    putImageData: () => {},
    beginPath: () => {},
    closePath: () => {},
    moveTo: () => {},
    lineTo: () => {},
    // The font is read AT THE CALL, which is the whole point: an assertion on `ctx.font`
    // after the fact reads whatever the last draw happened to leave there.
    fillText(text: string) {
      ops.push({ text, font: ctx.font })
    },
    measureText: (t: string) => ({ width: t.length * 6 }),
    fill: () => {},
    stroke: () => {},
    setLineDash: () => {},
    // The renderer's trace is clipped to its band.
    save: () => {},
    restore: () => {},
    rect: () => {},
    clip: () => {},
  }
  return { ctx, ops }
}

/** The LAYOUT box of the scope, in unzoomed CSS px — `offsetWidth`, which CSS `zoom` does not
 *  scale. The rect the browser reports is this times the zoom (Chromium), which is the pair
 *  `overlayTextScale` divides. */
const LAYOUT_W = 200
const LAYOUT_H = 100

/** Mount the scope as a browser at `zoom` would present it. */
function stubGeometry(zoom: number) {
  const w = LAYOUT_W * zoom
  const h = LAYOUT_H * zoom
  vi.spyOn(HTMLCanvasElement.prototype, 'getBoundingClientRect').mockReturnValue({
    x: 0, y: 0, width: w, height: h, top: 0, left: 0, right: w, bottom: h,
    toJSON: () => ({}),
  } as DOMRect)
  vi.spyOn(HTMLElement.prototype, 'offsetWidth', 'get').mockReturnValue(LAYOUT_W)
}

let realRaf: typeof requestAnimationFrame
let realCaf: typeof cancelAnimationFrame

/** A 2D paint target only: `webgl2` answers null, so the renderer draws on its canvas-2D path. */
function paintOn(ctx: object) {
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(((kind: string) =>
    kind === '2d' ? ctx : null) as unknown as HTMLCanvasElement['getContext'])
}

beforeEach(() => {
  rowsServed = 0
  rowShape = { loHz: 0, hiHz: 4000, source: 'rx' }
  const rec = recordingCtx()
  texts = rec.ops
  paintOn(rec.ctx)
  // Both of ImageData's forms: (width, height), which the renderer's band takes, and (data, w, h).
  globalThis.ImageData = class {
    data: Uint8ClampedArray
    width: number
    height: number
    constructor(a: Uint8ClampedArray | number, b: number, c?: number) {
      if (typeof a === 'number') {
        this.width = a
        this.height = b
        this.data = new Uint8ClampedArray(a * b * 4)
      } else {
        this.data = a
        this.width = b
        this.height = c ?? a.length / 4 / b
      }
    }
  } as unknown as typeof ImageData
  window.matchMedia = ((q: string) =>
    ({
      matches: false,
      media: q,
      addEventListener: () => {},
      removeEventListener: () => {},
    }) as unknown as MediaQueryList) as typeof window.matchMedia
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  realRaf = globalThis.requestAnimationFrame
  realCaf = globalThis.cancelAnimationFrame
  globalThis.requestAnimationFrame = ((cb: FrameRequestCallback) =>
    setTimeout(() => cb(performance.now()), 16) as unknown as number) as typeof requestAnimationFrame
  globalThis.cancelAnimationFrame = ((id: number) =>
    clearTimeout(id as unknown as NodeJS.Timeout)) as typeof cancelAnimationFrame
})

afterEach(() => {
  cleanup()
  globalThis.requestAnimationFrame = realRaf
  globalThis.cancelAnimationFrame = realCaf
  vi.restoreAllMocks()
})

const runFrames = (ms: number) => new Promise((r) => setTimeout(r, ms))

/** Font size in px parsed out of a canvas font shorthand. */
function px(font: string): number {
  const m = /(\d+(?:\.\d+)?)px/.exec(font)
  if (!m) throw new Error(`no px size in font ${JSON.stringify(font)}`)
  return Number(m[1])
}

/** The DIAL plate's font size, with the control that it was drawn at all. */
async function dialPlatePx(): Promise<number> {
  await runFrames(300)
  expect(rowsServed, 'control: the draw loop never ran, so nothing below is a measurement')
    .toBeGreaterThanOrEqual(3)
  const plate = texts.find((o) => o.text === 'DIAL')
  expect(plate, 'control: no DIAL plate was drawn').toBeDefined()
  return px(plate!.font)
}

describe('#215 the scope’s overlay text follows the UI scale', () => {
  it('the DIAL plate is drawn 1.5× larger when the UI is scaled 1.5×', async () => {
    stubGeometry(1)
    render(
      <PhoneScope transmitting={false} theme="dark" viewLoHz={0} viewHiHz={4000} carrierCentered />,
    )
    const at100 = await dialPlatePx()

    cleanup()
    vi.restoreAllMocks()
    const rec = recordingCtx()
    texts = rec.ops
    rowsServed = 0
    paintOn(rec.ctx)
    stubGeometry(1.5)
    render(
      <PhoneScope transmitting={false} theme="dark" viewLoHz={0} viewHiHz={4000} carrierCentered />,
    )
    const at150 = await dialPlatePx()

    // THE MEASUREMENT. Before the fix both reads were the same number — the font was
    // `10 * scaleY` and the zoom cancels out of `scaleY`, which is the defect.
    expect(at100, 'the 100 % plate is the size that shipped').toBe(10)
    expect(at150, 'the plate did not grow with the UI scale').toBe(15)
  })

  it('so do the frequency-scale digits on a native RF panadapter', async () => {
    // The other two draw sites: the RF dial plate and the axis labels an operator reads to
    // see where a click lands ("a bit difficult to see where a mouse click will take you",
    // operator 2026-08-22) — the scope's equivalent of the waterfall digits this issue's
    // Discord thread asked for.
    rowShape = { loHz: 14_000_000, hiHz: 14_200_000, source: 'civ' }
    stubGeometry(1.5)
    render(
      <PhoneScope
        transmitting={false}
        theme="dark"
        viewLoHz={0}
        viewHiHz={4000}
        dialHz={14_050_000}
      />,
    )
    await runFrames(300)

    expect(rowsServed, 'control: the draw loop never ran').toBeGreaterThanOrEqual(3)
    const digits = texts.filter((o) => /^\d+\.\d{3}$/.test(o.text))
    expect(digits.length, 'control: no frequency-scale labels were drawn at all')
      .toBeGreaterThan(0)
    // 9 px base × 1.5 = 13.5, rounded — the shipped expression with the zoom in it.
    expect(px(digits[0].font), 'the axis digits did not grow with the UI scale').toBe(14)
  })

  it('CONTROL: an unmeasured canvas draws the sizes that shipped, not a guess', async () => {
    // `overlayTextScale` reads 1 when it cannot measure (hidden, mid-layout, or an engine
    // that reports the unzoomed box and therefore already carries the zoom in its own
    // transform). Without this, a formula that silently inflated every unmeasured canvas
    // would pass the two tests above.
    vi.spyOn(HTMLCanvasElement.prototype, 'getBoundingClientRect').mockReturnValue({
      x: 0, y: 0, width: LAYOUT_W, height: LAYOUT_H, top: 0, left: 0,
      right: LAYOUT_W, bottom: LAYOUT_H, toJSON: () => ({}),
    } as DOMRect)
    // offsetWidth deliberately NOT stubbed: jsdom reports 0, the unmeasured case.
    render(
      <PhoneScope transmitting={false} theme="dark" viewLoHz={0} viewHiHz={4000} carrierCentered />,
    )
    expect(await dialPlatePx(), 'an unmeasured canvas must draw exactly what it did before')
      .toBe(10)
  })
})
