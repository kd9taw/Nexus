// The harness page: mounts the shipped PhoneScope or Waterfall, unmodified, in a real browser, with
// the desktop IPC bridge stood in by synthetic frames and the REAL clock left alone (a virtual-time
// budget starves a requestAnimationFrame loop, which is exactly what these components are).
//
// The stand-in sits where Tauri does — `window.__TAURI_INTERNALS__.invoke` — so everything above it
// is production code: api.ts's `getScopeRow`/`getSpectrumRow`, the fetch latch, the draw loop.
// Each answer is parsed from the JSON the backend would send, in a later task, as a real IPC reply
// arrives. It also answers the frame command (`get_scope_frame`) as the backend does: a sweep's
// number is its index + 1, and an ask naming a sweep the source has not advanced past gets null.
//
// The page reads its job from the URL and reports through `window.__harness`, which run.mjs polls
// over the DevTools protocol. Modes: `backend`, `pixel`, `cadence`, `perf`, `ipc` (see run.mjs).
//
// PhoneScope draws through the spectrum renderer, on a canvas of the renderer's own under its overlay,
// so its picture is read from THAT canvas (WebGL2 or canvas-2D, whichever is drawing) by copying it
// into a 2D one; the Waterfall's and MiniSpectrum's, the renderer's too, are read the same way.
import '../src/styles.css'
import { useMemo, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { PhoneScope } from '../src/components/PhoneScope'
import type { FtOverlay, ScopeSpot } from '../src/spectrum/overlays'
import { Waterfall } from '../src/components/Waterfall'
import { MiniSpectrum } from '../src/components/MiniSpectrum'
import { SpectrumRing } from '../src/spectrum/ring'
import { WSPR_WATERFALL_WINDOW, bakeLut, resolveColormap } from '../src/waterfall'
import { WF_PALETTE_KEY } from '../src/waterfallPalette'
import { axis, capability, loss, render, rperf } from './renderer'
import {
  CODE_SLOTS,
  INDEXED_SETS,
  PAD_ROWS,
  TIMED_SETS,
  decodeBits,
  f32Json,
  frameJson,
  frameReply,
  frameTail,
  indexedFrame,
  prng,
  slotCentre,
  timedFrame,
} from './frames'

interface HarnessState {
  done: boolean
  ready?: boolean
  error?: string
  result?: unknown
  start?: () => void
  stop?: () => unknown
  gpu?: () => Promise<unknown>
  setRx?: (hz: number) => Promise<void>
  box?: () => { left: number; top: number; width: number; height: number; drawnBy: string }
  clicks?: () => unknown[]
}
declare global {
  interface Window {
    __harness?: HarnessState
  }
}

const H: HarnessState = { done: false }
window.__harness = H
const fail = (why: string) => {
  if (!H.done) Object.assign(H, { done: true, error: why })
}
window.addEventListener('error', (e) => fail(`page error: ${e.message}`))
window.addEventListener('unhandledrejection', (e) => fail(`unhandled rejection: ${String(e.reason)}`))

const q = new URLSearchParams(location.search)
const mode = q.get('mode') ?? 'backend'
const comp = q.get('comp') === 'waterfall' ? 'waterfall' : q.get('comp') === 'minispectrum' ? 'minispectrum' : 'phonescope'
const palette = q.get('palette') ?? 'turbo'
const THEME = 'dark'

// The palette is pinned, not left to the default, so a change of default is not a fixture failure.
localStorage.setItem(WF_PALETTE_KEY, palette)
// PhoneScope's slow-scope look (`smooth`, the default, or `sweep`), when the probe names one.
const rowsLook = q.get('rows')
if (rowsLook) localStorage.setItem('nexus.phonescope.rows', rowsLook)
// The spectrum renderer's hidden backend setting (src/spectrum/choose.ts). WebGL2 on this browser is
// SwiftShader, a software rasteriser, so the renderer's own choice here is canvas-2D; PhoneScope's
// pictures and cadence were recorded on WebGL2 and keep being checked there, by this setting. `perf`
// is left to the renderer's choice, so it measures what an operator on this machine gets. Storage
// outlives the page in this profile, so every page sets the key or clears it.
// The offsets probe's WebGL2 arm (`backend=webgl2`) asks for WebGL2 the same way: its clicks are checked on both backends.
const backendSetting = q.get('setting') ?? (mode === 'pixel' || mode === 'cadence' || (mode === 'offsets' && q.get('backend') === 'webgl2') ? 'webgl2' : null)
if (backendSetting) localStorage.setItem('nexus.spectrum.backend', backendSetting)
else localStorage.removeItem('nexus.spectrum.backend')

// The renderer draws WebGL2 without preserving its drawing buffer, so a picture read in a later task
// than the one that drew it could come back blank. The probes that read PhoneScope's picture ask for
// a preserved buffer instead: the same pixels, kept until the next draw.
if (mode === 'pixel' || mode === 'cadence') {
  const getContext = HTMLCanvasElement.prototype.getContext as (this: HTMLCanvasElement, kind: string, attrs?: object) => unknown
  HTMLCanvasElement.prototype.getContext = function (this: HTMLCanvasElement, kind: string, attrs?: object) {
    return getContext.call(this, kind, kind === 'webgl2' ? { ...attrs, preserveDrawingBuffer: true } : attrs)
  } as typeof HTMLCanvasElement.prototype.getContext
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))
const frame = () => new Promise<number>((r) => requestAnimationFrame(r))
async function until(test: () => boolean, ms: number, what: string): Promise<void> {
  const end = performance.now() + ms
  while (!test()) {
    if (performance.now() > end) throw new Error(`timed out waiting for ${what}`)
    await sleep(20)
  }
}

