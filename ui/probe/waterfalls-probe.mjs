// Every mode's waterfall in real Chrome, for what jsdom cannot see: `scripts/browser-probe --desktop waterfalls`.
//
// The operator's ruling (2026-10-26, "width and height to be consistent"): every mode's waterfall starts at the
// SAME full width and the SAME default height, 25 % of its cockpit — FT8/FT4 in both layouts, Phone, CW, JS8,
// JTTY, PSK, RTTY and Tempo. SSTV keeps its bigger stage (its decoded picture shows there): its width is checked,
// its height is not. The probe serves the REAL app (index.html, main.tsx, App) with the app's own Vite config and
// src/api.ts swapped for probe/waterfalls-api.ts, opens each mode with fresh storage at 1024×768, 1366×768,
// 1920×1080 and 2560×1440 (the app fits its own zoom, as it does on those screens), and measures the strip each
// waterfall's height divider sizes. It asserts:
//   1. THE DEFAULT IS 25 % OF THE COCKPIT: the height the divider announces is 25 % of its shell's content box,
//      clamped by the range the divider announces (the layout's own floors: on a short window a cockpit's panes
//      and dock can hold the strip below 25 %, and the divider then stands at its ceiling, which is reported).
//   2. THE SAME WIDTH: every waterfall's strip is as wide as every other one at that size, within 10 px (the
//      shells' padding differs by 4 px a side; Phone's and CW's scope panels draw the picture 8 px in from each
//      edge, which the table reports).
//   3. THE SAME HEIGHT: every strip the layout lets reach 25 % is the same height on the screen, within 3 px,
//      wherever their cockpits are the same height.
//   4. Stop TX, Tune and Phone's PTT are on screen and the topmost thing where they stand; CW's pane region keeps
//      its floor and its shell does not scroll.
//   6. RTTY (2026-10-26): its tuning scope stands RIGHT of the waterfall in the same strip, so with the scope
//      shown the strip is exactly as tall as with it hidden (the scope costs the waterfall no height), and that is
//      the COMMON height — 25 % of the cockpit, as tall as JS8's, which has the same cockpit — wherever RTTY's own
//      panes let the strip reach it (from 1366×768 up; at 1024×768 the decoded text's and the log strip's floors
//      hold it lower with or without the scope, as PSK's and CW's are held, and the table says so). The square
//      keeps its size (at least what it was in its old place left of the decoded text, measured before the move:
//      TUNING_BEFORE), its nudges and words are on screen with nothing scrolled, RTTY's cockpit does not scroll,
//      and with the scope hidden the waterfall takes the strip's full width.
//   5. PHONE'S LEFT SIDE: with Band Activity on the side, the scope is exactly as wide as without it, and the side
//      starts under the TX strip. TEMPO, STACKED (1024×768 at a pinned 100 %): its strip is 25 % of the window and
//      stands clear of the cell under it.
// THE CONTROLS, run every time: JS8 with its divider moved one step (the default no longer 25 %) must fail check
// 1, and Phone with a planted 360 px margin beside its scope must fail check 2. If either passes, the checks
// cannot fail and the probe says so instead of passing. RTTY's two (6): a planted ceiling on its strip must fail the
// common height, and a planted 60 px square must fail the scope's size.
//
// exit: 0 every check held · 1 a red, or a control that could not fail · 2 a page never drew its waterfall
import { createServer } from 'vite'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const REAL_API = join(UI, 'src', 'api.ts')
const STUB_SRC = join(UI, 'probe', 'waterfalls-api.ts')
// Served under a name with no file behind it, beside the source (so its relative import resolves the same):
// the dependency scan reads files from disk, and the source alone lacks most of api.ts's exports.
const STUB_API = join(UI, 'probe', '__waterfalls-api.ts')
const SIZES = [[1024, 768], [1366, 768], [1920, 1080], [2560, 1440]]
const FEATURES = JSON.stringify({ profile: 'custom', enabled: { js8: true, jtty: true, rtty: true, psk: true, sstv: true } })
/** [name, hash, extra storage]. Roster is FT's stock layout. */
const VIEWS = [
  ['FT8 Roster', 'operate', {}],
  ['FT8 Classic', 'operate', { 'nexus.operate.layout': 'classic' }],
  ['Phone', 'phone', {}],
  ['CW', 'cw', {}],
  ['JS8', 'js8', {}],
  ['JTTY', 'jtty', {}],
  ['PSK', 'psk', {}],
  ['RTTY', 'rtty', {}],
  ['Tempo', 'chat', { 'nexus.workspace': 'msg' }],
  ['SSTV', 'sstv', {}],
]
const NO_HEIGHT = new Set(['SSTV'])
/** RTTY's tuning square on screen, px, in its old place left of the decoded text (measured on the tree before the
 *  2026-10-26 move, at these sizes): the size it keeps. */
