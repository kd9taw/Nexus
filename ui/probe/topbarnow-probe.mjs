// The top bar's height through transmit, in real Chrome, for what jsdom cannot see: `scripts/browser-probe --desktop topbarnow`.
//
// STOP TX NEVER MOVES, and the top bar is above every cockpit: a bar that changes height moves the whole cockpit, Stop TX
// with it. From md up the bar carries the now-bar as its last group (`.topbar-now`, cockpit-panes.css), and that group
// stood on the bar's first line where its own content width fit there and took a second line where it did not. So the
// line it took followed the width of everything on the first line and of its own chips: at 1920×1080 at 150 % OS
// scaling the TX/RX sign's "TX" let it fit and "RX" did not, and every transmit moved the cockpit and Stop TX 18.9 px
// (found 2026-11-12, in 1.24.0).
//
// The probe serves the REAL app (index.html, App, every cockpit) with src/api.ts swapped for probe/waterfalls-api.ts, as
// alarmcell-probe does, with a snapshot it can push live. On FT, CW and the Logbook (the bar's own Stop TX), at every
// supported screen and OS scaling (operator ruling 2026-11-11: 1366×768 to 4K at 100-200 %; the window is the screen
// over the scaling at that device pixel ratio, the app at its own fit), it steps the radio through idle, TX enabled,
// transmitting, tuning, a QSO recording, CAT lost and idle again, with the now-bar empty and with all three feed pills
// up, and asserts: the bar's height, the cockpit's top and every Stop TX on screen stand where they stood at the first
// state of that window, to 0.5 px, and every word of the now-bar is drawn whole, inside the bar and the window.
// Across the two now-bar contents, the same: the now-bar's chips may never decide the bar's height.
//
// THE CONTROL, run every time: the now-bar's old sizing put back (its own content width, `flex: 0 1 auto`) at
// 1920×1080 at 150 %, where the move was measured. There transmitting must move the cockpit; if it does not, the check
// cannot fail and the probe says so instead of passing.
//
// TOPBARNOW_SIZES ("1920x1080@150,1366x768@100") and TOPBARNOW_VIEWS ("operate,cw") narrow a local run.
//
// exit: 0 every case holds · 1 a red, or a control that could not fail · 2 a page never drew
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { chrome } from '../../remote/test/browser-runtime.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const REAL_API = join(UI, 'src', 'api.ts')
const STUB_SRC = join(UI, 'probe', 'waterfalls-api.ts')
const STUB_API = join(UI, 'probe', '__topbarnow-api.ts')
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

/** [screen w, screen h, OS scaling %] — every supported screen (the 2026-11-11 ruling). */
const ALL_SIZES = [[1366, 768, 100], [1366, 768, 125], [1366, 768, 150], [1366, 768, 175], [1440, 900, 150],
  [1920, 1080, 100], [1920, 1080, 125], [1920, 1080, 150], [1920, 1080, 175], [2560, 1440, 100], [2560, 1440, 150],
  [3840, 2160, 150], [3840, 2160, 200]]
const pick = process.env.TOPBARNOW_SIZES?.split(',').map((s) => s.match(/^(\d+)x(\d+)@(\d+)$/).slice(1).map(Number))
const SIZES = pick ?? ALL_SIZES
/** As the webview sees OS scaling: the CSS window is the screen over the scale, at that device pixel ratio. */
const win = ([sw, sh, pct]) => ({ w: Math.floor(sw / (pct / 100)), h: Math.floor(sh / (pct / 100)), dpr: pct / 100, name: `${sw}×${sh} @${pct} %` })
const VIEWS = process.env.TOPBARNOW_VIEWS?.split(',') ?? ['operate', 'cw', 'logbook']