function pct(xs: number[], p: number): number {
  if (xs.length === 0) return NaN
  const s = [...xs].sort((a, b) => a - b)
  return s[Math.min(s.length - 1, Math.max(0, Math.round(p * (s.length - 1))))]
}
const r3 = (x: number) => Math.round(x * 1000) / 1000
function dist(xs: number[]) {
  return {
    n: xs.length,
    min: r3(xs.length ? Math.min(...xs) : NaN),
    p50: r3(pct(xs, 0.5)),
    p95: r3(pct(xs, 0.95)),
    max: r3(xs.length ? Math.max(...xs) : NaN),
  }
}

// ---------------------------------------------------------------------------------------------
// The IPC stand-in.

/**
 * What the source hands back for one ask: a sweep, or null = refuse it. A sweep is its serialised
 * `Spectrum` for the row commands, and its number, time and `frameTail` for the frame command.
 * `'unchanged'` answers a frame ask that names a sweep the source has not advanced past: the
 * backend's null, which is not a row.
 */
type Answer = { json: string; seq: number; tMs: number; tail: string } | 'unchanged' | null
/** The sweep the caller last drew, on a frame ask; `undefined` on a row ask. */
type Drawn = number | undefined
const unexpected: string[] = []
/** Per answered row: the JSON parse, and the component's own work up to the next task. */
const rowCost: { parseMs: number; rowMs: number }[] = []
let collectRowCost = false
/** Wall-clock milliseconds for a page time, as the backend stamps `tMs`. */
const wallMs = (pageMs: number) => Math.round(performance.timeOrigin + pageMs)

/** `extra`: stand-ins for the few other commands a probe's mount asks (the overlays probe's
 *  `get_privilege_spans`); anything else is still refused and listed in `unexpected`. */
function installIpc(answer: (drawn: Drawn) => Answer, extra: Record<string, (args?: Record<string, unknown>) => unknown> = {}): void {
  const deliver = new MessageChannel()
  const after = new MessageChannel()
  const pending: (() => void)[] = []
  const afterQ: (() => void)[] = []
  deliver.port1.onmessage = () => pending.shift()?.()
  after.port1.onmessage = () => afterQ.shift()?.()
  const invoke = <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
    if (Object.prototype.hasOwnProperty.call(extra, cmd)) return Promise.resolve(extra[cmd](args) as T)
    const asksFrame = cmd === 'get_scope_frame'
    if (!asksFrame && cmd !== 'get_scope_row' && cmd !== 'get_spectrum_row') {
      unexpected.push(cmd)
      return Promise.reject(new Error(`harness: no stand-in for ${cmd}`))
    }
    // The backend refuses a frame ask without its required `lastSeq` (0 = nothing drawn yet).
    const drawn = args?.lastSeq
    if (asksFrame && typeof drawn !== 'number') {
      unexpected.push(`${cmd} without lastSeq`)
      return Promise.reject(new Error(`harness: ${cmd} needs lastSeq`))
    }
    const out = answer(asksFrame ? (drawn as number) : undefined)
    return new Promise<T>((resolve, reject) => {
      pending.push(() => {
        if (!out) {
          reject(new Error('harness: the source has stopped'))
          return
        }
        if (out === 'unchanged') {
          resolve(null as T)
          return
        }
        const json = asksFrame ? frameReply(out.seq, out.tMs, out.tail) : out.json
        const t0 = performance.now()
        const value = JSON.parse(json) as T
        const parseMs = performance.now() - t0
        resolve(value)
        // A task posted now runs after this task's microtasks, i.e. after the component has
        // drawn the row it was waiting for.
        if (collectRowCost) {
          afterQ.push(() => rowCost.push({ parseMs, rowMs: performance.now() - t0 - parseMs }))
          after.port2.postMessage(0)
        }
      })
      deliver.port2.postMessage(0)
    })
  }
  window.__TAURI_INTERNALS__ = { invoke }
}

// ---------------------------------------------------------------------------------------------
// Mounting.

/** The geometry a probe mounts at. `fixed` pins the CANVAS (not the header strip above it, whose
 *  height follows fonts), so a stored picture has the same size on every machine. */
type Shape = 'fixed' | 'cadence' | 'viewport'

