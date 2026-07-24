import { describe, expect, it } from 'vitest'
import { drawDss, DSS_ROWS } from './dss'
import { WaterfallHistory } from './waterfallHistory'

/** A minimal CanvasRenderingContext2D mock that counts the drawing calls drawDss makes. */
function mockCtx() {
  const calls = { fillRect: 0, fill: 0, stroke: 0, beginPath: 0 }
  const fillStyles = new Set<string>()
  const ctx = {
    save() {},
    restore() {},
    beginPath() {
      calls.beginPath++
    },
    moveTo() {},
    lineTo() {},
    closePath() {},
    fill() {
      calls.fill++
    },
    stroke() {
      calls.stroke++
    },
    fillRect() {
      calls.fillRect++
    },
    set fillStyle(v: string) {
      fillStyles.add(v)
    },
    get fillStyle() {
      return ''
    },
    strokeStyle: '',
    lineWidth: 1,
    lineJoin: 'round',
    lineCap: 'round',
    imageSmoothingEnabled: true,
  }
  return { ctx: ctx as unknown as CanvasRenderingContext2D, calls, fillStyles }
}

const lut = (() => {
  const l = new Uint8ClampedArray(256 * 4)
  for (let i = 0; i < 256; i++) {
    l[i * 4] = i
    l[i * 4 + 1] = i
    l[i * 4 + 2] = i
    l[i * 4 + 3] = 255
  }
  return l
})()

describe('drawDss', () => {
  it('paints only the background when history is empty', () => {
    const h = new WaterfallHistory(64)
    const { ctx, calls } = mockCtx()
    drawDss(ctx, 200, 100, h, lut, [0, 0, 0], { loHz: 0, hiHz: 4000 })
    expect(calls.fillRect).toBe(1) // the bg fill
    expect(calls.fill).toBe(0) // no ridges
  })

  it('draws per-column trapezoid fills + ridge strokes for stored rows', () => {
    const h = new WaterfallHistory(64)
    for (let r = 0; r < 10; r++) {
      const row = new Float32Array(64)
      row[32] = 1 // a carrier
      h.push(row, 0, 4000, r)
    }
    const { ctx, calls } = mockCtx()
    drawDss(ctx, 256, 120, h, lut, [10, 10, 10], { loHz: 0, hiHz: 4000 })
    expect(calls.fillRect).toBe(1)
    expect(calls.fill).toBeGreaterThan(100) // many per-column trapezoids across 10 rows
    expect(calls.stroke).toBeGreaterThan(100) // ridge lines too
  })

  it('caps the drawn depth at DSS_ROWS regardless of history length', () => {
    const h = new WaterfallHistory(400)
    for (let r = 0; r < 300; r++) h.push([0.2, 0.2], 0, 100, r)
    const { ctx, calls } = mockCtx()
    drawDss(ctx, 128, 80, h, lut, [0, 0, 0], { loHz: 0, hiHz: 100 })
    // One beginPath per column-fill + per ridge-line, across at most DSS_ROWS rows —
    // finite and bounded (not 300 rows' worth).
    expect(h.length).toBe(300)
    expect(calls.fill).toBeLessThanOrEqual(DSS_ROWS * 256)
    expect(calls.fill).toBeGreaterThan(0)
  })

  it('is a no-op on a degenerate view span', () => {
    const h = new WaterfallHistory(8)
    h.push([1, 1], 0, 100, 0)
    const { ctx, calls } = mockCtx()
    drawDss(ctx, 100, 50, h, lut, [0, 0, 0], { loHz: 1000, hiHz: 1000 })
    expect(calls.fillRect).toBe(1) // bg only
    expect(calls.fill).toBe(0)
  })
})
