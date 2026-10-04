// The renderer's probes (ui/src/spectrum): the same pixel fixtures through BOTH implementations, a
// forced WebGL2 context loss, and the renderer's budget. The renderer is mounted bare (it has no
// component yet), fed rows directly, and read back by copying its canvas into a 2D one in the task
// that drew it, which works for either backend.
//
// Modes (run.mjs drives them): `render` (one fixture, one backend), `capability`, `loss`, `rperf`.
import { createSpectrumRenderer, type SpectrumRenderer, type SpectrumScene } from '../src/spectrum'
import { bakeLut, resolveColormap } from '../src/waterfall'
import { dbfs, INDEXED_SETS, RENDERER_SETS, TIMED_SETS, timedFrame, type Frame } from './frames'

type Backend = 'webgl2' | 'canvas2d'

/** The fixed display range every fixture is drawn in (−100..−40 dBFS), so a picture depends on the
 *  renderer alone and never on an AGC. */
const RANGE = { floor: dbfs(-100), ceil: dbfs(-40) }

interface Fixture {
  set: string
  w: number
  h: number
  /** The trace band's height; 0 = the digital waterfall's layout. */
  traceH: number
  stripH: number
  mode: '2d' | 'dss'
  detector: 'peak' | 'average'
  view?: [number, number]
  newestAtTop?: boolean
  offsetRows?: number
}

/** The fixtures, by the id run.mjs names them with. Scope = a trace over a waterfall; wf = the
 *  digital cockpits' waterfall with its axis strip. */
export const FIXTURES: Record<string, Fixture> = {
  'scope-carrier': { set: 'carrier', w: 320, h: 192, traceH: 86, stripH: 0, mode: '2d', detector: 'peak' },
  'scope-two-tone': { set: 'two-tone', w: 320, h: 192, traceH: 86, stripH: 0, mode: '2d', detector: 'peak' },
  'scope-noise-step': { set: 'noise-step', w: 320, h: 192, traceH: 86, stripH: 0, mode: '2d', detector: 'peak' },
  'wf-ft8-slot': { set: 'ft8-slot', w: 320, h: 160, traceH: 0, stripH: 18, mode: '2d', detector: 'peak' },
  // History through each row's own span, zoomed in far enough (6.25 Hz a pixel, 7.8 Hz a bin) that
  // the rows interpolate: the carriers stay straight across the QSY.
  'wf-retune': { set: 'retune', w: 320, h: 160, traceH: 0, stripH: 18, mode: '2d', detector: 'peak', view: [1200, 3200] },
  // Scrollback with the flow flipped: newest at the top, 30 rows back.
  'wf-scrollback': { set: 'two-tone', w: 320, h: 160, traceH: 0, stripH: 18, mode: '2d', detector: 'peak', newestAtTop: true, offsetRows: 30 },
  'scope-wide-peak': { set: 'wide', w: 320, h: 192, traceH: 86, stripH: 0, mode: '2d', detector: 'peak' },
  'scope-wide-average': { set: 'wide', w: 320, h: 192, traceH: 86, stripH: 0, mode: '2d', detector: 'average' },
  'dss-two-tone': { set: 'two-tone', w: 320, h: 192, traceH: 0, stripH: 0, mode: 'dss', detector: 'peak' },
}

function setOf(id: string) {
  const set = RENDERER_SETS[id] ?? INDEXED_SETS[id]
  if (!set) throw new Error(`unknown set ${id}`)
  return set
}

const toFrame = (f: Frame, seq: number) => ({ seq, tMs: seq * 50, loHz: f.loHz, hiHz: f.hiHz, bins: f.row, dbPerUnit: 120 })

/** A positioned host of a fixed device-pixel size (the page runs at device scale 1). */
function host(w: number, h: number): HTMLDivElement {
  const el = document.createElement('div')
  el.style.cssText = `position:relative;width:${w}px;height:${h}px;flex:none`
  document.getElementById('root')!.appendChild(el)
  return el
}

function sceneFor(fx: Fixture, lut: Uint8ClampedArray, last: Frame | null, seq: number): SpectrumScene {
  const view = fx.view ?? (last ? [last.loHz, last.hiHz] : [0, 4000])
  return {
    view: { loHz: view[0], hiHz: view[1] },
    lut,
    layout: { traceH: fx.traceH, stripH: fx.stripH, lineWidth: 1 },
    detector: fx.detector,
    mode: fx.mode,
    offsetRows: fx.offsetRows ?? 0,
    newestAtTop: fx.newestAtTop ?? false,
    trace: last && fx.traceH > 0 ? { frame: toFrame(last, seq), range: RANGE } : null,
  }
}

