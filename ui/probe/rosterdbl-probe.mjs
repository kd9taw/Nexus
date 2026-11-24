// The Call Roster's double-click in the real app, in real Chrome: `scripts/browser-probe --desktop rosterdbl`.
//
// THE REPORT (operator, 1.25.0, with a video): a double-click on a Call Roster row worked a DIFFERENT station, the
// row above or below. The tooltip said "Double-click to work KM7CAB"; a moment later the roster stood one row lower
// and the QSO was with KB2DJJ, the row above. MEASURED: the roster's filter bar wraps, and its Spot button names the
// selected station ("Spot" with nothing selected, "Spot KM7CAB" after a click). The first click of the double-click
// selected the row, the longer label pushed the bar onto another line, the grid below it moved down a row, and the
// second click landed on the row that had moved under the pointer, whose double-click worked that station.
//
// The probe serves the REAL app (index.html, main.tsx, App) with the app's own Vite config and src/api.ts swapped for
// probe/waterfalls-api.ts with a heard roster pushed in, in the Roster layout, and checks two things:
//   THE BAR (the layout): the grid's top, measured from the roster's own top, is the same with nothing selected and
//     with the longest call selected (3D2/KD9TAW/P), at the app's fit and at its 175 % on 1366×768 and 1920×1080, at
//     the roster's own width there AND with the roster held at every width from 240 to 900 px in 10 px steps (the
//     pane can be any width its seam gives it; a bar that re-wraps at ONE width is the bug).
//   THE GESTURE (the action): a real double-click whose two clicks land on different rows, because the grid is moved
//     down one row between them (a box planted above it), works the station the FIRST click landed on.
// THE CONTROL, run every time: the Spot button let back to its label's own width must be caught by the bar check at
// some width, or the check cannot fail and the probe says so instead of passing.
//
// exit: 0 every check held · 1 a red, or a control that could not fail · 2 the roster never drew
import { createServer } from 'vite'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'
import { FHD, LAPTOP, css } from './sizes.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const REAL_API = join(UI, 'src', 'api.ts')
const STUB_SRC = join(UI, 'probe', 'waterfalls-api.ts')
const STUB_API = join(UI, 'probe', '__rosterdbl-api.ts')
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const LONG = '3D2/KD9TAW/P'
const SIZES = [LAPTOP, FHD]
const SCALES = ['auto', '175']
const WIDTHS = Array.from({ length: 67 }, (_, i) => 240 + i * 10)

// THE API SWAP — the waterfalls probe's stub, with the snapshot's subscription made real (`__push`) and every
// station worked recorded (`__worked`).
const realSrc = await readFile(REAL_API, 'utf8')
let stub = (await readFile(STUB_SRC, 'utf8'))
  .replace('export function subscribeSnapshot(_fn: unknown): () => void { return () => {} }',
    `const __subs: any[] = []
;(globalThis as any).__push = (top: any) => { const { radio, ...rest } = top || {}; Object.assign(snapshot, rest); Object.assign(snapshot.radio, radio || {}); const s = { ...snapshot, radio: { ...snapshot.radio } }; __subs.forEach((f) => f(s)); return __subs.length }
export function subscribeSnapshot(fn: any): () => void { __subs.push(fn); return () => { const i = __subs.indexOf(fn); if (i >= 0) __subs.splice(i, 1) } }`)
if (!stub.includes('__push')) throw new Error('rosterdbl: the stub seam __push did not apply; probe/waterfalls-api.ts changed shape')
stub += `
const __worked: string[] = []
;(globalThis as any).__worked = () => __worked
export async function callStation(call: string): Promise<any> { __worked.push(call); return { ...snapshot, radio: { ...snapshot.radio } } }
// A row's click selects it through the engine, which answers with the snapshot naming it, as the station does.
export async function selectPeer(peer: string | null): Promise<any> { snapshot.activePeer = peer; return { ...snapshot, radio: { ...snapshot.radio } } }
export async function getDeclination(): Promise<any> { return 0 }
// The recall card the selection opens asks these; answered as a station with no callbook does.
export async function resolveEntity(): Promise<any> { return 'United States' }
export async function qrzLookup(): Promise<any> { return null }
`
const names = [...realSrc.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1])
const own = new Set([...stub.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1]))
const stubCode = stub + `\nexport * from ${JSON.stringify(REAL_API)}\n` +
  names.filter((n) => !own.has(n)).map((n) => `export async function ${n}(..._a: unknown[]): Promise<any> { return {} }`).join('\n')
