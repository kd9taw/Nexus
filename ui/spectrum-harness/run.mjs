#!/usr/bin/env node
// ui/spectrum-harness/run.mjs — the spectrum display's real-browser instruments.
//
// WHY THIS EXISTS. jsdom never lays out and never has a graphics context, so every unit gate in
// ui/ stays green through a broken paint path, and `scripts/browser-probe` drives the hosted
// Remote page rather than the desktop scope. This mounts the SHIPPED PhoneScope and Waterfall in
// headless Chrome — no product code changed, the desktop IPC bridge stood in by synthetic frames
// (main.tsx), the real clock — and measures what an operator sees:
//
//   backend  the GL and raster backend, PINNED by flags and ASSERTED, so a stored picture is never
//            compared across backends (stored in baselines/backend.json beside the pictures).
//   pixel    four fixtures (a carrier, a two-tone, a noise-floor step, an FT8 period) through each
//            component, compared with the stored PNGs under compare.mjs's tolerance.
//   cadence  sources at their producers' rates (audio 50/s, CI-V, Flex 15/s, FT-710 84/s): how
//            many waterfall rows were committed, how many distinct sweeps they show, and how many
//            REPEAT the row before them, read back off the canvas.
//   perf     frame pacing, long animation frames and main-thread time with each component filling
//            a 1024x768 and a 3440x1440 window.
//   ipc      the main-thread cost of a `Spectrum` row parsed from its JSON at 60 Hz, at 512 and
//            2048 bins, beside the same values taken from a binary buffer.
//   render   the renderer core (ui/src/spectrum) on BOTH backends, WebGL2 and canvas-2D: the same
//            fixtures against stored pictures per backend, the two backends against each other,
//            and the 3-D stack's rejection of a one-row burst.
//   capability  which backend the renderer picks on a healthy context and on two broken ones.
//   loss     a forced WebGL2 context loss: canvas-2D must stand in, and WebGL2 must come back with
//            the same picture, without a reload.
//   rperf    the renderer at 2048 bins x 2048 rows filling a 1024x768 window, beside its budget.
//   axis     the scale's axis (ui/src/spectrum/scale.ts) against the picture: a −20 dBFS level drawn
//            by each backend must sit on the axis's −20 dBFS tick.
//
// Controls run every time, because an instrument that cannot fail proves nothing: a WRONG PALETTE
// must fail the pixel comparison (the components' and the renderer's, on each backend), a PLANTED
// extra row must be found by the cadence probe, a BROKEN context must fail the renderer's self-test,
// a DROPPED restore handler must fail the loss check, and an axis pinned to a WRONG REFERENCE must
// miss the drawn line. A run where any control comes back clean is red.
//
// A check can carry a KNOWN-FAILURE marker: it is expected red and keeps the run green while it is,
// and the run goes red the day it passes, so the marker cannot outlive the defect it names.
//
// usage: node ui/spectrum-harness/run.mjs [--only backend,pixel,cadence,perf,ipc,render,capability,loss,rperf,axis]
//          [--out DIR] [--record] [--palette NAME] [--plant N] [--chrome PATH] [--cpu-throttle N]
// exit:  0 everything as expected · 1 something is not (a picture moved, a control stayed clean, a
//        known failure passed, the backend is not the pinned one) · 2 could not run (usage, no
//        Chrome, the harness did not build)
import { spawn, spawnSync } from 'node:child_process'
import { createServer } from 'node:http'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, extname, join, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'
import { comparePixels } from './compare.mjs'
import { decodePng, encodePng } from './png.mjs'

const HERE = dirname(fileURLToPath(import.meta.url))
const UI = resolve(HERE, '..')
const BASELINES = join(HERE, 'baselines')

const USAGE = `usage: node ui/spectrum-harness/run.mjs [options]
  --only LIST         backend,pixel,cadence,perf,ipc,render,capability,loss,rperf,axis (default: all;
                      the backend always runs)
  --out DIR           results.json, rendered pictures and diffs (default: $TMPDIR/nexus-spectrum-harness)
  --record            re-record the stored pictures of the probes selected (pixel, render): each
                      fixture rendered twice, written only if the two renders are identical, with
                      the backend they were taken under
  --palette NAME      render the pixel fixtures in another palette (a manual control: they must fail)
  --plant N           plant an extra row at ask N in every cadence source (a manual control)
  --chrome PATH       Chrome binary (default: $CHROME_BIN or google-chrome)
  --cpu-throttle N    DevTools CPU throttling for the perf probe (default 1)`

const PROBES = ['backend', 'pixel', 'cadence', 'perf', 'ipc', 'render', 'capability', 'loss', 'rperf', 'axis']
const opt = {
  only: new Set(PROBES),
  out: join(tmpdir(), 'nexus-spectrum-harness'),
  record: false,
  palette: 'turbo',
  plant: 0,
  chrome: process.env.CHROME_BIN || 'google-chrome',
  throttle: 1,
}
{
  const a = process.argv.slice(2)
  for (let i = 0; i < a.length; i++) {
    const val = () => {
      if (i + 1 >= a.length) bail(2, `${a[i]} needs a value\n${USAGE}`)
      return a[++i]
    }
    switch (a[i]) {
      case '--only':
        opt.only = new Set(['backend', ...val().split(',')])
        break
      case '--out':
        opt.out = resolve(val())
        break
      case '--record':
        opt.record = true
        break
      case '--palette':
        opt.palette = val()
        break
      case '--plant':
        opt.plant = Number(val())
        break
      case '--chrome':
        opt.chrome = val()
        break
      case '--cpu-throttle':
        opt.throttle = Number(val())
        break
      case '-h':
      case '--help':
        console.log(USAGE)
        process.exit(0)
        break
      default:
        bail(2, `unknown option ${a[i]}\n${USAGE}`)
    }
  }
  for (const k of opt.only) {
    if (!PROBES.includes(k)) bail(2, `unknown probe ${k}\n${USAGE}`)
  }
}

