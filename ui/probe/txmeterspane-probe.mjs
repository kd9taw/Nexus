// The large TX meters pane on the real screens, in real Chrome: `scripts/browser-probe --desktop txmeterspane`.
//
// The operator's ruling on 1.24.0 (2026-11-12, "the dials on the new tx meters are way too large, reduce them by
// 50%", and then: shrink the pane too, so the panes beside it get the room): each needle dial (PO, ALC, SWR) is half
// as wide and half as high as 1.24.0 drew it on the same screen, with the readout (the meter's name and its value) no
// smaller than it was, in a shorter pane. The probe serves the REAL app
// (index.html, main.tsx, App) with the app's own Vite config and src/api.ts swapped for probe/waterfalls-api.ts,
// keys the radio with readings (100 W, ALC 30 %, SWR 1.2:1), opens FT8, Phone, CW and SSTV with the large meters
// pane ticked and otherwise fresh storage, on the supported screens at the OS's display scaling (PAGES: 1366×768 at
// 100–175 % to 4K at 200 %, the app's own scale left to fit itself), and asserts:
//   1. THE DIAL IS HALF: each face's scale arc on screen is 50 % ± 5 points (± 10 % of the half) of 1.24.0's on the
//      same page (BEFORE), in width and in height.
//   2. THE READOUT IS NO SMALLER: the name's and the value's glyph boxes are at least 1.24.0's high, each whole inside
//      its face and clear of the other and of the dial, and in view once the pane is scrolled to wherever 1.24.0's were.
//   3. THE PANE TAKES ROOM ONLY FROM ITSELF: the pane is shorter than in 1.24.0; and against the SAME tree with the
//      pane hidden (a MEASUREMENT RE-BASELINE, 2026-11-12, lead's pick (a)): the TX strip is the same height, Stop TX
//      is on screen, the topmost thing where it stands, at the same place, every other pane's computed floor is the
//      same, and no other pane gives up more than 1 px — save the panes that SHARE the pane's column by weight (SSTV's
//      gallery, CW's decode: measured giving up 1–148 px and 9–42 px), which together give up no more than the pane's
//      own height and one gap (review 2026-11-12: one allowance for all let a 150 px steal from Phone's log pass). Until then these
//      were held to 1.24.0's own layout, which fix/layout-new-sizes changes on purpose (the contact's words' own line
//      in FT's strip, the waterfalls' floors, the region floor yielding on the floor screen), so every page of it read
//      red for what was not the pane's doing. The dial, readout and scale-number checks (1, 2, 4) are still held to
//      1.24.0. It prints the room left above each dial.
//   4. THE SCALE'S NUMBERS STAY LEGIBLE (review 2026-11-12): they are drawn at 1.24.0's size around the half-size arc,
//      so each one's glyph box is at least 1.24.0's high, and on every face none overlaps another, the needle (which
//      reads 100 W, so it points straight at "100"), the band, or the drawing's top edge.
// THE CONTROLS, run every time: Phone at 1920×1080 100 % with 150 px taken from its log beside the pane must fail check
// 3; FT8 at 1920×1080 100 % with 1.24.0's dial drawn back by a planted style (the dial's own
// scale taken away) must fail check 1, and FT8 at the floor (1366×768 175 %) with the numbers planted at twice 1.24.0's
// size must fail check 4, and FT8 at 1920×1080 100 % with the numbers planted at half 1.24.0's size must fail check
// 4's height. If any passes, its check cannot fail and the probe says so.
//
// PROBE_TXMETERSPANE_PRINT=1 prints each page's measure as one JSON line (BEFORE was taken that way on 1.24.0) and
// asserts nothing. PROBE_TXMETERSPANE_ONLY=<prefix> runs the pages whose name starts so ("FT8 2560"), no control.
// PROBE_TXMETERSPANE_RATED=<watts> sets the radio's rated output, so the power scale's numbers run wider ("1500").
//
// exit: 0 every check held · 1 a red, or a control that could not fail · 2 a page never drew the pane
import { createServer } from 'vite'
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'
import { at } from './sizes.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const REAL_API = join(UI, 'src', 'api.ts')
const STUB_SRC = join(UI, 'probe', 'waterfalls-api.ts')
const STUB_API = join(UI, 'probe', '__txmeterspane-api.ts')
// The supported screens (operator, 2026-11-12: laptops of the last 8–10 years, 1366×768 up, to 4K) at the OS's
// display scaling, from ui/probe/sizes.mjs (`at()`: a CSS window of the screen divided by the scale, at that
// devicePixelRatio), with the app's own scale left at its default (auto-fit), so the app fits itself as it does on
// that laptop. The floor (1366×768 at 175 %, a 780×438 window) and every scaling of it, 1080p, 1200p, 1440p and 4K.
// The names are this probe's own (BEFORE is keyed by them).
/** [name, window W, H (CSS px), device scale]. */
const PAGES = [
  [1366, 768, 100], [1366, 768, 125], [1366, 768, 150], [1366, 768, 175],
  [1920, 1080, 100], [1920, 1080, 150], [1920, 1200, 100],
  [2560, 1440, 100], [2560, 1440, 150], [3840, 2160, 150], [3840, 2160, 200],
].map(([sw, sh, pct]) => { const s = at(sw, sh, pct); return [`${sw}×${sh} @${pct} %`, s.w, s.h, s.scale] })
const FEATURES = JSON.stringify({ profile: 'custom', enabled: { sstv: true } })
/** [name, hash, panel view]. */
const VIEWS = [
  ['FT8', 'operate', 'operate'],
  ['Phone', 'phone', 'phone'],
  ['CW', 'cw', 'cw'],
  ['SSTV', 'sstv', 'sstv'],
]
const RADIO = { rigKeyed: true, transmitting: true, txPoW: 100, txAlc: 0.3, txSwr: 1.2, swrScaleVerified: true, ratedWatts: Number(process.env.PROBE_TXMETERSPANE_RATED ?? 100) }
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
    // Every other pane's COMPUTED floor (its min-height as the browser computes it, ems resolved and the calc
    // simplified): what the shared frame's new knob must leave alone. FT's Band Activity's inline string changes with
    // its pair's split (1.3 → 1.115 on both sides of the division); its computed floor must not.
    floors: [...shell.querySelectorAll('[data-pane]')].filter((p) => vis(p) && p !== pane && !p.contains(pane) && !pane.contains(p))
      .map((p) => [p.dataset.pane, getComputedStyle(p).minHeight]),
    stop: stop ? { at: [Math.round(stop.getBoundingClientRect().left), Math.round(stop.getBoundingClientRect().top)], reach: reach(stop) } : null,
    strip: strip ? Math.round(strip.getBoundingClientRect().height) : null,
  }
  const scrolled = []
  for (let e = pane.parentElement; e; e = e.parentElement) if (e.scrollTop || e.scrollLeft || e.scrollHeight > e.clientHeight) scrolled.push([e, e.scrollTop, e.scrollLeft])
  pane.scrollIntoView({ block: 'nearest', inline: 'nearest' })
  await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)))
  const shown = body.getBoundingClientRect()
  const view = { l: Math.max(0, shown.left), t: Math.max(0, shown.top), r: Math.min(innerWidth, shown.right), b: Math.min(innerHeight, shown.bottom) }
  // THE SCALE'S NUMBERS (review 2026-11-12): their glyph boxes, the smallest one's height, and whether any two
  // overlap, any one touches the needle (the line from its pivot to its tip, as drawn, its stroke included) or the
  // band, or any runs out of the drawing (the viewBox's top edge, where the SVG would clip it).
  const numbers = (f) => {
    const nums = [...f.querySelectorAll('.txmeters-pane-scale')].map((t) => R(t.getBoundingClientRect()))
    if (!nums.length) return { n: 0, h: null, overlap: [], needle: [], band: [], clipped: [] }
    const svg = f.querySelector('svg')
    const vb = svg.viewBox.baseVal
    const top = new DOMPoint(0, vb.y).matrixTransform(svg.getScreenCTM()).y
    const line = f.querySelector('.ph-txneedle-needle')
    const m = line.getScreenCTM()
    const p0 = new DOMPoint(line.x1.baseVal.value, line.y1.baseVal.value).matrixTransform(m)
    const p1 = new DOMPoint(line.x2.baseVal.value, line.y2.baseVal.value).matrixTransform(m)
    const half = (parseFloat(getComputedStyle(line).strokeWidth) * Math.hypot(m.a, m.b)) / 2
    const band = R(f.querySelector('.txmeters-pane-band').getBoundingClientRect())
    const hitsNeedle = (r) => {
      for (let i = 0; i <= 400; i++) {
        const x = p0.x + ((p1.x - p0.x) * i) / 400, y = p0.y + ((p1.y - p0.y) * i) / 400
        if (y > band.t) continue // under the band the needle is covered
        if (x >= r.l - half && x <= r.r + half && y >= r.t - half && y <= r.b + half) return true
      }
      return false
    }
    const text = [...f.querySelectorAll('.txmeters-pane-scale')].map((t) => t.textContent)
    const overlap = []
    nums.forEach((a, i) => nums.slice(i + 1).forEach((b, j) => { if (!apart(a, b)) overlap.push(text[i] + '/' + text[i + 1 + j]) }))
    return {
      n: nums.length, h: Math.round(Math.min(...nums.map((r) => r.h)) * 10) / 10,
      overlap,
      needle: nums.flatMap((r, i) => (hitsNeedle(r) ? [text[i]] : [])),
      band: nums.flatMap((r, i) => (r.b > band.t + 0.5 ? [text[i]] : [])),
      clipped: nums.flatMap((r, i) => (r.t < top - 0.5 ? [text[i]] : [])),
    }
  }
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
        above: Math.round(Math.min(...[f.querySelector('.txmeters-pane-dial') ?? f.querySelector('svg'), ...f.querySelectorAll('.txmeters-pane-scale')].map((e) => e.getBoundingClientRect().top)) - box.t),
        nums: numbers(f),
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
async function measure(hash, view, [W, H, dpr = 1], css = null, hidden = false) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text)
    return r.result.value
  }
  try {
    await call('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: dpr, mobile: false })
    await call('Page.navigate', { url: `${origin}/probe-blank` })
    await sleep(300)
    const ls = {
      'nexus.workspace': 'dx', 'nexus.features.v1': FEATURES, 'tempo-theme': 'dark',
      'probe.radio': JSON.stringify(RADIO),
    }
    await evaluate(`localStorage.clear(); ${Object.entries(ls).map(([k, v]) => `localStorage.setItem(${JSON.stringify(k)}, ${JSON.stringify(v)})`).join(';')}; 1`)
    await call('Page.navigate', { url: `${origin}/#${hash}` })
    if (hidden) {
      // The baseline: the screen as it opens, the pane not ticked, once it settles.
      let last = null, b = null
      for (let i = 0; i < 160; i++) {
        await sleep(250)
        b = await evaluate(BARE).catch(() => null)
        if (b && last && JSON.stringify(b) === JSON.stringify(last)) return b
        last = b
      }
      return b
    }
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
      await writeFile(join(SHOT, `${hash}-${W}x${H}x${dpr}.png`), Buffer.from(shot.data, 'base64'))
      const page = await call('Page.captureScreenshot', { format: 'png' })
      await writeFile(join(SHOT, `${hash}-${W}x${H}x${dpr}-window.png`), Buffer.from(page.data, 'base64'))
    }
    return m
  } finally {
    await browser.call('Target.closeTarget', { targetId })
  }
}