const swap = {
  name: 'rosterdbl-probe-api', enforce: 'pre',
  async resolveId(source, importer, opts) {
    if (opts?.scan) return null
    if (source === STUB_API) return STUB_API
    if (!importer || importer === STUB_API || !/api(\.ts)?$/.test(source)) return null
    const r = await this.resolve(source, importer, { ...opts, skipSelf: true })
    return r && r.id.split('?')[0] === REAL_API ? STUB_API : null
  },
  load(id) { return id === STUB_API ? stubCode : null },
}

// The heard roster: fourteen stations, all this slot, strongest first so the order is fixed, the long call among them.
const CALLS = ['W1AW', 'TN8GD', 'KC4WQ', 'NM3B', 'K0PT', 'KB2DJJ', 'KM7CAB', LONG, 'NL7FJ', 'N0AK', 'KD2WDR', 'AE0LB', 'KM4SXE', 'N5RJR']
const STATIONS = CALLS.map((call, i) => ({ call, grid: 'EM28', snr: 10 - i, lastHeardSlot: 100, heardCount: 1, presence: 'heard', worked: false,
  freqHz: 600 + i * 100, calling: null, country: 'United States' }))

const ROSTER = `[...document.querySelectorAll('main.operate-cockpit .operate-roster')].find((e) => e.closest('[hidden]') == null && e.getBoundingClientRect().width > 0)`
/** The grid's top from the roster's own top and in the window, the bar's lines, Spot's label and the roster's width. */
const MEASURE = `(() => {
  const r = ${ROSTER}
  if (!r) return null
  const g = r.querySelector('.or-grid'), bar = r.querySelector('.or-filters'), spot = r.querySelector('.or-spot')
  const rr = r.getBoundingClientRect(), gr = g.getBoundingClientRect()
  // The bar's lines: its items' vertical centres, a new line wherever one sits below the last by more than 8 px.
  const mids = [...bar.children].filter((k) => k.getBoundingClientRect().width > 0).map((k) => { const q = k.getBoundingClientRect(); return q.top + q.height / 2 }).sort((a, b) => a - b)
  const lines = mids.reduce((n, m, i) => n + (i > 0 && m - mids[i - 1] > 8 ? 1 : 0), mids.length ? 1 : 0)
  return { rel: Math.round((gr.top - rr.top) * 10) / 10, abs: Math.round(gr.top * 10) / 10, lines, w: Math.round(rr.width),
    spot: spot ? spot.textContent : null, vp: document.documentElement.getAttribute('data-viewport'),
    zoom: getComputedStyle(document.documentElement).getPropertyValue('--ui-zoom').trim() || '1' }
})()`
const holdWidth = (w) => `(() => { const r = ${ROSTER}; if (!r) return 0; if (${w} == null) { r.style.removeProperty('flex'); r.style.removeProperty('width') } else { r.style.flex = 'none'; r.style.width = ${w} + 'px' } return 1 })()`
const rowCentre = (call) => `(() => { const r = ${ROSTER}; const row = [...r.querySelectorAll('.or-row[role=row]')].find((x) => x.querySelector('.or-call')?.textContent.includes(${JSON.stringify(call)}))
  if (!row) return null; row.scrollIntoView({ block: 'nearest' }); const q = row.getBoundingClientRect(); return { x: q.left + Math.min(40, q.width / 4), y: q.top + q.height / 2, h: q.height } })()`

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-rosterdbl-probe-'))
const server = await createServer({ root: UI, configFile: join(UI, 'vite.config.ts'), cacheDir, logLevel: 'warn', plugins: [swap],
  server: { host: '127.0.0.1', port: 0, strictPort: false, hmr: false } })