function open(fx: { w: number; h: number }, backend: Backend): SpectrumRenderer {
  const r = createSpectrumRenderer(host(fx.w, fx.h), { backend: backend === 'canvas2d' ? 'canvas2d' : 'auto' })
  if (r.backend !== backend) throw new Error(`asked for ${backend}, got ${r.backend} (${r.reason})`)
  r.resize(fx.w, fx.h)
  return r
}

/** Feed rows `from..to-1` of a set, drawing after each as a live host would. */
function feed(r: SpectrumRenderer, fx: Fixture, lut: Uint8ClampedArray, set: ReturnType<typeof setOf>, from: number, to: number): Frame | null {
  let last: Frame | null = null
  for (let i = from; i < to; i++) {
    last = set.frame(i)
    r.commitRow(toFrame(last, i), RANGE)
    r.draw(sceneFor(fx, lut, last, i))
  }
  return last
}

/** The renderer's picture, read in the task that drew it. */
function picture(r: SpectrumRenderer): { w: number; h: number; b64: string } {
  const src = r.canvas!
  const c = document.createElement('canvas')
  c.width = src.width
  c.height = src.height
  const ctx = c.getContext('2d')!
  ctx.drawImage(src, 0, 0)
  const px = ctx.getImageData(0, 0, c.width, c.height).data
  let s = ''
  for (let i = 0; i < px.length; i += 0x8000) s += String.fromCharCode(...px.subarray(i, i + 0x8000))
  return { w: c.width, h: c.height, b64: btoa(s) }
}

/** One fixture through one backend. `set` may be overridden (the 3-D burst check). */
export async function render(q: URLSearchParams, palette: string) {
  const id = q.get('fixture') ?? ''
  const base = FIXTURES[id]
  if (!base) throw new Error(`unknown fixture ${id}`)
  const fx = { ...base, set: q.get('set') ?? base.set }
  const backend = (q.get('backend') ?? 'webgl2') as Backend
  const lut = bakeLut(resolveColormap(palette, 'dark', false, null))
  const r = open(fx, backend)
  const set = setOf(fx.set)
  const last = feed(r, fx, lut, set, 0, set.count)
  // `redraw=N`: N more draws of the same scene, as a host drawing at display rate between rows does.
  const redraw = 1 + Number(q.get('redraw') ?? 0)
  for (let i = 0; i < redraw; i++) r.draw(sceneFor(fx, lut, last, set.count - 1))
  const pic = picture(r)
  const out = { ...pic, backend: r.backend, reason: r.reason, rows: r.rows, traceH: fx.traceH, mode: fx.mode }
  r.destroy()
  return out
}

/**
 * Which backend the renderer picks, on a healthy context and on two broken ones: `break=context`
 * (no WebGL2 context at all) and `break=upload` (a context that takes float uploads and keeps
 * nothing, the kind of driver fault a context check alone never sees). The broken ones are the
 * capability gate's positive controls: each must end on canvas-2D, and say why.
 */
export async function capability(q: URLSearchParams) {
  const broken = q.get('break') ?? 'none'
  if (broken === 'context') {
    const get = HTMLCanvasElement.prototype.getContext
    HTMLCanvasElement.prototype.getContext = function (this: HTMLCanvasElement, id: string, ...rest: unknown[]) {
      return id === 'webgl2' ? null : (get as (...a: unknown[]) => unknown).call(this, id, ...rest)
    } as typeof get
  } else if (broken === 'upload') {
    const sub = WebGL2RenderingContext.prototype.texSubImage2D
    WebGL2RenderingContext.prototype.texSubImage2D = function (this: WebGL2RenderingContext, ...a: unknown[]) {
      if (a.length >= 9 && a[6] === this.RED && a[7] === this.FLOAT) {
        a[8] = new Float32Array((a[4] as number) * (a[5] as number))
        a[9] = 0
      }
      return (sub as (...x: unknown[]) => void).apply(this, a)
    } as typeof sub
  }
  const r = createSpectrumRenderer(host(64, 32))
  const out = { broken, backend: r.backend, reason: r.reason }
  r.destroy()
  return out
}

/**
 * A forced context loss. A WebGL2 renderer draws rows 0–59; its context is lost (WEBGL_lose_context);
 * the renderer must go on drawing rows 60–89 on canvas-2D from the same history; the context is
 * restored and the renderer must take WebGL2 back, with no reload, and draw rows 90–119. Its picture
 * then has to equal a renderer that never lost anything, fed the same 120 rows, and the stand-in's
 * picture has to equal a canvas-2D renderer fed rows 0–89.
 *
 * `dropRestore=1` is the positive control: the page removes every `webglcontextrestored` listener
 * the renderer adds (the code path a forgotten handler leaves), and the check must then fail.
 */