// 1.24.0's measures, taken with PROBE_TXMETERSPANE_PRINT=1 on 8f510d4ba (1.24.0's tree with this probe): per page,
// the pane's height, a face's scale arc [w, h] on screen (PO, ALC and SWR are drawn alike), the value's and the name's
// glyph heights, the smallest scale number's glyph height, the other panes' computed floors, whether every readout was in view once the pane was scrolled to, the other panes' heights, Stop TX's
// place and the TX strip's height.
const BEFORE = {
  "FT8 1366×768 @100 %": { pane: 119, arc: [106.3, 19.3], value: 15, label: 10, num: 7, seen: true, panes: { callRoster: 323, bandActivity: 119, rxfreq: 119 }, floors: { callRoster: "min(140px, 100%)", bandActivity: "min(min(140px, 100%), 100%)", rxfreq: "min(140px, 100%)" }, stop: [246, 342], strip: 92 },
  "FT8 1366×768 @125 %": { pane: 91, arc: [80.7, 14.7], value: 11, label: 8, num: 6, seen: true, panes: { callRoster: 282, bandActivity: 91, rxfreq: 91 }, floors: { callRoster: "min(140px, 100%)", bandActivity: "min(min(140px, 100%), 100%)", rxfreq: "min(140px, 100%)" }, stop: [189, 251], strip: 74 },
  "FT8 1366×768 @150 %": { pane: 91, arc: [78.7, 14.3], value: 11, label: 8, num: 6, seen: true, panes: { callRoster: 168, bandActivity: 91, rxfreq: 91 }, floors: { callRoster: "min(140px, 100%)", bandActivity: "min(min(140px, 100%), 100%)", rxfreq: "min(140px, 100%)" }, stop: [189, 263], strip: 74 },
  "FT8 1366×768 @175 %": { pane: 91, arc: [64.8, 11.8], value: 9, label: 7, num: 5, seen: true, panes: { callRoster: 153, bandActivity: 91, rxfreq: 91 }, floors: { callRoster: "min(140px, 100%)", bandActivity: "min(min(140px, 100%), 100%)", rxfreq: "min(140px, 100%)" }, stop: [189, 203], strip: 74 },
  "FT8 1920×1080 @100 %": { pane: 148.3, arc: [127.3, 23.2], value: 17, label: 13, num: 9, seen: true, panes: { callRoster: 573, bandActivity: 248, rxfreq: 160 }, floors: { callRoster: "min(140px, 100%)", bandActivity: "min(min(140px, 100%), 100%)", rxfreq: "min(140px, 100%)" }, stop: [288, 415], strip: 79 },
  "FT8 1920×1080 @150 %": { pane: 112, arc: [99.9, 18.2], value: 14, label: 10, num: 7, seen: true, panes: { callRoster: 300, bandActivity: 112, rxfreq: 112 }, floors: { callRoster: "min(140px, 100%)", bandActivity: "min(min(140px, 100%), 100%)", rxfreq: "min(140px, 100%)" }, stop: [231, 322], strip: 88 },
  "FT8 1920×1200 @100 %": { pane: 169.7, arc: [155.2, 28.2], value: 21, label: 15, num: 11, seen: true, panes: { callRoster: 650, bandActivity: 282, rxfreq: 182 }, floors: { callRoster: "min(140px, 100%)", bandActivity: "min(min(140px, 100%), 100%)", rxfreq: "min(140px, 100%)" }, stop: [288, 458], strip: 79 },
  "FT8 2560×1440 @100 %": { pane: 223.6, arc: [225.7, 41.1], value: 30, label: 22, num: 15, seen: true, panes: { callRoster: 844, bandActivity: 369, rxfreq: 236 }, floors: { callRoster: "min(140px, 100%)", bandActivity: "min(min(140px, 100%), 100%)", rxfreq: "min(140px, 100%)" }, stop: [288, 539], strip: 44 },
  "FT8 2560×1440 @150 %": { pane: 140, arc: [125.5, 22.8], value: 17, label: 12, num: 9, seen: true, panes: { callRoster: 483, bandActivity: 187, rxfreq: 140 }, floors: { callRoster: "min(140px, 100%)", bandActivity: "min(min(140px, 100%), 100%)", rxfreq: "min(140px, 100%)" }, stop: [288, 385], strip: 79 },
  "FT8 3840×2160 @150 %": { pane: 223.6, arc: [225.7, 41.1], value: 30, label: 22, num: 15, seen: true, panes: { callRoster: 844, bandActivity: 369, rxfreq: 236 }, floors: { callRoster: "min(140px, 100%)", bandActivity: "min(min(140px, 100%), 100%)", rxfreq: "min(140px, 100%)" }, stop: [288, 539], strip: 44 },
  "FT8 3840×2160 @200 %": { pane: 148.3, arc: [127.3, 23.2], value: 17, label: 13, num: 9, seen: true, panes: { callRoster: 573, bandActivity: 248, rxfreq: 160 }, floors: { callRoster: "min(140px, 100%)", bandActivity: "min(min(140px, 100%), 100%)", rxfreq: "min(140px, 100%)" }, stop: [288, 415], strip: 79 },
  "Phone 1366×768 @100 %": { pane: 119, arc: [106.3, 19.3], value: 15, label: 10, num: 7, seen: true, panes: { bandActivity: 145, voiceKeyer: 63, receiver: 110, transmitter: 58, log: 349 }, floors: { bandActivity: "0px", voiceKeyer: "0px", receiver: "0px", transmitter: "0px", log: "min(140px, 100%)" }, stop: [249, 313], strip: 35 },
  "Phone 1366×768 @125 %": { pane: 91, arc: [80.7, 14.7], value: 11, label: 8, num: 6, seen: true, panes: { bandActivity: 104, voiceKeyer: 51, receiver: 85, transmitter: 47, log: 285 }, floors: { bandActivity: "0px", voiceKeyer: "0px", receiver: "0px", transmitter: "0px", log: "min(140px, 100%)" }, stop: [191, 247], strip: 28 },
  "Phone 1366×768 @150 %": { pane: 91, arc: [80.7, 14.7], value: 11, label: 8, num: 6, seen: true, panes: { bandActivity: 114, voiceKeyer: 51, receiver: 85, transmitter: 47, log: 209 }, floors: { bandActivity: "0px", voiceKeyer: "0px", receiver: "0px", transmitter: "0px", log: "min(140px, 100%)" }, stop: [191, 221], strip: 28 },
  "Phone 1366×768 @175 %": { pane: 97.1, arc: [80.7, 14.7], value: 11, label: 8, num: 6, seen: true, panes: { bandActivity: 104, voiceKeyer: 51, receiver: 85, transmitter: 47, log: 229 }, floors: { bandActivity: "0px", voiceKeyer: "0px", receiver: "0px", transmitter: "0px", log: "0px" }, stop: [191, 205], strip: 28 },
  "Phone 1920×1080 @100 %": { pane: 315.5, arc: [162.1, 29.5], value: 22, label: 16, num: 11, seen: true, panes: { bandActivity: 169, voiceKeyer: 74, receiver: 169, transmitter: 68, log: 577 }, floors: { bandActivity: "0px", voiceKeyer: "0px", receiver: "0px", transmitter: "0px", log: "min(140px, 100%)" }, stop: [292, 380], strip: 40 },
  "Phone 1920×1080 @150 %": { pane: 112, arc: [99.9, 18.2], value: 14, label: 10, num: 7, seen: true, panes: { bandActivity: 137, voiceKeyer: 60, receiver: 103, transmitter: 55, log: 326 }, floors: { bandActivity: "0px", voiceKeyer: "0px", receiver: "0px", transmitter: "0px", log: "min(140px, 100%)" }, stop: [235, 294], strip: 33 },
  "Phone 1920×1200 @100 %": { pane: 384.5, arc: [162.1, 29.5], value: 22, label: 16, num: 11, seen: true, panes: { bandActivity: 169, voiceKeyer: 74, receiver: 169, transmitter: 68, log: 646 }, floors: { bandActivity: "0px", voiceKeyer: "0px", receiver: "0px", transmitter: "0px", log: "min(140px, 100%)" }, stop: [292, 421], strip: 44 },
  "Phone 2560×1440 @100 %": { pane: 584.5, arc: [257, 46.8], value: 35, label: 25, num: 18, seen: true, panes: { bandActivity: 153, voiceKeyer: 74, receiver: 128, transmitter: 68, log: 547, stats: 246 }, floors: { bandActivity: "0px", voiceKeyer: "0px", receiver: "0px", transmitter: "0px", log: "min(140px, 100%)", stats: "0px" }, stop: [292, 502], strip: 44 },
  "Phone 2560×1440 @150 %": { pane: 140, arc: [125.5, 22.8], value: 17, label: 12, num: 9, seen: true, panes: { bandActivity: 152, voiceKeyer: 74, receiver: 128, transmitter: 68, log: 455 }, floors: { bandActivity: "0px", voiceKeyer: "0px", receiver: "0px", transmitter: "0px", log: "min(140px, 100%)" }, stop: [292, 382], strip: 40 },
  "Phone 3840×2160 @150 %": { pane: 584.5, arc: [257, 46.8], value: 35, label: 25, num: 18, seen: true, panes: { bandActivity: 152, voiceKeyer: 74, receiver: 128, transmitter: 68, log: 547, stats: 246 }, floors: { bandActivity: "0px", voiceKeyer: "0px", receiver: "0px", transmitter: "0px", log: "min(140px, 100%)", stats: "0px" }, stop: [292, 502], strip: 44 },
  "Phone 3840×2160 @200 %": { pane: 315.5, arc: [162.1, 29.5], value: 22, label: 16, num: 11, seen: true, panes: { bandActivity: 168, voiceKeyer: 74, receiver: 169, transmitter: 68, log: 577 }, floors: { bandActivity: "0px", voiceKeyer: "0px", receiver: "0px", transmitter: "0px", log: "min(140px, 100%)" }, stop: [292, 380], strip: 40 },
  "CW 1366×768 @100 %": { pane: 119, arc: [106.3, 19.3], value: 15, label: 10, num: 7, seen: true, panes: { decode: 119, bandActivity: 145, copilot: 51, log: 315 }, floors: { decode: "min(140px, 100%)", bandActivity: "0px", copilot: "0px", log: "min(140px, 100%)" }, stop: [249, 318], strip: 35 },
  "CW 1366×768 @125 %": { pane: 91, arc: [80.7, 14.7], value: 11, label: 8, num: 6, seen: true, panes: { decode: 91, bandActivity: 104, copilot: 41, log: 261 }, floors: { decode: "min(140px, 100%)", bandActivity: "0px", copilot: "0px", log: "min(140px, 100%)" }, stop: [191, 250], strip: 28 },
  "CW 1366×768 @150 %": { pane: 91, arc: [80.7, 14.7], value: 11, label: 8, num: 6, seen: true, panes: { decode: 91, bandActivity: 114, copilot: 41, log: 185 }, floors: { decode: "min(140px, 100%)", bandActivity: "0px", copilot: "0px", log: "min(140px, 100%)" }, stop: [191, 225], strip: 28 },
  "CW 1366×768 @175 %": { pane: 97.1, arc: [80.7, 14.7], value: 11, label: 8, num: 6, seen: true, panes: { decode: 90, bandActivity: 104, copilot: 41, log: 219 }, floors: { decode: "0px", bandActivity: "0px", copilot: "0px", log: "0px" }, stop: [191, 181], strip: 28 },
  "CW 1920×1080 @100 %": { pane: 277.5, arc: [144.1, 26.2], value: 20, label: 14, num: 10, seen: true, panes: { decode: 546, bandActivity: 184, copilot: 60, log: 546 }, floors: { decode: "min(140px, 100%)", bandActivity: "0px", copilot: "0px", log: "min(140px, 100%)" }, stop: [292, 380], strip: 40 },
  "CW 1920×1080 @150 %": { pane: 112, arc: [99.9, 18.2], value: 14, label: 10, num: 7, seen: true, panes: { decode: 112, bandActivity: 137, copilot: 49, log: 293 }, floors: { decode: "min(140px, 100%)", bandActivity: "0px", copilot: "0px", log: "min(140px, 100%)" }, stop: [235, 298], strip: 33 },
  "CW 1920×1200 @100 %": { pane: 342.5, arc: [162.1, 29.5], value: 22, label: 16, num: 11, seen: true, panes: { decode: 611, bandActivity: 184, copilot: 60, log: 611 }, floors: { decode: "min(140px, 100%)", bandActivity: "0px", copilot: "0px", log: "min(140px, 100%)" }, stop: [292, 421], strip: 44 },
  "CW 2560×1440 @100 %": { pane: 517.5, arc: [257, 46.8], value: 35, label: 25, num: 18, seen: true, panes: { decode: 770, bandActivity: 168, copilot: 60, log: 512, stats: 246 }, floors: { decode: "min(140px, 100%)", bandActivity: "0px", copilot: "0px", log: "min(140px, 100%)", stats: "0px" }, stop: [292, 502], strip: 44 },
  "CW 2560×1440 @150 %": { pane: 140, arc: [125.5, 22.8], value: 17, label: 12, num: 9, seen: true, panes: { decode: 140, bandActivity: 152, copilot: 60, log: 418 }, floors: { decode: "min(140px, 100%)", bandActivity: "0px", copilot: "0px", log: "min(140px, 100%)" }, stop: [292, 388], strip: 40 },
  "CW 3840×2160 @150 %": { pane: 518.5, arc: [257, 46.8], value: 35, label: 25, num: 18, seen: true, panes: { decode: 770, bandActivity: 167, copilot: 60, log: 512, stats: 246 }, floors: { decode: "min(140px, 100%)", bandActivity: "0px", copilot: "0px", log: "min(140px, 100%)", stats: "0px" }, stop: [292, 502], strip: 44 },
  "CW 3840×2160 @200 %": { pane: 278.5, arc: [144.8, 26.4], value: 20, label: 14, num: 10, seen: true, panes: { decode: 546, bandActivity: 183, copilot: 60, log: 546 }, floors: { decode: "min(140px, 100%)", bandActivity: "0px", copilot: "0px", log: "min(140px, 100%)" }, stop: [292, 380], strip: 40 },
  "SSTV 1366×768 @100 %": { pane: 119, arc: [106.3, 19.3], value: 15, label: 10, num: 7, seen: true, panes: { gallery: 119, txcompose: 116 }, floors: { gallery: "min(140px, 100%)", txcompose: "0px" }, stop: [249, 415], strip: 35 },
  "SSTV 1366×768 @125 %": { pane: 91, arc: [80.7, 14.7], value: 11, label: 8, num: 6, seen: true, panes: { gallery: 91, txcompose: 92 }, floors: { gallery: "min(140px, 100%)", txcompose: "0px" }, stop: [191, 331], strip: 28 },
  "SSTV 1366×768 @150 %": { pane: 91, arc: [80.7, 14.7], value: 11, label: 8, num: 6, seen: true, panes: { gallery: 91, txcompose: 92 }, floors: { gallery: "min(140px, 100%)", txcompose: "0px" }, stop: [191, 266], strip: 28 },
  "SSTV 1366×768 @175 %": { pane: 91, arc: [80.7, 14.7], value: 11, label: 8, num: 6, seen: true, panes: { gallery: 91, txcompose: 92 }, floors: { gallery: "min(140px, 100%)", txcompose: "0px" }, stop: [191, 275], strip: 28 },
  "SSTV 1920×1080 @100 %": { pane: 140, arc: [125.5, 22.8], value: 17, label: 12, num: 9, seen: true, panes: { gallery: 140, txcompose: 138 }, floors: { gallery: "min(140px, 100%)", txcompose: "0px" }, stop: [292, 563], strip: 40 },
  "SSTV 1920×1080 @150 %": { pane: 112, arc: [99.9, 18.2], value: 14, label: 10, num: 7, seen: true, panes: { gallery: 112, txcompose: 111 }, floors: { gallery: "min(140px, 100%)", txcompose: "0px" }, stop: [235, 389], strip: 33 },
  "SSTV 1920×1200 @100 %": { pane: 141.1, arc: [125.5, 22.8], value: 17, label: 12, num: 9, seen: true, panes: { gallery: 141, txcompose: 138 }, floors: { gallery: "min(140px, 100%)", txcompose: "0px" }, stop: [292, 632], strip: 44 },
  "SSTV 2560×1440 @100 %": { pane: 191.6, arc: [183.8, 33.5], value: 25, label: 18, num: 13, seen: true, panes: { gallery: 192, txcompose: 138 }, floors: { gallery: "min(140px, 100%)", txcompose: "0px" }, stop: [292, 771], strip: 44 },
  "SSTV 2560×1440 @150 %": { pane: 140, arc: [125.5, 22.8], value: 17, label: 12, num: 9, seen: true, panes: { gallery: 140, txcompose: 138 }, floors: { gallery: "min(140px, 100%)", txcompose: "0px" }, stop: [292, 518], strip: 40 },
  "SSTV 3840×2160 @150 %": { pane: 191.6, arc: [183.8, 33.5], value: 25, label: 18, num: 13, seen: true, panes: { gallery: 192, txcompose: 138 }, floors: { gallery: "min(140px, 100%)", txcompose: "0px" }, stop: [292, 771], strip: 44 },
  "SSTV 3840×2160 @200 %": { pane: 140, arc: [125.5, 22.8], value: 17, label: 12, num: 9, seen: true, panes: { gallery: 140, txcompose: 138 }, floors: { gallery: "min(140px, 100%)", txcompose: "0px" }, stop: [292, 563], strip: 40 },
}