await server.listen()
const origin = `http://127.0.0.1:${server.httpServer.address().port}`
const browser = await chrome()

async function openPage(s, scale) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (m, p) => browser.call(m, p, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text)
    return r.result.value
  }
  await call('Emulation.setDeviceMetricsOverride', { width: s.w, height: s.h, deviceScaleFactor: s.scale, mobile: false })
  await call('Page.navigate', { url: `${origin}/probe-blank` })
  await sleep(300)
  const ls = { 'nexus.workspace': 'dx', 'tempo-theme': 'dark', 'nexus.locale': 'en', 'nexus.operateLayout': 'roster', ...(scale !== 'auto' ? { 'nexus-ui-scale-mode': scale } : {}) }
  await evaluate(`localStorage.clear(); ${Object.entries(ls).map(([k, v]) => `localStorage.setItem(${JSON.stringify(k)}, ${JSON.stringify(v)})`).join(';')}; 1`)
  await call('Page.navigate', { url: `${origin}/#operate` })
  /** One press and release; `n` is the click count the browser reports (2 = the second click of a double-click). */
  const press = async (at, n) => {
    await call('Input.dispatchMouseEvent', { type: 'mouseMoved', x: at.x, y: at.y })
    await call('Input.dispatchMouseEvent', { type: 'mousePressed', x: at.x, y: at.y, button: 'left', clickCount: n })
    await call('Input.dispatchMouseEvent', { type: 'mouseReleased', x: at.x, y: at.y, button: 'left', clickCount: n })
  }
  for (let i = 0; i < 120; i++) { await sleep(250); if (await evaluate(`!!(${ROSTER})`).catch(() => false)) break }
  await evaluate(`globalThis.__push({ stations: ${JSON.stringify(STATIONS)}, radio: { slot: 100 } })`)
  await sleep(400)
  return { evaluate, press, close: () => browser.call('Target.closeTarget', { targetId }) }
}
const frames = (p) => p.evaluate('new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(() => r(1), 30))))')
/** Every width, measured; `null` is the roster's own width. */
async function sweep(p) {
  const out = new Map()
  for (const w of [null, ...WIDTHS]) {
    await p.evaluate(holdWidth(w))
    await frames(p)
    out.set(w, await p.evaluate(MEASURE))
  }
  await p.evaluate(holdWidth(null))
  await frames(p)
  return out
}

const t0 = Date.now()
const reds = []
const red = (s) => { reds.push(s); console.log('RED ' + s) }
let drew = 0

/** The bar check at one size and scale: every width, nothing selected against the long call selected. `style` plants
 *  a rule first (the control). Returns the widths where the grid moved. */
async function barCheck(s, scale, style) {
  const p = await openPage(s, scale)
  try {
    if (style) await p.evaluate(`(() => { const e = document.createElement('style'); e.textContent = ${JSON.stringify(style)}; document.head.appendChild(e); return 1 })()`)
    const first = await p.evaluate(MEASURE)
    if (!first) return null
    if (first.spot !== 'Spot') red(`${s.name} ${scale}: the Spot button reads "${first.spot}" with nothing selected`)
    const none = await sweep(p)
    const at = await p.evaluate(rowCentre(LONG))
    if (!at) { red(`${s.name} ${scale}: no ${LONG} row to select`); return [] }
    // In the window too, at the roster's own width: measured right before and right after the click (finding the
    // row may have scrolled the cockpit to it, so the sweep's own-width reading is not the before).
    await frames(p)
    const a0 = await p.evaluate(MEASURE)
    await p.press(at, 1)
    await sleep(150)
    await frames(p)
    const b0 = await p.evaluate(MEASURE)
    const sel = await sweep(p)
    const moved = []
    if (Math.abs(a0.abs - b0.abs) > 0.5) moved.push({ w: `at its own ${a0.w} px, in the window`, a: a0, b: b0 })
    for (const [w, a] of none) {
      const b = sel.get(w)
      if (!a || !b) continue
      if (b.spot !== `Spot ${LONG}`) red(`${s.name} ${scale}: Spot reads "${b.spot}" with ${LONG} selected`)
      if (Math.abs(a.rel - b.rel) > 0.5) moved.push({ w: w == null ? `at its own ${a.w} px` : `held at ${w} px`, a, b })
    }
    const own = none.get(null)
    if (!style) console.log(`${s.name} ${scale} (${css(s)}) [${own.vp} ×${own.zoom}]: roster ${own.w} px, bar ${own.lines} line(s); grid moved at ${moved.length}/${none.size} widths`)
    return moved
  } finally {
    await p.close()
  }
}