export async function loss(q: URLSearchParams, palette: string) {
  if (q.get('dropRestore') === '1') {
    const add = HTMLCanvasElement.prototype.addEventListener
    HTMLCanvasElement.prototype.addEventListener = function (this: HTMLCanvasElement, type: string, ...rest: unknown[]) {
      if (type === 'webglcontextrestored') return
      return (add as (...a: unknown[]) => void).call(this, type, ...rest)
    } as typeof add
  }
  const fx = FIXTURES['scope-carrier']
  const lut = bakeLut(resolveColormap(palette, 'dark', false, null))
  const set = setOf('two-tone')
  const r = open(fx, 'webgl2')
  feed(r, fx, lut, set, 0, 60)
  const glCanvas = r.canvas!
  const ext = glCanvas.getContext('webgl2')!.getExtension('WEBGL_lose_context')
  if (!ext) throw new Error('no WEBGL_lose_context on this context')
  const t0 = performance.now()
  ext.loseContext()
  const wait = async (want: string, ms: number) => {
    const end = performance.now() + ms
    while ((r.backend as string) !== want && performance.now() < end) await new Promise((ok) => setTimeout(ok, 10))
    return (r.backend as string) === want
  }
  const fellBack = await wait('canvas2d', 2000)
  const fallbackMs = performance.now() - t0
  const reasonWhileLost = r.reason
  const visible = [...glCanvas.parentElement!.querySelectorAll('canvas')].filter((c) => getComputedStyle(c).visibility !== 'hidden').length
  feed(r, fx, lut, set, 60, 90)
  const standIn = fellBack ? picture(r) : null
  const t1 = performance.now()
  ext.restoreContext()
  const restored = await wait('webgl2', 5000)
  const restoreMs = performance.now() - t1
  const last = feed(r, fx, lut, set, 90, 120)
  r.draw(sceneFor(fx, lut, last, 119))
  const after = picture(r)
  const result = {
    fellBack,
    fallbackMs: Math.round(fallbackMs),
    reasonWhileLost,
    visibleCanvasesWhileLost: visible,
    restored,
    restoreMs: Math.round(restoreMs),
    backendAtEnd: r.backend,
    reasonAtEnd: r.reason,
    rows: r.rows,
    after,
    standIn,
    reference: null as null | { w: number; h: number; b64: string },
    standInReference: null as null | { w: number; h: number; b64: string },
  }
  r.destroy()
  // The references: never lost, and canvas-2D fed what the stand-in had when it was read.
  const ref = open(fx, 'webgl2')
  const refLast = feed(ref, fx, lut, set, 0, 120)
  ref.draw(sceneFor(fx, lut, refLast, 119))
  result.reference = picture(ref)
  ref.destroy()
  const cpu = open(fx, 'canvas2d')
  feed(cpu, fx, lut, set, 0, 90)
  result.standInReference = picture(cpu)
  cpu.destroy()
  return result
}

/**
 * The budget: under 2 ms of GPU and 1 ms of main thread a frame at 2048 bins × 2048 rows, filling a
 * 1024×768 window (the supported floor). The ring is filled first, then every animation frame commits
 * one row (a source at least as fast as the display, the worst case) and redraws. Measured between
 * `start()` and `stop()` as the component perf probe is; `gpu()` then measures how long a frame takes
 * to reach pixels by reading one back after each draw, which stalls the main thread and is therefore
 * a separate run.
 */