const IDLE = { transmitting: false, rigKeyed: false, txEnabled: false, tuning: false, txBusyReason: null, qsoRecording: false, catOk: true }
const STEPS = [
  ['idle', IDLE],
  ['TX enabled', { txEnabled: true }],
  ['transmitting', { transmitting: true, rigKeyed: true }],
  ['back to receive', { transmitting: false, rigKeyed: false }],
  ['tuning', { tuning: true, txBusyReason: 'tune' }],
  ['a QSO recording', { tuning: false, txBusyReason: null, qsoRecording: true }],
  ['CAT lost', { qsoRecording: false, catOk: false }],
  ['idle again', IDLE],
]
const FEED = (state, lastEventSecs) => ({ enabled: true, state, lastEventSecs })
/** The now-bar's two contents: no feeds (the stub's), and the three feed pills up, one of them retrying. */
const NOWBARS = [
  ['empty', null],
  ['three feeds', { cluster: FEED('live', 12), phoneCluster: FEED('reconnecting', 245), pskr: FEED('live', 3), phoneClusterHost: 'dxc.example.net +2', phoneSpotsSeen: 4, clusterViaOtherWindow: false }],
]
// The control: the now-bar's sizing before the fix, its own content width.
const OLD_SIZING = '.topbar-now{flex:0 1 auto!important;width:auto!important;flex-basis:auto!important}'

const realSrc = await readFile(REAL_API, 'utf8')
let stubSrc = await readFile(STUB_SRC, 'utf8')
stubSrc = stubSrc.replace('export function subscribeSnapshot(_fn: unknown): () => void { return () => {} }',
  `const __subs: any[] = []
;(globalThis as any).__push = (radio: any) => { snapshot.radio = { ...snapshot.radio, ...(radio || {}) }; const s = { ...snapshot, radio: { ...snapshot.radio } }; __subs.forEach((f) => f(s)); return __subs.length }
export function subscribeSnapshot(fn: any): () => void { __subs.push(fn); return () => { const i = __subs.indexOf(fn); if (i >= 0) __subs.splice(i, 1) } }`)
  .replace('export async function getFeedHealth(): Promise<any> { return null }', 'export async function getFeedHealth(): Promise<any> { return (globalThis as any).__feed ?? null }')
if (!stubSrc.includes('__push') || !stubSrc.includes('__feed')) throw new Error('topbarnow-probe: the stub no longer has the snapshot subscriber or the feed health to patch')
stubSrc += "\nexport async function resolveEntity(): Promise<any> { return 'United States' }\nexport async function qrzLookup(): Promise<any> { return null }\nexport async function getVoiceMessages(): Promise<any> { return [] }\n"
const names = [...realSrc.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1])
const own = new Set([...stubSrc.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1]))
const stubCode = stubSrc + `\nexport * from ${JSON.stringify(REAL_API)}\n` +
  names.filter((n) => !own.has(n)).map((n) => `export async function ${n}(..._a: unknown[]): Promise<any> { return {} }`).join('\n')
const swap = {
  name: 'topbarnow-probe-api', enforce: 'pre',
  async resolveId(source, importer, opts) {
    if (opts?.scan) return null
    if (source === STUB_API) return STUB_API
    if (!importer || importer === STUB_API || !/api(\.ts)?$/.test(source)) return null
    const r = await this.resolve(source, importer, { ...opts, skipSelf: true })
    return r && r.id.split('?')[0] === REAL_API ? STUB_API : null
  },
  load(id) { return id === STUB_API ? stubCode : null },
}

const MEASURE = `(() => {
  const bar = document.querySelector('.topbar')
  if (!bar || !document.documentElement.getAttribute('data-viewport')) return null
  // Not before the app has published this window's size (see alarmcell-probe's MEASURE): a layout the app never paints.
  const d = document.documentElement, z = parseFloat(getComputedStyle(d).getPropertyValue('--ui-zoom')) || 1
  const vw = parseFloat(d.style.getPropertyValue('--vw-eff')), vh = parseFloat(d.style.getPropertyValue('--vh-eff'))
  if (!Number.isFinite(vw) || !Number.isFinite(vh) || Math.abs(vw * z - innerWidth) > 1 || Math.abs(vh * z - innerHeight) > 1) return null
  const vis = (e) => { const r = e.getBoundingClientRect(); return r.width > 0 && r.height > 0 }
  const r1 = (v) => Math.round(v * 10) / 10
  const b = bar.getBoundingClientRect()
  const main = [...document.querySelectorAll('main')].find(vis)
  const stops = [...document.querySelectorAll('button')].filter((e) => vis(e) && /^stop tx$/i.test(e.textContent.trim()))
    .map((e) => { const r = e.getBoundingClientRect(); return [r1(r.left), r1(r.top)] })
  const slot = bar.querySelector('.topbar-now')
  // Every word of the now-bar: each of its parts drawn, its text not cut inside it, inside the now-bar's box, the bar and
  // the window.
  let cut = null, line = null
  if (slot) {
    const s = slot.getBoundingClientRect()
    line = r1(s.top - b.top)
    const parts = [...slot.querySelectorAll('.now-bar > *')]
    cut = parts.filter((e) => {
      const r = e.getBoundingClientRect()
      return !(r.width > 0 && r.height > 0 && e.scrollWidth <= e.clientWidth + 1 && r.left >= s.left - 0.5 && r.right <= s.right + 0.5 &&
        r.top >= s.top - 0.5 && r.bottom <= s.bottom + 0.5 && r.left >= b.left - 0.5 && r.right <= b.right + 0.5 && r.bottom <= b.bottom + 0.5 && r.right <= innerWidth + 0.5)
    }).map((e) => '"' + e.textContent.trim().slice(0, 30) + '"')
  }
  return { vp: d.getAttribute('data-viewport'), zoom: z, bar: r1(b.height), main: main ? r1(main.getBoundingClientRect().top) : null,
    stops, now: !!slot, line, cut, ind: bar.querySelector('.txrx-indicator')?.textContent ?? null }
})()`