const TUNING_BEFORE = { '1024×768': 118, '1366×768': 142, '1920×1080': 201, '2560×1440': 202 }
const RTTY_NO_TUNING = { 'nexus.panels.rtty.main': JSON.stringify({ v: 1, state: { tuning: 'removed' }, share: {} }) }
const PHONE_SIDE = { 'nexus.panels.phone.main': JSON.stringify({ v: 2, state: {}, share: {}, leftSide: ['bandActivity'] }) }
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

/** The waterfall strip and everything the checks read, in CSS px (`cs`) and on-screen px (`px`). */
const MEASURE = `(() => {
  const vis = (el) => el && el.closest('[hidden]') == null && el.getBoundingClientRect().width > 0
  const zoomOf = (el) => el.currentCSSZoom || parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--ui-zoom')) || 1
  const content = (el) => {
    const s = getComputedStyle(el), n = (v) => parseFloat(v) || 0
    return el.getBoundingClientRect().height / zoomOf(el) - n(s.paddingTop) - n(s.paddingBottom) - n(s.borderTopWidth) - n(s.borderBottomWidth)
  }
  const sep = [...document.querySelectorAll('.pane-splitter.horizontal[role="separator"]')].filter(vis).find((s) => {
    const p = s.previousElementSibling
    return p && (p.matches('.waterfall-wrap, .ph-scope-panel') || p.querySelector('.waterfall-canvas, .ph-scope-canvas-wrap'))
  })
  // Tempo stacked: every divider is hidden in the collapse, and the strip is the workspace's own.
  const strip = sep ? sep.previousElementSibling : [...document.querySelectorAll('.layout[data-three-pane] > .waterfall-wrap')].find(vis)
  if (!strip || !strip.querySelector('.waterfall-canvas, .ph-scope-canvas-wrap')) return null
  const canvas = strip.querySelector('.waterfall-canvas, .ph-scope-canvas-wrap')
  const shell = strip.closest('main')
  const r = strip.getBoundingClientRect(), c = canvas.getBoundingClientRect()
  const num = (a) => (sep ? Number(sep.getAttribute(a)) : null)
  // Is this button the topmost thing at its own centre, inside the window?
  const reach = (b) => {
    const q = b.getBoundingClientRect()
    if (q.width < 1 || q.bottom > innerHeight || q.top < 0 || q.right > innerWidth) return false
    const t = document.elementFromPoint(q.left + q.width / 2, q.top + q.height / 2)
    return t != null && (t === b || b.contains(t))
  }
  const stops = [...shell.querySelectorAll('button')].filter((b) => vis(b) && /^(stop tx|tune|stop)$/i.test(b.textContent.trim()))
  const ptt = shell.querySelector('.ph-ptt')
  const region = shell.querySelector(':scope .cockpit-panes')
  const side = shell.querySelector('.cockpit-left')
  const txstrip = shell.querySelector('.cockpit-txstrip')
  return {
    vp: document.documentElement.getAttribute('data-viewport'),
    z: zoomOf(strip),
    now: num('aria-valuenow'), min: num('aria-valuemin'), max: num('aria-valuemax'),
    shellCs: content(shell), stripCs: r.height / zoomOf(strip),
    px: { w: Math.round(r.width), h: Math.round(r.height), top: Math.round(r.top), bottom: Math.round(r.bottom), canvasW: Math.round(c.width), canvasH: Math.round(c.height) },
    stops: stops.map((b) => [b.textContent.trim(), reach(b)]),
    ptt: ptt ? reach(ptt) : null,
    region: region ? { h: region.getBoundingClientRect().height / zoomOf(region), floor: parseFloat(getComputedStyle(region).minHeight) || 0 } : null,
    shellScrolls: shell.scrollHeight > shell.clientHeight + 1,
    wfW: Math.round(strip.querySelector('.waterfall-wrap')?.getBoundingClientRect().width ?? r.width),
    side: side ? { top: Math.round(side.getBoundingClientRect().top) } : null,
    txBottom: txstrip ? Math.round(txstrip.getBoundingClientRect().bottom) : null,
    next: (() => { const n = [...shell.children].find((e) => e.matches('.grid-stations')); return n ? Math.round(n.getBoundingClientRect().top) : null })(),
    vhEff: parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--vh-eff')) || null,
    // RTTY's tuning display: its square, where it stands against the waterfall, and whether its nudges and words
    // are on screen without its column scrolling.
    tuning: (() => {
      const col = shell.querySelector('.rtty-tuning')
      if (!vis(col)) return null
      const q = col.getBoundingClientRect(), b = col.querySelector('.rtty-tuning-box').getBoundingClientRect()
      const wf = shell.querySelector('.waterfall-wrap')
      const w = wf ? wf.getBoundingClientRect() : null
      const inCol = (el) => { const e = el.getBoundingClientRect(); return e.height > 0 && e.top >= q.top - 1 && e.bottom <= q.bottom + 1 && e.bottom <= innerHeight }
      return {
        side: Math.round(Math.min(b.width, b.height)), boxW: Math.round(b.width), boxH: Math.round(b.height),
        left: Math.round(q.left), top: Math.round(q.top), bottom: Math.round(q.bottom),
        wfRight: w ? Math.round(w.right) : null, inStrip: strip.contains(col),
        scrolls: col.scrollHeight > col.clientHeight + 1,
        controls: [...col.querySelectorAll('.rtty-tuning-btn, .rtty-tuning-status')].every(inCol),
      }
    })(),
  }
})()`