function bail(code, why) {
  console.error(why)
  process.exit(code)
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

// ---------------------------------------------------------------------------------------------
// The backend, pinned. Every pixel this harness reads is produced in software: the 2D canvas,
// rasterisation and compositing on the CPU, and WebGL through ANGLE on SwiftShader — requested
// explicitly, so that Chrome dropping its implicit software-WebGL fallback changes nothing here
// silently. A GPU is never needed. `backendKey` is what a stored picture is keyed on; the Chrome
// version is recorded beside it but is not part of the key (a new build moves anti-aliased edges
// by a few levels, which the tolerance is for).
const PINNED_FLAGS = [
  '--disable-gpu',
  '--disable-accelerated-2d-canvas',
  '--disable-gpu-rasterization',
  '--use-angle=swiftshader',
  '--enable-unsafe-swiftshader',
]
function backendKey(b) {
  const sw = (s) => (/software/.test(s ?? '') ? 'software' : String(s))
  const gl = /SwiftShader/.test(b.webgl2 ?? '') ? 'swiftshader' : String(b.webgl2)
  return `webgl2:${gl} 2d:${sw(b.canvas2d)} raster:${sw(b.rasterization)} compositing:${sw(b.compositing)}`
}
const PINNED_KEY = 'webgl2:swiftshader 2d:software raster:software compositing:software'

// ---------------------------------------------------------------------------------------------
// Chrome over the DevTools protocol (Node's own WebSocket; no dependency).

async function launchChrome() {
  const profile = mkdtempSync(join(tmpdir(), 'nexus-spectrum-chrome-'))
  const child = spawn(
    opt.chrome,
    [
      '--headless=new',
      '--no-sandbox',
      '--disable-dev-shm-usage',
      '--no-first-run',
      '--no-default-browser-check',
      '--disable-background-networking',
      '--disable-background-timer-throttling',
      '--disable-renderer-backgrounding',
      '--disable-backgrounding-occluded-windows',
      '--disable-extensions',
      '--disable-default-apps',
      '--hide-scrollbars',
      '--mute-audio',
      ...PINNED_FLAGS,
      '--remote-debugging-port=0',
      `--user-data-dir=${profile}`,
      'about:blank',
    ],
    { stdio: 'ignore', detached: true },
  )
  let exited = false
  child.once('error', () => (exited = true))
  child.once('exit', () => (exited = true))
  const kill = () => {
    try {
      process.kill(-child.pid, 'SIGKILL')
    } catch {
      /* already gone */
    }
    rmSync(profile, { recursive: true, force: true, maxRetries: 8, retryDelay: 100 })
  }
  let endpoint
  const deadline = Date.now() + 30_000
  while (!endpoint && Date.now() < deadline) {
    if (exited) break
    try {
      const [port, path] = readFileSync(join(profile, 'DevToolsActivePort'), 'utf8').split('\n')
      if (/^\d+$/.test(port) && path?.startsWith('/')) endpoint = `ws://127.0.0.1:${port}${path}`
    } catch {
      /* not written yet */
    }
    await sleep(50)
  }
  if (!endpoint) {
    kill()
    bail(2, `Chrome could not start (${opt.chrome}). A very long TMPDIR can stop it: run with the default.`)
  }
  const ws = new WebSocket(endpoint)
  await new Promise((ok, no) => {
    ws.addEventListener('open', ok, { once: true })
    ws.addEventListener('error', () => no(new Error('DevTools connection failed')), { once: true })
  })
  let next = 0
  const pending = new Map()
  ws.addEventListener('message', (e) => {
    const m = JSON.parse(e.data)
    const p = m.id && pending.get(m.id)
    if (!p) return
    pending.delete(m.id)
    clearTimeout(p.timer)
    if (m.error) p.no(new Error(`${p.method}: ${m.error.message}`))
    else p.ok(m.result)
  })
  const call = (method, params = {}, sessionId) =>
    new Promise((ok, no) => {
      const id = ++next
      const timer = setTimeout(() => {
        pending.delete(id)
        no(new Error(`DevTools command timed out: ${method}`))
      }, 60_000)
      pending.set(id, { ok, no, timer, method })
      ws.send(JSON.stringify({ id, method, params, sessionId }))
    })
  const version = (await call('Browser.getVersion')).product
  return {
    call,
    version,
    async close() {
      await call('Browser.close').catch(() => {})
      ws.close()
      await sleep(300)
      kill()
    },
  }
}

async function openPage(cdp, url, { width = 1280, height = 900 } = {}) {
  const { targetId } = await cdp.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await cdp.call('Target.attachToTarget', { targetId, flatten: true })
  await cdp.call('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: false }, sessionId)
  if (opt.throttle > 1) await cdp.call('Emulation.setCPUThrottlingRate', { rate: opt.throttle }, sessionId)
  await cdp.call('Performance.enable', { timeDomain: 'timeTicks' }, sessionId)
  await cdp.call('Page.navigate', { url }, sessionId)
  const evaluate = async (expression) => {
    const r = await cdp.call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true }, sessionId)
    if (r.exceptionDetails) throw new Error(`page: ${r.exceptionDetails.exception?.description ?? r.exceptionDetails.text}`)
    return r.result.value
  }
  const metrics = async () => {
    const { metrics: list } = await cdp.call('Performance.getMetrics', {}, sessionId)
    return Object.fromEntries(list.map((m) => [m.name, m.value]))
  }
  return { evaluate, metrics, close: () => cdp.call('Target.closeTarget', { targetId }).catch(() => {}) }
}

/** Poll the page until `__harness` says done (or ready, for perf); returns its result. */
async function waitFor(page, what, ms) {
  const end = Date.now() + ms
  for (;;) {
    const s = await page.evaluate(
      `(() => { const h = window.__harness; return h && (h.done || h.ready) ? JSON.stringify({ done: h.done, ready: !!h.ready, error: h.error ?? null, result: h.result ?? null }) : null })()`,
    )
    if (s) {
      const v = JSON.parse(s)
      if (v.error) throw new Error(v.error)
      if (what === 'ready' ? v.ready : v.done) return v.result
    }
    if (Date.now() > end) throw new Error(`timed out waiting for the page (${what})`)
    await sleep(200)
  }
}

// ---------------------------------------------------------------------------------------------
// Building and serving the page.

