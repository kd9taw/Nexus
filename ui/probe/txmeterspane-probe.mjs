// The large TX meters pane on the real screens, in real Chrome: `scripts/browser-probe --desktop txmeterspane`.
//
// The operator's ruling on 1.24.0 (2026-11-12, "the dials on the new tx meters are way too large, reduce them by
// 50%", and then: shrink the pane too, so the panes beside it get the room): each needle dial (PO, ALC, SWR) is half
// as wide and half as high as 1.24.0 drew it on the same screen, with the readout (the meter's name and its value) no
// smaller than it was, in a shorter pane. The probe serves the REAL app
// (index.html, main.tsx, App) with the app's own Vite config and src/api.ts swapped for probe/waterfalls-api.ts,
// keys the radio with readings (100 W, ALC 30 %, SWR 1.2:1), opens FT8, Phone, CW and SSTV with the large meters
// pane ticked and otherwise fresh storage, at 1024×768, 1920×1080 and 2560×1440, each at 65 %, 100 % and 175 %,
// and asserts:
//   1. THE DIAL IS HALF: each face's scale arc on screen is 50 % ± 5 points (± 10 % of the half) of 1.24.0's on the
//      same page (BEFORE), in width and in height.
//   2. THE READOUT IS NO SMALLER: the name's and the value's glyph boxes are at least 1.24.0's high, each whole inside
//      its face and clear of the other and of the dial, and in view once the pane is scrolled to wherever 1.24.0's were.
//   3. THE PANE GIVES THE ROOM BACK AND NOTHING ELSE LOSES: the pane is shorter than in 1.24.0, no other pane on the
//      screen is shorter (each one's gain is printed), the TX strip is its 1.24.0 height, and Stop TX is on screen,
//      the topmost thing where it stands, where it stood in 1.24.0. It prints the room left above each dial.
// THE CONTROL, run every time: FT8 at 1920×1080 100 % with 1.24.0's dial drawn back by a planted style (the dial's own
// scale taken away) must fail check 1. If it passes, the check cannot fail and the probe says so.
//
// PROBE_TXMETERSPANE_PRINT=1 prints each page's measure as one JSON line (BEFORE was taken that way on 1.24.0) and
// asserts nothing. PROBE_TXMETERSPANE_ONLY=<prefix> runs the pages whose name starts so ("FT8 2560"), no control.
//
// exit: 0 every check held · 1 a red, or a control that could not fail · 2 a page never drew the pane
import { createServer } from 'vite'
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const REAL_API = join(UI, 'src', 'api.ts')
const STUB_SRC = join(UI, 'probe', 'waterfalls-api.ts')
const STUB_API = join(UI, 'probe', '__txmeterspane-api.ts')
const SIZES = [[1024, 768], [1920, 1080], [2560, 1440]]
const ZOOMS = ['65', '100', '175']
const FEATURES = JSON.stringify({ profile: 'custom', enabled: { sstv: true } })
/** [name, hash, panel view]. */
const VIEWS = [
  ['FT8', 'operate', 'operate'],
  ['Phone', 'phone', 'phone'],
  ['CW', 'cw', 'cw'],
  ['SSTV', 'sstv', 'sstv'],
]
const RADIO = { rigKeyed: true, transmitting: true, txPoW: 100, txAlc: 0.3, txSwr: 1.2, swrScaleVerified: true, ratedWatts: 100 }
const PRINT = process.env.PROBE_TXMETERSPANE_PRINT === '1'
const ONLY = process.env.PROBE_TXMETERSPANE_ONLY ?? null
/** PROBE_TXMETERSPANE_SHOT=<dir> saves each page's pane as a picture there. */
const SHOT = process.env.PROBE_TXMETERSPANE_SHOT ?? null
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

