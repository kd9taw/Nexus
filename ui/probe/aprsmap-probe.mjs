// The APRS cockpit's map in real Chrome, for what jsdom cannot see: `scripts/browser-probe --desktop aprsmap`.
//
// The APRS map took the Connect map's tools: a toolbar (the map picker, the age and kind filters, Tracks,
// Labels, Map on the left, zoom and Centre), a Layers panel, trails and labels, and the station card over the
// map. The probe serves the REAL app (index.html, main.tsx, App) with the app's own Vite config and src/api.ts
// swapped for probe/waterfalls-api.ts with probe/aprsmap-api.ts appended (a small local net, one car with a
// track), opens APRS with fresh storage at 1024×768 and 1920×1080, and asserts:
//   1. Nothing scrolls sideways: not the page, not the cockpit.
//   2. Stop TX and the TX latch are on screen and the topmost thing where they stand.
//   3. Every control of the map's toolbar is inside the map and the topmost thing where it stands, and the
//      Layers panel is on the map.
//   4. The map draws: its canvas is the map's size and has stations and land painted on it.
//   5. A station picked in the list opens its card inside the map, below the toolbar, with Follow on screen,
//      and Stop TX and the latch are still on screen with the card open.
// THE CONTROL, run every time: a planted rule that stops the toolbar wrapping (one 3000 px row) at 1024×768
// must make check 3 fail. If it passes, the check cannot fail and the probe says so instead of passing.
//
// exit: 0 every check held · 1 a red, or a control that could not fail · 2 the page never drew APRS
import { createServer } from 'vite'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const REAL_API = join(UI, 'src', 'api.ts')
const STUB_API = join(UI, 'probe', '__aprsmap-api.ts')
const SIZES = [
  [1024, 768],
  [1920, 1080],
]
const FEATURES = JSON.stringify({ profile: 'custom', enabled: { aprs: true } })
const PLANT = '.aprs-map-toolbar { flex-wrap: nowrap !important } .aprs-map-toolbar::after { content: ""; flex: 0 0 3000px }'
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

const MEASURE = `(() => {
  const shell = document.querySelector('main.aprs-cockpit')
  const map = shell?.querySelector('.aprs-map')
  const canvas = map?.querySelector('.map-canvas-wrap > canvas')
  if (!shell || !map || !canvas || !document.documentElement.getAttribute('data-viewport')) return null
  const vis = (el) => el && el.closest('[hidden]') == null && el.getBoundingClientRect().width > 0 && el.getBoundingClientRect().height > 0
  const top = (b) => {
    const q = b.getBoundingClientRect()
    const t = document.elementFromPoint(q.left + q.width / 2, q.top + q.height / 2)
    return t != null && (t === b || b.contains(t))
  }
  const reach = (b) => {
    if (!vis(b)) return false
    const q = b.getBoundingClientRect()
    if (q.bottom > innerHeight + 0.5 || q.top < -0.5 || q.right > innerWidth + 0.5 || q.left < -0.5) return false
    return top(b)
  }
  const btn = (re) => [...shell.querySelectorAll('button')].find((b) => vis(b) && re.test(b.textContent.trim()))
  const m = map.getBoundingClientRect()
  const inMap = (el) => { const q = el.getBoundingClientRect(); return q.left >= m.left - 0.5 && q.right <= m.right + 0.5 && q.top >= m.top - 0.5 && q.bottom <= m.bottom + 0.5 }
  const bar = map.querySelector('.aprs-map-toolbar')
  const tools = bar ? [...bar.querySelectorAll('button, select')].filter((e) => e.closest('[hidden]') == null) : []
  const off = tools.filter((e) => !(vis(e) && inMap(e) && reach(e))).map((e) => e.getAttribute('aria-label') || e.title || e.textContent.trim())
  // What the canvas holds: painted pixels in a coarse grid (the base map and the stations both count).
  const ctx = canvas.getContext('2d')
  let painted = 0
  try {
    const d = ctx.getImageData(0, 0, canvas.width, canvas.height).data
    for (let i = 0; i < d.length; i += 4 * 97) if (d[i + 3] > 0) painted++
  } catch { painted = -1 }
  const card = map.querySelector('.aprs-card')
  const se = document.scrollingElement
  return {
    vp: document.documentElement.getAttribute('data-viewport'),
    pageX: Math.max(0, se.scrollWidth - se.clientWidth),
    pageY: Math.max(0, se.scrollHeight - se.clientHeight),
    shellX: Math.max(0, shell.scrollWidth - shell.clientWidth),
    stops: [['Stop TX', btn(/^stop tx$/i)], ['TX latch', shell.querySelector('.cockpit-txstrip button[aria-pressed]') ?? btn(/^tx (on|off)$/i)]]
      .map(([n, b]) => [n, b ? reach(b) : null]),
    tools: tools.length,
    off,
    layers: !!map.querySelector('.map-layers') && inMap(map.querySelector('.map-layers')),
    canvas: { w: Math.round(canvas.getBoundingClientRect().width), h: Math.round(canvas.getBoundingClientRect().height), painted },
    wrap: (() => { const r = map.querySelector('.map-canvas-wrap').getBoundingClientRect(); return { w: Math.round(r.width), h: Math.round(r.height) } })(),
    card: card ? (() => {
      const c = card.getBoundingClientRect(), b = bar.getBoundingClientRect()
      const follow = [...card.querySelectorAll('button')].find((x) => /^follow$/i.test(x.textContent.trim()))
      return { inMap: inMap(card), belowBar: c.top >= b.bottom - 0.5, follow: follow ? reach(follow) : null }
    })() : null,
  }
})()`