function mount(shape: Shape, opts: { fixedWindow?: boolean; rowMs?: number; stillTrace?: boolean } = {}): void {
  const host = document.getElementById('root')!
  host.className = `hx-${shape}`
  // A still trace: no peak hold and no averaging, so the trace is the last row's own shape. Both
  // run on the REAL clock between rows, and a residue of e^-8 of an earlier row, timed by the clock,
  // still moves a WebGL2 trace's anti-aliased edge by a level on a pixel or two from run to run.
  if (opts.stillTrace) localStorage.setItem('nexus.scope.phone', JSON.stringify({ averageMs: 0 }))
  const el =
    comp === 'phonescope' ? (
      // The native-RF props the Phone cockpit passes with its "Full" span (the whole sweep);
      // on an audio row they are the plain 0–4 kHz window. No dial, marker or carrier line, so
      // nothing is drawn as text over the picture.
      <PhoneScope transmitting={false} theme={THEME} viewLoHz={-1e9} viewHiHz={1e9} traceHoldMs={opts.stillTrace ? 0 : undefined} />
    ) : comp === 'minispectrum' ? (
      // A trace of the newest row only, so it is polled as fast as the rows are served.
      <MiniSpectrum pollMs={20} height={96} />
    ) : (
      <Waterfall
        transmitting={false}
        theme={THEME}
        rxOffsetHz={1500}
        txOffsetHz={1500}
        rowMs={opts.rowMs}
        fixedWindow={opts.fixedWindow ? { lo: 0, hi: 4000 } : undefined}
      />
    )
  createRoot(host).render(el)
}

/** The canvas the waterfall rows land on. PhoneScope and the Waterfall both draw through the spectrum
 *  renderer: PhoneScope's canvas is in `.ph-scope-render`, under its overlay; the Waterfall's sits in
 *  its own layer (`.waterfall-render`) over the gesture canvas and under the overlay. The one showing
 *  is the one drawing (a stand-in for a lost context hides the other). */
function rowCanvas(): HTMLCanvasElement {
  if (comp === 'phonescope') {
    const cv = [...document.querySelectorAll<HTMLCanvasElement>('.ph-scope-render canvas')].find((c) => c.style.visibility !== 'hidden')
    if (!cv) throw new Error('no renderer canvas in .ph-scope-render')
    return cv
  }
  // MiniSpectrum's trace is the renderer's too, in the strip's own box.
  const layer = comp === 'minispectrum' ? '.mini-spectrum-canvas' : '.waterfall-render'
  const shown = [...document.querySelectorAll<HTMLCanvasElement>(`${layer} canvas`)].filter((c) => c.style.visibility !== 'hidden')
  if (shown.length !== 1) throw new Error(`${shown.length} renderer canvases showing in the ${comp}`)
  return shown[0]
}

/** Which backend drew the picture (the renderer's canvas has one context or the other). */
function drawnBy(): string {
  const cv = rowCanvas()
  return cv.getContext('webgl2') ? 'webgl2' : cv.getContext('2d') ? 'canvas2d' : 'none'
}

/** `backend=canvas2d`: take WebGL2 away before mounting, so the renderer stands on canvas-2D, the
 *  path a webview without a WebGL2 context takes (the `capability` probe's `break=context`). */
function withoutWebGl2(): void {
  const get = HTMLCanvasElement.prototype.getContext
  HTMLCanvasElement.prototype.getContext = function (this: HTMLCanvasElement, id: string, ...rest: unknown[]) {
    return id === 'webgl2' ? null : (get as (...a: unknown[]) => unknown).call(this, id, ...rest)
  } as typeof HTMLCanvasElement.prototype.getContext
}
const pickBackend = () => {
  if (q.get('backend') === 'canvas2d') withoutWebGl2()
}

/** Keep a WebGL2 canvas's drawing buffer after it is shown, so the picture can be read in a later
 *  task than the frame that drew it, as the pixel and cadence probes read it. Changes no pixel. */
function keepDrawingBuffer(): void {
  const get = HTMLCanvasElement.prototype.getContext
  HTMLCanvasElement.prototype.getContext = function (this: HTMLCanvasElement, id: string, attrs?: Record<string, unknown>) {
    return (get as (...a: unknown[]) => unknown).call(this, id, id === 'webgl2' ? { ...attrs, preserveDrawingBuffer: true } : attrs)
  } as typeof HTMLCanvasElement.prototype.getContext
}

/** A canvas's pixels, whatever its context: copied into a 2D canvas and read there. */
function pixelsOf(cv: HTMLCanvasElement): Uint8ClampedArray {
  const c = document.createElement('canvas')
  c.width = cv.width
  c.height = cv.height
  const ctx = c.getContext('2d')!
  ctx.drawImage(cv, 0, 0)
  return ctx.getImageData(0, 0, c.width, c.height).data
}

function capture(cv: HTMLCanvasElement): { w: number; h: number; b64: string } {
  const px = pixelsOf(cv)
  let s = ''
  for (let i = 0; i < px.length; i += 0x8000) s += String.fromCharCode(...px.subarray(i, i + 0x8000))
  return { w: cv.width, h: cv.height, b64: btoa(s) }
}

// ---------------------------------------------------------------------------------------------
// Modes.

async function backend() {
  const gl = (kind: 'webgl2' | 'webgl') => {
    const c = document.createElement('canvas').getContext(kind) as WebGLRenderingContext | null
    if (!c) return null
    const dbg = c.getExtension('WEBGL_debug_renderer_info')
    return {
      renderer: String(c.getParameter(dbg ? dbg.UNMASKED_RENDERER_WEBGL : c.RENDERER)),
      vendor: String(c.getParameter(dbg ? dbg.UNMASKED_VENDOR_WEBGL : c.VENDOR)),
      version: String(c.getParameter(c.VERSION)),
    }
  }
  return { webgl2: gl('webgl2'), webgl: gl('webgl'), userAgent: navigator.userAgent }
}

