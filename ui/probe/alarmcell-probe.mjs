// The transmitter alarm cell in real Chrome, for what jsdom cannot see: `scripts/browser-probe --desktop alarmcell`.
//
// The cell (TxAlarmBanner, cockpit-panes.css `.ch-txalarm`) stands in every operating screen's TX strip and is ALWAYS
// DRAWN at one constant size, so a transmitter alarm coming or going must never change the strip's height or move
// Stop TX. It broke twice by language: a floor too wide for the TX state's line put the cell on a line of its own
// (the strip grew and controls under it could not be reached), and a cell with no height of its own made that line
// 0 px tall until an alarm arrived (Stop TX moved 26.5 px in Russian, 23 px in Spanish, Chinese, Japanese and German).
//
// The probe serves the REAL app (index.html, App, every cockpit) with src/api.ts swapped for probe/waterfalls-api.ts,
// as digishell-probe does, with a snapshot it can push live. On FT, Phone, CW, RTTY, PSK, JTTY, SSTV and JS8, at
// 150 and 175 % (the xs and sm tiers) at 700×800, 900×600, 1024×768 and 1366×768, in ALL EIGHT languages (switched
// live, so one page serves a screen and a zoom), it pushes a transmitter alarm and takes it away again, and asserts:
// the strip is the same height and Stop TX at the same place with the alarm as without, and the alarm's words are on
// screen and topmost at their centre.
//
// THE CONTROL, run every time: the cell's one height taken off (`height: auto`, the shape that shipped the second
// break) on the screens and languages where that break was measured. There an alarm must move Stop TX; if it does
// not, the check cannot fail and the probe says so instead of passing.
// THE REACH CONTROL: the words left unpainted (`visibility: hidden`) on FT at 900×600 175 %, the screen whose strip
// is reached by scrolling. Every position of the shell's scroll is tried, so the words must then be judged hidden;
// if they are not, the reach check cannot fail.
//
// exit: 0 every case holds · 1 a red, or a control that could not fail · 2 a page never drew its strip
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { chrome } from '../../remote/test/browser-runtime.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const REAL_API = join(UI, 'src', 'api.ts')
const STUB_SRC = join(UI, 'probe', 'waterfalls-api.ts')
const STUB_API = join(UI, 'probe', '__alarmcell-api.ts')
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const FEATURES = JSON.stringify({ profile: 'custom', enabled: { rtty: true, psk: true, sstv: true, js8: true, jtty: true } })
const ALL_SCREENS = [['FT', 'operate'], ['Phone', 'phone'], ['CW', 'cw'], ['RTTY', 'rtty'], ['PSK', 'psk'], ['JTTY', 'jtty'], ['SSTV', 'sstv'], ['JS8', 'js8']]
// `ALARMCELL_SCREENS=FT,RTTY` narrows a local run to those screens (CI runs them all).
const SCREENS = process.env.ALARMCELL_SCREENS ? ALL_SCREENS.filter(([n]) => process.env.ALARMCELL_SCREENS.split(',').includes(n)) : ALL_SCREENS
const SCALES = process.env.ALARMCELL_SCALES ? process.env.ALARMCELL_SCALES.split(',') : ['150', '175']
const SIZES = process.env.ALARMCELL_SIZES ? process.env.ALARMCELL_SIZES.split(',').map((s) => s.split('x').map(Number)) : [[700, 800], [900, 600], [1024, 768], [1366, 768]]
const LOCALES = process.env.ALARMCELL_LOCALES ? process.env.ALARMCELL_LOCALES.split(',') : ['en', 'ru', 'de', 'fr', 'es', 'ja', 'zh', 'zh-hant']
const ALARM = { id: 1, text: 'High SWR: 3.2:1 — transmit was stopped', radioId: 1, radioName: 'IC-7300', atMs: Date.UTC(2026, 10, 8, 14, 22, 33) }
const NO_HEIGHT = '.ch-txalarm{height:auto!important}'
// Where the cell stands on a line of its own and its missing height was measured to move Stop TX (700x800 150 %).
const CONTROL = [['RTTY', 'rtty', '150', 'es', [700, 800]], ['CW', 'cw', '150', 'de', [700, 800]]]
const UNPAINTED = '.ch-txalarm-text{visibility:hidden!important}'
const REACH_CONTROL = ['FT', 'operate', '175', 'en', [900, 600]]

