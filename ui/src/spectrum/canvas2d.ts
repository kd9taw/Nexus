// The canvas-2D renderer: today's paint code behind the renderer contract. It is a first-class
// path, not a fallback afterthought: on Linux and the Pi the webview hands it most operators, and it
// draws the same pixel fixtures as the WebGL2 path.
//
// It is PhoneScope's and Waterfall's drawing, moved. The waterfall band is a retained RGBA buffer
// scrolled with `copyWithin` and blitted with one `putImageData`. Every cold change (palette, view,
// flow, detector, layout, resize, scrollback, a cleared or widened ring) rebuilds the band from the
// ring through the same per-pixel mapping the hot path writes with, so a rebuild reproduces the live
// picture instead of re-deriving a different one (the divergence `waterfallHistory.ts`'s header
// records). The trace band is redrawn on every `draw`: the palette's gradient under the curve and
// its brightest colour along it, as the rig scope draws it.
//
// The canvas is never read back (no willReadFrequently), so the browser may keep it on the GPU.

import { aggregateRow, lutIndex, safeDbPerUnit, strengthIn } from './aggregate'
import { DSS_COLS, DSS_ROWS, drawDss2d, dssStrengths } from './dss'
import { M_CEIL, M_DB_PER_UNIT, M_FLOOR, M_HI, M_LO, M_N, META, type SpectrumRing } from './ring'
import type { Backend, Detector, SpectrumLayout, SpectrumScene } from './types'

/** The three bands of a `height`-pixel canvas for a layout, clamped so they always fit. */
export function bandsOf(layout: SpectrumLayout, height: number): { traceH: number; plotH: number; stripH: number } {
  const stripH = Math.max(0, Math.min(height, Math.round(layout.stripH)))
  const plotH = height - stripH
  const traceH = Math.max(0, Math.min(plotH, Math.round(layout.traceH)))
  return { traceH, plotH, stripH }
}

/** Palette entries the trace gradient is built from: 0.3, 0.7 and 1.0 of the way up (PhoneScope's
 *  `sampleLut(name, 0.3 | 0.7 | 1.0)`, read off the baked table). */
export const TRACE_STOPS = [Math.round(0.3 * 255), Math.round(0.7 * 255), 255] as const

/** What the retained band was drawn for; any difference is a cold rebuild. */
interface BandKey {
  w: number
  top: number
  bandH: number
  loHz: number
  hiHz: number
  lut: Uint8ClampedArray
  newestAtTop: boolean
  detector: Detector
  generation: number
}

export class Canvas2dBackend implements Backend {
  readonly kind = 'canvas2d'
  readonly canvas: HTMLCanvasElement
  private readonly ctx: CanvasRenderingContext2D
  private readonly ring: SpectrumRing
  private w = 0
  private h = 0
  private px = new Float32Array(0)
  private band: ImageData | null = null
  private bandKey: BandKey | null = null
  /** Serial of the newest row the band shows. */
  private shown = 0
  private grad: CanvasGradient | null = null
  private gradLut: Uint8ClampedArray | null = null
  private gradH = 0
  private stroke = ''
  private dssKey = ''
  private dssLut: Uint8ClampedArray | null = null
  private readonly dssField = new Float32Array(DSS_ROWS * DSS_COLS)
  private readonly dssPresent = new Uint8Array(DSS_ROWS)

  static create(canvas: HTMLCanvasElement, ring: SpectrumRing): Canvas2dBackend | null {
    const ctx = canvas.getContext('2d')
    return ctx ? new Canvas2dBackend(canvas, ctx, ring) : null
  }

  private constructor(canvas: HTMLCanvasElement, ctx: CanvasRenderingContext2D, ring: SpectrumRing) {
    this.canvas = canvas
    this.ctx = ctx
    this.ring = ring
  }

  resize(width: number, height: number): void {
    if (width === this.w && height === this.h) return
    // Assigning the size clears the backing store; everything is redrawn from the ring.
    this.canvas.width = width
    this.canvas.height = height
    this.w = width
    this.h = height
    this.px = new Float32Array(width)
    this.band = null
    this.bandKey = null
    this.dssKey = ''
  }

