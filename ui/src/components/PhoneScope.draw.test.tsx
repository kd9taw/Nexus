// @vitest-environment jsdom
//
// The rig scope's per-row allocations (2026-08-15).
//
// The operator's complaint about the CW/Phone scope was "basic, lagging, not crisp". Part of
// that is work done PER ROW that only changes when the palette or the geometry does: a
// `createLinearGradient` plus three `sampleLut` calls and three colour-stop parses, 20 times a
// second, forever, for a value with two inputs.
//
// WHY THIS TEST CAN EXIST AT ALL. jsdom has no 2D canvas, so every other PhoneScope test stops
// at `getContext('2d') === null` before the draw path runs (see Waterfall.flow.test.tsx, which
// says so). But PhoneScope's context surface is thirteen methods wide and it reads none of them
// back, so a hand-written recording fake is a faithful stand-in — not a mock of the thing under
// test, just of the paint target. What is measured here is real: the component's own effect,
// its own rAF loop, its own drawRow.
//
// Geometry in jsdom is 1x1 (getBoundingClientRect is all zeros, clamped to 1), which is exactly
// why this asserts a RATIO and never a duration or a pixel. The carrier-axis block at the bottom
// is the one exception: it stubs a 200×100 rect FOR ITS OWN TESTS ONLY, because "the dial is at
// the middle pixel" is not a statement you can make about a canvas one pixel wide.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { PhoneScope } from './PhoneScope'
import { TRACE_HOLD_MS } from '../waterfall'
import { WaterfallHistory } from '../waterfallHistory'

/** A 512-bin row with one carrier, shaped like what the engine publishes. */
const ROW = Array.from({ length: 512 }, (_, i) => (i === 200 ? 0.9 : 0.1))

let rowsServed = 0
let spanAsked: [number, number] | null = null
vi.mock('../api', () => ({
  getScopeRow: (_tx: boolean, loHz: number, hiHz: number) => {
    rowsServed++
    spanAsked = [loHz, hiHz]
    // Answer WIDER than asked — the real backend's refusal path (native RF live, span not a
    // sane audio window, narrow row not produced yet). The row states its own extent, and
    // that extent is what everything downstream must read.
    return Promise.resolve({ row: ROW, loHz: 0, hiHz: 4000, source: 'rx' })
  },
}))

/** One recorded path op — the method and the coordinates it was handed. */
type Op = { op: string; args: number[] }

/** Records the calls PhoneScope makes; every method it uses is listed explicitly.
 *  The path methods record their ARGUMENTS too: they used to be no-ops, which made every
 *  coordinate the draw path computes — including a NaN — invisible to this suite. */
function recordingCtx() {
  const calls: Record<string, number> = {}
  const ops: Op[] = []
  const bump = (k: string) => {
    calls[k] = (calls[k] ?? 0) + 1
  }
  const rec = (op: string, ...args: number[]) => {
    bump(op)
    ops.push({ op, args })
  }
  const ctx = {
    fillStyle: '',
    strokeStyle: '',
    lineWidth: 1,
    font: '',
    textAlign: 'left',
    textBaseline: 'alphabetic',
    createLinearGradient: () => {
      bump('createLinearGradient')
      return { addColorStop: () => {} }
    },
    fillRect: () => bump('fillRect'),
    putImageData: () => bump('putImageData'),
    beginPath: () => bump('beginPath'),
    closePath: () => bump('closePath'),
    moveTo: (x: number, y: number) => rec('moveTo', x, y),
    lineTo: (x: number, y: number) => rec('lineTo', x, y),
    fillText: (_t: string, x: number, y: number) => rec('fillText', x, y),
    fill: () => bump('fill'),
    stroke: () => bump('stroke'),
    setLineDash: () => {},
  }
  return { ctx, calls, ops }
}

let calls: Record<string, number>
let ops: Op[]
let realRaf: typeof requestAnimationFrame
let realCaf: typeof cancelAnimationFrame