const realSrc = await readFile(REAL_API, 'utf8')
let stubSrc = await readFile(STUB_SRC, 'utf8')
stubSrc = stubSrc.replace('export function subscribeSnapshot(_fn: unknown): () => void { return () => {} }',
  `const __subs: any[] = []
;(globalThis as any).__push = (radio: any, top: any) => { snapshot.radio = { ...snapshot.radio, ...(radio || {}) }; Object.assign(snapshot, top || {}); const s = { ...snapshot, radio: { ...snapshot.radio } }; __subs.forEach((f) => f(s)); return __subs.length }
export function subscribeSnapshot(fn: any): () => void { __subs.push(fn); return () => { const i = __subs.indexOf(fn); if (i >= 0) __subs.splice(i, 1) } }`)
stubSrc += "\nexport async function resolveEntity(): Promise<any> { return 'United States' }\nexport async function qrzLookup(): Promise<any> { return null }\n"
// Lists the screens map over must be lists: Phone's voice keyer threw on the generic `{}` and Phone never drew.
stubSrc += "export async function getVoiceMessages(): Promise<any> { return [] }\n"
if (!stubSrc.includes('__push')) throw new Error('alarmcell-probe: the stub no longer has the snapshot subscriber to patch')
const names = [...realSrc.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1])
const own = new Set([...stubSrc.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1]))
const stubCode = stubSrc + `\nexport * from ${JSON.stringify(REAL_API)}\n` +
  names.filter((n) => !own.has(n)).map((n) => `export async function ${n}(..._a: unknown[]): Promise<any> { return {} }`).join('\n')
const swap = {
  name: 'alarmcell-probe-api', enforce: 'pre',
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
  const vis = (e) => { const r = e.getBoundingClientRect(); return r.width > 0 && r.height > 0 }
  const strip = [...document.querySelectorAll('.cockpit-qso, .cockpit-txstrip')].find(vis)
  if (!strip || !document.documentElement.getAttribute('data-viewport')) return null
  // NOT BEFORE THE APP HAS PUBLISHED THIS WINDOW'S SIZE. useViewport writes data-viewport, --vw-eff and --vh-eff
  // (and through them the spacing scale) in an animation frame after a resize, and a page Chrome holds hidden
  // runs none: one of four pages sharing a window was measured at 1366x768 with the 700x800 values still up
  // (xs, not sm), and padded Stop TX 1.3 px less a side, until it came to the front between two measures and
  // read as an alarm moving Stop TX 9 px. The app never paints that layout: the frame runs before a paint.
  // Nothing published at all (no frame has run) is not a match either: NaN compares false, and passed.
  const d = document.documentElement, z = parseFloat(getComputedStyle(d).getPropertyValue('--ui-zoom')) || 1
  const vw = parseFloat(d.style.getPropertyValue('--vw-eff')), vh = parseFloat(d.style.getPropertyValue('--vh-eff'))
  if (!Number.isFinite(vw) || !Number.isFinite(vh) || Math.abs(vw * z - innerWidth) > 1 || Math.abs(vh * z - innerHeight) > 1) return null
  const stop = [...strip.querySelectorAll('button')].find((b) => /^stop tx$/i.test(b.textContent.trim()) && vis(b))
  const s = strip.getBoundingClientRect(), st = stop?.getBoundingClientRect()
  const w = strip.querySelector('.ch-txalarm-text')
  // Geometry above is taken first. Where the words are off the window (FT at 150-175 %, whose strip scrolls with
  // its screen, Stop TX with it) the operator scrolls to them, as to Stop TX: they are judged scrolled into
  // view, where nothing may cover them. The scroll is put back before returning, so it never reads as a move.
  let seen = null, why = null
  if (w) {
    let sc = strip; while (sc && !(sc.scrollHeight > sc.clientHeight + 1 && /auto|scroll/.test(getComputedStyle(sc).overflowY))) sc = sc.parentElement
    const saved = sc ? sc.scrollTop : null
    const wr = w.getBoundingClientRect()
    if (wr.top < 0 || wr.bottom > innerHeight) w.scrollIntoView({ block: 'nearest' })
    // Seen = the words themselves are what a point at their middle hits: on screen, topmost and painted (a word
    // that is not painted, or anything over it, is hit through or instead).
    const look = () => { const r = w.getBoundingClientRect(); const x = r.left + r.width / 2, y = r.top + r.height / 2
      const hit = r.width > 0 && x >= 0 && y >= 0 && x < innerWidth && y < innerHeight ? document.elementFromPoint(x, y) : null
      return { r, x, y, hit, seen: !!(hit && w.contains(hit)) } }
    let L = look()
    // A sticky strip (Stop TX parked at the shell's foot) spends a scrollIntoView on its own offset and the words
    // stay where they were. Then the operator's own scroll of the shell, top to end every 6 px, as digishell's
    // reach: the words must be on screen and topmost at some position of it.
    if (!L.seen && sc) { const max = sc.scrollHeight - sc.clientHeight; for (let p = 0; p <= max + 5 && !L.seen; p += 6) { sc.scrollTop = Math.min(p, max); L = look() } }
    const r = L.r, x = L.x, y = L.y, hit = L.hit
    seen = L.seen
    why = { y: Math.round(y), vh: innerHeight, hit: hit ? (hit.className?.toString?.() || hit.tagName).slice(0, 50) : null }
    if (sc && saved != null) sc.scrollTop = saved
  }
  return { lang: document.documentElement.lang, h: Math.round(s.height * 10) / 10, top: Math.round(s.top * 10) / 10,
    stop: st ? [Math.round(st.left * 10) / 10, Math.round(st.top * 10) / 10] : null, words: !!w, seen, why }
})()`

await writeFile(STUB_API, '// generated by ui/probe/alarmcell-probe.mjs; deleted on exit\n')
const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-alarmcell-probe-'))
const server = await createServer({ root: UI, configFile: join(UI, 'vite.config.ts'), cacheDir, logLevel: 'warn', plugins: [swap],
  server: { host: '127.0.0.1', port: 0, strictPort: false, hmr: false } })
await server.listen()
const origin = `http://127.0.0.1:${server.httpServer.address().port}`
const browser = await chrome()