  rowCommitted(): void {
    // Nothing to do until the next draw: the band scrolls by however many rows arrived.
  }

  draw(scene: SpectrumScene): void {
    const { w, h } = this
    if (w <= 0 || h <= 0) return
    const lut = scene.lut
    const { traceH, plotH, stripH } = bandsOf(scene.layout, h)
    const ctx = this.ctx
    if (stripH > 0) {
      ctx.fillStyle = `rgb(${lut[0]},${lut[1]},${lut[2]})`
      ctx.fillRect(0, plotH, w, stripH)
    }
    if (scene.mode === 'dss') {
      // The 2D band is gone from the canvas; it is rebuilt when the 2D view comes back.
      this.bandKey = null
      const { generation, serial } = this.ring
      const key = `${generation}:${serial}:${scene.offsetRows}:${scene.view.loHz}:${scene.view.hiHz}:${scene.detector}:${w}:${plotH}`
      if (key === this.dssKey && lut === this.dssLut) return
      this.dssKey = key
      this.dssLut = lut
      const offset = Math.max(0, scene.offsetRows)
      const rows = dssStrengths(this.ring, DSS_COLS, scene.view, offset, scene.detector, this.dssField, this.dssPresent)
      drawDss2d(ctx, w, plotH, this.dssField, this.dssPresent, rows, DSS_COLS, lut)
      return
    }
    this.dssKey = ''
    const bandH = plotH - traceH
    if (bandH > 0) this.drawBand(scene, traceH, bandH)
    if (traceH > 0) this.drawTrace(scene, traceH)
  }

  destroy(): void {
    this.band = null
    this.canvas.width = 0
    this.canvas.height = 0
  }

  /** The waterfall band: scroll the retained buffer by the rows that arrived, or rebuild it. */
  private drawBand(scene: SpectrumScene, top: number, bandH: number): void {
    const ring = this.ring
    const w = this.w
    const offset = Math.max(0, Math.floor(scene.offsetRows))
    const want = ring.serial - 1 - offset
    const k = this.bandKey
    const cold =
      !this.band ||
      !k ||
      k.w !== w ||
      k.top !== top ||
      k.bandH !== bandH ||
      k.loHz !== scene.view.loHz ||
      k.hiHz !== scene.view.hiHz ||
      k.lut !== scene.lut ||
      k.newestAtTop !== scene.newestAtTop ||
      k.detector !== scene.detector ||
      k.generation !== ring.generation
    const step = want - this.shown
    if (!cold && step === 0) return
    if (cold || step < 0 || step >= bandH) {
      if (!this.band || this.band.width !== w || this.band.height !== bandH) this.band = new ImageData(w, bandH)
      const data = this.band.data
      for (let y = 0; y < bandH; y++) {
        const age = offset + (scene.newestAtTop ? y : bandH - 1 - y)
        this.paintRow(data, y, age, scene)
      }
      this.bandKey = {
        w,
        top,
        bandH,
        loHz: scene.view.loHz,
        hiHz: scene.view.hiHz,
        lut: scene.lut,
        newestAtTop: scene.newestAtTop,
        detector: scene.detector,
        generation: ring.generation,
      }
    } else {
      // The hot path: `step` new rows. Shift the picture away from the leading edge and paint the
      // new rows into the gap, newest at the edge. The direction is read once, so the shift and the
      // rows it makes room for can never disagree.
      const data = this.band!.data
      const rowBytes = w * 4
      if (scene.newestAtTop) {
        data.copyWithin(step * rowBytes, 0, (bandH - step) * rowBytes)
        for (let y = 0; y < step; y++) this.paintRow(data, y, offset + y, scene)
      } else {
        data.copyWithin(0, step * rowBytes)
        for (let j = 0; j < step; j++) this.paintRow(data, bandH - 1 - j, offset + j, scene)
      }
    }
    this.shown = want
    try {
      this.ctx.putImageData(this.band!, 0, top)
    } catch {
      /* zero-size mid-layout */
    }
  }