await writeFile(STUB_API, '// generated by ui/probe/topbarnow-probe.mjs; deleted on exit\n')
const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-topbarnow-probe-'))
const server = await createServer({ root: UI, configFile: join(UI, 'vite.config.ts'), cacheDir, logLevel: 'warn', plugins: [swap],
  server: { host: '127.0.0.1', port: 0, strictPort: false, hmr: false } })
await server.listen()
const origin = `http://127.0.0.1:${server.httpServer.address().port}`
const browser = await chrome()

async function openPage(route, feed, first) {
  // A window of its own: the only tab of its window is the visible one, so its animation frames run.
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank', newWindow: true })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (m, p) => browser.call(m, p, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text)
    return r.result.value
  }
  const size = (s) => call('Emulation.setDeviceMetricsOverride', { width: s.w, height: s.h, deviceScaleFactor: s.dpr, mobile: false })
  await size(first)
  await call('Page.navigate', { url: `${origin}/probe-blank` })
  await sleep(300)
  const ls = { 'nexus.workspace': 'dx', 'tempo-theme': 'dark', 'nexus-ui-scale-mode': 'auto', 'nexus.locale': 'en' }
  await evaluate(`localStorage.clear(); ${Object.entries(ls).map(([k, v]) => `localStorage.setItem(${JSON.stringify(k)}, ${JSON.stringify(v)})`).join(';')}; 1`)
  await call('Page.enable', {})
  await call('Page.addScriptToEvaluateOnNewDocument', { source: `globalThis.__feed = ${JSON.stringify(feed)}` })
  await call('Page.navigate', { url: `${origin}/#${route}` })
  return { evaluate, size, close: () => browser.call('Target.closeTarget', { targetId }) }
}
async function settle(p) {
  let last = null
  for (let i = 0; i < 120; i++) {
    await sleep(i < 2 ? 150 : 80)
    const m = await p.evaluate(MEASURE).catch(() => null)
    if (!m) continue
    const k = JSON.stringify(m)
    if (k === last) return m
    last = k
  }
  return last ? JSON.parse(last) : null
}
const fmt = (m) => `bar ${m.bar}, cockpit ${m.main}, Stop TX ${JSON.stringify(m.stops)}, now-bar at +${m.line} [${m.ind}]`

/** One view with one now-bar content: every size, every radio state. Rows: [size name, step, measure]. */
async function sweep(route, [nbName, feed], sizes, css) {
  const wins = sizes.map(win)
  const p = await openPage(route, feed, wins[0])
  const rows = []
  try {
    if (!(await settle(p))) return [{ route, nbName, error: 'the top bar never drew' }]
    if (!(await p.evaluate(`globalThis.__push(${JSON.stringify(IDLE)})`))) return [{ route, nbName, error: 'no snapshot subscriber' }]
    if (css) await p.evaluate(`(() => { const s = document.createElement('style'); s.textContent = ${JSON.stringify(css)}; document.head.appendChild(s); return 1 })()`)
    for (const s of wins) {
      await p.size(s)
      await settle(p)
      for (const [step, radio] of STEPS) {
        await p.evaluate(`globalThis.__push(${JSON.stringify(radio)})`)
        const m = await settle(p)
        rows.push({ route, nbName, size: s.name, step, m, error: m ? null : `no measurement at "${step}"` })
      }
    }
  } finally { await p.close() }
  return rows
}
const moved = (a, b) => Math.max(Math.abs(a.bar - b.bar), Math.abs((a.main ?? 0) - (b.main ?? 0)),
  a.stops.length !== b.stops.length ? Infinity : 0, ...a.stops.map((s, i) => Math.max(Math.abs(s[0] - b.stops[i][0]), Math.abs(s[1] - b.stops[i][1]))))

