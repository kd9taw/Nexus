// FT's next-over line in real Chrome, for what jsdom cannot see: `scripts/browser-probe --desktop ftline`.
//
// FT's QSO strip (OperateQsoStrip, `.cockpit-qso`) breaks into two stable lines at `.cq-break`: the TX cluster and
// Stop TX first, then the next-over box (`.cq-now`, "TX CQ KR4FQG EM64" with ↻), Free text, Send and Log. That second
// line starts at the strip's left edge. 1.24.0 put the transmitter alarm cell (TxAlarmBanner, `.ch-txalarm-own`) first
// on it, a zero-basis grower: wherever the line had room for it and the next-over box, the EMPTY cell stood before the
// box and took half the line's free room, and the box started ~620 px in at 2560x1440 (operator report).
//
// The probe serves the REAL app (index.html, App, OperateCockpit) with src/api.ts swapped for probe/waterfalls-api.ts,
// as alarmcell-probe does, at 1024x768, 1366x768, 1920x1080 and 2560x1440, each at 65, 100 and 175 %, and asserts:
//   (a) wherever the next-over box is NOT on Stop TX's line, its left edge is the strip's content edge (its left
//       border and padding), to 1 px;
//   (b) Stop TX is wholly inside the window and topmost at its centre; with a transmitter alarm up, the alarm's words
//       are on screen and topmost (scrolled to, as alarmcell-probe judges them, where the strip scrolls).
// FTLINE_OUT=<file> writes every row's geometry (each strip control's box, the alarm's words) for a before/after diff.
//
// THE CONTROL, run every time: the 1.24.0 placement planted (`.ch-txalarm-own` placed first on the second line and
// growing: the cell ordered back before the box, wherever the strip holds it) at 2560x1440 100 %.
// The box must then be judged off its edge; if it is not, (a) cannot fail and the probe says so instead of passing.
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
const STUB_API = join(UI, 'probe', '__ftline-api.ts')
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const SCALES = process.env.FTLINE_SCALES ? process.env.FTLINE_SCALES.split(',') : ['65', '100', '175']
const SIZES = process.env.FTLINE_SIZES ? process.env.FTLINE_SIZES.split(',').map((s) => s.split('x').map(Number)) : [[1024, 768], [1366, 768], [1920, 1080], [2560, 1440]]
const ALARM = { id: 1, text: 'High SWR: 3.2:1 — transmit was stopped', radioId: 1, radioName: 'IC-7300', atMs: Date.UTC(2026, 10, 8, 14, 22, 33) }
// The 1.24.0 placement: the empty alarm cell first on the next-over line, growing into its room.
const PLANT = '.cockpit-qso > .cq-now, .cockpit-qso > .cq-now ~ * { order: 1 } .cockpit-qso > .ch-txalarm.ch-txalarm-own { order: 0 }'
const CONTROL = ['100', true, [2560, 1440]]

const realSrc = await readFile(REAL_API, 'utf8')
let stubSrc = await readFile(STUB_SRC, 'utf8')
stubSrc = stubSrc.replace('export function subscribeSnapshot(_fn: unknown): () => void { return () => {} }',
  `const __subs: any[] = []
;(globalThis as any).__push = (radio: any, top: any) => { snapshot.radio = { ...snapshot.radio, ...(radio || {}) }; Object.assign(snapshot, top || {}); const s = { ...snapshot, radio: { ...snapshot.radio } }; __subs.forEach((f) => f(s)); return __subs.length }
export function subscribeSnapshot(fn: any): () => void { __subs.push(fn); return () => { const i = __subs.indexOf(fn); if (i >= 0) __subs.splice(i, 1) } }`)
stubSrc += "\nexport async function resolveEntity(): Promise<any> { return 'United States' }\nexport async function qrzLookup(): Promise<any> { return null }\n"
if (!stubSrc.includes('__push')) throw new Error('ftline-probe: the stub no longer has the snapshot subscriber to patch')
const names = [...realSrc.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1])
const own = new Set([...stubSrc.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1]))
const stubCode = stubSrc + `\nexport * from ${JSON.stringify(REAL_API)}\n` +
  names.filter((n) => !own.has(n)).map((n) => `export async function ${n}(..._a: unknown[]): Promise<any> { return {} }`).join('\n')
