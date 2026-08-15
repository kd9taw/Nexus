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
// why this asserts a RATIO and never a duration or a pixel.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { PhoneScope } from './PhoneScope'
import { TRACE_HOLD_MS } from '../waterfall'

/** A 512-bin row with one carrier, shaped like what the engine publishes. */
const ROW = Array.from({ length: 512 }, (_, i) => (i === 200 ? 0.9 : 0.1))

let rowsServed = 0
let spanAsked: [number, number] | null = null
vi.mock('../api', () => ({
  getScopeRow: (_tx: boolean, loHz: number, hiHz: number) => {
    rowsServed++
    spanAsked = [loHz, hiHz]
    // The backend answers over the span it was asked for (or falls back wide); either way the
    // row states its own extent, which is what the component reads.
    return Promise.resolve({ row: ROW, loHz, hiHz, source: 'rx' })
  },
}))

/** Records the calls PhoneScope makes; every method it uses is listed explicitly. */
function recordingCtx() {
  const calls: Record<string, number> = {}
  const bump = (k: string) => {
    calls[k] = (calls[k] ?? 0) + 1
  }
  const ctx = {
    fillStyle: '',
    strokeStyle: '',
    lineWidth: 1,
    createLinearGradient: () => {
      bump('createLinearGradient')
      return { addColorStop: () => {} }
    },
    fillRect: () => bump('fillRect'),
    putImageData: () => bump('putImageData'),
    beginPath: () => bump('beginPath'),
    closePath: () => bump('closePath'),
    moveTo: () => {},
    lineTo: () => {},
    fill: () => bump('fill'),
    stroke: () => bump('stroke'),
    setLineDash: () => {},
  }
  return { ctx, calls }
}

let calls: Record<string, number>
let realRaf: typeof requestAnimationFrame
let realCaf: typeof cancelAnimationFrame

beforeEach(() => {
  rowsServed = 0
  const rec = recordingCtx()
  calls = rec.calls
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

  it('asks for the row over the window it is drawing, not the whole capture', () => {
    render(<PhoneScope transmitting={false} theme="dark" viewLoHz={300} viewHiHz={1100} />)
    return runFrames(150).then(() => {
      expect(rowsServed, 'control: nothing was polled').toBeGreaterThanOrEqual(1)
      expect(spanAsked).toEqual([300, 1100])
    })
  })
})