function build(outDir) {
  // Type-check the page against the components it mounts first: a prop the scope dropped must be
  // a red here, not a dead attribute the bundler passes along without a word.
  const tsc = spawnSync(join(UI, 'node_modules', '.bin', 'tsc'), ['-p', join(HERE, 'tsconfig.json')], {
    cwd: UI,
    encoding: 'utf8',
  })
  if (tsc.status !== 0) {
    console.error(tsc.stdout, tsc.stderr)
    bail(1, 'the harness does not type-check against the components it mounts')
  }
  return import('vite').then(async ({ build: viteBuild }) => {
    const react = (await import('@vitejs/plugin-react')).default
    await viteBuild({
      root: HERE,
      configFile: false,
      logLevel: 'warn',
      base: './',
      plugins: [react()],
      define: { __BUILD_ID__: JSON.stringify('spectrum-harness') },
      build: { outDir, emptyOutDir: true, sourcemap: false, reportCompressedSize: false, chunkSizeWarningLimit: 8192 },
    })
  })
}

const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.png': 'image/png', '.json': 'application/json' }
function serve(root) {
  const server = createServer((req, res) => {
    const path = decodeURIComponent(new URL(req.url, 'http://h').pathname)
    const file = resolve(root, `.${path === '/' ? '/index.html' : path}`)
    if (!file.startsWith(root + sep) || !existsSync(file)) {
      res.writeHead(404).end()
      return
    }
    // Cross-origin isolated, so performance.now() resolves to 5 µs instead of 100 µs.
    res.writeHead(200, {
      'content-type': TYPES[extname(file)] ?? 'application/octet-stream',
      'cache-control': 'no-store',
      'cross-origin-opener-policy': 'same-origin',
      'cross-origin-embedder-policy': 'require-corp',
    })
    res.end(readFileSync(file))
  })
  return new Promise((ok) => server.listen(0, '127.0.0.1', () => ok({ base: `http://127.0.0.1:${server.address().port}`, close: () => server.close() })))
}

// ---------------------------------------------------------------------------------------------
// The checks.

const results = { startedAt: new Date().toISOString(), options: { ...opt, only: [...opt.only] }, checks: [] }
let red = false
const line = (tag, name, text, verdict) => console.log(`${tag.padEnd(8)} ${name.padEnd(38)} ${text}${verdict ? `  … ${verdict}` : ''}`)
function record(check) {
  results.checks.push(check)
  if (check.outcome === 'fail') red = true
}

const FIXTURE_SETS = ['carrier', 'two-tone', 'noise-step', 'ft8-slot']
const COMPONENTS = ['phonescope', 'waterfall']
const WRONG_PALETTE = 'viridis'

async function renderFixture(cdp, base, comp, set, palette) {
  const page = await openPage(cdp, `${base}/index.html?mode=pixel&comp=${comp}&set=${set}&palette=${palette}`)
  try {
    const r = await waitFor(page, 'done', 90_000)
    if (r.unexpected.length) throw new Error(`the component asked for ${r.unexpected.join(', ')}`)
    return { width: r.w, height: r.h, rgba: new Uint8Array(Buffer.from(r.b64, 'base64')) }
  } finally {
    await page.close()
  }
}

async function pixelChecks(cdp, base, backend) {
  mkdirSync(join(opt.out, 'pixels'), { recursive: true })
  if (opt.record) {
    for (const comp of COMPONENTS) {
      for (const set of FIXTURE_SETS) {
        const id = `${comp}-${set}`
        const a = await renderFixture(cdp, base, comp, set, opt.palette)
        const b = await renderFixture(cdp, base, comp, set, opt.palette)
        const same = comparePixels(a, b, { channel: 0, maxFraction: 0 })
        if (!same.match) {
          record({ kind: 'pixel', id, outcome: 'fail', detail: `two renders differ (${same.reason}); not recorded` })
          line('PIXEL', id, same.reason, 'NOT DETERMINISTIC — not recorded')
          continue
        }
        mkdirSync(BASELINES, { recursive: true })
        writeFileSync(join(BASELINES, `${id}.png`), encodePng(a.width, a.height, a.rgba))
        record({ kind: 'pixel', id, outcome: 'recorded', detail: `${a.width}x${a.height}, two renders identical` })
        line('PIXEL', id, `${a.width}x${a.height}, two renders identical`, 'recorded')
      }
    }
    writeFileSync(
      join(BASELINES, 'backend.json'),
      `${JSON.stringify({ key: backendKey(backend), palette: opt.palette, chrome: backend.chrome, webgl2: backend.webgl2, canvas2d: backend.canvas2d, recordedAt: results.startedAt.slice(0, 10) }, null, 2)}\n`,
    )
    return
  }
  const storedPath = join(BASELINES, 'backend.json')
  const stored = existsSync(storedPath) ? JSON.parse(readFileSync(storedPath, 'utf8')) : null
  if (!stored || stored.key !== backendKey(backend)) {
    record({ kind: 'pixel', id: 'backend', outcome: 'fail', detail: `stored pictures were taken under ${stored?.key ?? 'nothing'}; this run is ${backendKey(backend)}` })
    line('PIXEL', 'stored pictures', `taken under "${stored?.key ?? 'nothing'}"`, 'NOT COMPARABLE with this backend — refused')
    return
  }
  const compareOne = async (comp, set, palette) => {
    const id = `${comp}-${set}`
    const actual = await renderFixture(cdp, base, comp, set, palette)
    const suffix = palette === stored.palette ? '' : `.${palette}`
    writeFileSync(join(opt.out, 'pixels', `${id}${suffix}.png`), encodePng(actual.width, actual.height, actual.rgba))
    const expected = decodePng(readFileSync(join(BASELINES, `${id}.png`)))
    const cmp = comparePixels(actual, expected)
    if (!cmp.match && cmp.diff) {
      writeFileSync(join(opt.out, 'pixels', `${id}${suffix}.diff.png`), encodePng(actual.width, actual.height, cmp.diff))
    }
    return { id, actual, cmp }
  }
  for (const comp of COMPONENTS) {
    for (const set of FIXTURE_SETS) {
      const { id, actual, cmp } = await compareOne(comp, set, opt.palette)
      const text = `${actual.width}x${actual.height}  ${cmp.differing} px over tolerance (${(cmp.fraction * 100).toFixed(3)}%), max Δ ${cmp.maxDelta}`
      record({ kind: 'pixel', id, palette: opt.palette, outcome: cmp.match ? 'pass' : 'fail', detail: cmp.reason || text, differing: cmp.differing, maxDelta: cmp.maxDelta })
      line('PIXEL', id, text, cmp.match ? 'ok' : `FAIL (${cmp.reason})`)
    }
  }
  // CONTROL: the same fixture in the wrong palette must be rejected.
  const { cmp } = await compareOne('phonescope', 'carrier', WRONG_PALETTE)
  const fired = !cmp.match
  record({ kind: 'control', id: `wrong palette (${WRONG_PALETTE})`, outcome: fired ? 'control-fired' : 'fail', detail: cmp.reason || 'matched the stored picture' })
  line('CONTROL', `wrong palette: phonescope-carrier`, `${(cmp.fraction * 100).toFixed(1)}% of pixels differ`, fired ? 'rejected, as it must be' : 'ACCEPTED — the comparator is blind')
}