const swap = {
  name: 'ftline-probe-api', enforce: 'pre',
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
  const strip = [...document.querySelectorAll('.cockpit-qso')].find(vis)
  if (!strip || !document.documentElement.getAttribute('data-viewport')) return null
  // Not before the app has published this window's size (alarmcell-probe's MEASURE says why).
  const d = document.documentElement, z = parseFloat(getComputedStyle(d).getPropertyValue('--ui-zoom')) || 1
  const vw = parseFloat(d.style.getPropertyValue('--vw-eff')), vh = parseFloat(d.style.getPropertyValue('--vh-eff'))
  if (!Number.isFinite(vw) || !Number.isFinite(vh) || Math.abs(vw * z - innerWidth) > 1 || Math.abs(vh * z - innerHeight) > 1) return null
  const now = strip.querySelector(':scope > .cq-now')
  const stop = [...strip.querySelectorAll('button')].find((b) => /^stop tx$/i.test(b.textContent.trim()) && vis(b))
  if (!now || !stop) return null
  const r1 = (v) => Math.round(v * 10) / 10
  const box = (e) => { const r = e.getBoundingClientRect(); return [r1(r.left), r1(r.top), r1(r.width), r1(r.height)] }
  const s = strip.getBoundingClientRect(), cs = getComputedStyle(strip), sz = s.width / strip.offsetWidth
  const edge = s.left + ((parseFloat(cs.borderLeftWidth) || 0) + (parseFloat(cs.paddingLeft) || 0)) * sz
  const nr = now.getBoundingClientRect(), sr = stop.getBoundingClientRect()
  const top = (r) => { const x = r.left + r.width / 2, y = r.top + r.height / 2
    if (r.left < 0 || r.top < 0 || r.right > innerWidth || r.bottom > innerHeight) return 'off-window'
    const hit = document.elementFromPoint(x, y); return hit && (hit === stop || stop.contains(hit)) ? 'ok' : 'covered by ' + (hit?.className?.toString?.() || hit?.tagName || 'nothing').slice(0, 40) }
  // Every control of the strip, by its words, for the before/after diff of (b).
  const ctl = {}
  for (const b of strip.querySelectorAll('button, input, .cq-state, .cq-next')) if (vis(b)) ctl[(b.getAttribute('aria-label') || b.textContent || b.className).trim().slice(0, 24) + '#' + b.tagName] = box(b)
  const w = strip.querySelector('.ch-txalarm-text')
  let words = null
  if (w) {
    let sc = strip; while (sc && !(sc.scrollHeight > sc.clientHeight + 1 && /auto|scroll/.test(getComputedStyle(sc).overflowY))) sc = sc.parentElement
    const saved = sc ? sc.scrollTop : null
    const look = () => { const r = w.getBoundingClientRect(); const x = r.left + r.width / 2, y = r.top + r.height / 2
      const hit = r.width > 0 && x >= 0 && y >= 0 && x < innerWidth && y < innerHeight ? document.elementFromPoint(x, y) : null
      return { r, seen: !!(hit && w.contains(hit)) } }
    let L = look(); const first = L.seen
    if (!L.seen && sc) { const max = sc.scrollHeight - sc.clientHeight; for (let p = 0; p <= max + 5 && !L.seen; p += 6) { sc.scrollTop = Math.min(p, max); L = look() } }
    words = { box: box(w), seenUnscrolled: first, seen: L.seen }
    if (sc && saved != null) sc.scrollTop = saved
  }
  // WHAT STANDS BEFORE THE BOX ON ITS LINE: the strip's content edge, or the right edge of the nearest child drawn
  // before it on the same line, plus the strip's column gap. Not the break, the absorber, or the alarm cell: an empty
  // box before the next-over box is exactly what (a) is about, and the alarm's words are the operator's, not a slot.
  const gap = (parseFloat(cs.columnGap) || 0) * sz
  let lead = edge, leadGap = 0
  for (const c of strip.children) {
    if (c === now || c.matches('.cq-break, .cq-spacer, .ch-txalarm') || !vis(c)) continue
    const r = c.getBoundingClientRect()
    if (r.right <= nr.left + 0.5 && r.bottom > nr.top + 1 && r.top < nr.bottom - 1 && r.right > lead) { lead = r.right; leadGap = gap }
  }
  const brk = strip.querySelector(':scope > .cq-break')
  return { strip: box(strip), edge: r1(edge), lead: r1(lead + leadGap), now: box(now), stop: box(stop), stopTop: top(sr),
    ownLine: nr.top >= sr.bottom - 1, rail: !!document.querySelector('.shell[data-dash-rail="on"]'), broke: !!brk && getComputedStyle(brk).display !== 'none', ctl, words }
})()`

await writeFile(STUB_API, '// generated by ui/probe/ftline-probe.mjs; deleted on exit\n')
const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-ftline-probe-'))
const server = await createServer({ root: UI, configFile: join(UI, 'vite.config.ts'), cacheDir, logLevel: 'warn', plugins: [swap],
  server: { host: '127.0.0.1', port: 0, strictPort: false, hmr: false } })
await server.listen()
const origin = `http://127.0.0.1:${server.httpServer.address().port}`
const browser = await chrome()