const MEASURE = `(async () => {
  const vis = (el) => el && el.closest('[hidden]') == null && el.getBoundingClientRect().width > 0
  const pane = [...document.querySelectorAll('[data-pane="txMetersPane"]')].find(vis)
  if (!pane) return null
  const faces = [...pane.querySelectorAll('.txmeters-pane-meter')]
  if (faces.length !== 3) return null
  const shell = pane.closest('main')
  const body = pane.querySelector('.pane-body') ?? pane
  const R = (q) => ({ l: q.left, t: q.top, r: q.right, b: q.bottom, w: q.width, h: q.height })
  const rect = (el) => R(el.getBoundingClientRect())
  // A readout's glyph box: the text's own box (SVG text) or its text's range (HTML), so line-height never counts.
  const glyph = (el) => {
    if (el instanceof SVGElement) return rect(el)
    const range = document.createRange(); range.selectNodeContents(el); return R(range.getBoundingClientRect())
  }
  const inside = (a, b) => a.l >= b.l - 0.5 && a.r <= b.r + 0.5 && a.t >= b.t - 0.5 && a.b <= b.b + 0.5
  const apart = (a, b) => a.r <= b.l + 0.5 || b.r <= a.l + 0.5 || a.b <= b.t + 0.5 || b.b <= a.t + 0.5
  const reach = (b) => {
    const q = b.getBoundingClientRect()
    if (q.width < 1 || q.bottom > innerHeight || q.top < 0 || q.right > innerWidth) return false
    const t = document.elementFromPoint(q.left + q.width / 2, q.top + q.height / 2)
    return t != null && (t === b || b.contains(t))
  }
  const stop = [...shell.querySelectorAll('button')].find((b) => vis(b) && /^stop tx$/i.test(b.textContent.trim()))
  const strip = [...shell.querySelectorAll('.cockpit-txstrip, .cockpit-ftstrip')].find(vis)
  const round = (o) => Object.fromEntries(Object.entries(o).map(([k, v]) => [k, Math.round(v * 10) / 10]))
  // Where everything stands with no scroll moved, then the pane brought into view (the operator scrolls to it
  // where it stands below the fold) for its faces, and every scroller put back.
  const layout = {
    vp: document.documentElement.getAttribute('data-viewport'),
    pane: round(rect(pane)),
    panes: [...shell.querySelectorAll('[data-pane]')].filter((p) => vis(p) && p !== pane && !p.contains(pane) && !pane.contains(p))
      .map((p) => [p.dataset.pane, Math.round(p.getBoundingClientRect().height)]),
    stop: stop ? { at: [Math.round(stop.getBoundingClientRect().left), Math.round(stop.getBoundingClientRect().top)], reach: reach(stop) } : null,
    strip: strip ? Math.round(strip.getBoundingClientRect().height) : null,
  }
  const scrolled = []
  for (let e = pane.parentElement; e; e = e.parentElement) if (e.scrollTop || e.scrollLeft || e.scrollHeight > e.clientHeight) scrolled.push([e, e.scrollTop, e.scrollLeft])
  pane.scrollIntoView({ block: 'nearest', inline: 'nearest' })
  await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)))
  const shown = body.getBoundingClientRect()
  const view = { l: Math.max(0, shown.left), t: Math.max(0, shown.top), r: Math.min(innerWidth, shown.right), b: Math.min(innerHeight, shown.bottom) }
  const out = {
    ...layout,
    seenAt: round(rect(pane)),
    faces: faces.map((f) => {
      const box = rect(f)
      const arc = rect(f.querySelector('.ph-txneedle-scale'))
      const label = glyph(f.querySelector('.txmeters-pane-label'))
      const value = glyph(f.querySelector('.txmeters-pane-value'))
      return {
        key: f.dataset.meter,
        text: f.querySelector('.txmeters-pane-value').textContent,
        face: round(box), arc: round(arc), label: round(label), value: round(value),
        // The room above the dial in its face (its numbers at the top): what the drawing does not fill.
        above: Math.round(((f.querySelector('.txmeters-pane-dial') ?? f.querySelector('svg')).getBoundingClientRect().top) - box.t),
        whole: inside(label, box) && inside(value, box),
        seen: inside(label, view) && inside(value, view),
        clear: apart(label, value) && apart(label, arc) && apart(value, arc),
      }
    }),
  }
  if (!window.__keepScroll) for (const [e, t, l] of scrolled) { e.scrollTop = t; e.scrollLeft = l }
  return out
})()`

