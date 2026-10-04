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
import '../src/styles.css'
import { createRoot } from 'react-dom/client'
import { PhoneScope } from '../src/components/PhoneScope'
import { Waterfall } from '../src/components/Waterfall'
import { bakeLut, resolveColormap } from '../src/waterfall'
import { WF_PALETTE_KEY } from '../src/waterfallPalette'
import { capability, loss, render, rperf } from './renderer'
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
const comp = q.get('comp') === 'waterfall' ? 'waterfall' : 'phonescope'
const palette = q.get('palette') ?? 'turbo'
const THEME = 'dark'

// The palette is pinned, not left to the default, so a change of default is not a fixture failure.
localStorage.setItem(WF_PALETTE_KEY, palette)

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

function installIpc(answer: (drawn: Drawn) => Answer): void {
  const deliver = new MessageChannel()
  const after = new MessageChannel()
  const pending: (() => void)[] = []
  const afterQ: (() => void)[] = []
  deliver.port1.onmessage = () => pending.shift()?.()
  after.port1.onmessage = () => afterQ.shift()?.()
  const invoke = <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
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

function mount(shape: Shape, opts: { fixedWindow?: boolean; rowMs?: number } = {}): void {
  const host = document.getElementById('root')!
  host.className = `hx-${shape}`
  const el =
    comp === 'phonescope' ? (
      // The native-RF props the Phone cockpit passes with its "Full" span (the whole sweep);
      // on an audio row they are the plain 0–4 kHz window. No dial, marker or carrier line, so
      // nothing is drawn as text over the picture.
      <PhoneScope transmitting={false} theme={THEME} viewLoHz={-1e9} viewHiHz={1e9} />
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

/** The canvas the waterfall rows land on (the Waterfall's overlay canvas is a separate layer). */
function rowCanvas(): HTMLCanvasElement {
  const sel = comp === 'phonescope' ? '.ph-scope-canvas' : '.waterfall-canvas'
  const cv = document.querySelector<HTMLCanvasElement>(sel)
  if (!cv) throw new Error(`no ${sel} mounted`)
  return cv
}

function capture(cv: HTMLCanvasElement): { w: number; h: number; b64: string } {
  const px = cv.getContext('2d')!.getImageData(0, 0, cv.width, cv.height).data
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
  mount('fixed', { rowMs: 50 })
  // The ask after the last row starts only once that row's draw has finished.
  await until(() => refused, 60_000, `${total} rows`)
  await frame()
  return { served, rows: total, ...capture(rowCanvas()), unexpected }
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
  const px = cv.getContext('2d')!.getImageData(0, 0, W, cv.height).data
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
 * own log is the cross-check: today every answered ask is one committed row.
 */
async function cadence() {
  const set = TIMED_SETS[q.get('set') ?? '']
  if (!set) throw new Error(`unknown timed set ${q.get('set')}`)
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
  let t0 = -1
  let lastSeq = -1
  let endServed = false
  let stopped = false
  installIpc((drawn) => {
    const now = performance.now()
    if (t0 < 0) t0 = now
    if (now - t0 < windowMs) {
      const due = Math.min(published - 1, Math.floor(((now - t0) * set.rate) / 1000))
      if (drawn !== undefined && due + 1 <= drawn) return 'unchanged'
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
    committed: seqs.length,
    distinct: new Set(seqs).size,
    repeats: repeats.length,
    repeatRows: repeats.slice(0, 5),
    outOfOrder,
    askGapMs: dist(askGaps),
    api: { answered: log.length, distinct: apiDistinct, repeats: apiRepeats },
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

const MODES: Record<string, () => Promise<unknown>> = {
  backend,
  pixel,
  cadence,
  perf,
  ipc,
  // The renderer core (ui/src/spectrum), which no component mounts yet: renderer.ts.
  render: () => render(q, palette),
  capability: () => capability(q),
  loss: () => loss(q, palette),
  rperf: () => rperf(q, palette, H),
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