async function openPage([w, h], scale, rail) {
  // A window of its own: the only tab of its window is the visible one, so its animation frames run.
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
  const ls = { 'nexus.workspace': 'dx', 'tempo-theme': 'dark', 'nexus-ui-scale-mode': scale, 'nexus.locale': 'en', ...(rail ? { 'nexus.dashrail.sections': '{"operate":true}' } : {}) }
  await evaluate(`localStorage.clear(); ${Object.entries(ls).map(([k, v]) => `localStorage.setItem(${JSON.stringify(k)}, ${JSON.stringify(v)})`).join(';')}; 1`)
  await call('Page.navigate', { url: `${origin}/#operate` })
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

/** One zoom: every size, without an alarm and with one. */
async function sweep(scale, rail, sizes, css) {
  const rows = []
  const p = await openPage(sizes[0], scale, rail)
  try {
    if (!(await settle(p))) return [{ scale, error: 'the strip never drew' }]
    if (!(await push(p, []))) return [{ scale, error: 'no snapshot subscriber' }]
    if (css) await p.evaluate(`(() => { const s = document.createElement('style'); s.textContent = ${JSON.stringify(css)}; document.head.appendChild(s); return 1 })()`)
    for (const [w, h] of sizes) {
      await p.size(w, h)
      await push(p, []); const off = await settle(p)
      await push(p, [ALARM]); const on = await settle(p)
      await push(p, [])
      if (!off || !on) { rows.push({ scale, w, h, error: `no measurement ${off ? 'with' : 'without'} the alarm` }); continue }
      // (a): the box stands right after what is drawn before it on its line: the strip's edge where it starts the
      // second line, the TX state's readouts on the xl one-row strip (as 1.23.0 drew both).
      const offEdge = (m) => Math.abs(m.now[0] - m.lead) > 1
      rows.push({ scale, rail, w, h, off, on, offEdge: offEdge(off) || offEdge(on),
        stopBad: off.stopTop !== 'ok' || on.stopTop !== 'ok', hidden: !on.words || !on.words.seen })
    }
  } finally { await p.close() }
  return rows
}

let code = 0
try {
  // One zoom at a time: the pages share an origin, so their localStorage (the zoom pin) is one store.
  const all = []
  for (const s of SCALES) for (const rail of [false, true]) all.push(...await sweep(s, rail, SIZES))
  for (const r of all) {
    const at = `FT at ${r.w ?? '?'}×${r.h ?? '?'} ${r.scale} % rail ${r.rail ? (r.off?.rail ? 'on' : 'on (folded)') : 'off'}`
    if (r.error) { console.error(`ftline-probe: ${at}: ${r.error}`); code = 2; continue }
    console.log(`ftline-probe: ${at}: next-over box at x=${r.off.now[0]} (lead ${r.off.lead}, strip edge ${r.off.edge}; ${r.off.ownLine ? 'own line' : "on Stop TX's line"}; with an alarm x=${r.on.now[0]}), Stop TX ${r.off.stopTop}/${r.on.stopTop}, alarm words ${r.on.words ? `${r.on.words.seenUnscrolled ? 'seen' : r.on.words.seen ? 'seen once scrolled' : 'HIDDEN'} at ${JSON.stringify(r.on.words.box)}` : 'MISSING'}, strip ${r.off.strip[3]} → ${r.on.strip[3]} px`)
    if (r.offEdge) { console.error(`ftline-probe: RED ${at} — the next-over box does not start where its line does: x=${r.off.now[0]} / ${r.on.now[0]} vs ${r.off.lead} / ${r.on.lead}`); code = code || 1 }
    if (r.stopBad) { console.error(`ftline-probe: RED ${at} — Stop TX is not on screen and topmost: ${r.off.stopTop} / ${r.on.stopTop}`); code = code || 1 }
    if (r.hidden) { console.error(`ftline-probe: RED ${at} — the alarm's words are not on screen and topmost`); code = code || 1 }
  }
  if (process.env.FTLINE_OUT) await writeFile(process.env.FTLINE_OUT, JSON.stringify(all, null, 1))
  const ok = all.filter((r) => !r.error)
  console.log(`ftline-probe: ${ok.length} rows (${SCALES.length} zooms × ${SIZES.length} sizes × rail off/on), ${ok.filter((r) => r.offEdge).length} off the edge, ${ok.filter((r) => r.stopBad).length} Stop TX bad, ${ok.filter((r) => r.hidden).length} alarm hidden`)
  {
    const [scale, rail, size] = CONTROL
    const [r] = await sweep(scale, rail, [size], PLANT)
    const caught = r && !r.error && r.offEdge
    console.log(`ftline-probe: CONTROL FT ${size.join('×')} ${scale} %, the empty alarm cell planted before the box: ${r?.error ?? (caught ? `box at x=${r.off.now[0]} vs ${r.off.lead} (caught)` : `box at x=${r.off.now[0]}, judged where its line starts`)}`)
    if (!caught) { console.error('ftline-probe: CONTROL FAILED — the planted cell did not put the box off the edge; (a) cannot fail'); code = code || 1 }
  }
  if (code === 0) console.log(`ftline-probe: PASS — FT's next-over box starts where its line does (the strip's left edge on the second line), Stop TX is on screen and topmost and the alarm's words are seen, at ${SCALES.join(', ')} % with the dashboard rail off and on (${SIZES.map((s) => s.join('×')).join(', ')})`)
  process.exitCode = code
} finally {
  await browser.stop(); await server.close(); await rm(cacheDir, { recursive: true, force: true }); await rm(STUB_API, { force: true })
}