// THE API SWAP — the waterfalls probe's answers with the APRS ones appended.
const realSrc = await readFile(REAL_API, 'utf8')
const stubSrc = (await readFile(join(UI, 'probe', 'waterfalls-api.ts'), 'utf8')) + '\n' + (await readFile(join(UI, 'probe', 'aprsmap-api.ts'), 'utf8'))
const names = [...realSrc.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1])
const own = new Set([...stubSrc.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1]))
const stubCode =
  stubSrc +
  `\nexport * from ${JSON.stringify(REAL_API)}\n` +
  names
    .filter((n) => !own.has(n))
    .map((n) => `export async function ${n}(..._a: unknown[]): Promise<any> { return (globalThis as any).__DEFAULT_ARR ? [] : {} }`)
    .join('\n')
const swap = {
  name: 'aprsmap-probe-api',
  enforce: 'pre',
  async resolveId(source, importer, opts) {
    if (opts?.scan) return null
    if (source === STUB_API) return STUB_API
    if (!importer || importer === STUB_API || !/api(\.ts)?$/.test(source)) return null
    const r = await this.resolve(source, importer, { ...opts, skipSelf: true })
    return r && r.id.split('?')[0] === REAL_API ? STUB_API : null
  },
  load(id) {
    return id === STUB_API ? stubCode : null
  },
}

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-aprsmap-probe-'))
const server = await createServer({
  root: UI, configFile: join(UI, 'vite.config.ts'), cacheDir, logLevel: 'warn', plugins: [swap],
  server: { host: '127.0.0.1', port: 0, strictPort: false, hmr: false },
})
await server.listen()
const origin = `http://127.0.0.1:${server.httpServer.address().port}`
const browser = await chrome()

async function measure([W, H], css = null) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text)
    return r.result.value
  }
  const settle = async () => {
    let m = null
    let last = null
    for (let i = 0; i < 40; i++) {
      await sleep(250)
      m = await evaluate(MEASURE)
      if (last && JSON.stringify(m) === JSON.stringify(last)) break
      last = m
    }
    return m
  }
  try {
    await call('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: 1, mobile: false })
    await call('Page.navigate', { url: `${origin}/probe-blank` })
    await sleep(300)
    const ls = { 'nexus.workspace': 'dx', 'nexus.features.v1': FEATURES, 'tempo-theme': 'dark' }
    await evaluate(`localStorage.clear(); ${Object.entries(ls).map(([k, v]) => `localStorage.setItem(${JSON.stringify(k)}, ${JSON.stringify(v)})`).join(';')}; 1`)
    await call('Page.navigate', { url: `${origin}/#aprs` })
    let m = null
    for (let i = 0; i < 120 && !m; i++) {
      await sleep(250)
      m = await evaluate(MEASURE).catch(() => null)
    }
    if (!m) return null
    if (css) await evaluate(`(() => { const s = document.createElement('style'); s.textContent = ${JSON.stringify(css)}; document.head.appendChild(s); return 1 })()`)
    const before = await settle()
    // Pick the car in the station list: its card opens over the map.
    await evaluate(`(() => { const r = [...document.querySelectorAll('.aprs-table tbody tr')].find((x) => x.textContent.includes('KD9TAW-9')); r?.click(); return !!r })()`)
    const after = await settle()
    return { before, after }
  } finally {
    await browser.call('Target.closeTarget', { targetId })
  }
}