/** A fixture: serve its rows by CALL, refuse every ask after the last, then read the canvas. */
async function pixel() {
  const set = INDEXED_SETS[q.get('set') ?? '']
  if (!set) throw new Error(`unknown pixel set ${q.get('set')}`)
  pickBackend()
  keepDrawingBuffer()
  const total = set.count + PAD_ROWS
  const frames = Array.from({ length: total }, (_, i) => indexedFrame(set, i))
  const payloads = frames.map(frameJson)
  const tails = frames.map(frameTail)
  let served = 0
  let refused = false
  // Served by call, so every ask is a sweep the caller has not drawn.
  installIpc(() => {
    if (served < total) {
      const i = served++
      return { json: payloads[i], seq: i + 1, tMs: wallMs(performance.now()), tail: tails[i] }
    }
    refused = true
    return null
  })
  // Rows land on the canvas 1:1 with answers whatever the cadence, so the FT waterfall is run at
  // the 50 ms its live-instrument hosts use rather than its 120 ms default: same picture, sooner.
  mount('fixed', { rowMs: 50, stillTrace: true })
  // The ask after the last row starts only once that row's draw has finished.
  await until(() => refused, 60_000, `${total} rows`)
  // The waterfall draws a committed row on the next frame (it draws nothing in between).
  await frame()
  await frame()
  return { served, rows: total, ...capture(rowCanvas()), drawnBy: drawnBy(), unexpected }
}

/** Map a canvas pixel back to the palette index it was painted with. */
function lutIndexer() {
  const lut = bakeLut(resolveColormap(palette, THEME, false, null))
  const cache = new Map<number, number>()
  return (r: number, g: number, b: number): number => {
    const key = (r << 16) | (g << 8) | b
    const hit = cache.get(key)
    if (hit !== undefined) return hit
    let best = 0
    let bestD = Infinity
    for (let i = 0; i < 256; i++) {
      const d = (lut[i * 4] - r) ** 2 + (lut[i * 4 + 1] - g) ** 2 + (lut[i * 4 + 2] - b) ** 2
      if (d < bestD) {
        bestD = d
        best = i
      }
    }
    cache.set(key, best)
    return best
  }
}

/** Read every pixel row of the canvas as a barcode: a sweep number, the terminator, or nothing. */
function decodeRows(cv: HTMLCanvasElement): (number | 'end' | null)[] {
  const W = cv.width
  const px = pixelsOf(cv)
  const index = lutIndexer()
  // The drawn view is the whole row for every mount here (PhoneScope's Full span, the Waterfall's
  // pinned 0–4 kHz window), so a slot's x is its fraction of the row.
  const xs = Array.from({ length: CODE_SLOTS }, (_, s) => Math.round(slotCentre(s) * W))
  const rows: (number | 'end' | null)[] = []
  for (let y = 0; y < cv.height; y++) {
    const bits = xs.map((x) => {
      let r = 0
      let g = 0
      let b = 0
      for (let dx = -1; dx <= 1; dx++) {
        const o = (y * W + Math.min(W - 1, Math.max(0, x + dx))) * 4
        r += px[o]
        g += px[o + 1]
        b += px[o + 2]
      }
      return index(Math.round(r / 3), Math.round(g / 3), Math.round(b / 3)) > 127
    })
    rows.push(decodeBits(bits))
  }
  return rows
}

/**
 * The cadence probe. A timed source publishes sweeps on the real clock for `window` ms; the page
 * then serves ONE terminator row and refuses everything after it, and reads the canvas back.
 *
 * The terminator marks the top of the waterfall band whatever the component's geometry: it was
 * committed once, so the lowest pixel row showing it is the band's newest row (a trace drawn above
 * the band can show it too, never below). Every row under it is a committed row, newest first.
 *
 * Reported: rows committed, the distinct sweeps they show, and REPEATS — a committed row showing
 * the same sweep as the row before it, i.e. the waterfall advancing with no new data. The stand-in's
 * own log is the cross-check: every answered ask is one committed row, and under PhoneScope's smooth
 * scroll so is every ask the source had nothing new for.
 *
 * MARKED or not. Where the component draws through the spectrum renderer, every row it commits is
 * logged as it enters the renderer's ring, with the frame number it carries; the band's rows are
 * matched to that log newest first. A repeat is MARKED when its ring row carries the number of the
 * row before it (smooth scroll's repeat: the sweep again, under its own number), and UNMARKED when
 * the number differs — a row that claims to be new data and shows an old sweep, which is the defect.
 * A row whose number is not the sweep its pixels show is MISALIGNED: the history would be lying
 * about what is on screen.
 */
