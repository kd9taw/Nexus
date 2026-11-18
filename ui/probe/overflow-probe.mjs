// The cockpit header's ⋯ group in real Chrome, for what jsdom cannot see: `scripts/browser-probe --desktop overflow`.
//
// THE REPORT (a 1.25 beta tester): on Phone the ⋯ group (memories, the rotator, split, REC) opened off the window's
// left edge. It was always right-aligned to its button, and when the header wraps (the Dashboard rail on at 1080p,
// or a narrower lg/md window) ⋯ is the first thing on the header's second line, about 145 px in, so its 616 px group
// started at −441 px, the rest under the mode bar. CW draws the same group. Now it opens on the side with room,
// rightward first (components/HeaderOverflow `placeOverflow`, cockpit-panes.css `.ch-overflow-start`).
//
// The probe serves the REAL app with src/api.ts swapped for probe/waterfalls-api.ts, opens Phone and CW with fresh
// storage, the rail off and on, at 1920×1080 (the reported screen), 1536×864 and 1440×900 (a narrow lg, and md, where
// the header wraps without the rail too), 1920×1200 and 2560×1440 at 125 % (a wide lg), clicks ⋯ and asserts:
//   1. the open group lies inside the cockpit (`main`) and inside the window, to a px;
//   2. it is the topmost thing at its four inner corners and its centre (nothing, the mode bar included, paints
//      over it).
// THE CONTROL, run every time: the group's old shape (right-aligned to the button, whatever the room) planted at
// 1920×1080 with the rail on, on Phone. It must go red there; if it does not, the check cannot fail and the probe
// says so instead of passing.
//
// exit: 0 held everywhere · 1 a red, or a control that could not fail · 2 a screen never drew its ⋯
import { createServer } from 'vite'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'
import { FHD, FHD_125, MID, WUXGA, at, metrics } from './sizes.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const REAL_API = join(UI, 'src', 'api.ts')
const STUB_API = join(UI, 'probe', '__overflow-api.ts')
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const SIZES = [FHD, FHD_125, MID, WUXGA, at(2560, 1440, 125)]
const SCREENS = ['phone', 'cw']
// Every operating screen on, as an operator who uses them has it (a fresh install's profile leaves some off, and
// the app then moves away from one it was opened on).
const FEATURES = JSON.stringify({ profile: 'custom', enabled: { js8: true, jtty: true, rtty: true, psk: true, sstv: true } })
const OLD_SHAPE = '.ch-overflow-panel { left: auto !important; right: 0 !important }'

const stub = await readFile(join(UI, 'probe', 'waterfalls-api.ts'), 'utf8')
const names = [...(await readFile(REAL_API, 'utf8')).matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1])
const own = new Set([...stub.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1]))
const stubCode = stub + `\nexport * from ${JSON.stringify(REAL_API)}\n` +
  names.filter((n) => !own.has(n)).map((n) => `export async function ${n}(..._a: unknown[]): Promise<any> { return (globalThis as any).__DEFAULT_ARR ? [] : {} }`).join('\n')
const swap = {
  name: 'overflow-api', enforce: 'pre',
  async resolveId(source, importer, opts) {
    if (opts?.scan) return null
    if (source === STUB_API) return STUB_API
    if (!importer || importer === STUB_API || !/api(\.ts)?$/.test(source)) return null
    const r = await this.resolve(source, importer, { ...opts, skipSelf: true })
    return r && r.id.split('?')[0] === REAL_API ? STUB_API : null
  },
  load(id) { return id === STUB_API ? stubCode : null },
}
const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-overflow-probe-'))
const server = await createServer({ root: UI, configFile: join(UI, 'vite.config.ts'), cacheDir, logLevel: 'warn', plugins: [swap],
  server: { host: '127.0.0.1', port: 0, strictPort: false, hmr: false } })
await server.listen()
const origin = `http://127.0.0.1:${server.httpServer.address().port}`
// Vite answers the first page only once it has scanned and bundled the app's dependencies, which on a loaded box
// takes longer than the browser runtime's 10 s per command: ask for the page once before Chrome does.
await fetch(`${origin}/`).then((r) => r.text())
const browser = await chrome()

