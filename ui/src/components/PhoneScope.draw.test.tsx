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
/** What the served row claims to be. Default = the demodulated-audio row every existing
 *  test here assumes; the native-panadapter tests swap in a real RF extent. */
let rowShape: { loHz: number; hiHz: number; source: string } = { loHz: 0, hiHz: 4000, source: 'rx' }
vi.mock('../api', () => ({
  getScopeRow: (_tx: boolean, loHz: number, hiHz: number) => {
    rowsServed++
    spanAsked = [loHz, hiHz]
    // Answer WIDER than asked — the real backend's refusal path (native RF live, span not a
    // sane audio window, narrow row not produced yet). The row states its own extent, and
    // that extent is what everything downstream must read.
    return Promise.resolve({ row: ROW, ...rowShape })
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
  rowShape = { loHz: 0, hiHz: 4000, source: 'rx' }
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

// The row's carrier is bin 200 of 512 over 0–4000 Hz = 1565 Hz of receiver audio.
//
// The carrier-centered axis is ASYMMETRIC: W = 4000 Hz of occupied sideband plus a W/8 =
// 500 Hz guard band on the empty side, so 200 px carry 4500 Hz at 22.5 Hz/px. Axis 0 (the
// dial) is therefore 500/4500 = the 1/9 mark on USB — x=22 — and its mirror x=178 on LSB.
// The 1565.6 Hz carrier lands at (500+1565.6)/22.5 = x=92, and on LSB at its reflection
// about the dial, (4000−1565.6)/22.5 = x=108.
//
// The numbers these rule out: x=100 for the dial and 139/61 for the peak (the OLDEST
// symmetric ±4000 axis, which gave the voice ~30% of the panel); x=50 and 109/91 (the W/3
// guard that shipped 2026-08-16 to 1.8.0, which the operator read as the dial sitting off to
// the left of its own signal); and x=78 for both sidebands (the plain 0–4000 audio window,
// dial off the left edge entirely).
const DIAL_X_USB = 22
const DIAL_X_LSB = 178
const PEAK_X_USB = 92
const PEAK_X_LSB = 108

describe('carrier-centered Phone axis', () => {
  it('draws the dial at the 1/9 mark of the canvas (USB), labelled', async () => {
    stubRect()
    render(
      <PhoneScope transmitting={false} theme="dark" viewLoHz={0} viewHiHz={4000} carrierCentered />,
    )
    await runFrames(300)

    expect(rowsServed, 'control: the draw loop never ran').toBeGreaterThanOrEqual(3)
    expect(calls.fill, 'control: drawRow did not reach the trace').toBeGreaterThanOrEqual(3)

    // The line is drawn from axis coordinate 0, not from a second placement constant — so
    // it CANNOT disagree with the axis the row is painted on, whichever mark that puts it at.
    expect(verticalRules(), 'the carrier line lands on the 1/9 mark').toContain(DIAL_X_USB)
    const label = ops.filter((o) => o.op === 'fillText')
    expect(label.length, 'the line is labelled, like a rig marks its dial').toBeGreaterThan(0)
    expect(Math.abs(label[0].args[0] - DIAL_X_USB), 'the label sits at the line').toBeLessThan(12)
    // The guard band is DISPLAY-ONLY. Widening the REQUEST to cover it would mean asking for
    // a negative lo, which the backend refuses by handing back the whole 0–4000 row — the
    // resolution the narrow presets exist for, thrown away.
    expect(spanAsked, 'the row request is 0..W, untouched by the guard band').toEqual([0, 4000])
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

  it('paints the guard band with no data as floor, and never computes a NaN coordinate', async () => {
    // The per-column loop indexes the row by frequency. The guard band is BELOW the row's
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
      pts.filter((p) => p.x < DIAL_X_USB).every((p) => p.y === TRACE_H),
      'below the dial there is no data — the guard band is the floor',
    ).toBe(true)
    // …and the occupied side of the same control: it is NOT floor, so the assertion above is
    // about the axis and not about a trace that never drew anything.
    expect(pts.some((p) => p.x > DIAL_X_USB && p.y < TRACE_H), 'the USB side carries the signal').toBe(true)
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
    expect(pts.some((p) => p.x < DIAL_X_LSB && p.y < TRACE_H), 'the LSB side carries the signal').toBe(true)
    expect(
      pts.filter((p) => p.x > DIAL_X_LSB).every((p) => p.y === TRACE_H),
      'above the dial there is no data on LSB — that is the guard band',
    ).toBe(true)
    // The mirror itself, and not merely "something is on the left": the same carrier that
    // paints at x=109 on USB must paint at its reflection about the dial.
    expect(Math.abs(peakTraceX() - PEAK_X_LSB), `peak at ${peakTraceX()}, want ${PEAK_X_LSB}`).toBeLessThanOrEqual(1)
    // …and the axis flips with it: the dial is at the 8/9 mark, so the guard band is the
    // strip ABOVE it. Derived from axis 0 by the same code that drew the USB line at 1/4.
    expect(verticalRules(), 'the dial is the 8/9 mark on LSB').toContain(DIAL_X_LSB)
    // The AGC/readout window is indexed in ROW Hz, and a mirrored axis is not — its bounds
    // are the row's negated and swapped. A symmetric ±W axis hid that (it negates to
    // itself); an asymmetric one does not, and unwound it windows the wrong third of the
    // row, missing the carrier entirely. ROW peak 0.9 over a 0.1 median = 96 dB; the broken
    // window sees only floor and reads ▲0 dB.
    expect(
      document.querySelector('.ph-scope-dyn')?.textContent,
      'peak-over-noise is measured where the voice actually is',
    ).toBe('▲96 dB')
  })
})

// ─────────────────────────────────────────────────────────────────────────────────────────
// THE DIAL ON A NATIVE RF PANADAPTER (operator request, 2026-08-20, from an IC-7300 user:
// "The RF Panadapter is now displayed! Is there a way to place a dial indicator on the freq
// tuned?").
//
// The carrier line the block above tests is `!isRfScopeSource` by construction — it marks
// audio axis 0, which a real RF row does not have — so a native panadapter drew a spectrum
// with nothing marking the tuned frequency. These serve a REAL RF row (source 'civ', an
// absolute-Hz extent) to the same component and assert where the mark lands.
//
// The geometry, stated so the numbers below are checkable rather than recorded: the row runs
// 14.0000–14.2000 MHz over W=200 px, so 1 kHz per pixel.
const RF_LO = 14_000_000
const RF_HI = 14_200_000

/** Serve a native CI-V panadapter row spanning RF_LO..RF_HI. */
function rfRow() {
  rowShape = { loHz: RF_LO, hiHz: RF_HI, source: 'civ' }
}

describe('the dial mark on a native RF panadapter', () => {
  it('draws a labelled line at the tuned frequency', async () => {
    stubRect()
    rfRow()
    // Dial a quarter of the way up the row: 14.05 MHz → x = (50_000/200_000)*200 = 50.
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
    const rules = verticalRules()
    expect(rules.length, 'control: no full-height rule was drawn at all').toBeGreaterThan(0)
    // scopeView centres the RF window on the dial when the dial is inside the row, so the
    // mark lands mid-canvas. What is asserted is that it lands ON THE DIAL, whatever the
    // window arithmetic decides — the x is derived from the same lo/hi the row was painted
    // with, so it cannot disagree with the picture.
    const label = ops.filter((o) => o.op === 'fillText')
    expect(label.length, 'the line is labelled, like a rig marks its dial').toBeGreaterThan(0)
    const labelX = label[0].args[0]
    expect(
      rules.some((x) => Math.abs(x - labelX) < 12),
      'the DIAL label sits at a full-height rule',
    ).toBe(true)
  })

  it('CONTROL: an RF row with no dial known draws no dial mark', async () => {
    // The same check against a scope that must NOT trip it. Without this, "a rule was drawn"
    // would pass on a component that ruled every column — and, more to the point, on the
    // OLD code, which drew nothing here and would have to be caught by this test failing.
    stubRect()
    rfRow()
    render(<PhoneScope transmitting={false} theme="dark" viewLoHz={0} viewHiHz={4000} dialHz={null} />)
    await runFrames(300)

    expect(rowsServed, 'control: the draw loop never ran').toBeGreaterThanOrEqual(3)
    expect(ops.filter((o) => o.op === 'fillText'), 'nothing may be labelled DIAL').toHaveLength(0)
  })

  it('does not mark a dial that is outside the window', async () => {
    // A dial off the edge is not clamped to the edge: a line pinned to the left of the
    // panadapter saying DIAL, while the VFO is a megahertz further down, is worse than no
    // line. scopeView falls back to the row centre when the dial is outside the row, and the
    // draw refuses on that fallback rather than marking a frequency nobody is tuned to.
    stubRect()
    rfRow()
    render(
      <PhoneScope
        transmitting={false}
        theme="dark"
        viewLoHz={0}
        viewHiHz={4000}
        dialHz={21_000_000}
      />,
    )
    await runFrames(300)

    expect(rowsServed, 'control: the draw loop never ran').toBeGreaterThanOrEqual(3)
    expect(ops.filter((o) => o.op === 'fillText'), 'a dial off the row must not be drawn').toHaveLength(0)
  })
})
