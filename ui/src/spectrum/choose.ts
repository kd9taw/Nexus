// Which backend draws: the one that is FASTER on this machine, not merely one that works.
//
// WebGL2 is the fast path on a GPU: the history lives in a texture and every redraw is a uniform
// change. On a software rasteriser it is the slow one. SwiftShader, Mesa's llvmpipe and softpipe, and
// Windows' Microsoft Basic Render Driver all create a WebGL2 context and pass the self-test, then cost
// several times what canvas-2D does: in headless Chrome on SwiftShader, the Phone scope filling a
// 3440×1440 window took 8.4–10.5 ms of main thread a frame on WebGL2 and 1.8–2.0 ms on canvas-2D
// (2026-10-04). A machine with no GPU (a VM, a GPU-less Linux box) is that case. `auto` asks two
// questions, cheapest first:
//
//  1. The renderer string (WEBGL_debug_renderer_info), read once per page from a context of its own.
//     A software rasteriser's name means canvas-2D, decided before a program is compiled; any other
//     name means a GPU, and WebGL2.
//  2. Where the string is masked (WebKitGTK masks it by default, so most Linux operators and the Pi),
//     a timed probe, after the first paint. The same representative frame, a row committed and a full
//     waterfall band redrawn (the frame's dominant cost), is drawn over and over by each backend and
//     read back every STEP frames, so the time is the drawing's and not only the calls'. A real frame
//     never waits on a read-back; STEP shares its round trip out, so on a GPU it stays a small part of
//     the time (a backend slow enough to need no sharing reads back every frame). WebGL2 draws through the renderer's own program (webgl2.ts `scratch`), so nothing is
//     compiled; canvas-2D on a canvas of its own. Neither touches the history or the picture on
//     screen. Each backend draws for two windows of at least SPAN_MS, long enough for a clock rounded
//     to the millisecond, unless WebGL2's one timed frame has already decided: on a software
//     rasteriser it is slower by so wide a factor (`decisive`) that the probe stops there, after two
//     WebGL2 frames, the cold one and the timed one. Those two frames are its floor: it cannot decide
//     without them, so on the machines it is for its cost is counted in WebGL2 frames, not in ms.
//     WebGL2 draws until the probe has run; it runs once per page, and every renderer made later
//     takes its verdict.
//
// WebGL2 loses the probe only when it is more than MARGIN times slower AND over FLOOR_NS a pixel. A
// software rasteriser measures many times slower and far over the floor; anything closer stays on
// WebGL2, whose 3-D view is a GPU pass where canvas-2D's costs tens of ms of main thread a frame.
// The floor is for a GPU where canvas-2D is very cheap: there the probe's time is mostly the
// read-back's round trip, which says nothing about drawing.
//
// `backend: 'webgl2' | 'canvas2d'` skips the choice (never the self-test), and so does the hidden
// setting below. The spectrum harness keeps testing WebGL2 that way on its software-rendered Chrome.

import { Canvas2dBackend } from './canvas2d'
import { SpectrumRing } from './ring'
import type { SpectrumFrame, SpectrumScene } from './types'
import type { WebGl2Backend } from './webgl2'

/** The hidden setting: `webgl2` or `canvas2d` takes the choice away from `auto` on this machine.
 *  Anything else is ignored. */
const BACKEND_KEY = 'nexus.spectrum.backend'

export type Choice = 'webgl2' | 'canvas2d'

/** What was chosen, and why ('' for WebGL2, which needs no explaining). */
export interface Verdict {
  pick: Choice
  why: string
  /** When the probe decided: ms a frame on each backend, how long the probe took, and how many
   *  frames WebGL2 drew for it (the cold one included). */
  probe?: { glMs: number; cpuMs: number; tookMs: number; glFrames: number }
}

/** The backend the hidden setting asks for, or null. */
export function askedFor(): Choice | null {
  try {
    const v = localStorage.getItem(BACKEND_KEY)
    return v === 'webgl2' || v === 'canvas2d' ? v : null
  } catch {
    return null
  }
}

const SOFTWARE = /swiftshader|llvmpipe|softpipe|basic render|software/i