// THE API SWAP: every import that resolves to src/api.ts gets the probe's module, which answers the shapes the
// cockpits read and `{}` (or `[]`) for every other export, so no call reaches for a backend that is not there.
const realSrc = await readFile(REAL_API, 'utf8')
const stubSrc = await readFile(STUB_SRC, 'utf8')
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
  name: 'waterfalls-probe-api',
  enforce: 'pre',
  async resolveId(source, importer, opts) {
    // The dependency scan reads modules from disk: it keeps the real api.ts, which is all it needs.
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

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-waterfalls-probe-'))
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

/** One page: the app at `hash` in a W×H window with fresh storage plus `extra`, measured once it settles.
 *  `before(evaluate)` may act on the page first (a control's plant). */
async function measure(hash, [W, H], extra = {}, before = null) {
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
    const ls = { 'nexus.workspace': 'dx', 'nexus.features.v1': FEATURES, 'tempo-theme': 'dark', ...extra }
    await evaluate(`localStorage.clear(); ${Object.entries(ls).map(([k, v]) => `localStorage.setItem(${JSON.stringify(k)}, ${JSON.stringify(v)})`).join(';')}; 1`)
    await call('Page.navigate', { url: `${origin}/#${hash}` })
    let m = null
    for (let i = 0; i < 120 && !m; i++) {
      await sleep(250)
      m = await evaluate(MEASURE).catch(() => null)
    }
    if (!m) return null
    if (before) await before(evaluate)
    // Settled: two reads in a row agree (the viewport stamp, the fit zoom and the observers run a frame or two late).
    let last = null
    for (let i = 0; i < 40; i++) {
      await sleep(250)
      m = await evaluate(MEASURE)
      if (last && JSON.stringify(m) === JSON.stringify(last)) break
      last = m
    }
    return m
  } finally {
    await browser.call('Target.closeTarget', { targetId })
  }
}

/** Check 1 for one page: the announced height is 25 % of the shell, clamped by the announced range. */
function defaultRed(name, m) {
  const want = Math.min(m.max, Math.max(m.min, 0.25 * m.shellCs))
  if (Math.abs(m.now - want) > 2) return `${name}: the divider stands at ${m.now} CSS px, not 25 % of its ${m.shellCs.toFixed(0)} px cockpit (${want.toFixed(0)}, range ${m.min}–${m.max})`
  if (Math.abs(m.stripCs - m.now) > 2) return `${name}: the strip renders ${m.stripCs.toFixed(0)} CSS px, not the ${m.now} its divider announces`
  return null
}
/** Check 6 for RTTY with its tuning scope shown (`m`), against the same page with it hidden (`bare`): the
 *  scope costs no height, the strip is the common height wherever the cockpit's own panes allow it, and the
 *  scope's size and place. */