async function cadence() {
  const set = TIMED_SETS[q.get('set') ?? '']
  if (!set) throw new Error(`unknown timed set ${q.get('set')}`)
  pickBackend()
  keepDrawingBuffer()
  const windowMs = Number(q.get('window') ?? 6000)
  const plant = Number(q.get('plant') ?? 0)
  // Sweeps whose time starts inside the window.
  const published = Math.ceil((windowMs * set.rate) / 1000)
  const frames = Array.from({ length: published }, (_, s) => timedFrame(set, s, true))
  const payloads = frames.map(frameJson)
  const tails = frames.map(frameTail)
  const end = timedFrame(set, null, true)
  const terminator = frameJson(end)
  const terminatorTail = frameTail(end)
  // The log holds the asks that were answered with a sweep: a frame ask answered null is no row.
  const log: { tMs: number; seq: number }[] = []
  /** Frame asks inside the window the source had nothing new for (answered null). */
  let unchanged = 0
  // Every row entering a renderer's ring, in commit order, with the number it carries.
  const ring: number[] = []
  const push = SpectrumRing.prototype.push
  SpectrumRing.prototype.push = function (this: SpectrumRing, frame, range) {
    ring.push(frame.seq)
    return push.call(this, frame, range)
  }
  let t0 = -1
  let lastSeq = -1
  let endServed = false
  let stopped = false
  installIpc((drawn) => {
    const now = performance.now()
    if (t0 < 0) t0 = now
    if (now - t0 < windowMs) {
      const due = Math.min(published - 1, Math.floor(((now - t0) * set.rate) / 1000))
      if (drawn !== undefined && due + 1 <= drawn) {
        unchanged++
        return 'unchanged'
      }
      // The positive control: the ask numbered `plant` gets the sweep before it AGAIN — one extra
      // row with no new data, which the probe must find. A frame ask gets it under the new sweep's
      // number, the one way such a row could reach a scope that commits only new frames.
      let seq = due
      if (plant > 0 && log.length + 1 === plant && lastSeq >= 0) seq = lastSeq
      lastSeq = seq
      log.push({ tMs: r3(now - t0), seq })
      return { json: payloads[seq], seq: due + 1, tMs: wallMs(t0 + (due * 1000) / set.rate), tail: tails[seq] }
    }
    if (!endServed) {
      endServed = true
      return { json: terminator, seq: published + 1, tMs: wallMs(now), tail: terminatorTail }
    }
    stopped = true
    return null
  })
  mount('cadence', { fixedWindow: true })
  await until(() => stopped, windowMs + 15_000, 'the cadence window to close')
  await frame()
  await frame()
  const cv = rowCanvas()
  const rows = decodeRows(cv)
  const top = rows.lastIndexOf('end')
  if (top < 0) throw new Error('no terminator row on the canvas: the waterfall band could not be located')
  const seqs: number[] = []
  for (let y = top + 1; y < rows.length; y++) {
    const d = rows[y]
    if (typeof d !== 'number') break
    seqs.push(d)
  }
  const repeats: { row: number; seq: number }[] = []
  let outOfOrder = 0
  for (let i = 0; i + 1 < seqs.length; i++) {
    if (seqs[i] === seqs[i + 1]) repeats.push({ row: i + 1, seq: seqs[i] })
    if (seqs[i] < seqs[i + 1]) outOfOrder++
  }
  // The band against the ring, newest first: the ring's last row is the terminator (its number is
  // `published + 1`), and band row i is the ring row i + 1 before it. A sweep's number is its index
  // + 1 here, so a row whose pixels show sweep k must carry k + 1. Only PhoneScope's ring carries the
  // source's numbers: the Waterfall numbers its rows itself (its row command numbers nothing), so its
  // ring says nothing about which sweep a row shows and is not read.
  let marks: { rows: number; terminator: boolean; misaligned: number; marked: number; unmarked: number; unmarkedRows: number[] } | null = null
  if (comp === 'phonescope' && ring.length > 0) {
    const t = ring.length - 1
    const at = (i: number) => ring[t - 1 - i]
    let misaligned = 0
    let marked = 0
    const unmarkedRows: number[] = []
    for (let i = 0; i < seqs.length; i++) if (at(i) !== seqs[i] + 1) misaligned++
    for (const r of repeats) {
      if (at(r.row - 1) === at(r.row)) marked++
      else unmarkedRows.push(r.row)
    }
    marks = { rows: ring.length, terminator: ring[t] === published + 1, misaligned, marked, unmarked: unmarkedRows.length, unmarkedRows: unmarkedRows.slice(0, 5) }
  }
  // How often the component asks: the row cadence it actually runs at, which is not its constant.
  const askGaps = log.slice(1).map((e, i) => e.tMs - log[i].tMs)
  const apiDistinct = new Set(log.map((e) => e.seq)).size
  let apiRepeats = 0
  for (let i = 1; i < log.length; i++) if (log[i].seq === log[i - 1].seq) apiRepeats++
  return {
    set: set.id,
    rate: set.rate,
    windowMs,
    published,
    canvas: { w: cv.width, h: cv.height, bandTop: top + 1 },
    drawnBy: drawnBy(),
    committed: seqs.length,
    distinct: new Set(seqs).size,
    repeats: repeats.length,
    repeatRows: repeats.slice(0, 5),
    outOfOrder,
    askGapMs: dist(askGaps),
    api: { answered: log.length, distinct: apiDistinct, repeats: apiRepeats, unchanged },
    marks,
    unexpected,
  }
}

/**
 * The perf probe: mount filling the viewport, serve a timed source, then measure between
 * `start()` and `stop()` (run.mjs brackets them with the DevTools main-thread counters).
 */