/** Ticks the large meters pane in the shown screen's ⊞ Panels menu, and closes the menu with its own button (Escape
 *  is the app's halt key). */
const TICK = `(async () => {
  const vis = (el) => el && el.closest('[hidden]') == null && el.getBoundingClientRect().width > 0
  if ([...document.querySelectorAll('[data-pane="txMetersPane"]')].some(vis)) return 'shown'
  const btn = [...document.querySelectorAll('main .panels-menu-btn')].find(vis)
  if (!btn) return null
  btn.click()
  await new Promise((r) => setTimeout(r, 300))
  const box = [...document.querySelectorAll('.panels-menu-pop label')].find((l) => l.textContent.trim() === 'TX Meters, large')?.querySelector('input')
  if (box && !box.checked) box.click()
  await new Promise((r) => setTimeout(r, 100))
  if (btn.getAttribute('aria-expanded') === 'true') btn.click()
  btn.blur()
  return box ? 'ticked' : null
})()`

const realSrc = await readFile(REAL_API, 'utf8')
const stubSrc = await readFile(STUB_SRC, 'utf8')
const names = [...realSrc.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1])
const own = new Set([...stubSrc.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1]))
const snapshotNames = new Set(
  [...realSrc.matchAll(/^export (?:async )?function (\w+)\((?:(?!\nexport )[\s\S])*?\)\s*:\s*Promise<AppSnapshot>\s*\{/gm)].map((m) => m[1]),
)
const stubCode =
  stubSrc +
  `\nexport * from ${JSON.stringify(REAL_API)}\n` +
  names
    .filter((n) => !own.has(n))
    .map((n) =>
      snapshotNames.has(n)
        ? `export async function ${n}(..._a: unknown[]): Promise<any> { return snapshot }`
        : `export async function ${n}(..._a: unknown[]): Promise<any> { return (globalThis as any).__DEFAULT_ARR ? [] : {} }`,
    )
    .join('\n')
const swap = {
  name: 'txmeterspane-probe-api',
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

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-txmeterspane-probe-'))
const server = await createServer({
  root: UI,
  configFile: join(UI, 'vite.config.ts'),
  cacheDir,
  logLevel: 'warn',
  plugins: [swap],
  server: { host: '127.0.0.1', port: 0, strictPort: false, hmr: false },
})
await server.listen()
const origin = `http://127.0.0.1:${server.httpServer.address().port}`
const browser = await chrome()

/** One page: `hash` in a W×H window at `zoom`, the meters pane ticked, measured once it settles. `css` is planted
 *  once the pane has drawn. */
async function measure(hash, view, [W, H], zoom, css = null) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text)
    return r.result.value
  }
  try {
    await call('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: 1, mobile: false })
    await call('Page.navigate', { url: `${origin}/probe-blank` })
    await sleep(300)
    const ls = {
      'nexus.workspace': 'dx', 'nexus.features.v1': FEATURES, 'tempo-theme': 'dark', 'nexus-ui-scale-mode': zoom,
      'probe.radio': JSON.stringify(RADIO),
    }
    await evaluate(`localStorage.clear(); ${Object.entries(ls).map(([k, v]) => `localStorage.setItem(${JSON.stringify(k)}, ${JSON.stringify(v)})`).join(';')}; 1`)
    await call('Page.navigate', { url: `${origin}/#${hash}` })
    // The pane ticked the operator's way, in ⊞ Panels (FT places it as the menu does; a stored state alone does not).
    for (let i = 0; i < 120; i++) {
      await sleep(250)
      const ok = await evaluate(TICK).catch(() => null)
      if (ok === 'ticked' || ok === 'shown') break
    }
    let m = null
    for (let i = 0; i < 120 && !m; i++) {
      await sleep(250)
      m = await evaluate(MEASURE).catch(() => null)
    }
    if (!m) return null
    if (css) await evaluate(`(() => { const s = document.createElement('style'); s.textContent = ${JSON.stringify(css)}; document.head.appendChild(s); return 1 })()`)
    let last = null
    for (let i = 0; i < 40; i++) {
      await sleep(250)
      m = await evaluate(MEASURE)
      if (last && JSON.stringify(m) === JSON.stringify(last)) break
      last = m
    }
    if (SHOT && m) {
      await evaluate('window.__keepScroll = true; 1')
      const { seenAt: p } = await evaluate(MEASURE)
      const shot = await call('Page.captureScreenshot', { format: 'png', clip: { x: p.l, y: p.t, width: p.w, height: p.h, scale: 1 } })
      await writeFile(join(SHOT, `${hash}-${W}x${H}-${zoom}.png`), Buffer.from(shot.data, 'base64'))
      const page = await call('Page.captureScreenshot', { format: 'png' })
      await writeFile(join(SHOT, `${hash}-${W}x${H}-${zoom}-window.png`), Buffer.from(page.data, 'base64'))
    }
    return m
  } finally {
    await browser.call('Target.closeTarget', { targetId })
  }
}