function rttyRed(tag, m, bare, js8) {
  const reds = []
  const want = 0.25 * m.shellCs
  if (Math.abs(m.stripCs - bare.stripCs) > 2)
    reds.push(`RTTY at ${tag}: the strip is ${m.stripCs.toFixed(0)} CSS px with the tuning scope, ${bare.stripCs.toFixed(0)} without it`)
  // The common height, wherever the strip without the scope reaches it.
  if (bare.stripCs >= want - 2) {
    if (Math.abs(m.now - want) > 2 || Math.abs(m.stripCs - want) > 2)
      reds.push(`RTTY at ${tag}: the strip is ${m.stripCs.toFixed(0)} CSS px (divider ${m.now}, range ${m.min}–${m.max}), not the common 25 % of its ${m.shellCs.toFixed(0)} px cockpit (${want.toFixed(0)})`)
    if (js8 && Math.abs(m.shellCs - js8.shellCs) <= 2 && Math.abs(m.px.h - js8.px.h) > 3) reds.push(`RTTY at ${tag}: the strip is ${m.px.h} px tall, JS8's ${js8.px.h}`)
  }
  const t = m.tuning
  if (!t) return [...reds, `RTTY at ${tag}: no tuning scope on screen`]
  if (!t.inStrip) reds.push(`RTTY at ${tag}: the tuning scope is not in the waterfall's strip`)
  if (t.wfRight != null && t.left < t.wfRight) reds.push(`RTTY at ${tag}: the tuning scope starts at ${t.left}, left of the waterfall's right edge ${t.wfRight}`)
  if (t.side < TUNING_BEFORE[tag] - 1) reds.push(`RTTY at ${tag}: the tuning square is ${t.side} px, smaller than its ${TUNING_BEFORE[tag]} px before the move`)
  if (!t.controls) reds.push(`RTTY at ${tag}: the tuning scope's nudges or words are cut off`)
  if (t.scrolls) reds.push(`RTTY at ${tag}: the tuning scope scrolls`)
  if (m.shellScrolls) reds.push(`RTTY at ${tag}: the cockpit scrolls`)
  return reds
}
/** Checks 2 and 3 across one size's pages. */
function sameRed(size, rows) {
  const reds = []
  const ws = rows.map(([n, m]) => [n, m.px.w])
  const wMin = Math.min(...ws.map(([, w]) => w)), wMax = Math.max(...ws.map(([, w]) => w))
  if (wMax - wMin > 10) reds.push(`${size}: waterfall widths differ by ${wMax - wMin} px: ${ws.map(([n, w]) => `${n} ${w}`).join(', ')}`)
  const free = rows.filter(([n, m]) => !NO_HEIGHT.has(n) && m.now < m.max - 1)
  const groups = new Map()
  for (const [n, m] of free) {
    const key = [...groups.keys()].find((k) => Math.abs(k - m.shellCs) <= 2) ?? m.shellCs
    groups.set(key, [...(groups.get(key) ?? []), [n, m.px.h]])
  }
  for (const g of groups.values()) {
    const hs = g.map(([, h]) => h)
    if (Math.max(...hs) - Math.min(...hs) > 3) reds.push(`${size}: strips in same-height cockpits differ: ${g.map(([n, h]) => `${n} ${h}`).join(', ')}`)
  }
  return reds
}