async function perf() {
  const set = TIMED_SETS[q.get('set') ?? '']
  if (!set) throw new Error(`unknown timed set ${q.get('set')}`)
  pickBackend()
  // A pool built BEFORE the clock starts: formatting floats is the backend's work, not the page's.
  const frames = Array.from({ length: 64 }, (_, s) => timedFrame(set, s, false))
  const pool = frames.map(frameJson)
  const tails = frames.map(frameTail)
  let t0 = -1
  let answered = 0
  installIpc((drawn) => {
    const now = performance.now()
    if (t0 < 0) t0 = now
    const due = Math.floor(((now - t0) * set.rate) / 1000)
    if (drawn !== undefined && due + 1 <= drawn) return 'unchanged'
    answered++
    const k = due % pool.length
    return { json: pool[k], seq: due + 1, tMs: wallMs(t0 + (due * 1000) / set.rate), tail: tails[k] }
  })
  mount('viewport')
  const loaf: PerformanceEntry[] = []
  try {
    new PerformanceObserver((l) => loaf.push(...l.getEntries())).observe({ type: 'long-animation-frame' })
  } catch {
    /* not supported: reported as null below */
  }
  const loafSupported = PerformanceObserver.supportedEntryTypes.includes('long-animation-frame')
  const stamps: number[] = []
  let recording = false
  const tick = (ts: number) => {
    if (recording) stamps.push(ts)
    requestAnimationFrame(tick)
  }
  requestAnimationFrame(tick)
  await sleep(1000)
  // A zero from the long-frame observer means nothing until it has been seen to fire: hold one
  // frame for 80 ms during the warm-up and require an entry for it.
  requestAnimationFrame(() => {
    const until = performance.now() + 80
    while (performance.now() < until) {
      /* the planted long frame */
    }
  })
  await sleep(500)
  const loafWorks = loafSupported && loaf.some((e) => e.duration >= 80)
  let startAt = 0
  let answeredAtStart = 0
  H.start = () => {
    stamps.length = 0
    rowCost.length = 0
    loaf.length = 0
    collectRowCost = true
    recording = true
    startAt = performance.now()
    answeredAtStart = answered
  }
  H.stop = () => {
    recording = false
    collectRowCost = false
    const secs = (performance.now() - startAt) / 1000
    const gaps = stamps.slice(1).map((t, i) => t - stamps[i])
    const lf = loaf as (PerformanceEntry & { blockingDuration?: number })[]
    const cv = rowCanvas()
    return {
      set: set.id,
      secs: r3(secs),
      canvas: { w: cv.width, h: cv.height },
      // The context the picture is drawn with: the renderer's choice, for PhoneScope.
      drawnOn: cv.getContext('webgl2') ? 'webgl2' : '2d',
      drawnBy: drawnBy(),
      frames: stamps.length,
      fps: r3(stamps.length / secs),
      frameGapMs: dist(gaps),
      over25ms: gaps.filter((g) => g > 25).length,
      longFrames: loafWorks
        ? {
            n: lf.length,
            maxMs: r3(Math.max(0, ...lf.map((e) => e.duration))),
            blockingMs: r3(lf.reduce((a, e) => a + (e.blockingDuration ?? 0), 0)),
          }
        : null,
      rows: answered - answeredAtStart,
      rowsPerSec: r3((answered - answeredAtStart) / secs),
      parseMs: dist(rowCost.map((c) => c.parseMs)),
      rowMs: dist(rowCost.map((c) => c.rowMs)),
    }
  }
  H.ready = true
  return null
}

/**
 * IPC cost at display rate: on every animation frame, parse one `Spectrum` of `bins` values from
 * the JSON the backend writes — what a 60 Hz pull of native-resolution rows would cost the main
 * thread — and, beside it, take the same values from a binary buffer, the alternative transport.
 */
async function ipc() {
  const bins = Number(q.get('bins') ?? 512)
  const windowMs = Number(q.get('window') ?? 5000)
  const r = prng(bins)
  // Full-precision values, as a computed spectrum has (CI-V's byte/160 would be shorter).
  const values = () => Array.from({ length: bins }, () => Math.fround(0.15 + 0.7 * r()))
  const json = Array.from({ length: 64 }, () => {
    const row = values()
    return `{"row":[${row.map(f32Json).join(',')}],"loHz":0.0,"hiHz":4000.0,"source":"audio"}`
  })
  const bin = Array.from({ length: 64 }, () => new Float32Array(values()).buffer)
  const parse: number[] = []
  const binary: number[] = []
  const stamps: number[] = []
  let sink = 0
  const end = performance.now() + windowMs
  await new Promise<void>((done) => {
    let k = 0
    const tick = (ts: number) => {
      stamps.push(ts)
      let t = performance.now()
      const v = JSON.parse(json[k % 64]) as { row: number[] }
      for (const x of v.row) sink += x
      parse.push(performance.now() - t)
      t = performance.now()
      const f = new Float32Array(bin[k % 64].slice(0))
      for (const x of f) sink += x
      binary.push(performance.now() - t)
      k++
      if (performance.now() < end) requestAnimationFrame(tick)
      else done()
    }
    requestAnimationFrame(tick)
  })
  const secs = (stamps[stamps.length - 1] - stamps[0]) / 1000
  const perSec = stamps.length / secs
  const bytes = Math.round(json.reduce((a, s) => a + s.length, 0) / json.length)
  return {
    bins,
    calls: stamps.length,
    callsPerSec: r3(perSec),
    jsonBytes: bytes,
    jsonBytesPerSec: Math.round(bytes * perSec),
    binaryBytes: bins * 4,
    parseMs: dist(parse),
    parseMsPerSec: r3(parse.reduce((a, x) => a + x, 0) / secs),
    binaryMs: dist(binary),
    sink: sink > 0,
  }
}