const KNOWN_REPEATED_ROWS =
  'the repeated-row defect: PhoneScope commits a waterfall row on every 50 ms poll whether or not a new ' +
  'sweep arrived, so a source slower than 20 sweeps a second scrolls copies of its last sweep. Expected ' +
  'red until the scope commits a row only when the source frame counter advances; that change turns ' +
  'this into "known failure now passes" — delete the marker in the same change.'
const CADENCE = [
  { comp: 'phonescope', set: 'audio-50', expect: 'pass' },
  { comp: 'waterfall', set: 'audio-50', expect: 'pass' },
  { comp: 'phonescope', set: 'ft710-84', expect: 'pass' },
  { comp: 'phonescope', set: 'civ-3', expect: 'known-failure', why: KNOWN_REPEATED_ROWS },
  { comp: 'phonescope', set: 'civ-10', expect: 'known-failure', why: KNOWN_REPEATED_ROWS },
  { comp: 'phonescope', set: 'flex-15', expect: 'known-failure', why: KNOWN_REPEATED_ROWS },
  // CONTROL: one planted extra row in a source that is otherwise clean must be found, exactly once.
  { comp: 'phonescope', set: 'audio-50', plant: 40, expect: 'control' },
]
const CADENCE_WINDOW_MS = 6000

async function cadenceChecks(cdp, base) {
  for (const c of CADENCE) {
    const plant = c.plant ?? opt.plant
    const name = `${c.comp}/${c.set}${plant ? ` +row@${plant}` : ''}`
    const page = await openPage(cdp, `${base}/index.html?mode=cadence&comp=${c.comp}&set=${c.set}&window=${CADENCE_WINDOW_MS}&plant=${plant}&palette=${opt.palette}`, { width: 1100, height: 800 })
    let r
    try {
      r = await waitFor(page, 'done', CADENCE_WINDOW_MS + 30_000)
    } finally {
      await page.close()
    }
    const consistent = r.committed === r.api.answered && r.outOfOrder === 0 && r.unexpected.length === 0
    const clean = r.repeats === 0
    const gap = r.askGapMs
    const text = `${r.committed} rows (asks ${Math.round(gap.min)}-${Math.round(gap.max)} ms apart), ${r.distinct} sweeps of ${r.published} published, ${r.repeats} repeated, ${r.published - r.distinct} never drawn`
    let outcome
    let verdict
    if (!consistent) {
      outcome = 'fail'
      verdict = `FAIL (canvas ${r.committed} rows vs ${r.api.answered} answers, ${r.outOfOrder} out of order${r.unexpected.length ? `, asked for ${r.unexpected.join(',')}` : ''})`
    } else if (c.expect === 'control') {
      outcome = r.repeats === 1 ? 'control-fired' : 'fail'
      verdict = r.repeats === 1 ? `found at row ${r.repeatRows[0].row}, as it must be` : `FAIL (expected exactly 1 repeated row, read ${r.repeats})`
    } else if (c.expect === 'known-failure' && !opt.plant) {
      outcome = clean ? 'fail' : 'known-failure'
      verdict = clean ? 'KNOWN FAILURE NOW PASSES — remove its marker in run.mjs' : 'known failure (repeated rows)'
    } else {
      outcome = clean ? 'pass' : 'fail'
      verdict = clean ? 'ok' : 'FAIL (repeated rows)'
    }
    record({ kind: 'cadence', id: name, expect: c.expect, why: c.why, outcome, ...r })
    line('CADENCE', name, text, verdict)
  }
}

const VIEWPORTS = [
  [1024, 768],
  [3440, 1440],
]
const PERF = [
  { comp: 'phonescope', set: 'audio-50' },
  { comp: 'phonescope', set: 'flex-15' },
  { comp: 'waterfall', set: 'audio-50' },
]
const PERF_WINDOW_MS = 6000

async function perfChecks(cdp, base) {
  for (const [w, h] of VIEWPORTS) {
    for (const p of PERF) {
      const name = `${p.comp}/${p.set} @${w}x${h}`
      const page = await openPage(cdp, `${base}/index.html?mode=perf&comp=${p.comp}&set=${p.set}&palette=${opt.palette}`, { width: w, height: h })
      try {
        await waitFor(page, 'ready', 30_000)
        const m0 = await page.metrics()
        await page.evaluate('window.__harness.start()')
        await sleep(PERF_WINDOW_MS)
        const s = await page.evaluate('window.__harness.stop()')
        const m1 = await page.metrics()
        const secs = m1.Timestamp - m0.Timestamp
        const task = (m1.TaskDuration - m0.TaskDuration) * 1000
        const script = (m1.ScriptDuration - m0.ScriptDuration) * 1000
        const r = {
          ...s,
          mainThreadBusyPct: Math.round((task / (secs * 1000)) * 1000) / 10,
          mainThreadMsPerFrame: Math.round((task / Math.max(1, s.frames)) * 100) / 100,
          scriptMsPerSec: Math.round(script / secs),
        }
        // Measured, not asserted. The only demand is that the probe measured something.
        const ok = s.frames > 0 && s.rows > 0
        record({ kind: 'perf', id: name, outcome: ok ? 'measured' : 'fail', ...r })
        const lf = s.longFrames ? `${s.longFrames.n} long frames (max ${s.longFrames.maxMs} ms)` : 'long frames n/a'
        line('PERF', name, `${s.fps} fps, gap p95 ${s.frameGapMs.p95} ms, ${lf}, main ${r.mainThreadMsPerFrame} ms/frame (${r.mainThreadBusyPct}%), row ${s.rowMs.p50}/${s.rowMs.p95} ms p50/p95, canvas ${s.canvas.w}x${s.canvas.h}`, ok ? '' : 'FAIL (measured nothing)')
      } finally {
        await page.close()
      }
    }
  }
}