let code = 0
try {
  const jobs = VIEWS.flatMap((v) => NOWBARS.map((n) => [v, n]))
  const all = []
  const worker = async () => { for (let j = jobs.shift(); j; j = jobs.shift()) all.push(...await sweep(j[0], j[1], SIZES)) }
  await Promise.all([worker(), worker(), worker()])
  let reds = 0
  for (const route of VIEWS) {
    for (const s of SIZES.map(win)) {
      const rows = all.filter((r) => r.route === route && (r.size === s.name || r.error))
      for (const r of rows.filter((x) => x.error)) { console.error(`topbarnow-probe: ${route} ${r.nbName} ${r.size ?? ''}: ${r.error}`); code = 2 }
      const ok = rows.filter((r) => !r.error && r.size === s.name)
      if (!ok.length) continue
      const base = ok[0]
      console.log(`topbarnow-probe: ${route} at ${s.name} (${s.w}×${s.h}, ${base.m.vp}, zoom ${base.m.zoom}): ${fmt(base.m)}`)
      if (!base.m.now && base.m.vp !== 'sm' && base.m.vp !== 'xs') console.log(`topbarnow-probe:   (no now-bar in the bar at ${base.m.vp})`)
      for (const r of ok) {
        const d = moved(base.m, r.m)
        const at = `${route} at ${s.name}, now-bar ${r.nbName}, ${r.step}`
        if (d > 0.5) { console.error(`topbarnow-probe: RED ${at} — moved ${Number.isFinite(d) ? Math.round(d * 10) / 10 : 'a Stop TX appeared/vanished'} px from ${base.nbName}/${base.step}: ${fmt(base.m)} → ${fmt(r.m)}`); reds++ }
        if (r.m.cut?.length) { console.error(`topbarnow-probe: RED ${at} — now-bar words cut: ${r.m.cut.join(', ')}`); reds++ }
      }
    }
  }
  if (reds) code = code || 1
  const n = all.filter((r) => !r.error).length
  console.log(`topbarnow-probe: ${n} rows (${VIEWS.length} views × ${NOWBARS.length} now-bars × ${SIZES.length} sizes × ${STEPS.length} states), ${reds} red`)
  // THE CONTROL: the old sizing at the measured size must move the cockpit when the radio transmits.
  if (!pick || pick.some(([w, h, p]) => w === 1920 && h === 1080 && p === 150)) {
    const rows = await sweep('operate', NOWBARS[0], [[1920, 1080, 150]], OLD_SIZING)
    const ok = rows.filter((r) => !r.error)
    const d = ok.length ? Math.max(...ok.map((r) => moved(ok[0].m, r.m))) : 0
    console.log(`topbarnow-probe: CONTROL operate at 1920×1080 @150 %, the now-bar's old sizing: ${ok.map((r) => `${r.step} ${fmt(r.m)}`).join('; ')} — moves ${Math.round(d * 10) / 10} px${d > 0.5 ? ' (caught)' : ''}`)
    if (!(d > 0.5)) { console.error('topbarnow-probe: CONTROL FAILED — with the old sizing transmitting did not move the cockpit; the check cannot fail'); code = code || 1 }
  }
  if (code === 0) console.log(`topbarnow-probe: PASS — the top bar's height, the cockpit's top and Stop TX stand still through idle, transmit, tune, recording and CAT lost, with an empty and a busy now-bar, its words whole, on ${VIEWS.join(', ')} at ${SIZES.map((s) => win(s).name).join(', ')}; the old sizing moves them (control)`)
  process.exitCode = code
} finally {
  await browser.stop(); await server.close(); await rm(cacheDir, { recursive: true, force: true }); await rm(STUB_API, { force: true })
}