/** The renderer string, unmasked; null where the browser masks it. */
export function rendererName(gl: WebGL2RenderingContext): string | null {
  const dbg = gl.getExtension('WEBGL_debug_renderer_info')
  const name: unknown = dbg ? gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL) : null
  return typeof name === 'string' && name !== '' ? name : null
}

/** What a renderer string decides: canvas-2D for a software rasteriser, WebGL2 for any other name,
 *  and nothing where it is masked. */
export function byName(name: string | null): Verdict | null {
  if (name === null) return null
  return SOFTWARE.test(name)
    ? { pick: 'canvas2d', why: `WebGL2 is software-rendered here (${name}); canvas-2D is faster` }
    : { pick: 'webgl2', why: '' }
}

/** The probe's frame: a waterfall band 1024×256 device px, full of history at a bin a pixel. */
const PROBE = { w: 1024, h: 256, cols: 1024 }
/** WebGL2 loses the probe only when it takes more than this many times canvas-2D's time... */
const MARGIN = 2
/** ...and more than this many ns a pixel: many times what a GPU takes, and an eighth of what
 *  SwiftShader measured (2026-10-04). */
const FLOOR_NS = 1

/** What the probe's times decide, each in ms a frame. */
export function byTimes(glMs: number, cpuMs: number): Verdict {
  return glMs > MARGIN * cpuMs && glMs > (FLOOR_NS * PROBE.w * PROBE.h) / 1e6
    ? {
        pick: 'canvas2d',
        why: `the probe timed WebGL2 at ${glMs.toFixed(2)} ms a frame and canvas-2D at ${cpuMs.toFixed(2)} ms`,
      }
    : { pick: 'webgl2', why: '' }
}

/** WebGL2's one timed frame decides alone when WebGL2 would still lose with that frame's time
 *  divided by this: more than 32 times canvas-2D and over 16 ns a pixel. SwiftShader measured
 *  127–178 times canvas-2D and 30–68 ns a pixel on CI's runners (2026-10-04). A GPU's frame passes
 *  for decisive only if something else stalls it (a collection, a busy GPU process) by more than
 *  4 ms, or by more than 32 of canvas-2D's frames where those cost more than 0.13 ms. */
const DECISIVE = 16

/** Whether WebGL2's timed frame (ms) already decides, against canvas-2D's ms a frame. */
export function decisive(glMs: number, cpuMs: number): boolean {
  return byTimes(glMs / DECISIVE, cpuMs).pick === 'canvas2d'
}

/** The page's verdict, once the renderer string or the probe has given one. */
let verdict: Verdict | null = null
/** The renderer string has been read (masked or not). */
let named = false
let pending: Promise<Verdict | null> | null = null

/** The page's verdict, reading the renderer string the first time a context can be had. Null:
 *  undecided, so WebGL2 draws and the probe should run. */
export function settled(): Verdict | null {
  if (!named) {
    let gl: WebGL2RenderingContext | null = null
    try {
      gl = document.createElement('canvas').getContext('webgl2')
    } catch {
      gl = null
    }
    if (gl) {
      named = true
      verdict = byName(rendererName(gl))
      gl.getExtension('WEBGL_lose_context')?.loseContext()
    }
  }
  return verdict
}

/**
 * The probe's verdict, timed on `gl` (the first renderer to ask lends its WebGL2 backend). It runs
 * once per page, after the first paint, and every caller shares it. Null when `gl` could not draw by
 * then (destroyed, or its context lost): nothing is decided, and the next renderer to ask probes again.
 */
export function probeOnce(gl: WebGl2Backend): Promise<Verdict | null> {
  pending ??= new Promise((done) =>
    requestAnimationFrame(() =>
      setTimeout(() => {
        verdict = probe(gl)
        if (!verdict) pending = null
        done(verdict)
      }),
    ),
  )
  return pending
}

/** The probe this page has started, if any (the spectrum harness awaits it). */
export function probed(): Promise<Verdict | null> | null {
  return pending
}

/** Frames drawn between two read-backs... */
const STEP = 4
/** ...unless one frame takes this long on its own (ms): then there is no round trip worth sharing
 *  out, and a slow backend overruns a window by one frame at most. */