  /** One band row from the ring row `age` back, in that row's own span and display range. */
  private paintRow(data: Uint8ClampedArray, y: number, age: number, scene: SpectrumScene): void {
    const ring = this.ring
    const lut = scene.lut
    const w = this.w
    const o = y * w * 4
    const r = ring.indexAt(age)
    if (r < 0) {
      for (let x = 0; x < w; x++) {
        const p = o + x * 4
        data[p] = lut[0]
        data[p + 1] = lut[1]
        data[p + 2] = lut[2]
        data[p + 3] = 255
      }
      return
    }
    const m = r * META
    const floor = ring.meta[m + M_FLOOR]
    const ceil = ring.meta[m + M_CEIL]
    const px = this.px
    aggregateRow(
      ring.values,
      r * ring.cols,
      ring.meta[m + M_N],
      ring.meta[m + M_LO],
      ring.meta[m + M_HI],
      scene.view.loHz,
      scene.view.hiHz,
      px,
      scene.detector,
      ring.meta[m + M_DB_PER_UNIT],
    )
    for (let x = 0; x < w; x++) {
      const v = px[x]
      // NaN = this column's frequency is outside the row's span (a retune, a view wider than the
      // feed): the palette floor, as the rebuild and the live path both paint it.
      const li = Number.isNaN(v) ? 0 : lutIndex(v, floor, ceil) * 4
      const p = o + x * 4
      data[p] = lut[li]
      data[p + 1] = lut[li + 1]
      data[p + 2] = lut[li + 2]
      data[p + 3] = 255
    }
  }

  /** The trace band: cleared to the floor, then the filled curve and its line. */
  private drawTrace(scene: SpectrumScene, traceH: number): void {
    const ctx = this.ctx
    const lut = scene.lut
    const w = this.w
    ctx.fillStyle = `rgb(${lut[0]},${lut[1]},${lut[2]})`
    ctx.fillRect(0, 0, w, traceH)
    const trace = scene.trace
    if (!trace) return
    const f = trace.frame
    const px = this.px
    const { loHz, hiHz } = scene.view
    aggregateRow(f.bins, 0, f.bins.length, f.loHz, f.hiHz, loHz, hiHz, px, scene.detector, safeDbPerUnit(f.dbPerUnit))
    const { floor, ceil } = trace.range
    if (!this.grad || this.gradLut !== lut || this.gradH !== traceH) {
      const [i0, i1, i2] = TRACE_STOPS.map((i) => i * 4)
      const g = ctx.createLinearGradient(0, traceH, 0, 0)
      g.addColorStop(0, `rgba(${lut[i0]},${lut[i0 + 1]},${lut[i0 + 2]},0.45)`)
      g.addColorStop(0.6, `rgba(${lut[i1]},${lut[i1 + 1]},${lut[i1 + 2]},0.8)`)
      g.addColorStop(1, `rgba(${lut[i2]},${lut[i2 + 1]},${lut[i2 + 2]},0.95)`)
      this.grad = g
      this.gradLut = lut
      this.gradH = traceH
      this.stroke = `rgb(${lut[i2]},${lut[i2 + 1]},${lut[i2 + 2]})`
    }
    // Outside the frame's span there is nothing to draw: the floor, explicitly (an unguarded NaN
    // breaks the path without throwing).
    const yFor = (x: number) => {
      const v = px[x]
      return traceH - (Number.isNaN(v) ? 0 : strengthIn(v, floor, ceil)) * (traceH - 1)
    }
    ctx.beginPath()
    ctx.moveTo(0, traceH)
    for (let x = 0; x < w; x++) ctx.lineTo(x, yFor(x))
    ctx.lineTo(w, traceH)
    ctx.closePath()
    ctx.fillStyle = this.grad
    ctx.fill()
    ctx.beginPath()
    ctx.moveTo(0, yFor(0))
    for (let x = 1; x < w; x++) ctx.lineTo(x, yFor(x))
    ctx.strokeStyle = this.stroke
    ctx.lineWidth = Math.max(1, scene.layout.lineWidth)
    ctx.stroke()
  }
}