async function openPage(route, [w, h], scale) {
  // A window of its own: the only tab of its window is the visible one, so its animation frames run (see MEASURE).
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank', newWindow: true })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (m, p) => browser.call(m, p, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text)
    return r.result.value
  }
  const size = (sw, sh) => call('Emulation.setDeviceMetricsOverride', { width: sw, height: sh, deviceScaleFactor: 1, mobile: false })
  await size(w, h)
  await call('Page.navigate', { url: `${origin}/probe-blank` })
  await sleep(300)
  const ls = { 'nexus.workspace': 'dx', 'nexus.features.v1': FEATURES, 'tempo-theme': 'dark', 'nexus-ui-scale-mode': scale, 'nexus.locale': 'en' }
  await evaluate(`localStorage.clear(); ${Object.entries(ls).map(([k, v]) => `localStorage.setItem(${JSON.stringify(k)}, ${JSON.stringify(v)})`).join(';')}; 1`)
  // The Page domain on first: without it the script below never ran, so the page errors the probe reports were never
  // collected (and a planted script, such as one disabling animation frames, did nothing).
  await call('Page.enable', {})
  await call('Page.addScriptToEvaluateOnNewDocument', { source: "globalThis.__errs=[]; addEventListener('error', (e) => __errs.push(String(e.message).slice(0, 200))); addEventListener('unhandledrejection', (e) => __errs.push('rejection ' + String(e.reason && (e.reason.message || e.reason)).slice(0, 200)))" })
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
const push = (p, alarms) => p.evaluate(`globalThis.__push(null, ${JSON.stringify({ txAlarms: alarms })})`)
const locale = (p, loc) => p.evaluate(`import('/src/i18n/index.ts').then((m) => { m.setLocale(${JSON.stringify(loc)}); return m.getLocale() })`)

/** One screen at one zoom: every size, every language, the alarm pushed and taken away. */
async function sweep(name, route, scale, sizes, locales, css) {
  const rows = []
  const p = await openPage(route, sizes[0], scale)
  try {
    if (!(await settle(p))) return [{ name, scale, error: 'the strip never drew' }]
    if (!(await push(p, []))) return [{ name, scale, error: 'no snapshot subscriber' }]
    if (css) await p.evaluate(`(() => { const s = document.createElement('style'); s.textContent = ${JSON.stringify(css)}; document.head.appendChild(s); return 1 })()`)
    for (const [w, h] of sizes) {
      await p.size(w, h)
      await settle(p)
      for (const loc of locales) {
        if ((await locale(p, loc)) !== loc) { rows.push({ name, scale, w, h, loc, error: `the language did not switch to ${loc}` }); continue }
        await push(p, []); let off = await settle(p)
        if (!off) { await sleep(2000); off = await settle(p) }
        await push(p, [ALARM]); let on = await settle(p)
        if (!on) { await sleep(2000); on = await settle(p) }
        if (!off || !on) {
          const raw = await p.evaluate(`JSON.stringify({ m: ${MEASURE}, hash: location.hash, vp: document.documentElement.getAttribute('data-viewport'), mains: [...document.querySelectorAll('main')].map((e) => e.className.toString().slice(0, 40) + (e.getBoundingClientRect().height > 0 ? '' : ' (hidden)')), strips: [...document.querySelectorAll('.cockpit-txstrip, .cockpit-qso')].map((e) => Math.round(e.getBoundingClientRect().width) + 'x' + Math.round(e.getBoundingClientRect().height)), errs: (globalThis.__errs || []).slice(0, 3), body: document.body.innerText.slice(0, 300) })`).catch((e) => `threw: ${String(e.message ?? e).slice(0, 160)}`)
          rows.push({ name, scale, w, h, loc, error: `no measurement (${off ? 'with' : 'without'} the alarm; now ${raw})` })
          continue
        }
        const moved = off.h !== on.h || JSON.stringify(off.stop) !== JSON.stringify(on.stop) || off.top !== on.top
        rows.push({ name, scale, w, h, loc, off, on, moved, hidden: on.words && !on.seen })
      }
    }
  } finally { await p.close() }
  return rows
}