// ---------------------------------------------------------------------------------------------
// The offsets probe: what a click on the waterfall sets, through real input events.

/**
 * The waterfall mounted with a live audio source under it (so the picture under the pointer is a
 * real drawn one) and an RX offset run.mjs can move. run.mjs dispatches real mouse events over the
 * DevTools protocol; the page logs every mousedown as it reaches the document: where it was, which
 * element the browser hit, whether the waterfall cancelled its default, and what `onTune` was handed
 * during it. `backend=canvas2d` takes WebGL2 away first; `zoom` is the picker's span, or `wspr` for
 * WSPR's fixed sub-band. Mounted as `comp=waterfall`.
 */
async function offsets() {
  pickBackend()
  const zoom = q.get('zoom') ?? '0'
  if (zoom !== 'wspr') localStorage.setItem('nexus.waterfall.zoom', zoom)
  const set = TIMED_SETS['audio-50']
  const pool = Array.from({ length: 64 }, (_, s) => timedFrame(set, s, false))
  const payloads = pool.map(frameJson)
  const tails = pool.map(frameTail)
  let n = 0
  installIpc(() => {
    const k = n++ % pool.length
    return { json: payloads[k], seq: n, tMs: wallMs(performance.now()), tail: tails[k] }
  })
  let tuned: { hz: number; target: string } | null = null
  const log: unknown[] = []
  document.addEventListener('mousedown', (e) => {
    const el = e.target as Element
    log.push({ x: e.clientX, y: e.clientY, button: e.button, hit: el.getAttribute('class') ?? el.tagName, prevented: e.defaultPrevented, tuned })
    tuned = null
  })
  document.addEventListener('contextmenu', (e) => log.push({ menu: true, prevented: e.defaultPrevented }))
  const api: { setRx?: (hz: number) => void } = {}
  function Host() {
    const [rx, setRx] = useState(1500)
    api.setRx = setRx
    return (
      <Waterfall
        transmitting={false}
        theme={THEME}
        rxOffsetHz={rx}
        txOffsetHz={1000}
        rowMs={50}
        fixedWindow={zoom === 'wspr' ? WSPR_WATERFALL_WINDOW : undefined}
        onTune={(hz, target) => {
          tuned = { hz, target }
        }}
      />
    )
  }
  const host = document.getElementById('root')!
  host.className = 'hx-fixed'
  createRoot(host).render(<Host />)
  // A picture under the pointer before the first click: rows have landed.
  await until(() => n > 10, 10_000, 'rows under the waterfall')
  H.setRx = async (hz) => {
    api.setRx!(hz)
    await frame()
    await frame()
  }
  H.box = () => {
    const r = document.querySelector('.waterfall-canvas')!.getBoundingClientRect()
    return { left: r.left, top: r.top, width: r.width, height: r.height, drawnBy: drawnBy() }
  }
  H.clicks = () => log.splice(0)
  H.ready = true
  return null
}

/**
 * The overlays (`src/spectrum/overlays.ts`) in a real browser, on the Flex set (14.000–14.200 MHz, the
 * dial at 14.100): the licence-class tint from a stand-in `get_privilege_spans`, a spot's tick and the
 * FT RX offset, each read back BY VALUE off the overlays' own canvas. Then a spot storm with the scope
 * paused, the picture's draw calls counted beside the overlays' redraws: a spot change may redraw the
 * overlays and never the picture. The control is palette changes under the same count, which must
 * redraw the picture. Live, the picture's draw calls with and without a storm are measured only.
 */