for (const s of SIZES) for (const scale of SCALES) {
  const moved = await barCheck(s, scale)
  if (moved == null) { console.log(`${s.name} ${scale}: no roster drawn`); continue }
  drew++
  for (const m of moved.slice(0, 6)) red(`${s.name} ${scale}, the roster ${m.w}: selecting ${LONG} moves the grid ${Math.round((m.b.rel - m.a.rel) * 10) / 10} px (in the window ${Math.round((m.b.abs - m.a.abs) * 10) / 10}) (bar ${m.a.lines} → ${m.b.lines} lines; window top ${m.a.abs} → ${m.b.abs})`)
  if (moved.length > 6) red(`${s.name} ${scale}: … and at ${moved.length - 6} more widths`)
}

// THE GESTURE: a double-click whose second click lands one row lower, because the grid moved between the clicks.
{
  const p = await openPage(FHD, 'auto')
  try {
    const target = 'KM7CAB'
    const at = await p.evaluate(rowCentre(target))
    if (!at) red(`gesture: no ${target} row`)
    else {
      await p.press(at, 1)
      await sleep(60)
      // The shift the re-wrapping bar made: one row's height pushed in above the grid.
      await p.evaluate(`(() => { const r = ${ROSTER}; const d = document.createElement('div'); d.id = 'shift'; d.style.cssText = 'flex:none;height:${at.h}px'; r.insertBefore(d, r.querySelector('.or-grid')); return 1 })()`)
      await frames(p)
      const under = await p.evaluate(`(() => { const e = document.elementFromPoint(${at.x}, ${at.y}); return e?.closest('.or-row')?.querySelector('.or-call')?.textContent.replace('↗', '').trim() ?? null })()`)
      await p.press(at, 2)
      await sleep(200)
      const worked = await p.evaluate('globalThis.__worked()')
      console.log(`gesture: first click on ${target}, the second on ${under}; worked ${JSON.stringify(worked)}`)
      if (under === target || under == null) red(`gesture: the planted shift did not move another row under the pointer (${under}) — the check cannot fail`)
      if (worked.length !== 1 || worked[0] !== target) red(`gesture: a double-click begun on ${target} worked ${JSON.stringify(worked)}`)
    }
  } finally {
    await p.close()
  }
}

// THE CONTROL: Spot back to its label's own width must re-wrap the bar at some width.
const control = await barCheck(LAPTOP, 'auto', '.or-spot{width:auto!important;min-width:0!important;max-width:none!important}')
const caught = (control?.length ?? 0) > 0
console.log(`CONTROL — Spot at its label's own width: ${caught ? `caught at ${control.length} widths` : 'NOT caught'}`)

await browser.close?.()
await server.close()
await rm(cacheDir, { recursive: true, force: true }).catch(() => {})
if (!drew) { console.log('rosterdbl: ERROR the roster never drew'); process.exit(2) }
if (!caught) red('CONTROL: Spot at its label\'s own width was not caught — the bar check cannot fail')
console.log(reds.length ? `rosterdbl: FAIL — ${reds.length} red` : `rosterdbl: PASS — ${drew} size/scale pairs, every width held, the gesture worked its first row, the control caught, in ${Math.round((Date.now() - t0) / 1000)} s`)
process.exit(reds.length ? 1 : 0)