let code = 0
try {
  // Four pages at a time in the one Chrome: each page is its own target with its own viewport.
  const jobs = SCREENS.flatMap(([name, route]) => SCALES.map((scale) => [name, route, scale]))
  const all = []
  const worker = async () => { for (let j = jobs.shift(); j; j = jobs.shift()) all.push(...await sweep(j[0], j[1], j[2], SIZES, LOCALES)) }
  await Promise.all([worker(), worker(), worker(), worker()])
  all.sort((a, b) => `${a.name}${a.scale}${a.w}${a.loc}`.localeCompare(`${b.name}${b.scale}${b.w}${b.loc}`))
  for (const r of all) {
    const at = `${r.name} at ${r.w ?? '?'}×${r.h ?? '?'} ${r.scale} % ${r.loc ?? ''}`
    if (r.error) { console.error(`alarmcell-probe: ${at}: ${r.error}`); code = 2; continue }
    if (r.moved) { console.error(`alarmcell-probe: RED ${at} — an alarm moved the strip or Stop TX: strip ${r.off.h} → ${r.on.h} px, Stop TX ${JSON.stringify(r.off.stop)} → ${JSON.stringify(r.on.stop)}`); code = code || 1 }
    if (r.hidden) { console.error(`alarmcell-probe: RED ${at} — the alarm's words are not on screen and topmost: ${JSON.stringify(r.on.why)}`); code = code || 1 }
  }
  const ok = all.filter((r) => !r.error)
  console.log(`alarmcell-probe: ${ok.length} rows (${SCREENS.length} screens × ${SCALES.length} zooms × ${SIZES.length} sizes × ${LOCALES.length} languages), ${ok.filter((r) => r.moved).length} moved, ${ok.filter((r) => r.hidden).length} hidden`)
  // THE CONTROL: the cell's one height taken off must make an alarm move Stop TX somewhere it was measured to.
  let bit = 0
  for (const [name, route, scale, loc, size] of CONTROL) {
    const [r] = await sweep(name, route, scale, [size], [loc], NO_HEIGHT)
    console.log(`alarmcell-probe: CONTROL ${name} ${size.join('×')} ${scale} % ${loc}, the cell's height taken off: ${r?.error ?? `strip ${r.off.h} → ${r.on.h} px, Stop TX ${JSON.stringify(r.off.stop)} → ${JSON.stringify(r.on.stop)}`}`)
    if (r && !r.error && r.moved) bit++
  }
  if (bit === 0) { console.error('alarmcell-probe: CONTROL FAILED — with the cell’s height taken off no alarm moved Stop TX; the check cannot fail'); code = code || 1 }
  {
    const [name, route, scale, loc, size] = REACH_CONTROL
    const [r] = await sweep(name, route, scale, [size], [loc], UNPAINTED)
    const caught = r && !r.error && r.hidden
    console.log(`alarmcell-probe: CONTROL ${name} ${size.join('×')} ${scale} % ${loc}, the words unpainted: ${r?.error ?? (caught ? 'judged hidden at every scroll position (caught)' : 'judged seen')}`)
    if (!caught) { console.error('alarmcell-probe: CONTROL FAILED — unpainted alarm words were judged seen; the reach check cannot fail'); code = code || 1 }
  }
  if (code === 0) console.log(`alarmcell-probe: PASS — a transmitter alarm coming and going leaves every operating screen's TX strip the same height and Stop TX where it was, its words on screen, in all ${LOCALES.length} languages at ${SCALES.join(' and ')} % (${SIZES.map((s) => s.join('×')).join(', ')}); the cell without its height moves Stop TX (control, ${bit} of ${CONTROL.length})`)
  process.exitCode = code
} finally {
  await browser.stop(); await server.close(); await rm(cacheDir, { recursive: true, force: true }); await rm(STUB_API, { force: true })
}