let code = 0
const red = (msg) => {
  console.error(`waterfalls-probe: RED — ${msg}`)
  code = 1
}
try {
  const table = []
  const bySize = new Map()
  const rttyBare = new Map()
  for (const size of SIZES) {
    const tag = `${size[0]}×${size[1]}`
    const rows = []
    for (const [name, hash, extra] of VIEWS) {
      const m = await measure(hash, size, extra)
      if (!m) {
        console.error(`waterfalls-probe: ${name} at ${tag} never drew a waterfall with a height divider`)
        process.exitCode = 2
        throw new Error('no waterfall')
      }
      rows.push([name, m])
      table.push(`${tag.padEnd(10)} ${name.padEnd(12)} ${String(m.px.w).padStart(5)} × ${String(m.px.h).padStart(3)} px  picture ${String(m.px.canvasW).padStart(4)} × ${String(m.px.canvasH).padStart(3)}  (25 % of ${m.shellCs.toFixed(0)} CSS px = ${(0.25 * m.shellCs).toFixed(0)}, divider ${m.now} of ${m.min}–${m.max}${m.now >= m.max - 1 ? ', AT ITS CEILING' : ''})`)
      if (m.now == null) red(`${name} at ${tag}: no height divider beside the waterfall`)
      else if (!NO_HEIGHT.has(name)) {
        const r = defaultRed(`${name} at ${tag}`, m)
        if (r) red(r)
      }
      for (const [label, ok] of m.stops) if (!ok) red(`${name} at ${tag}: ${label} is not on screen or something covers it`)
      if (m.ptt === false) red(`${name} at ${tag}: PTT is not on screen or something covers it`)
      if (name === 'RTTY') {
        // The same page with the tuning scope hidden: the waterfall takes the strip's full width.
        const bare = await measure('rtty', size, RTTY_NO_TUNING)
        if (!bare) throw new Error(`RTTY without its tuning scope at ${tag}: no waterfall`)
        rttyBare.set(tag, bare)
        console.log(`waterfalls-probe: RTTY at ${tag}: strip ${m.px.h} px tall with the tuning scope, ${bare.px.h} without (25 %: ${(0.25 * m.shellCs * m.z).toFixed(0)}); square ${m.tuning?.side} px (before the move ${TUNING_BEFORE[tag]}); waterfall ${m.wfW} × picture ${m.px.canvasH} px with it, ${bare.wfW} × ${bare.px.canvasH} without, of the ${m.px.w} px strip`)
        for (const r of rttyRed(tag, m, bare, rows.find(([n]) => n === 'JS8')?.[1])) red(r)
        if (bare.tuning) red(`RTTY at ${tag}: the tuning scope is still shown with its entry hidden`)
        if (Math.abs(bare.wfW - bare.px.w) > 1) red(`RTTY without its tuning scope at ${tag}: the waterfall is ${bare.wfW} px of a ${bare.px.w} px strip`)
        for (const [label, ok] of bare.stops) if (!ok) red(`RTTY (no tuning) at ${tag}: ${label} is not on screen or something covers it`)
      }
      if (name === 'CW') {
        if (!m.region || m.region.h < m.region.floor - 1) red(`CW at ${tag}: its pane region is ${m.region?.h.toFixed(0)} CSS px, under its ${m.region?.floor} px floor`)
        if (m.shellScrolls) red(`CW at ${tag}: the cockpit scrolls`)
      }
    }
    bySize.set(tag, rows)
    for (const r of sameRed(tag, rows)) red(r)
  }
  console.log('waterfalls-probe: the strip each waterfall\'s divider sizes, on screen (its box, and the picture in it):')
  for (const line of table) console.log(`  ${line}`)

  // 5. Phone's left side: the scope as wide as without it, the side under the TX strip.
  for (const size of SIZES.slice(1)) {
    const tag = `${size[0]}×${size[1]}`
    const plain = bySize.get(tag).find(([n]) => n === 'Phone')[1]
    const m = await measure('phone', size, PHONE_SIDE)
    if (!m?.side) red(`Phone at ${tag}: the left side did not show`)
    else {
      console.log(`waterfalls-probe: Phone with its left side at ${tag}: scope ${m.px.w} px (without: ${plain.px.w}), side top ${m.side.top}, TX strip bottom ${m.txBottom}`)
      if (Math.abs(m.px.w - plain.px.w) > 2) red(`Phone at ${tag}: the scope is ${m.px.w} px beside the left side, ${plain.px.w} without it`)
      if (m.side.top < m.txBottom) red(`Phone at ${tag}: the left side starts above the TX strip's bottom`)
      if (Math.abs(m.px.h - plain.px.h) > 2) red(`Phone at ${tag}: the scope is ${m.px.h} px tall beside the side, ${plain.px.h} without it`)
      for (const [label, ok] of m.stops) if (!ok) red(`Phone (side) at ${tag}: ${label} is not on screen or something covers it`)
      if (m.ptt === false) red(`Phone (side) at ${tag}: PTT is not on screen or something covers it`)
    }
  }
  // Tempo stacked: 1024×768 at a pinned 100 % is the narrow collapse.
  {
    const m = await measure('chat', [1024, 768], { 'nexus.workspace': 'msg', 'nexus-ui-scale-mode': '100' })
    if (!m) red('Tempo stacked: no waterfall')
    else {
      const want = Math.max(Math.min(8 * 16, 0.28 * m.vhEff), 0.25 * m.vhEff)
      console.log(`waterfalls-probe: Tempo stacked (${m.vp}) at 1024×768 100 %: strip ${m.px.w} × ${m.px.h} px (25 % of the window: ${want.toFixed(0)}), next cell at ${m.next}`)
      if (!/^(sm|xs)$/.test(m.vp)) red(`Tempo stacked: the viewport is ${m.vp}, not the collapse — the case tests nothing`)
      if (Math.abs(m.stripCs - want) > 3) red(`Tempo stacked: the strip is ${m.stripCs.toFixed(0)} CSS px, not 25 % of the window (${want.toFixed(0)})`)
      if (m.next != null && m.next < m.px.bottom) red('Tempo stacked: the cell under the strip starts inside it')
    }
  }

  // THE CONTROLS.
  const step = await measure('js8', [1366, 768], {}, async (evaluate) => {
    await evaluate(`(() => { const s = document.querySelector('.js8-cockpit > .pane-splitter.horizontal'); s.focus(); s.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true })); return 1 })()`)
  })
  if (!step || defaultRed('JS8 moved one step', step) == null) red('CONTROL FAILED — JS8 with its divider moved off the default passed check 1; the check cannot fail')
  else console.log(`waterfalls-probe: CONTROL — ${defaultRed('JS8 moved one step', step)} (caught)`)
  const wide = await measure('phone', [1366, 768], {}, async (evaluate) => {
    await evaluate(`(() => { const s = document.createElement('style'); s.textContent = '.phone-cockpit > .ph-scope-panel { margin-right: 360px }'; document.head.appendChild(s); return 1 })()`)
  })
  const planted = bySize.get('1366×768').map(([n, m]) => [n, n === 'Phone' ? wide : m])
  const caught = wide ? sameRed('1366×768 (planted)', planted).filter((r) => r.includes('widths')) : []
  if (caught.length === 0) red('CONTROL FAILED — Phone with a planted 360 px margin passed check 2; the check cannot fail')
  else console.log(`waterfalls-probe: CONTROL — ${caught[0]} (caught)`)

  // RTTY's controls (check 6): a ceiling planted on its strip, and a planted 60 px square.
  for (const [what, css] of [
    ['a ceiling planted on its strip', '.rtty-cockpit > .rtty-wf-row, .rtty-cockpit > .waterfall-wrap { max-height: 100px !important }'],
    ['a planted 60 px square', '.rtty-tuning-box { width: 60px !important }'],
  ]) {
    const m = await measure('rtty', [1366, 768], {}, async (evaluate) => {
      await evaluate(`(() => { const s = document.createElement('style'); s.textContent = ${JSON.stringify(css)}; document.head.appendChild(s); return 1 })()`)
    })
    const caught = m ? rttyRed('1366×768', m, rttyBare.get('1366×768'), bySize.get('1366×768').find(([n]) => n === 'JS8')[1]) : []
    if (caught.length === 0) red(`CONTROL FAILED — RTTY with ${what} passed check 6; the check cannot fail`)
    else console.log(`waterfalls-probe: CONTROL — RTTY with ${what}: ${caught[0]} (caught)`)
  }

  if (code === 0) console.log('waterfalls-probe: PASS — every waterfall starts at 25 % of its cockpit (or its layout ceiling), as wide as every other, RTTY\'s with its tuning scope beside it at its own size, and every control was caught')
  process.exitCode = process.exitCode || code
} catch (e) {
  if (!process.exitCode) {
    console.error(`waterfalls-probe: ${e?.stack ?? e}`)
    process.exitCode = 2
  }
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