/** The panes that share the meters pane's column by weight, per view, and so give it room: measured on every page
 *  (FT8's and Phone's give up nothing). Every other pane may give up at most 1 px. */
const SHARED = { operate: [], phone: [], cw: ['decode'], sstv: ['gallery'] }
/** The same page with the pane HIDDEN: the other panes, their floors, Stop TX and the strip (check 3's baseline). */
const BARE = `(() => {
  const vis = (el) => el && el.closest('[hidden]') == null && el.getBoundingClientRect().width > 0
  const shell = [...document.querySelectorAll('main')].find(vis)
  if (!shell || [...document.querySelectorAll('[data-pane="txMetersPane"]')].some(vis)) return null
  const reach = (b) => { const q = b.getBoundingClientRect(); if (q.width < 1 || q.bottom > innerHeight || q.top < 0 || q.right > innerWidth) return false
    const t = document.elementFromPoint(q.left + q.width / 2, q.top + q.height / 2); return t != null && (t === b || b.contains(t)) }
  const stop = [...shell.querySelectorAll('button')].find((b) => vis(b) && /^stop tx$/i.test(b.textContent.trim()))
  const strip = [...shell.querySelectorAll('.cockpit-txstrip, .cockpit-ftstrip')].find(vis)
  if (!stop || !strip) return null
  const panes = [...shell.querySelectorAll('[data-pane]')].filter(vis)
  return {
    panes: panes.map((p) => [p.dataset.pane, Math.round(p.getBoundingClientRect().height)]),
    floors: panes.map((p) => [p.dataset.pane, getComputedStyle(p).minHeight]),
    stop: { at: [Math.round(stop.getBoundingClientRect().left), Math.round(stop.getBoundingClientRect().top)], reach: reach(stop) },
    strip: Math.round(strip.getBoundingClientRect().height),
  }
})()`