beforeEach(() => {
  rowsServed = 0
  const rec = recordingCtx()
  calls = rec.calls
  ops = rec.ops
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(
    rec.ctx as unknown as CanvasRenderingContext2D,
  )
  // jsdom ships no ImageData constructor; the retained waterfall buffer needs one.
  globalThis.ImageData = class {
    data: Uint8ClampedArray
    width: number
    height: number
    constructor(d: Uint8ClampedArray, w: number, h: number) {
      this.data = d
      this.width = w
      this.height = h
    }
  } as unknown as typeof ImageData
  // jsdom implements neither of these; the component reads both on mount.
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
  // ⚠️ jsdom's rAF stamps its callback from the WINDOW's time origin, while the component's
  // accumulator is seeded from `performance.now()` — Node's, ~800 ms further along by the time
  // a test mounts. The first `now - last` is then hugely negative and the loop draws nothing
  // for the best part of a second, which is a jsdom artifact and not something the app can
  // ever see: in a browser a DOMHighResTimeStamp IS on the performance.now clock. Restore that
  // one property and the real loop runs as shipped.
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

/** Let the real rAF loop run long enough to draw several rows (ROW_MS = 50). */
const runFrames = (ms: number) => new Promise((r) => setTimeout(r, ms))

describe('rig scope per-row work', () => {
  it('builds the trace gradient on change, not on every row', async () => {
    render(<PhoneScope transmitting={false} theme="dark" traceHoldMs={TRACE_HOLD_MS.fast} />)
    await runFrames(300)

    // POSITIVE CONTROL FIRST. Without this the assertion below passes trivially when the draw
    // path never ran — which is the default outcome in jsdom and the whole reason this comment
    // block exists. Rows must actually have been drawn for a per-row count to mean anything.
    expect(rowsServed, 'control: the draw loop never ran, so nothing below is a measurement')
      .toBeGreaterThanOrEqual(3)
    expect(calls.fill, 'control: drawRow did not reach the trace fill').toBeGreaterThanOrEqual(3)

    // The measurement: one gradient for the whole run, no matter how many rows were drawn.
    expect(calls.createLinearGradient).toBe(1)
  })

  it('stores the ROW’s own bins and span in history, not screen pixels over the view', async () => {
    // THE DEFECT: the row was interpolated to device width and pushed with the VIEW's span, so
    // history was clipped to whatever was on screen when each row arrived — permanently. The
    // scope has pause + wheel-scrollback, and scrollback could therefore never widen: the data
    // outside the view was not merely hidden, it was never stored.
    const pushes: Array<{ n: number; lo: number; hi: number }> = []
    vi.spyOn(WaterfallHistory.prototype, 'push').mockImplementation(function (row, loHz, hiHz) {
      pushes.push({ n: row.length, lo: loHz, hi: hiHz })
    })
    render(<PhoneScope transmitting={false} theme="dark" viewLoHz={300} viewHiHz={1100} />)
    await runFrames(300)

    expect(pushes.length, 'control: no rows reached history').toBeGreaterThanOrEqual(3)
    const last = pushes[pushes.length - 1]
    expect(last.n, 'the row’s own bins, not one value per device column').toBe(ROW.length)
    expect(
      [last.lo, last.hi],
      'the ROW’s span (0-4000), not the 300-1100 window it happens to be drawn in',
    ).toEqual([0, 4000])
  })

  it('asks for the row over the window it is drawing, not the whole capture', () => {
    render(<PhoneScope transmitting={false} theme="dark" viewLoHz={300} viewHiHz={1100} />)
    return runFrames(150).then(() => {
      expect(rowsServed, 'control: nothing was polled').toBeGreaterThanOrEqual(1)
      expect(spanAsked).toEqual([300, 1100])
    })
  })
})

// ---- The carrier-centered Phone axis (operator, on air 2026-08-16) -------------------
//
// "Tuned to a busy frequency the signal paints at the LEFT of the scope; my FTdx10 and
// IC-9700 draw it in the middle." They do, because a rig's panadapter axis is RF offset
// from the dial. Audio 0 Hz IS the dial (the carrier is suppressed), so on the plain audio
// window the dial was the leftmost pixel and every signal hung off it to the right.
//
// These tests need real geometry — "the dial is at the middle pixel" says nothing about a
// canvas one pixel wide — so they stub a 200×100 rect. TRACE_FRAC 0.45 puts the trace
// baseline at y=45 and the dial at x=100.
const W = 200
const H = 100
const TRACE_H = 45 // Math.round(H * TRACE_FRAC)

function stubRect() {
  vi.spyOn(HTMLCanvasElement.prototype, 'getBoundingClientRect').mockReturnValue({
    x: 0, y: 0, width: W, height: H, top: 0, left: 0, right: W, bottom: H,
    toJSON: () => ({}),
  } as DOMRect)
}

/** x of every full-height vertical rule drawn (moveTo(x,0) → lineTo(x,H)). */
function verticalRules(): number[] {
  return ops.flatMap((o, i) => {
    const next = ops[i + 1]
    return o.op === 'moveTo' &&
      o.args[1] === 0 &&
      next?.op === 'lineTo' &&
      next.args[0] === o.args[0] &&
      next.args[1] === H
      ? [o.args[0]]
      : []
  })
}

/** Trace points, as drawn: x → y. The floor is y === TRACE_H. */
const tracePoints = () => ops.filter((o) => o.op === 'lineTo').map((o) => ({ x: o.args[0], y: o.args[1] }))

/** x of the tallest point on the trace — where the row's one carrier actually painted. */
function peakTraceX(): number {
  return tracePoints().reduce((best, p) => (p.y < best.y ? p : best), { x: -1, y: Infinity }).x
}

// The row's carrier is bin 200 of 512 over 0–4000 Hz = 1565 Hz of receiver audio. On a
// ±4000 Hz carrier-centered axis 200 px wide that is x=139 on USB (dial+1565) and its
// mirror x=61 on LSB (dial−1565). On the OLD plain 0–4000 window it was x=78 for both,
// which is what these two numbers are here to rule out.
const PEAK_X_USB = 139
const PEAK_X_LSB = 61

describe('carrier-centered Phone axis', () => {
  it('draws the dial at the exact middle of the canvas, labelled', async () => {
    stubRect()
    render(
      <PhoneScope transmitting={false} theme="dark" viewLoHz={0} viewHiHz={4000} carrierCentered />,
    )
    await runFrames(300)

    expect(rowsServed, 'control: the draw loop never ran').toBeGreaterThanOrEqual(3)
    expect(calls.fill, 'control: drawRow did not reach the trace').toBeGreaterThanOrEqual(3)

    expect(verticalRules(), 'the carrier line lands on the middle pixel').toContain(W / 2)
    const label = ops.filter((o) => o.op === 'fillText')
    expect(label.length, 'the centre line is labelled, like a rig marks its dial').toBeGreaterThan(0)
    expect(Math.abs(label[0].args[0] - W / 2), 'the label sits at the line').toBeLessThan(12)
  })

  it('CONTROL: the plain audio window draws no carrier line at all', async () => {
    // The same check against a scope that must NOT trip it — without this, "the line is at
    // the middle" would pass just as well on a component that drew a rule down every column.
    stubRect()
    render(<PhoneScope transmitting={false} theme="dark" viewLoHz={0} viewHiHz={4000} />)
    await runFrames(300)

    expect(rowsServed, 'control: the draw loop never ran').toBeGreaterThanOrEqual(3)
    expect(verticalRules()).toEqual([])
  })

  it('paints the half with no data as floor, and never computes a NaN coordinate', async () => {
    // The per-column loop indexes the row by frequency. Half this axis is BELOW the row's
    // first bin, so without a guard it read row[negative] → undefined → NaN, which is a
    // black column in the waterfall and a broken trace path — silent, because a NaN
    // coordinate throws nothing.
    stubRect()
    render(
      <PhoneScope transmitting={false} theme="dark" viewLoHz={0} viewHiHz={4000} carrierCentered />,
    )
    await runFrames(300)

    expect(rowsServed, 'control: the draw loop never ran').toBeGreaterThanOrEqual(3)
    for (const o of ops) {
      for (const a of o.args) {
        expect(Number.isFinite(a), `${o.op}(${o.args.join(', ')}) — a non-finite coordinate`).toBe(true)
      }
    }
    const pts = tracePoints()
    expect(pts.length, 'control: no trace was drawn').toBeGreaterThan(W)
    expect(
      pts.filter((p) => p.x < W / 2).every((p) => p.y === TRACE_H),
      'below the dial there is no data — that half is the floor',
    ).toBe(true)
    // …and the positive half of the same control: the occupied side is NOT floor, so the
    // assertion above is about the axis and not about a trace that never drew anything.
    expect(pts.some((p) => p.x > W / 2 && p.y < TRACE_H), 'the USB side carries the signal').toBe(true)
    expect(Math.abs(peakTraceX() - PEAK_X_USB), `peak at ${peakTraceX()}, want ${PEAK_X_USB}`).toBeLessThanOrEqual(1)
  })

  it('LSB paints below the dial — the occupied half flips with the sideband', async () => {
    // On LSB the audio at f Hz is at dial−f, so the voice belongs LEFT of centre. This is
    // the same picture the rig's own panadapter draws, and the reason the axis is RF offset
    // rather than audio Hz.
    stubRect()
    render(
      <PhoneScope
        transmitting={false}
        theme="dark"
        viewLoHz={0}
        viewHiHz={4000}
        sideband="LSB"
        carrierCentered
      />,
    )
    await runFrames(300)

    expect(rowsServed, 'control: the draw loop never ran').toBeGreaterThanOrEqual(3)
    const pts = tracePoints()
    expect(pts.some((p) => p.x < W / 2 && p.y < TRACE_H), 'the LSB side carries the signal').toBe(true)
    expect(
      pts.filter((p) => p.x > W / 2).every((p) => p.y === TRACE_H),
      'above the dial there is no data on LSB',
    ).toBe(true)
    // The mirror itself, and not merely "something is on the left": the same carrier that
    // paints at x=139 on USB must paint at its reflection about the dial.
    expect(Math.abs(peakTraceX() - PEAK_X_LSB), `peak at ${peakTraceX()}, want ${PEAK_X_LSB}`).toBeLessThanOrEqual(1)
    expect(verticalRules(), 'and the dial is still the middle pixel').toContain(W / 2)
  })
})