// 1.24.0's measures, taken with PROBE_TXMETERSPANE_PRINT=1 on 8f510d4ba (1.24.0's tree with this probe): per page,
// the pane's height, a face's scale arc [w, h] on screen (PO, ALC and SWR are drawn alike), the value's and the name's glyph heights,
// whether every readout was in view once the pane was scrolled to, the other panes' heights, Stop TX's place and
// the TX strip's height. Phone and CW at 1024×768 175 % (xs) had the readouts below the window with nothing left
// to scroll in 1.24.0; check 2 holds those two to that, and the report names them.
const BEFORE = {
  "FT8 1024×768 65%": { pane: 91, arc: [80.7, 14.7], value: 11, label: 8, seen: true, panes: { callRoster: 346, bandActivity: 148, rxfreq: 96 }, stop: [189, 341], strip: 74 },
  "FT8 1024×768 100%": { pane: 140, arc: [125.5, 22.8], value: 17, label: 12, seen: true, panes: { callRoster: 364, bandActivity: 140, rxfreq: 140 }, stop: [197, 394], strip: 109 },
  "FT8 1024×768 175%": { pane: 245, arc: [221.6, 40.3], value: 30, label: 21, seen: true, panes: { callRoster: 637, bandActivity: 245, rxfreq: 245 }, stop: [336, 598], strip: 300 },
  "FT8 1920×1080 65%": { pane: 181.4, arc: [190.8, 34.7], value: 26, label: 19, seen: true, panes: { callRoster: 678, bandActivity: 297, rxfreq: 189 }, stop: [189, 364], strip: 30 },
  "FT8 1920×1080 100%": { pane: 148.3, arc: [127.3, 23.2], value: 17, label: 13, seen: true, panes: { callRoster: 573, bandActivity: 248, rxfreq: 160 }, stop: [288, 415], strip: 79 },
  "FT8 1920×1080 175%": { pane: 245, arc: [221.6, 40.3], value: 30, label: 21, seen: true, panes: { callRoster: 637, bandActivity: 245, rxfreq: 245 }, stop: [342, 556], strip: 182 },
  "FT8 2560×1440 65%": { pane: 275.4, arc: [259.6, 47.2], value: 35, label: 25, seen: true, panes: { callRoster: 1016, bandActivity: 447, rxfreq: 283 }, stop: [189, 386], strip: 30 },
  "FT8 2560×1440 100%": { pane: 223.6, arc: [225.7, 41.1], value: 30, label: 22, seen: true, panes: { callRoster: 844, bandActivity: 369, rxfreq: 236 }, stop: [288, 539], strip: 44 },
  "FT8 2560×1440 175%": { pane: 245, arc: [221.6, 40.3], value: 30, label: 21, seen: true, panes: { callRoster: 534, bandActivity: 245, rxfreq: 245 }, stop: [501, 700], strip: 182 },
  "Phone 1024×768 65%": { pane: 91, arc: [80.7, 14.7], value: 11, label: 8, seen: true, panes: { bandActivity: 114, voiceKeyer: 51, receiver: 85, transmitter: 47, log: 384 }, stop: [191, 295], strip: 30 },
  "Phone 1024×768 100%": { pane: 145.5, arc: [125.5, 22.8], value: 17, label: 12, seen: true, panes: { bandActivity: 151, voiceKeyer: 72, receiver: 126, transmitter: 66, log: 336 }, stop: [200, 435], strip: 40 },
  "Phone 1024×768 175%": { pane: 247.1, arc: [221.6, 40.3], value: 30, label: 21, seen: false, panes: { bandActivity: 279, voiceKeyer: 117, receiver: 212, transmitter: 108, log: 633 }, stop: [339, 454], strip: 67 },
  "Phone 1920×1080 65%": { pane: 480.2, arc: [204.2, 37.2], value: 28, label: 20, seen: true, panes: { bandActivity: 104, voiceKeyer: 51, receiver: 85, transmitter: 47, log: 451, stats: 169 }, stop: [191, 364], strip: 30 },
  "Phone 1920×1080 100%": { pane: 315.5, arc: [162.1, 29.5], value: 22, label: 16, seen: true, panes: { bandActivity: 169, voiceKeyer: 74, receiver: 169, transmitter: 68, log: 577 }, stop: [292, 380], strip: 40 },
  "Phone 1920×1080 175%": { pane: 250.5, arc: [221.6, 40.3], value: 30, label: 21, seen: true, panes: { bandActivity: 258, voiceKeyer: 121, receiver: 217, transmitter: 112, log: 582 }, stop: [348, 694], strip: 67 },
  "Phone 2560×1440 65%": { pane: 750.2, arc: [307.5, 56], value: 41, label: 30, seen: true, panes: { bandActivity: 104, voiceKeyer: 51, receiver: 85, transmitter: 47, log: 721, stats: 169 }, stop: [191, 454], strip: 30 },
  "Phone 2560×1440 100%": { pane: 584.5, arc: [257, 46.8], value: 35, label: 25, seen: true, panes: { bandActivity: 153, voiceKeyer: 74, receiver: 128, transmitter: 68, log: 547, stats: 246 }, stop: [292, 502], strip: 44 },
  "Phone 2560×1440 175%": { pane: 245, arc: [221.6, 40.3], value: 30, label: 21, seen: true, panes: { bandActivity: 289, voiceKeyer: 124, receiver: 221, transmitter: 116, log: 620 }, stop: [508, 606], strip: 67 },
  "CW 1024×768 65%": { pane: 91, arc: [80.7, 14.7], value: 11, label: 8, seen: true, panes: { decode: 91, bandActivity: 114, copilot: 41, log: 358 }, stop: [191, 299], strip: 30 },
  "CW 1024×768 100%": { pane: 145.5, arc: [125.5, 22.8], value: 17, label: 12, seen: true, panes: { decode: 136, bandActivity: 151, copilot: 59, log: 321 }, stop: [200, 404], strip: 40 },
  "CW 1024×768 175%": { pane: 247.1, arc: [221.6, 40.3], value: 30, label: 21, seen: false, panes: { decode: 232, bandActivity: 279, copilot: 96, log: 609 }, stop: [339, 405], strip: 67 },
  "CW 1920×1080 65%": { pane: 432.9, arc: [204.2, 37.2], value: 28, label: 20, seen: true, panes: { decode: 604, bandActivity: 114, copilot: 41, log: 428, stats: 169 }, stop: [191, 364], strip: 30 },
  "CW 1920×1080 100%": { pane: 277.5, arc: [144.1, 26.2], value: 20, label: 14, seen: true, panes: { decode: 546, bandActivity: 184, copilot: 60, log: 546 }, stop: [292, 380], strip: 40 },
  "CW 1920×1080 175%": { pane: 250.5, arc: [221.6, 40.3], value: 30, label: 21, seen: true, panes: { decode: 235, bandActivity: 258, copilot: 99, log: 556 }, stop: [348, 688], strip: 67 },
  "CW 2560×1440 65%": { pane: 713.5, arc: [299.2, 54.4], value: 40, label: 29, seen: true, panes: { decode: 874, bandActivity: 104, copilot: 41, log: 698, stats: 169 }, stop: [191, 454], strip: 30 },
  "CW 2560×1440 100%": { pane: 517.5, arc: [257, 46.8], value: 35, label: 25, seen: true, panes: { decode: 770, bandActivity: 168, copilot: 60, log: 512, stats: 246 }, stop: [292, 502], strip: 44 },
  "CW 2560×1440 175%": { pane: 245, arc: [221.6, 40.3], value: 30, label: 21, seen: true, panes: { decode: 245, bandActivity: 289, copilot: 102, log: 554 }, stop: [508, 616], strip: 67 },
  "SSTV 1024×768 65%": { pane: 91, arc: [80.7, 14.7], value: 11, label: 8, seen: true, panes: { gallery: 91, txcompose: 92 }, stop: [191, 417], strip: 30 },
  "SSTV 1024×768 100%": { pane: 140, arc: [125.5, 22.8], value: 17, label: 12, seen: true, panes: { gallery: 140, txcompose: 136 }, stop: [200, 515], strip: 40 },
  "SSTV 1024×768 175%": { pane: 245, arc: [221.6, 40.3], value: 30, label: 21, seen: true, panes: { gallery: 245, txcompose: 226 }, stop: [339, 608], strip: 67 },
  "SSTV 1920×1080 65%": { pane: 156.4, arc: [158.1, 28.8], value: 21, label: 15, seen: true, panes: { gallery: 156, txcompose: 92 }, stop: [191, 577], strip: 30 },
  "SSTV 1920×1080 100%": { pane: 140, arc: [125.5, 22.8], value: 17, label: 12, seen: true, panes: { gallery: 140, txcompose: 138 }, stop: [292, 563], strip: 40 },
  "SSTV 1920×1080 175%": { pane: 245, arc: [221.6, 40.3], value: 30, label: 21, seen: true, panes: { gallery: 245, txcompose: 231 }, stop: [348, 883], strip: 67 },
  "SSTV 2560×1440 65%": { pane: 242.1, arc: [270.2, 49.2], value: 36, label: 26, seen: true, panes: { gallery: 242, txcompose: 92 }, stop: [191, 765], strip: 30 },
  "SSTV 2560×1440 100%": { pane: 191.6, arc: [183.8, 33.5], value: 25, label: 18, seen: true, panes: { gallery: 192, txcompose: 138 }, stop: [292, 771], strip: 44 },
  "SSTV 2560×1440 175%": { pane: 245, arc: [221.6, 40.3], value: 30, label: 21, seen: true, panes: { gallery: 245, txcompose: 235 }, stop: [508, 781], strip: 67 },
}