async function ipcChecks(cdp, base) {
  for (const bins of [512, 2048]) {
    const page = await openPage(cdp, `${base}/index.html?mode=ipc&bins=${bins}&window=5000`)
    try {
      const r = await waitFor(page, 'done', 30_000)
      record({ kind: 'ipc', id: `${bins} bins`, outcome: r.calls > 0 && r.sink ? 'measured' : 'fail', ...r })
      line('IPC', `${bins} bins @${Math.round(r.callsPerSec)} Hz`, `JSON ${r.jsonBytes} B/row (${Math.round(r.jsonBytesPerSec / 1024)} KiB/s), parse p50 ${r.parseMs.p50} p95 ${r.parseMs.p95} ms = ${r.parseMsPerSec} ms/s; binary ${r.binaryBytes} B, p50 ${r.binaryMs.p50} ms`)
    } finally {
      await page.close()
    }
  }
}

// ---------------------------------------------------------------------------------------------
// The renderer core (ui/src/spectrum, the page's renderer.ts). No component mounts it yet, so it is
// driven bare: rows fed directly, its canvas copied out in the task that drew it.

const RENDER_FIXTURES = [
  'scope-carrier',
  'scope-two-tone',
  'scope-noise-step',
  'wf-ft8-slot',
  'wf-retune',
  'wf-scrollback',
  'scope-wide-peak',
  'scope-wide-average',
  'dss-two-tone',
]
/** Fixtures checked only backend against backend, with no stored picture (see the loop). */
const CROSS_ONLY = ['wf-uhf']
const RENDER_BACKENDS = ['webgl2', 'canvas2d']
const RENDER_BASELINES = join(BASELINES, 'renderer')
const EXACT = { channel: 0, maxFraction: 0 }

/** Rows `y0..` of a picture. */
function rowsFrom(p, y0) {
  return { width: p.width, height: p.height - y0, rgba: p.rgba.subarray(y0 * p.width * 4) }
}

async function renderFixture2(cdp, base, fixture, backend, palette, set, redraw = 0) {
  const page = await openPage(cdp, `${base}/index.html?mode=render&fixture=${fixture}&backend=${backend}&palette=${palette}${set ? `&set=${set}` : ''}&redraw=${redraw}`)
  try {
    const r = await waitFor(page, 'done', 90_000)
    if (r.backend !== backend) throw new Error(`${fixture}: drew on ${r.backend}, not ${backend} (${r.reason})`)
    return { width: r.w, height: r.h, rgba: new Uint8Array(Buffer.from(r.b64, 'base64')), traceH: r.traceH, mode: r.mode }
  } finally {
    await page.close()
  }
}