async function overlays() {
  const set = TIMED_SETS['flex-15']
  const pool = Array.from({ length: 64 }, (_, s) => timedFrame(set, s, false))
  const payloads = pool.map(frameJson)
  const tails = pool.map(frameTail)
  let n = 0
  installIpc(
    () => {
      const k = n++ % pool.length
      return { json: payloads[k], seq: n, tMs: wallMs(performance.now()), tail: tails[k] }
    },
    // A stand-in, not the gate's table: one span, so the view holds a privilege edge at each end of it.
    { get_privilege_spans: (args) => ({ class: 'general', mode: args?.mode, unrestricted: false, spans: [[14.025, 14.15]] }) },
  )
  // What is drawn, per canvas: any draw call on the renderer's (the picture, in .ph-scope-render), and
  // each clear of the overlays' (one per redraw of them).
  const counts = { picture: 0, overlays: 0 }
  const wrap = (proto: object, names: string[]) => {
    const p = proto as Record<string, (...a: unknown[]) => unknown>
    for (const name of names) {
      const f = p[name]
      p[name] = function (this: { canvas: unknown }, ...a: unknown[]) {
        const cv = this.canvas
        if (cv instanceof HTMLCanvasElement) {
          if (cv.closest('.ph-scope-render')) counts.picture++
          else if (name === 'clearRect' && cv.classList.contains('ph-scope-overlays')) counts.overlays++
        }
        return f.apply(this, a)
      }
    }
  }
  wrap(CanvasRenderingContext2D.prototype, ['putImageData', 'drawImage', 'fillRect', 'fill', 'stroke', 'clearRect'])
  wrap(WebGL2RenderingContext.prototype, ['drawArrays', 'drawElements'])
  const spotsFor = (k: number): ScopeSpot[] => [
    {
      spot: { call: `K${k}XYZ`, entity: '', zone: 0, band: '20m', freqMhz: 14.05, mode: 'CW', spotter: 'W1AW', corroborators: [], ageSecs: 0, comment: '', licensed: true },
      ink: null,
    },
  ]
  const ft: FtOverlay = { side: 1, rxHz: 1500, txHz: 1700, decodes: [] }
  const api: { spots?: (k: number) => void; theme?: (t: string) => void } = {}
  function Host() {
    const [k, setK] = useState(0)
    const [theme, setTheme] = useState(THEME)
    api.spots = setK
    api.theme = setTheme
    const spots = useMemo(() => spotsFor(k), [k])
    return (
      <PhoneScope transmitting={false} theme={theme} viewLoHz={-1e9} viewHiHz={1e9} dialHz={14_100_000} privilegeMode="cw" spots={spots} ft={ft} />
    )
  }
  const host = document.getElementById('root')!
  host.className = 'hx-fixed'
  createRoot(host).render(<Host />)
  await until(() => n > 10, 10_000, 'rows under the scope')
  await sleep(300)

  // ---- By value, off the overlays' canvas: alpha at points whose frequency is known ----
  const cv = document.querySelector<HTMLCanvasElement>('.ph-scope-overlays')
  if (!cv) throw new Error('no overlays canvas')
  const W = cv.width
  const px = cv.getContext('2d')!.getImageData(0, 0, W, cv.height).data
  const alpha = (x: number, y: number) => px[(Math.round(y) * W + Math.min(W - 1, Math.max(0, Math.round(x)))) * 4 + 3]
  const peak = (x: number, y: number) => Math.max(alpha(x - 1, y), alpha(x, y), alpha(x + 1, y))
  const xOf = (hz: number) => ((hz - 14_000_000) / 200_000) * W
  const mid = cv.height / 2
  const reading = {
    w: W,
    h: cv.height,
    // Outside the span (no privilege), inside it, and its two edges.
    below: alpha(xOf(14_010_000), mid),
    inside: alpha(xOf(14_080_000), mid),
    above: alpha(xOf(14_180_000), mid),
    edgeLo: peak(xOf(14_025_000), mid),
    edgeHi: peak(xOf(14_150_000), mid),
    // The RX offset at dial + 1500 Hz, and the spot's tick under the tag lane (27–33 px).
    rx: peak(xOf(14_101_500), mid),
    tick: peak(xOf(14_050_000), 30),
    beside: alpha(xOf(14_050_000) + 25, 30),
  }

  // ---- A spot storm with the picture held: the picture must not be drawn for it ----
  const pause = [...document.querySelectorAll<HTMLButtonElement>('.ph-scope-btn')].find((b) => b.textContent === '⏸')
  if (!pause) throw new Error('no pause button')
  pause.click()
  await sleep(300)
  counts.picture = 0
  counts.overlays = 0
  const changes = 40
  for (let i = 1; i <= changes; i++) {
    api.spots!(i)
    await sleep(25)
  }
  await sleep(300)
  const storm = { ...counts, changes }
  // The control: the same count, and changes that do redraw the picture.
  counts.picture = 0
  for (let i = 0; i < 6; i++) {
    api.theme!(i % 2 ? THEME : 'light')
    await sleep(50)
  }
  await sleep(200)
  const control = { picture: counts.picture }

  // ---- Live: the picture's own draw calls, quiet and in a storm (measured only) ----
  pause.click()
  await sleep(300)
  counts.picture = 0
  await sleep(2000)
  const quiet = counts.picture
  counts.picture = 0
  let k = 100
  const end = performance.now() + 2000
  while (performance.now() < end) {
    api.spots!(k++)
    await sleep(20)
  }
  const live = { quiet, storm: counts.picture, changes: k - 100 }
  return { reading, storm, control, live, drawnBy: drawnBy(), unexpected }
}

const MODES: Record<string, () => Promise<unknown>> = {
  backend,
  pixel,
  cadence,
  perf,
  ipc,
  offsets,
  overlays,
  // The renderer core (ui/src/spectrum), mounted bare: renderer.ts.
  render: () => render(q, palette),
  capability: () => capability(q),
  loss: () => loss(q, palette),
  rperf: () => rperf(q, palette, H),
  // The scale's axis against the renderer's picture (ui/src/spectrum/scale.ts): renderer.ts.
  axis: () => axis(q, palette),
}
// Every timing below rests on performance.now(), which Chrome coarsens to 100 µs unless the page is
// cross-origin isolated (run.mjs serves it so: 5 µs). A sub-millisecond cost read at 100 µs is noise.
if ((mode === 'perf' || mode === 'ipc' || mode === 'rperf') && !self.crossOriginIsolated) fail('the page is not cross-origin isolated: timers are 100 µs')
const run = MODES[mode]
if (!run) fail(`unknown mode ${mode}`)
else
  run()
    .then((result) => {
      if (result !== null && !H.done) Object.assign(H, { done: true, result })
    })
    .catch((e) => fail(e instanceof Error ? e.message : String(e)))