const SLOW_MS = 2
/** How long each backend draws, twice, interleaved with the other: ms. */
const SPAN_MS = 4
const RANGE = { floor: 0, ceil: 1 }

/** One backend drawing the probe's frames. */
interface Rig {
  frame: () => void
  /** Read one pixel back: it returns once everything drawn so far has been drawn. */
  settle: () => void
  done: () => void
  /** Frames drawn between two read-backs. */
  step: number
  frames: number
  ms: number
}

function probe(backend: WebGl2Backend): Verdict | null {
  const t0 = performance.now()
  const scratch = backend.scratch(PROBE.w, PROBE.h, PROBE.cols)
  if (!scratch) return null
  const gl: Rig = { ...scratch, step: STEP, frames: 0, ms: 0 }
  const cpu = cpuRig()
  if (!cpu) {
    gl.done()
    // No canvas-2D to compare with, so nothing to hand over to.
    return { pick: 'webgl2', why: '' }
  }
  // The first frame of each is the cold one (first use of the framebuffer, the band built whole);
  // the second, timed alone, sets the step.
  let timed = 0
  for (const r of [gl, cpu]) {
    r.frame()
    r.settle()
    const t = performance.now()
    r.frame()
    r.settle()
    const ms = performance.now() - t
    if (ms >= SLOW_MS) r.step = 1
    if (r === gl) timed = ms
  }
  // Canvas-2D's first window costs little. Against it, WebGL2's timed frame may be the answer already,
  // and WebGL2 then draws no more; otherwise each draws its two windows, interleaved.
  run(cpu)
  const early = decisive(timed, cpu.ms / cpu.frames)
  if (!early) {
    run(gl)
    run(cpu)
    run(gl)
  }
  gl.done()
  cpu.done()
  const glMs = early ? timed : gl.ms / gl.frames
  const cpuMs = cpu.ms / cpu.frames
  return { ...byTimes(glMs, cpuMs), probe: { glMs, cpuMs, tookMs: performance.now() - t0, glFrames: 2 + gl.frames } }
}

/** Canvas-2D drawing the same frame: a canvas, a ring and a band of its own. */
function cpuRig(): Rig | null {
  const canvas = document.createElement('canvas')
  const ring = new SpectrumRing(PROBE.h, PROBE.cols)
  const cpu = Canvas2dBackend.create(canvas, ring)
  const ctx = canvas.getContext('2d')
  if (!cpu || !ctx) return null
  const rows = Array.from({ length: 8 }, (_, seq): SpectrumFrame => {
    const bins = new Float32Array(PROBE.cols)
    for (let i = 0; i < bins.length; i++) bins[i] = (((i + seq * 131) * 7919) % 997) / 997
    return { seq, tMs: seq * 50, loHz: 0, hiHz: PROBE.cols, bins, dbPerUnit: 120 }
  })
  for (let i = 0; i < ring.depth; i++) ring.push(rows[i % rows.length], RANGE)
  const lut = new Uint8ClampedArray(1024)
  for (let i = 0; i < 256; i++) lut.set([i, i, i, 255], i * 4)
  const scene: SpectrumScene = {
    view: { loHz: 0, hiHz: PROBE.cols },
    lut,
    layout: { traceH: 0, stripH: 0, lineWidth: 1 },
    detector: 'peak',
    mode: '2d',
    offsetRows: 0,
    newestAtTop: false,
    trace: null,
  }
  cpu.resize(PROBE.w, PROBE.h)
  let n = 0
  return {
    frame: () => {
      ring.push(rows[n++ % rows.length], RANGE)
      cpu.rowCommitted()
      cpu.draw(scene)
    },
    settle: () => void ctx.getImageData(0, 0, 1, 1),
    done: () => cpu.destroy(),
    step: STEP,
    frames: 0,
    ms: 0,
  }
}

/** Frames for at least SPAN_MS, read back every `step` frames. */
function run(r: Rig): void {
  const t0 = performance.now()
  let t = t0
  do {
    for (let i = 0; i < r.step; i++) r.frame()
    r.settle()
    r.frames += r.step
    t = performance.now()
  } while (t - t0 < SPAN_MS)
  r.ms += t - t0
}