async function renderChecks(cdp, base, backend) {
  const dir = join(opt.out, 'renderer')
  mkdirSync(dir, { recursive: true })
  if (opt.record) {
    for (const be of RENDER_BACKENDS) {
      for (const fx of RENDER_FIXTURES) {
        const id = `${be}-${fx}`
        const a = await renderFixture2(cdp, base, fx, be, opt.palette)
        const b = await renderFixture2(cdp, base, fx, be, opt.palette)
        const same = comparePixels(a, b, EXACT)
        if (!same.match) {
          record({ kind: 'render', id, outcome: 'fail', detail: `two renders differ (${same.reason}); not recorded` })
          line('RENDER', id, same.reason, 'NOT DETERMINISTIC — not recorded')
          continue
        }
        mkdirSync(RENDER_BASELINES, { recursive: true })
        writeFileSync(join(RENDER_BASELINES, `${id}.png`), encodePng(a.width, a.height, a.rgba))
        record({ kind: 'render', id, outcome: 'recorded', detail: `${a.width}x${a.height}, two renders identical` })
        line('RENDER', id, `${a.width}x${a.height}, two renders identical`, 'recorded')
      }
    }
    writeFileSync(
      join(RENDER_BASELINES, 'backend.json'),
      `${JSON.stringify({ key: backendKey(backend), palette: opt.palette, chrome: backend.chrome, webgl2: backend.webgl2, canvas2d: backend.canvas2d, recordedAt: results.startedAt.slice(0, 10) }, null, 2)}\n`,
    )
    return
  }
  const storedPath = join(RENDER_BASELINES, 'backend.json')
  const stored = existsSync(storedPath) ? JSON.parse(readFileSync(storedPath, 'utf8')) : null
  if (!stored || stored.key !== backendKey(backend)) {
    record({ kind: 'render', id: 'backend', outcome: 'fail', detail: `stored pictures were taken under ${stored?.key ?? 'nothing'}; this run is ${backendKey(backend)}` })
    line('RENDER', 'stored pictures', `taken under "${stored?.key ?? 'nothing'}"`, 'NOT COMPARABLE with this backend — refused')
    return
  }
  const compareOne = async (fx, be, palette) => {
    const id = `${be}-${fx}`
    const actual = await renderFixture2(cdp, base, fx, be, palette)
    const suffix = palette === stored.palette ? '' : `.${palette}`
    writeFileSync(join(dir, `${id}${suffix}.png`), encodePng(actual.width, actual.height, actual.rgba))
    const cmp = comparePixels(actual, decodePng(readFileSync(join(RENDER_BASELINES, `${id}.png`))))
    if (!cmp.match && cmp.diff) writeFileSync(join(dir, `${id}${suffix}.diff.png`), encodePng(actual.width, actual.height, cmp.diff))
    return { id, actual, cmp }
  }
  const drawn = {}
  for (const be of RENDER_BACKENDS) {
    for (const fx of RENDER_FIXTURES) {
      const { id, actual, cmp } = await compareOne(fx, be, opt.palette)
      drawn[id] = actual
      const text = `${actual.width}x${actual.height}  ${cmp.differing} px over tolerance (${(cmp.fraction * 100).toFixed(3)}%), max Δ ${cmp.maxDelta}`
      record({ kind: 'render', id, palette: opt.palette, outcome: cmp.match ? 'pass' : 'fail', detail: cmp.reason || text, differing: cmp.differing, maxDelta: cmp.maxDelta })
      line('RENDER', id, text, cmp.match ? 'ok' : `FAIL (${cmp.reason})`)
    }
  }
  // The two backends against each other. Below the trace both draw the waterfall band through the
  // same per-pixel mapping (aggregate.ts and its GLSL twin), so there they must agree within the
  // comparator's tolerance, detectors included. The trace line and the 3-D stack are rasterised
  // differently on each (a stroked path against a shader ribbon; 256 columns against one per
  // pixel), so the whole picture's difference is reported, not asserted.
  const pct = (c) => `${c.differing} px over tolerance (${(c.fraction * 100).toFixed(3)}%), max Δ ${c.maxDelta}`
  for (const fx of RENDER_FIXTURES) {
    const a = drawn[`webgl2-${fx}`]
    const b = drawn[`canvas2d-${fx}`]
    const whole = comparePixels(a, b)
    if (a.mode === 'dss') {
      record({ kind: 'render-cross', id: fx, outcome: 'measured', detail: pct(whole), fraction: whole.fraction, maxDelta: whole.maxDelta })
      line('BOTH', `webgl2 vs canvas2d: ${fx}`, `whole ${pct(whole)}`, 'reported')
      continue
    }
    const band = comparePixels(rowsFrom(a, a.traceH), rowsFrom(b, b.traceH))
    record({ kind: 'render-cross', id: fx, outcome: band.match ? 'pass' : 'fail', detail: band.reason || `band ${pct(band)}; whole ${pct(whole)}`, band: { differing: band.differing, maxDelta: band.maxDelta }, whole: { fraction: whole.fraction, maxDelta: whole.maxDelta } })
    line('BOTH', `webgl2 vs canvas2d: ${fx}`, `band ${pct(band)}; whole ${(whole.fraction * 100).toFixed(3)}%`, band.match ? 'agree' : `FAIL (${band.reason})`)
  }
  // Absolute RF far from where the history began. WebGL2 carries hertz as float32 offsets from an
  // origin it moves to the newest rows; canvas-2D works in float64 and is exact, so the two must
  // agree. Without the re-basing a zoomed 23 cm span lands carriers pixels apart.
  for (const fx of CROSS_ONLY) {
    const a = await renderFixture2(cdp, base, fx, 'webgl2', opt.palette)
    const b = await renderFixture2(cdp, base, fx, 'canvas2d', opt.palette)
    writeFileSync(join(dir, `webgl2-${fx}.png`), encodePng(a.width, a.height, a.rgba))
    writeFileSync(join(dir, `canvas2d-${fx}.png`), encodePng(b.width, b.height, b.rgba))
    const cmp = comparePixels(a, b)
    record({ kind: 'render-cross', id: fx, outcome: cmp.match ? 'pass' : 'fail', detail: cmp.reason || pct(cmp), differing: cmp.differing, maxDelta: cmp.maxDelta })
    line('BOTH', `webgl2 vs canvas2d: ${fx}`, pct(cmp), cmp.match ? 'agree' : `FAIL (${cmp.reason})`)
  }
  // Drawing again changes nothing. A host draws at display rate and commits rows far slower, so a
  // draw that leaves anything behind (canvas-2D blits its band only when a row arrives) builds up
  // between rows. The fixture whose trace lies on the floor, drawn 30 more times, must be the same
  // picture to the bit.
  for (const be of RENDER_BACKENDS) {
    const again = await renderFixture2(cdp, base, 'scope-wide-average', be, opt.palette, undefined, 30)
    const cmp = comparePixels(again, drawn[`${be}-scope-wide-average`], EXACT)
    record({ kind: 'render', id: `${be} redraw is idempotent`, outcome: cmp.match ? 'pass' : 'fail', detail: cmp.reason || 'identical' })
    line('RENDER', `${be} 30 more draws, no new row`, cmp.match ? 'identical' : cmp.reason, cmp.match ? 'ok' : 'FAIL (a draw leaves something behind)')
  }
  // The 3-D stack's median of three: with identical rows around it, a one-row broadband burst must
  // leave the stack exactly as it was. The same burst MUST show on the 2-D waterfall, which has no
  // median, or the check proves nothing.
  for (const be of RENDER_BACKENDS) {
    const clean = await renderFixture2(cdp, base, 'dss-two-tone', be, opt.palette, 'steady')
    const burst = await renderFixture2(cdp, base, 'dss-two-tone', be, opt.palette, 'steady-burst')
    const stack = comparePixels(burst, clean, EXACT)
    record({ kind: 'render', id: `${be} 3-D burst rejected`, outcome: stack.match ? 'pass' : 'fail', detail: stack.reason || 'identical' })
    line('RENDER', `${be} 3-D: one-row burst rejected`, stack.match ? 'identical to the stack without it' : stack.reason, stack.match ? 'ok' : 'FAIL (the burst reached the stack)')
    const flatClean = await renderFixture2(cdp, base, 'wf-ft8-slot', be, opt.palette, 'steady')
    const flatBurst = await renderFixture2(cdp, base, 'wf-ft8-slot', be, opt.palette, 'steady-burst')
    const flat = comparePixels(flatBurst, flatClean, EXACT)
    record({ kind: 'control', id: `${be} burst shows on 2-D`, outcome: flat.match ? 'fail' : 'control-fired', detail: flat.reason || 'identical' })
    line('CONTROL', `${be} 2-D: the same burst shows`, flat.reason || 'identical', flat.match ? 'NOT SEEN — the burst check proves nothing' : 'seen, as it must be')
  }
  // CONTROL: the wrong palette must fail each backend's stored picture.
  for (const be of RENDER_BACKENDS) {
    const { cmp } = await compareOne('scope-carrier', be, WRONG_PALETTE)
    const fired = !cmp.match
    record({ kind: 'control', id: `${be} wrong palette (${WRONG_PALETTE})`, outcome: fired ? 'control-fired' : 'fail', detail: cmp.reason || 'matched the stored picture' })
    line('CONTROL', `wrong palette: ${be}-scope-carrier`, `${(cmp.fraction * 100).toFixed(1)}% of pixels differ`, fired ? 'rejected, as it must be' : 'ACCEPTED — the comparator is blind')
  }
}