/** What is wrong with one page: its dial and readout against 1.24.0's (`before`), its room against the same page with
 *  the pane hidden (`bare`); empty when nothing is. */
function faults(tag, m, before, bare, view = null) {
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
    const n = face.nums
    if (before.num != null && !(n.h >= before.num - 0.5)) f.push(`${face.key}'s scale numbers are ${n.h} px high, under 1.24.0's ${before.num}`)
    if (n.overlap.length) f.push(`${face.key}'s scale numbers overlap: ${n.overlap.join(', ')}`)
    if (n.needle.length) f.push(`${face.key}'s needle crosses its scale numbers ${n.needle.join(', ')}`)
    if (n.band.length) f.push(`${face.key}'s scale numbers ${n.band.join(', ')} run into the band`)
    if (n.clipped.length) f.push(`${face.key}'s scale numbers ${n.clipped.join(', ')} are clipped at the drawing's top`)
  }
  if (!(m.pane.h < before.pane - 1)) f.push(`the pane is ${m.pane.h} px high, not shorter than 1.24.0's ${before.pane}`)
  if (!bare) f.push('the page never drew with the pane hidden')
  else {
    const was = Object.fromEntries(bare.floors)
    for (const [id, v] of m.floors) if (was[id] != null && v !== was[id]) f.push(`the ${id} pane's computed floor is ${v} with the pane, ${was[id]} without`)
    const h0 = Object.fromEntries(bare.panes)
    const shared = new Set(SHARED[view] ?? [])
    let given = 0
    for (const [id, h] of m.panes) {
      const lost = Math.max(0, (h0[id] ?? h) - h)
      if (shared.has(id)) given += lost
      else if (lost > 1) f.push(`the ${id} pane gives up ${lost} px to the pane (it shares no column with it)`)
    }
    if (given > m.pane.h + 16) f.push(`the panes sharing its column give up ${given} px for a ${m.pane.h} px pane`)
    if (!m.stop?.reach) f.push('Stop TX is not on screen or something covers it')
    else if (JSON.stringify(m.stop.at) !== JSON.stringify(bare.stop.at)) f.push(`Stop TX moved from ${bare.stop.at} (pane hidden) to ${m.stop.at}`)
    if (m.strip !== bare.strip) f.push(`the TX strip is ${m.strip} px high with the pane, ${bare.strip} without`)
  }
  return f.map((s) => `${tag}: ${s}`)
}