const VISIBLE_BTN = `[...document.querySelectorAll('main .ch-overflow-btn')].find((e) => e.closest('[hidden]') == null && e.getBoundingClientRect().width > 0)`
/** The open group against its cockpit and the window, and what is topmost at five points inside it. */
const MEASURE = `(() => {
  const p = [...document.querySelectorAll('main .ch-overflow-panel.ch-overflow-open')].find((e) => e.getBoundingClientRect().width > 0)
  if (!p) return null
  const q = p.getBoundingClientRect(), m = p.closest('main').getBoundingClientRect(), b = (${VISIBLE_BTN}).getBoundingClientRect()
  const bound = { l: Math.max(0, m.left), r: Math.min(innerWidth, m.right) }
  const top = (x, y) => { const e = document.elementFromPoint(x, y); return e && p.contains(e) ? 'panel' : e ? String(e.className || e.tagName).slice(0, 32) : 'off screen' }
  const i = 4, pts = { tl: [q.left + i, q.top + i], tr: [q.right - i, q.top + i], bl: [q.left + i, q.bottom - i], br: [q.right - i, q.bottom - i], c: [q.left + q.width / 2, q.top + q.height / 2] }
  const hits = Object.fromEntries(Object.entries(pts).map(([k, [x, y]]) => [k, top(x, y)]))
  return { l: Math.round(q.left), r: Math.round(q.right), w: Math.round(q.width), btnL: Math.round(b.left), bound: { l: Math.round(bound.l), r: Math.round(bound.r) },
    side: p.classList.contains('ch-overflow-start') ? 'rightward' : 'leftward',
    inside: q.left >= bound.l - 1 && q.right <= bound.r + 1, hits, topmost: Object.values(hits).every((h) => h === 'panel') }
})()`

async function measure(size, screen, railOn, plant) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (m, p) => browser.call(m, p, sessionId)
  const ev = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text)
    return r.result.value
  }
  // A navigation still under way when the runtime's 10 s per command runs out (a loaded box) is waited for by
  // the polls below, not failed: they ask for what the page must draw.
  const go = (url) => call('Page.navigate', { url }).catch((e) => { if (!/timed out: Page\.navigate/.test(String(e))) throw e })
  try {
    await call('Emulation.setDeviceMetricsOverride', metrics(size))
    await go(`${origin}/probe-blank`)
    await sleep(300)
    const ls = { 'nexus.workspace': 'dx', 'nexus.features.v1': FEATURES, 'tempo-theme': 'dark', ...(railOn ? { 'nexus.dashrail.sections': JSON.stringify({ [screen]: true }) } : {}) }
    await ev(`localStorage.clear(); ${Object.entries(ls).map(([k, v]) => `localStorage.setItem(${JSON.stringify(k)}, ${JSON.stringify(v)})`).join(';')}; 1`)
    await go(`${origin}/#${screen}`)
    const drawn = async (tries) => {
      for (let i = 0; i < tries; i++) { if (await ev(`!!(${VISIBLE_BTN})`)) return true; await sleep(250) }
      return false
    }
    // The header settles (fonts, the rail's width, the size class) before the click, and ⋯ must still be there.
    const vp = () => ev(`JSON.stringify({ vp: document.documentElement.getAttribute('data-viewport'), hash: location.hash,
      btns: [...document.querySelectorAll('.ch-overflow-btn')].map((e) => ({ main: !!e.closest('main'), hidden: !!e.closest('[hidden]'), w: Math.round(e.getBoundingClientRect().width) })),
      heads: document.querySelectorAll(".cockpit-header").length, text: (document.querySelector("main") || document.body).innerText.slice(0, 400) })`)
    if (!(await drawn(120))) return { error: `no ⋯ drawn (viewport ${await vp()})` }
    await sleep(1200)
    if (!(await drawn(20))) return { error: `⋯ went away as the page settled (viewport ${await vp()})` }
    if (plant) await ev(`(() => { const s = document.createElement('style'); s.textContent = ${JSON.stringify(plant)}; document.head.appendChild(s); return 1 })()`)
    await ev(`(${VISIBLE_BTN}).click(), 1`)
    await sleep(300)
    return (await ev(MEASURE)) ?? { error: 'the group did not open' }
  } finally {
    await browser.call('Target.closeTarget', { targetId })
  }
}

let code = 0
try {
  for (const size of SIZES) {
    for (const screen of SCREENS) {
      for (const railOn of [false, true]) {
        const r = await measure(size, screen, railOn, null)
        const where = `${screen} ${size.name} (${size.w}×${size.h} CSS) rail ${railOn ? 'on' : 'off'}`
        if (r.error) {
          console.error(`overflow-probe: ${where}: ${r.error}`)
          code = Math.max(code, 2)
          continue
        }
        const ok = r.inside && r.topmost
        console.log(`overflow-probe: ${ok ? 'ok ' : 'RED'} ${where}: ⋯ at ${r.btnL}, opens ${r.side}, ${r.l}…${r.r} in ${r.bound.l}…${r.bound.r}${ok ? '' : ` — inside ${r.inside}, topmost ${JSON.stringify(r.hits)}`}`)
        if (!ok) code = Math.max(code, 1)
      }
    }
  }
  const c = await measure(FHD, 'phone', true, OLD_SHAPE)
  if (c.error || (c.inside && c.topmost)) {
    console.error(`overflow-probe: CONTROL FAILED — the old right-aligned shape held at 1920×1080 with the rail (${JSON.stringify(c)}); the check cannot fail`)
    code = Math.max(code, 1)
  } else {
    console.log(`overflow-probe: control caught — the old shape at 1920×1080 with the rail opens ${c.l}…${c.r}`)
  }
  if (code === 0) console.log('overflow-probe: PASS — the ⋯ group opens inside the cockpit and on top, on Phone and CW, rail off and on, 1440 to 2048 wide')
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