/** The capability gate, both directions: a healthy context must get WebGL2, and each broken one
 *  must end on canvas-2D with the reason. */
async function capabilityChecks(cdp, base) {
  const cases = [
    { broken: 'none', want: 'webgl2', why: '' },
    { broken: 'upload', want: 'canvas2d', why: 'self-test:' },
    { broken: 'context', want: 'canvas2d', why: 'no WebGL2 context' },
  ]
  for (const c of cases) {
    const page = await openPage(cdp, `${base}/index.html?mode=capability&break=${c.broken}`)
    let r
    try {
      r = await waitFor(page, 'done', 30_000)
    } finally {
      await page.close()
    }
    const ok = r.backend === c.want && (c.why ? r.reason.startsWith(c.why) : r.reason === '')
    const control = c.broken !== 'none'
    record({ kind: control ? 'control' : 'capability', id: `capability, ${c.broken === 'none' ? 'healthy' : `broken ${c.broken}`}`, outcome: ok ? (control ? 'control-fired' : 'pass') : 'fail', detail: `${r.backend}: ${r.reason || 'no reason'}` })
    line(control ? 'CONTROL' : 'CAPABLE', `${c.broken === 'none' ? 'a healthy context' : `a broken ${c.broken}`}`, `${r.backend}${r.reason ? ` (${r.reason})` : ''}`, ok ? (control ? 'refused, as it must be' : 'ok') : `FAIL (expected ${c.want}${c.why ? `, "${c.why}…"` : ''})`)
  }
}

async function lossOnce(cdp, base, dropRestore) {
  const page = await openPage(cdp, `${base}/index.html?mode=loss&palette=${opt.palette}&dropRestore=${dropRestore ? 1 : 0}`)
  try {
    return await waitFor(page, 'done', 60_000)
  } finally {
    await page.close()
  }
}

/** The loss check's verdict: everything that must hold, and what did not. */
function lossVerdict(r) {
  const pic = (p) => p && { width: p.w, height: p.h, rgba: new Uint8Array(Buffer.from(p.b64, 'base64')) }
  const faults = []
  if (!r.fellBack) faults.push('canvas-2D never stood in')
  if (r.visibleCanvasesWhileLost !== 1) faults.push(`${r.visibleCanvasesWhileLost} canvases visible while lost`)
  if (!r.restored) faults.push('WebGL2 never came back')
  if (r.backendAtEnd !== 'webgl2') faults.push(`ended on ${r.backendAtEnd}`)
  const after = comparePixels(pic(r.after), pic(r.reference), EXACT)
  if (!after.match) faults.push(`after the restore the picture differs from a renderer that never lost its context (${after.reason})`)
  const standIn = r.standIn ? comparePixels(pic(r.standIn), pic(r.standInReference), EXACT) : { match: false, reason: 'no stand-in picture' }
  if (!standIn.match) faults.push(`the stand-in's picture differs from canvas-2D fed the same rows (${standIn.reason})`)
  return { faults, after, standIn }
}

async function lossChecks(cdp, base) {
  const r = await lossOnce(cdp, base, false)
  const v = lossVerdict(r)
  const text = `fell back in ${r.fallbackMs} ms, restored in ${r.restoreMs} ms, ${r.rows} rows; after restore ${v.after.differing} px differ from never-lost, stand-in ${v.standIn.differing ?? '?'} px from canvas-2D`
  record({ kind: 'loss', id: 'lose and restore', outcome: v.faults.length ? 'fail' : 'pass', detail: v.faults.join('; ') || text, fallbackMs: r.fallbackMs, restoreMs: r.restoreMs, reasonWhileLost: r.reasonWhileLost })
  line('LOSS', 'lose, stand in, restore, no reload', text, v.faults.length ? `FAIL (${v.faults.join('; ')})` : 'ok')
  // CONTROL: with the restore handler dropped, the check must fail.
  const c = await lossOnce(cdp, base, true)
  const cv = lossVerdict(c)
  const fired = cv.faults.length > 0
  record({ kind: 'control', id: 'dropped restore handler', outcome: fired ? 'control-fired' : 'fail', detail: cv.faults.join('; ') || 'passed without a restore handler' })
  line('CONTROL', 'loss with the restore handler dropped', cv.faults.join('; ') || 'passed', fired ? 'failed, as it must' : 'PASSED — the loss check is blind')
}

const RPERF = [
  { backend: 'webgl2', layout: 'scope' },
  { backend: 'canvas2d', layout: 'scope' },
  { backend: 'webgl2', layout: 'dss' },
  { backend: 'canvas2d', layout: 'dss' },
]
/** The renderer's budget: < 2 ms GPU and < 1 ms main thread per frame, 2048 bins x 2048 rows, on the
 *  1024x768 floor. Printed beside the numbers, never asserted: this runner is nobody's shack PC. */
const BUDGET = { mainMs: 1, gpuMs: 2 }