let code = 0
const red = (msg) => {
  console.error(`txmeterspane-probe: RED — ${msg}`)
  code = 1
}
try {
  for (const [name, hash, view] of VIEWS) for (const [page, w, h, dpr] of PAGES) {
    const tag = `${name} ${page}`
    if (ONLY && !tag.startsWith(ONLY)) continue
    const m = await measure(hash, view, [w, h, dpr])
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
    const bare = await measure(hash, view, [w, h, dpr], null, true)
    const face = m.faces[0]
    const h0 = Object.fromEntries(bare?.panes ?? [])
    const gains = m.panes.filter(([id, h]) => h0[id] != null && h !== h0[id]).map(([id, h]) => `${id} ${h - h0[id] > 0 ? '+' : ''}${h - h0[id]}`)
    console.log(`txmeterspane-probe: ${tag} (${m.vp}): pane ${before?.pane} → ${m.pane.h} px; dial ${face.arc.w}×${face.arc.h} (1.24.0 ${before?.arc.join('×')}), ${face.above} px above it; value ${face.value.h} px (1.24.0 ${before?.value}), name ${face.label.h} px (1.24.0 ${before?.label}); the others against the pane hidden: ${gains.length ? gains.join(', ') : 'unchanged'}; Stop TX at ${m.stop?.at} (hidden: ${bare?.stop.at})`)
    if (!before) red(`${tag}: no 1.24.0 measure to hold it to`)
    else for (const r of faults(tag, m, before, bare, view)) red(r)
  }
  if (!PRINT && !ONLY) {
    // THE CONTROL: 1.24.0's dial drawn back (the dial's own scale planted away) must fail check 1.
    const tag = 'FT8 1920×1080 @100 %'
    const m = await measure('operate', 'operate', [1920, 1080, 1], '.txmeters-pane-dial { transform: none !important }')
    const caught = m ? faults(tag, m, BEFORE[tag]) : []
    console.log(`txmeterspane-probe: CONTROL 1.24.0's dial drawn back: caught ${JSON.stringify(caught)}`)
    if (!caught.some((s) => s.includes('dial is'))) red("CONTROL FAILED: 1.24.0's dial drawn back was not seen; the check cannot fail")
  }
  if (!PRINT && !ONLY) {
    // CHECK 3'S CONTROL: 150 px stolen from Phone's log by the pane's page alone (a planted margin under the log, which
    // shares no column with the meters pane) must be caught.
    const tag = 'Phone 1920×1080 @100 %'
    const bare = await measure('phone', 'phone', [1920, 1080, 1], null, true)
    const m = await measure('phone', 'phone', [1920, 1080, 1], '[data-pane="log"] { margin-bottom: 150px !important }')
    const caught = m ? faults(tag, m, BEFORE[tag], bare, 'phone') : []
    console.log(`txmeterspane-probe: CONTROL 150 px stolen from Phone's log: caught ${JSON.stringify(caught.filter((s) => s.includes('gives up')))}`)
    if (!caught.some((s) => s.includes('the log pane gives up'))) red("CONTROL FAILED: 150 px taken from Phone's log was not seen; check 3 cannot fail")
  }
  if (!PRINT && !ONLY) {
    // THE NUMBERS' CONTROL: the scale's numbers planted at twice 1.24.0's size must be caught crowding one another.
    const tag = 'FT8 1366×768 @175 %'
    const m = await measure('operate', 'operate', [780, 438, 1.75], '.txmeters-pane-scale { font-size: 9.2px !important }')
    const caught = m ? faults(tag, m, BEFORE[tag]) : []
    console.log(`txmeterspane-probe: CONTROL the scale's numbers at twice 1.24.0's size: caught ${JSON.stringify(caught)}`)
    if (!caught.some((s) => s.includes('scale numbers overlap') || s.includes('needle crosses'))) red("CONTROL FAILED: crowded scale numbers were not seen; the check cannot fail")
  }
  if (!PRINT && !ONLY) {
    // THE NUMBERS' HEIGHT CONTROL: the scale's numbers planted at half 1.24.0's size must be caught as smaller.
    const tag = 'FT8 1920×1080 @100 %'
    const m = await measure('operate', 'operate', [1920, 1080, 1], '.txmeters-pane-scale { font-size: 2.3px !important }')
    const caught = m ? faults(tag, m, BEFORE[tag]) : []
    console.log(`txmeterspane-probe: CONTROL the scale's numbers at half 1.24.0's size: caught ${JSON.stringify(caught)}`)
    if (!caught.some((s) => /scale numbers are .* under 1\.24\.0's/.test(s))) red("CONTROL FAILED: halved scale numbers were not seen; the check cannot fail")
  }
  if (code === 0 && !PRINT) console.log('txmeterspane-probe: PASS — every dial half of 1.24.0\'s with its readout no smaller, whole and in view, in a pane shorter than 1.24.0\'s that takes room only from itself (against the same page with it hidden: no floor changed, no pane outside its column giving up more than 1 px and those in it no more than its own height, the TX strip and Stop TX unmoved), on FT8, Phone, CW and SSTV at every size and zoom; the controls caught')
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