export async function rperf(q: URLSearchParams, palette: string, H: { ready?: boolean; start?: () => void; stop?: () => unknown; gpu?: () => Promise<unknown> }) {
  const backend = (q.get('backend') ?? 'webgl2') as Backend
  const mode = q.get('layout') === 'dss' ? 'dss' : '2d'
  const depth = Number(q.get('depth') ?? 2048)
  const lut = bakeLut(resolveColormap(palette, 'dark', false, null))
  const el = document.createElement('div')
  el.style.cssText = 'position:relative;flex:1;min-height:0'
  const root = document.getElementById('root')!
  root.className = 'hx-viewport'
  root.appendChild(el)
  const rect = el.getBoundingClientRect()
  const w = Math.round(rect.width * devicePixelRatio)
  const h = Math.round(rect.height * devicePixelRatio)
  const r = createSpectrumRenderer(el, { backend: backend === 'canvas2d' ? 'canvas2d' : 'auto', depth })
  if (r.backend !== backend) throw new Error(`asked for ${backend}, got ${r.backend} (${r.reason})`)
  r.resize(w, h)
  const set = TIMED_SETS['flex-15']
  // Built before the clock starts, so the measured cost is the renderer's, not the harness's.
  const pool = Array.from({ length: 64 }, (_, s) => {
    const f = timedFrame(set, s, false)
    return { loHz: f.loHz, hiHz: f.hiHz, bins: Float32Array.from(f.row), dbPerUnit: 120 }
  })
  const frameOf = (seq: number) => ({ seq, tMs: seq, ...pool[seq % pool.length] })
  const range = { floor: 0.15, ceil: 0.85 }
  let seq = 0
  for (; seq < depth; seq++) r.commitRow(frameOf(seq), range)
  const scene = (): SpectrumScene => ({
    view: { loHz: set.loHz, hiHz: set.hiHz },
    lut,
    layout: { traceH: Math.round(h * 0.45), stripH: 0, lineWidth: 1 },
    detector: 'peak',
    mode,
    offsetRows: 0,
    newestAtTop: false,
    trace: { frame: frameOf(seq), range },
  })
  r.draw(scene())
  const cost: number[] = []
  const stamps: number[] = []
  let recording = false
  let running = true
  const tick = (ts: number) => {
    if (!running) return
    const t = performance.now()
    r.commitRow(frameOf(seq++), range)
    r.draw(scene())
    if (recording) {
      cost.push(performance.now() - t)
      stamps.push(ts)
    }
    requestAnimationFrame(tick)
  }
  requestAnimationFrame(tick)
  await new Promise((ok) => setTimeout(ok, 1000))
  let startAt = 0
  H.start = () => {
    cost.length = 0
    stamps.length = 0
    recording = true
    startAt = performance.now()
  }
  H.stop = () => {
    recording = false
    const secs = (performance.now() - startAt) / 1000
    const gaps = stamps.slice(1).map((t, i) => t - stamps[i])
    return { backend: r.backend, mode, canvas: { w, h }, depth, bins: set.bins, rows: r.rows, frames: stamps.length, fps: stamps.length / secs, frameGapMs: dist(gaps), mainMs: dist(cost) }
  }
  H.gpu = async () => {
    running = false
    await new Promise((ok) => setTimeout(ok, 100))
    // Draw-to-pixels, one frame at a time: draw, then read one pixel back, which waits for the
    // frame's GPU work. On SwiftShader that work is the CPU rasteriser's. The timer query is used
    // as well where the context offers one.
    const gl = backend === 'webgl2' ? r.canvas!.getContext('webgl2') : null
    const tq = gl?.getExtension('EXT_disjoint_timer_query_webgl2') ?? null
    const ctx2d = backend === 'canvas2d' ? r.canvas!.getContext('2d') : null
    const sync: number[] = []
    const px = new Uint8Array(4)
    for (let i = 0; i < 40; i++) {
      await new Promise((ok) => requestAnimationFrame(ok))
      const t = performance.now()
      r.commitRow(frameOf(seq++), range)
      r.draw(scene())
      if (gl) gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px)
      else ctx2d?.getImageData(0, 0, 1, 1)
      sync.push(performance.now() - t)
    }
    const timer: number[] = []
    if (gl && tq) {
      for (let i = 0; i < 40; i++) {
        await new Promise((ok) => requestAnimationFrame(ok))
        const query = gl.createQuery()!
        gl.beginQuery(tq.TIME_ELAPSED_EXT, query)
        r.commitRow(frameOf(seq++), range)
        r.draw(scene())
        gl.endQuery(tq.TIME_ELAPSED_EXT)
        for (let k = 0; k < 100 && !gl.getQueryParameter(query, gl.QUERY_RESULT_AVAILABLE); k++) await new Promise((ok) => setTimeout(ok, 5))
        if (gl.getQueryParameter(query, gl.QUERY_RESULT_AVAILABLE) && !gl.getParameter(tq.GPU_DISJOINT_EXT)) timer.push(gl.getQueryParameter(query, gl.QUERY_RESULT) / 1e6)
        gl.deleteQuery(query)
      }
    }
    return { drawToPixelsMs: dist(sync), timerQuery: tq ? dist(timer) : null }
  }
  H.ready = true
  return null
}

function pct(xs: number[], p: number): number {
  if (xs.length === 0) return NaN
  const s = [...xs].sort((a, b) => a - b)
  return s[Math.min(s.length - 1, Math.max(0, Math.round(p * (s.length - 1))))]
}
const r3 = (x: number) => Math.round(x * 1000) / 1000
function dist(xs: number[]) {
  return { n: xs.length, p50: r3(pct(xs, 0.5)), p95: r3(pct(xs, 0.95)), max: r3(xs.length ? Math.max(...xs) : NaN) }
}