/** What is wrong with one page against 1.24.0's; empty when nothing is. */
function faults(tag, m, before) {
  const f = []
  for (const face of m.faces) {
    for (const [dim, k, i] of [['wide', 'w', 0], ['high', 'h', 1]]) {
      const ratio = face.arc[k] / before.arc[i]
      if (Math.abs(ratio - 0.5) > 0.05) f.push(`${face.key}'s dial is ${face.arc[k]} px ${dim}, ${(ratio * 100).toFixed(0)} % of 1.24.0's ${before.arc[i]}`)
    }
    if (face.value.h < before.value - 0.5) f.push(`${face.key}'s value is ${face.value.h} px high, under 1.24.0's ${before.value}`)
    if (face.label.h < before.label - 0.5) f.push(`${face.key}'s name is ${face.label.h} px high, under 1.24.0's ${before.label}`)
    if (!face.whole) f.push(`${face.key}'s readout runs out of its face`)
    if (before.seen && !face.seen) f.push(`${face.key}'s readout is outside what the pane shows`)
    if (!face.clear) f.push(`${face.key}'s name, value and dial overlap`)
  }
  if (!(m.pane.h < before.pane - 1)) f.push(`the pane is ${m.pane.h} px high, not shorter than 1.24.0's ${before.pane}`)
  for (const [id, h] of m.panes) if (before.panes[id] != null && h < before.panes[id] - 1) f.push(`the ${id} pane is ${h} px high, under 1.24.0's ${before.panes[id]}`)
  if (!m.stop?.reach) f.push('Stop TX is not on screen or something covers it')
  else if (JSON.stringify(m.stop.at) !== JSON.stringify(before.stop)) f.push(`Stop TX moved from ${before.stop} to ${m.stop.at}`)
  if (m.strip !== before.strip) f.push(`the TX strip is ${m.strip} px high, 1.24.0's ${before.strip}`)
  return f.map((s) => `${tag}: ${s}`)
}