async function rperfChecks(cdp, base) {
  for (const p of RPERF) {
    const name = `${p.backend} ${p.layout} 2048x2048 @1024x768`
    const page = await openPage(cdp, `${base}/index.html?mode=rperf&backend=${p.backend}&layout=${p.layout}&palette=${opt.palette}`, { width: 1024, height: 768 })
    try {
      await waitFor(page, 'ready', 60_000)
      const m0 = await page.metrics()
      await page.evaluate('window.__harness.start()')
      await sleep(PERF_WINDOW_MS)
      const s = await page.evaluate('window.__harness.stop()')
      const m1 = await page.metrics()
      const g = await page.evaluate('window.__harness.gpu()')
      const secs = m1.Timestamp - m0.Timestamp
      const task = (m1.TaskDuration - m0.TaskDuration) * 1000
      const r = { ...s, ...g, mainThreadMsPerFrame: Math.round((task / Math.max(1, s.frames)) * 100) / 100, mainThreadBusyPct: Math.round((task / (secs * 1000)) * 1000) / 10 }
      const ok = s.frames > 0 && s.rows === s.depth
      record({ kind: 'rperf', id: name, outcome: ok ? 'measured' : 'fail', budget: BUDGET, ...r })
      const tq = g.timerQuery ? `, timer query p50 ${g.timerQuery.p50} ms` : ', no timer query'
      line('RPERF', name, `${Math.round(s.fps)} fps, renderer ${s.mainMs.p50}/${s.mainMs.p95} ms p50/p95 on the main thread (task ${r.mainThreadMsPerFrame} ms/frame), draw-to-pixels ${g.drawToPixelsMs.p50}/${g.drawToPixelsMs.p95} ms${tq}`, ok ? '' : 'FAIL (measured nothing)')
    } finally {
      await page.close()
    }
  }
}

// ---------------------------------------------------------------------------------------------

/** The axis against the picture, on each backend: the −20 dBFS tick must be on the line the renderer drew
 *  for a −20 dBFS level (its centre half a line width under its top edge, within a pixel), and the same
 *  tick on an axis pinned to a −100 dBFS floor, the control, must miss it. */
async function axisChecks(cdp, base) {
  for (const backend of ['webgl2', 'canvas2d']) {
    const page = await openPage(cdp, `${base}/index.html?mode=axis&backend=${backend}&palette=${opt.palette}`)
    let r
    try {
      r = await waitFor(page, 'done', 30_000)
    } finally {
      await page.close()
    }
    const drawnY = r.topRow + 1
    const ok = r.backend === backend && r.topRow >= 0 && r.tickY !== null && Math.abs(drawnY - r.tickY) <= 1
    const detail = `${r.backend}: line at y ${drawnY}, −20 dBFS tick at ${r.tickY?.toFixed(2) ?? 'none'}, reads ${r.reading.toFixed(2)} dBFS`
    record({ kind: 'axis', id: `axis, ${backend}`, outcome: ok ? 'pass' : 'fail', detail })
    line('AXIS', `−20 dBFS on ${backend}`, detail, ok ? 'ok' : 'FAIL (the tick is off the drawn line)')
    const fired = r.topRow >= 0 && (r.wrongY === null || Math.abs(drawnY - r.wrongY) > 1)
    const wrong = `a −100 dBFS floor puts the tick at ${r.wrongY?.toFixed(2) ?? 'none'}`
    record({ kind: 'control', id: `axis, ${backend}, wrong reference`, outcome: fired ? 'control-fired' : 'fail', detail: wrong })
    line('CONTROL', `wrong reference on ${backend}`, wrong, fired ? 'misses the line, as it must' : 'FAIL (a wrong axis passed)')
  }
}

async function main() {
  if (spawnSync(opt.chrome, ['--version'], { encoding: 'utf8' }).status !== 0) bail(2, `no Chrome at "${opt.chrome}" (set --chrome or CHROME_BIN)`)
  mkdirSync(opt.out, { recursive: true })
  const site = join(opt.out, 'site')
  const t0 = Date.now()
  try {
    await build(site)
  } catch (e) {
    bail(2, `the harness page did not build: ${e.message}`)
  }
  const server = await serve(site)
  const cdp = await launchChrome()
  try {
    // BACKEND — asserted before anything is measured, so no number below is from another one.
    const page = await openPage(cdp, `${server.base}/index.html?mode=backend`)
    const gl = await waitFor(page, 'done', 30_000)
    await page.close()
    const fs = (await cdp.call('SystemInfo.getInfo')).gpu?.featureStatus ?? {}
    const backend = {
      chrome: cdp.version,
      webgl2: gl.webgl2?.renderer ?? null,
      canvas2d: fs['2d_canvas'],
      rasterization: fs.rasterization,
      compositing: fs.gpu_compositing,
    }
    results.backend = { ...backend, key: backendKey(backend), webglVersion: gl.webgl2?.version ?? null }
    const pinned = backendKey(backend) === PINNED_KEY
    record({ kind: 'backend', id: 'pinned', outcome: pinned ? 'pass' : 'fail', detail: backendKey(backend) })
    line('BACKEND', backend.chrome, `WebGL2 "${backend.webgl2}", 2D canvas ${backend.canvas2d}`, pinned ? 'pinned, as asserted' : `FAIL (expected ${PINNED_KEY})`)
    if (pinned) {
      if (opt.only.has('pixel')) await pixelChecks(cdp, server.base, backend)
      if (opt.only.has('cadence')) await cadenceChecks(cdp, server.base)
      if (opt.only.has('perf')) await perfChecks(cdp, server.base)
      if (opt.only.has('ipc')) await ipcChecks(cdp, server.base)
      if (opt.only.has('render')) await renderChecks(cdp, server.base, backend)
      if (opt.only.has('capability')) await capabilityChecks(cdp, server.base)
      if (opt.only.has('loss')) await lossChecks(cdp, server.base)
      if (opt.only.has('rperf')) await rperfChecks(cdp, server.base)
      if (opt.only.has('axis')) await axisChecks(cdp, server.base)
    }
  } catch (e) {
    record({ kind: 'harness', id: 'run', outcome: 'fail', detail: e.message })
    console.error(`harness error: ${e.message}`)
  } finally {
    await cdp.close()
    server.close()
  }
  results.seconds = Math.round((Date.now() - t0) / 1000)
  writeFileSync(join(opt.out, 'results.json'), `${JSON.stringify(results, null, 2)}\n`)
  const known = results.checks.filter((c) => c.outcome === 'known-failure').length
  const controls = results.checks.filter((c) => c.outcome === 'control-fired').length
  console.log(
    `${red ? 'RED' : 'GREEN'}    ${results.checks.length} checks in ${results.seconds} s; ${controls} control(s) fired, ` +
      `${known} known failure(s) still failing; results in ${join(opt.out, 'results.json')}`,
  )
  process.exit(red ? 1 : 0)
}

main().catch((e) => bail(2, `harness error: ${e.stack ?? e}`))
