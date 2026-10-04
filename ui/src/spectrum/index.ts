// The spectrum renderer, chosen and kept alive. `createSpectrumRenderer(host)` puts a canvas in the
// host element and draws through WebGL2 or canvas-2D behind one contract (types.ts).
//
// CHOOSING (choose.ts). WebGL2 is used when it is the faster backend here, AND a context is actually
// created on the canvas, AND a picture drawn through the real waterfall program reads back as
// expected (webgl2.ts `selfTest`). Faster: a renderer string naming a software rasteriser sends
// `auto` to canvas-2D before anything is built; where the string is masked, WebGL2 draws while a
// timed probe runs after the first paint, and hands over to canvas-2D if the probe finds it slower.
// An explicit `backend`, or the hidden setting, skips that question but never the self-test. Never
// `gpu.ts`: its probe fails closed when WEBGL_debug_renderer_info is masked, which WebKitGTK does by
// default, so it would send every Linux operator to canvas-2D whatever their GPU can do. Anything
// else gets canvas-2D, which is a first-class path, and `reason` says why.
//
// SURVIVING A LOST CONTEXT (sleep, a driver reset, the browser's context cap). The history lives in
// the ring, not on the GPU, so the facade swaps a canvas-2D canvas in the moment WebGL2 is lost and
// draws the same picture from the same ring; when the browser restores the context, the WebGL2
// backend rebuilds everything, re-uploads the ring, passes its self-test again and takes over. No
// reload, and no history lost either way. `preventDefault()` on the loss is what asks the browser
// for the context back; without it there is never a restore.
//
// The host owns geometry: it measures its box (device pixels) and calls `resize`, and its element
// must be a positioned box, since the canvas fills it absolutely. Pointer handlers go on the host,
// because the canvas is swapped while a context is away.

import { Canvas2dBackend } from './canvas2d'
import { askedFor, probeOnce, settled, type Verdict } from './choose'
import { SpectrumRing } from './ring'
import type {
  Backend,
  BackendKind,
  DisplayRange,
  RowFrame,
  SpectrumFrame,
  SpectrumRenderer,
  SpectrumRendererOptions,
  SpectrumScene,
} from './types'
import { WebGl2Backend } from './webgl2'

export type {
  BackendKind,
  Detector,
  DisplayRange,
  RowFrame,
  SpectrumFrame,
  SpectrumLayout,
  SpectrumMode,
  SpectrumRenderer,
  SpectrumRendererOptions,
  SpectrumScene,
} from './types'

export function createSpectrumRenderer(host: HTMLElement, opts: SpectrumRendererOptions = {}): SpectrumRenderer {
  return new Renderer(host, opts)
}

function newCanvas(): HTMLCanvasElement {
  const c = document.createElement('canvas')
  c.className = 'spectrum-canvas'
  // Out of flow, so the canvas's own size can never feed back into the box that sizes it (the
  // `.waterfall-canvas` rule's reason).
  c.style.cssText = 'position:absolute;inset:0;width:100%;height:100%;display:block'
  return c
}

class Renderer implements SpectrumRenderer {
  private readonly host: HTMLElement
  private readonly ring: SpectrumRing
  /** The WebGL2 backend, drawing or waiting for its context to come back. */
  private gl: WebGl2Backend | null = null
  /** The canvas-2D backend, when it is the choice or is standing in for a lost context. */
  private cpu: Canvas2dBackend | null = null
  private active: Backend | null = null
  private w = 0
  private h = 0
  private scene: SpectrumScene | null = null
  reason = ''