let code = 0
const red = (msg) => {
  console.error(`txmeterspane-probe: RED — ${msg}`)
  code = 1
}
try {
  for (const [name, hash, view] of VIEWS) for (const size of SIZES) for (const zoom of ZOOMS) {
    const tag = `${name} ${size[0]}×${size[1]} ${zoom}%`
    if (ONLY && !tag.startsWith(ONLY)) continue
    const m = await measure(hash, view, size, zoom)
    if (!m) {
      console.error(`txmeterspane-probe: ${tag} never drew the large TX meters pane`)
      code = 2
      continue
    }
    if (PRINT) {
      console.log(`${JSON.stringify(tag)}: ${JSON.stringify(m)},`)
      continue
    }
    const before = BEFORE[tag]
    const face = m.faces[0]
    const gains = before ? m.panes.filter(([id, h]) => before.panes[id] != null && h !== before.panes[id]).map(([id, h]) => `${id} ${h - before.panes[id] > 0 ? '+' : ''}${h - before.panes[id]}`) : []
    console.log(`txmeterspane-probe: ${tag} (${m.vp}): pane ${before?.pane} → ${m.pane.h} px; dial ${face.arc.w}×${face.arc.h} (1.24.0 ${before?.arc.join('×')}), ${face.above} px above it; value ${face.value.h} px (1.24.0 ${before?.value}), name ${face.label.h} px (1.24.0 ${before?.label}); the others: ${gains.length ? gains.join(', ') : 'unchanged'}; Stop TX at ${m.stop?.at}`)
    if (!before) red(`${tag}: no 1.24.0 measure to hold it to`)
    else for (const r of faults(tag, m, before)) red(r)
  }
  if (!PRINT && !ONLY) {
    // THE CONTROL: 1.24.0's dial drawn back (the dial's own scale planted away) must fail check 1.
    const tag = 'FT8 1920×1080 100%'
    const m = await measure('operate', 'operate', [1920, 1080], '100', '.txmeters-pane-dial { transform: none !important }')
    const caught = m ? faults(tag, m, BEFORE[tag]) : []
    console.log(`txmeterspane-probe: CONTROL 1.24.0's dial drawn back: caught ${JSON.stringify(caught)}`)
    if (!caught.some((s) => s.includes('dial is'))) red("CONTROL FAILED: 1.24.0's dial drawn back was not seen; the check cannot fail")
  }
  if (code === 0 && !PRINT) console.log('txmeterspane-probe: PASS — every dial half of 1.24.0\'s with its readout no smaller, whole and in view, in a shorter pane with no other pane shorter, and Stop TX where it stood, on FT8, Phone, CW and SSTV at every size and zoom; the control caught')
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