function reds(tag, { before: m, after: a }) {
  const out = []
  if (m.pageX > 0 || m.pageY > 0) out.push(`${tag}: the page scrolls (${m.pageX} sideways, ${m.pageY} down)`)
  if (m.shellX > 0) out.push(`${tag}: the APRS cockpit scrolls ${m.shellX} px sideways`)
  for (const [label, ok] of m.stops) if (ok !== true) out.push(`${tag}: ${label} is ${ok === null ? 'missing' : 'not on screen or covered'}`)
  if (m.tools < 9) out.push(`${tag}: the map toolbar has ${m.tools} controls, expected the picker, two filters, Tracks, Labels, Map on the left, zoom and Centre`)
  if (m.off.length) out.push(`${tag}: map toolbar controls outside the map or covered: ${m.off.join(', ')}`)
  if (!m.layers) out.push(`${tag}: no Layers panel on the map`)
  if (m.canvas.w < m.wrap.w - 2 || m.canvas.h < m.wrap.h - 2 || m.canvas.h < 100) out.push(`${tag}: the map canvas is ${m.canvas.w}×${m.canvas.h} in a ${m.wrap.w}×${m.wrap.h} map`)
  if (!(m.canvas.painted > 50)) out.push(`${tag}: the map canvas is blank (${m.canvas.painted} painted samples)`)
  if (!a.card) out.push(`${tag}: picking a station in the list opened no card`)
  else {
    if (!a.card.inMap) out.push(`${tag}: the station card runs outside the map`)
    if (!a.card.belowBar) out.push(`${tag}: the station card covers the map toolbar`)
    if (a.card.follow !== true) out.push(`${tag}: the card's Follow is ${a.card.follow === null ? 'missing' : 'not on screen'}`)
  }
  for (const [label, ok] of a.stops) if (ok !== true) out.push(`${tag}: with the card open, ${label} is not on screen or covered`)
  return out
}

let code = 0
const red = (msg) => {
  console.error(`aprsmap-probe: RED — ${msg}`)
  code = 1
}
try {
  for (const size of SIZES) {
    const tag = `${size[0]}×${size[1]}`
    const r = await measure(size)
    if (!r) {
      console.error(`aprsmap-probe: APRS at ${tag} never drew`)
      process.exitCode = 2
      throw new Error('no APRS')
    }
    const m = r.before
    console.log(`aprsmap-probe: ${tag.padEnd(10)} viewport ${m.vp}, map ${m.wrap.w}×${m.wrap.h}, ${m.tools} toolbar controls, canvas painted ${m.canvas.painted}, card ${r.after.card ? 'open' : 'none'}`)
    for (const x of reds(tag, r)) red(x)
  }
  // THE CONTROL.
  const r = await measure(SIZES[0], PLANT)
  const caught = r ? reds('1024×768 (planted)', r).filter((x) => x.includes('toolbar controls outside')) : []
  if (caught.length === 0) red('CONTROL FAILED — a toolbar forced onto one 3000 px row passed check 3; the check cannot fail')
  else console.log(`aprsmap-probe: CONTROL — ${caught[0]} (caught)`)
  if (code === 0) console.log('aprsmap-probe: PASS — the APRS map and its toolbar fit at 1024×768 and 1920×1080, the map draws, the card opens inside it, Stop TX and the TX latch stay on screen, and the control was caught')
  process.exitCode = process.exitCode || code
} catch (e) {
  if (!process.exitCode) {
    console.error(`aprsmap-probe: ${e?.stack ?? e}`)
    process.exitCode = 2
  }
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