  constructor(host: HTMLElement, opts: SpectrumRendererOptions) {
    this.host = host
    this.ring = new SpectrumRing(opts.depth)
    const asked = opts.backend === 'webgl2' || opts.backend === 'canvas2d' ? opts.backend : askedFor()
    const known = asked ? null : settled()
    if (asked === 'canvas2d') {
      this.reason = 'canvas-2D was asked for'
    } else if (known?.pick === 'canvas2d') {
      this.reason = known.why
    } else {
      const canvas = newCanvas()
      const made = WebGl2Backend.create(canvas, this.ring)
      if (typeof made === 'string') {
        this.reason = made
      } else {
        this.gl = made
        canvas.addEventListener('webglcontextlost', this.onLost)
        canvas.addEventListener('webglcontextrestored', this.onRestored)
        host.appendChild(canvas)
        this.active = made
        // Neither asked for nor named: WebGL2 draws until the probe has had its say.
        if (!asked && !known) void probeOnce(made).then(this.onVerdict)
      }
    }
    if (!this.gl) this.standIn()
  }

  get backend(): BackendKind {
    return this.active?.kind ?? 'none'
  }

  get canvas(): HTMLCanvasElement | null {
    return this.active?.canvas ?? null
  }

  get rows(): number {
    return this.ring.count
  }

  resize(width: number, height: number): void {
    const w = Math.max(0, Math.floor(width))
    const h = Math.max(0, Math.floor(height))
    this.w = w
    this.h = h
    // The waiting WebGL2 canvas keeps the size too, so a restore comes back at the right one.
    this.gl?.resize(w, h)
    this.cpu?.resize(w, h)
  }

  commitRow(frame: SpectrumFrame, range: DisplayRange): void {
    this.ring.push(frame, range)
    this.active?.rowCommitted()
  }

  rowAt(age: number): RowFrame | null {
    return this.ring.frameAt(age)
  }

  clearHistory(): void {
    this.ring.clear()
  }

  draw(scene: SpectrumScene): void {
    this.scene = scene
    this.active?.draw(scene)
  }

  destroy(): void {
    this.scene = null
    this.dropGl()
    this.dropCpu()
    this.active = null
  }

  /** The probe found canvas-2D faster: it takes over for good, drawing the same history. */
  private readonly onVerdict = (v: Verdict | null) => {
    if (!this.gl || v?.pick !== 'canvas2d') return
    this.reason = v.why
    this.dropGl()
    this.standIn()
  }

  private readonly onLost = (e: Event) => {
    e.preventDefault()
    if (!this.gl) return
    this.gl.lost()
    this.reason = 'the WebGL2 context was lost; canvas-2D draws until it is restored'
    this.standIn()
  }

  private readonly onRestored = () => {
    const gl = this.gl
    if (!gl) return
    const why = gl.restore()
    if (why) {
      this.reason = `the restored WebGL2 context failed (${why}); canvas-2D draws from here on`
      this.dropGl()
      this.standIn()
      return
    }
    gl.resize(this.w, this.h)
    gl.canvas.style.visibility = ''
    this.dropCpu()
    this.active = gl
    this.reason = ''
    if (this.scene) gl.draw(this.scene)
  }

  /** Draw on canvas-2D: the choice, or the stand-in while the WebGL2 context is away. */
  private standIn(): void {
    if (!this.cpu) {
      const canvas = newCanvas()
      const cpu = Canvas2dBackend.create(canvas, this.ring)
      if (!cpu) {
        // No canvas context of either kind (jsdom): the renderer stays inert rather than throwing.
        this.active = null
        this.reason ||= 'no canvas context'
        return
      }
      cpu.resize(this.w, this.h)
      this.host.appendChild(canvas)
      this.cpu = cpu
    }
    if (this.gl) this.gl.canvas.style.visibility = 'hidden'
    this.active = this.cpu
    if (this.scene) this.cpu.draw(this.scene)
  }

  private dropGl(): void {
    const gl = this.gl
    if (!gl) return
    this.gl = null
    gl.canvas.removeEventListener('webglcontextlost', this.onLost)
    gl.canvas.removeEventListener('webglcontextrestored', this.onRestored)
    gl.destroy()
    gl.canvas.remove()
    if (this.active === gl) this.active = null
  }

  private dropCpu(): void {
    const cpu = this.cpu
    if (!cpu) return
    this.cpu = null
    cpu.destroy()
    cpu.canvas.remove()
    if (this.active === cpu) this.active = null
  }
}
