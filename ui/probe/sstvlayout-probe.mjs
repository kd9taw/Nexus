// The SSTV screen's layout in real Chrome, for what jsdom cannot see: `scripts/browser-probe --desktop sstvlayout`.
//
// The SSTV screen's panes stand in one column under the TX strip: the RF scope pane and the large TX meters pane
// when ticked, the gallery taking the spare height, and the Transmit composer at the foot, directly above the
// Send/Stop bar. The composer lays the picture and its options side by side and wraps them by its own width. This
// probe serves the REAL app (index.html, main.tsx, App) with the app's own Vite config and src/api.ts swapped for
// probe/waterfalls-api.ts, opens SSTV with fresh storage at every size class (xs, sm, md, lg, xl) at 100 % and at
// 175 %, in English and in Russian, and asserts:
//   1. THE COMPOSER IS AT THE FOOT. On screen, the composer stands below the gallery (and below the meters pane
//      when it shows), and the Send/Stop bar is below the composer.
//   2. NO STOP CONTROL MOVES. Stop TX (the strip) and Stop (the bar) stand where they stood, to the pixel, after the
//      large meters pane is ticked (from the strip at md and up, from ⊞ Panels below), after a picture is loaded
//      into the composer, and after a warning appears in the composer.
//   3. NO CONTROL IS UNREACHABLE. Every button, select and input of the header, the strip, the panes and the bar,
//      with a picture loaded and the meters pane showing, is the topmost thing at its centre once scrolled to
//      (scrollIntoView, every scroller between), or at some position of the shell's own scroll.
//   4. NO HIDDEN ALARM. The composer's warning (an upscaled picture) and the bar's alert (a picture the engine
//      dropped) are on screen and topmost once scrolled to, and the bar's alert stays on screen and topmost at the
//      top and at the end of the shell's scroll; nothing in the composer runs past its pane sideways.
//   5. THE WATERFALL STAYS RESIZABLE BY ITS DIVIDER: real ArrowUp (or ArrowDown, at the bottom of its range) keys on the
//      divider size the stage wherever the divider offers any travel, and
//      Backspace gives the stock size back.
//   6. THE METERS PANE CAN BE SEEN WHOLE (2026-11-12): at some position of the shell's scroll (and its column's), the
//      large TX meters pane is on screen from its head to its foot and topmost there, and so is every face's name and
//      value. With the header pinned over 6em of room, 1366×768 at 175 % showed 117 px of a 140 px pane between the
//      parked header, strip and bar, and the readouts were under the header wherever the pane was scrolled to.
//      On the supported screens only: the xs windows are dragged smaller than any (a 900×600 window at 175 % leaves
//      the shell 179 px under the top bar and the nav row, less the strip and the bar), and are printed there.
//   And it prints whether the composer reads sideways or stacked at each size. It PRINTS, and does not fail on, the
//   strip's Stop TX under a bar grown by its alert on a small window at a large zoom (the bar's own Stop stays on
//   screen and is checked): the strip and the bar are not this layout's, and main does the same there. sm and xs
//   hide every divider in the app, so check 5 runs from md up.
// THE CONTROLS, run every time: the composer ordered above the gallery must fail check 1; a region sized by its
// content (so the meters pane takes height from the stage) must fail check 2; and a composer body clipped to two
// lines must fail checks 3 and 4; an opaque band planted over all but the window's last 120 px must fail check 6.
//
// exit: 0 every check held · 1 a red, or a control that could not fail · 2 the page never drew the screen
import { createServer } from 'vite'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'
import { FHD, FLOOR, LAPTOP, QHD, UHD } from './sizes.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const REAL_API = join(UI, 'src', 'api.ts')
const STUB_SRC = join(UI, 'probe', 'waterfalls-api.ts')
const STUB_API = join(UI, 'probe', '__sstvlayout-api.ts')
/** [class, window W, H, pinned UI scale, the OS's scaling as a device pixel ratio]: each class at 100 % and at 175 %
 *  (effective width = W / scale). The screens are probe/sizes.mjs's — sm at 100 % is the floor (1366×768 at the OS's
 *  175 %); xs (700×800, 900×600) and 4320×2160 are windows off the matrix, the only way to those classes there. */
const S = (cls, s, scale) => [cls, s.w, s.h, scale, s.scale]
const SIZES = [
  ['xs', 700, 800, '100'], S('sm', FLOOR, '100'), S('md', LAPTOP, '100'), S('lg', FHD, '100'), S('xl', QHD, '100'),
  ['xs', 900, 600, '175'], S('sm', FHD, '175'), S('md', QHD, '175'), S('lg', UHD, '175'), ['xl', 4320, 2160, '175'],
  // The floor screen unscaled at the app's 150 and 175 % (check 6 was measured red at 175 %).
  S('sm', LAPTOP, '150'), S('sm', LAPTOP, '175'),
]
const LANGS = ['en', 'ru']
/** SSTVLAYOUT_ONLY=<class>:<scale>:<lang> runs that one page and no controls, and SSTVLAYOUT_ONLY=controls the
 *  controls alone (debugging aids). */
const ONLY = process.env.SSTVLAYOUT_ONLY?.split(':') ?? null
const FEATURES = JSON.stringify({ profile: 'custom', enabled: { sstv: true } })
const SEL = 'main.sstv-view'
/** The engine's sentence for a dropped picture, long enough to wrap at 175 %. */
const DROPPED = 'SSTV: the picture was dropped while it waited for the transmitter, because transmit went off.'
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

/** The large meters pane's ⊞ label in each language, read from the catalogs (the menu lists it by name). */
const label = async (lang) => {
  const src = await readFile(join(UI, 'src', 'i18n', `${lang}.ts`), 'utf8')
  const m = /['"]meters\.tx\.pane\.title['"]\s*:\s*(['"])(.*?)\1/.exec(src)
  if (!m) throw new Error(`no meters.tx.pane.title in ${lang}.ts`)
  return m[2]
}

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
  name: 'sstvlayout-probe-api',
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

/** Where the two stop controls stand, rounded to the pixel, with the shell at the top of its scroll. */
const STOPS = `(async () => {
  const s = document.querySelector(${JSON.stringify(SEL)}); s.scrollTop = 0
  await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)))
  const at = (sel) => { const e = s.querySelector(sel); if (!e) return null; const q = e.getBoundingClientRect(); return [q.left, q.top, q.width, q.height].map(Math.round).join(',') }
  return { stopTx: at(':scope > .cockpit-txstrip .op-btn.stop'), stop: at(':scope > .sstv-tx-bar .sstv-tx-stop') }
})()`

/** Check 1, and the composer's reading: where the panes stand on screen, at the end of every scroll. */
const ORDER = `(async () => {
  const s = document.querySelector(${JSON.stringify(SEL)}); const col = s.querySelector('.cockpit-col')
  s.scrollTop = s.scrollHeight; if (col) col.scrollTop = col.scrollHeight
  await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)))
  const box = (sel) => { const e = s.querySelector(sel); return e ? e.getBoundingClientRect() : null }
  const comp = box('[data-pane="txcompose"]'), gal = box('[data-pane="gallery"]'), met = box('[data-pane="txMetersPane"]'), bar = box(':scope > .sstv-tx-bar')
  const out = []
  if (!comp || !gal || !bar) out.push('a pane or the bar is missing')
  else {
    if (comp.top < gal.bottom - 0.5) out.push('the composer is not below the gallery (' + Math.round(comp.top) + ' < ' + Math.round(gal.bottom) + ')')
    if (met && comp.top < met.bottom - 0.5) out.push('the composer is not below the meters pane')
    if (bar.top < comp.bottom - 0.5) out.push('the Send/Stop bar is not below the composer')
  }
  const st = s.querySelector('.sstv-compose-stage'), op = s.querySelector('.sstv-compose-opts')
  let reads = 'empty'
  if (st && op) { const a = st.getBoundingClientRect(), b = op.getBoundingClientRect(); reads = b.top < a.bottom - 1 && b.left >= a.right - 1 ? 'sideways' : b.top >= a.bottom - 1 ? 'stacked' : 'overlapping' }
  if (reads === 'overlapping') out.push('the composer\\'s options overlap its picture')
  s.scrollTop = 0; if (col) col.scrollTop = 0
  return { out, reads }
})()`

/** Checks 3 and 4: every control reachable and topmost; the two alarms seen; nothing in the composer sideways. */
/** Checks 3 and 4, in steps (one browser command each stays well inside its 10 s): helpers and the control list,
 *  one control at a time, then the two alarms and the composer's width. */
const REACH_SETUP = `(() => {
  const s = document.querySelector(${JSON.stringify(SEL)})
  const frame = () => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)))
  const vis = (e) => e.closest('[hidden]') == null && e.getBoundingClientRect().width > 0 && e.getBoundingClientRect().height > 0 && getComputedStyle(e).visibility !== 'hidden'
  const name = (e) => (e.getAttribute('aria-label') || e.textContent || e.title || e.type || e.tagName).replace(/\\s+/g, ' ').trim().slice(0, 28)
  const topmost = (e) => { const q = e.getBoundingClientRect(); const x = q.left + q.width / 2, y = q.top + Math.min(q.height / 2, 8)
    if (x < 0 || y < 0 || x >= innerWidth || y >= innerHeight) return false
    const t = document.elementFromPoint(x, y); return t != null && (t === e || e.contains(t) || (e.labels && [...e.labels].some((l) => l.contains(t)))) }
  // A box that clips (overflow hidden or clip) is no scroller to the operator, though a script can scroll it: a
  // control reached only by scrolling one is not reached.
  const clipScrolled = (e) => { for (let a = e.parentElement; a && a !== s.parentElement; a = a.parentElement) { const c = getComputedStyle(a)
    if (((c.overflowY === 'hidden' || c.overflowY === 'clip') && a.scrollTop > 0) || ((c.overflowX === 'hidden' || c.overflowX === 'clip') && a.scrollLeft > 0)) return a } return null }
  const reach = async (e) => {
    for (const block of ['center', 'nearest', 'start', 'end']) {
      e.scrollIntoView({ block, inline: 'nearest' }); await frame()
      const a = clipScrolled(e); if (a) { a.scrollTop = 0; a.scrollLeft = 0; await frame(); continue }
      if (topmost(e)) return true
    }
    const max = s.scrollHeight - s.clientHeight
    for (let y = 0; y <= max + 5; y += 24) { s.scrollTop = Math.min(y, max); await frame(); if (topmost(e)) return true }
    return false
  }
  // A file input is drawn hidden inside its label (the "Choose image" button): the label is what is clicked.
  const ctls = [...s.querySelectorAll('button, select, input, textarea')].filter((e) => e.type !== 'file' && vis(e))
  const files = [...s.querySelectorAll('input[type=file]')].map((e) => e.closest('label')).filter((l) => l && vis(l))
  window.__probe = { s, frame, topmost, reach, name, list: [...ctls, ...files] }
  return window.__probe.list.length
})()`
const REACH_ONE = (i) => `(async () => { const P = window.__probe; const e = P.list[${i}]; return (await P.reach(e)) ? null : P.name(e) })()`
const REACH_END = `(async () => {
  const { s, frame, topmost, reach } = window.__probe
  const alarms = [], notes = []
  const warn = s.querySelector('.sstv-tx-notice.warn')
  if (!warn) alarms.push('the composer warning never appeared')
  else if (!(await reach(warn))) alarms.push('the composer warning is hidden')
  const alert = s.querySelector(':scope > .sstv-tx-bar [role=alert]')
  if (!alert) alarms.push('the bar alert never appeared')
  else {
    for (const where of ['top', 'end']) {
      s.scrollTop = where === 'top' ? 0 : s.scrollHeight; await frame()
      if (!topmost(alert)) alarms.push('the bar alert is hidden at the ' + where + ' of the scroll')
      for (const [sel, what] of [[':scope > .cockpit-txstrip .op-btn.stop', 'Stop TX'], [':scope > .sstv-tx-bar .sstv-tx-stop', 'Stop']]) {
        const b = s.querySelector(sel); if (b && !topmost(b)) (what === 'Stop' ? alarms : notes).push('with the bar alert up, ' + what + ' is covered or off screen at the ' + where + ' of the scroll')
      }
    }
  }
  const body = s.querySelector('[data-pane="txcompose"] .pane-body')
  const sideways = body && body.scrollWidth > body.clientWidth + 1 ? 'the composer runs ' + (body.scrollWidth - body.clientWidth) + ' px past its pane sideways' : null
  s.scrollTop = 0
  for (const c of s.querySelectorAll('.cockpit-col, .pane-body')) c.scrollTop = 0
  await frame()
  return { alarms, notes, sideways, page: Math.max(0, document.scrollingElement.scrollHeight - innerHeight) }
})()`

/** Check 6: the meters pane whole on screen and topmost, and its readouts, at some position of the shell's scroll and of
 *  the column it stands in (every 8 px of the shell's, every 24 of the column's). Synchronous: a hit test lays out. */
const METERS_WHOLE = `(() => {
  const s = document.querySelector(${JSON.stringify(SEL)})
  const pane = s && s.querySelector('[data-pane="txMetersPane"]')
  if (!pane) return { pane: false }
  const col = pane.closest('.cockpit-col')
  const reads = [...pane.querySelectorAll('.txmeters-pane-value, .txmeters-pane-label')]
  const hits = (e, x, y) => { if (x < 0 || y < 0 || x >= innerWidth || y >= innerHeight) return false; const t = document.elementFromPoint(x, y); return t != null && (t === e || e.contains(t)) }
  const whole = () => { const r = pane.getBoundingClientRect(); const x = r.left + r.width / 2
    return r.top >= 0 && r.bottom <= innerHeight && hits(pane, x, r.top + 2) && hits(pane, x, r.bottom - 2) &&
      reads.every((e) => { const q = e.getBoundingClientRect(); return hits(e, q.left + q.width / 2, q.top + q.height / 2) }) }
  const saved = [s.scrollTop, col ? col.scrollTop : 0]
  let found = null
  const cmax = col ? col.scrollHeight - col.clientHeight : 0, smax = s.scrollHeight - s.clientHeight
  for (let c = 0; c <= cmax + 23 && !found; c += 24) {
    if (col) col.scrollTop = Math.min(c, cmax)
    for (let y = 0; y <= smax + 7 && !found; y += 8) { s.scrollTop = Math.min(y, smax); if (whole()) found = [s.scrollTop, col ? col.scrollTop : 0] }
  }
  s.scrollTop = saved[0]; if (col) col.scrollTop = saved[1]
  const head = s.querySelector('.cockpit-header')
  return { pane: true, reads: reads.length, found, h: Math.round(pane.getBoundingClientRect().height), pin: head ? head.getAttribute('data-pin') : null }
})()`

/** Load a small picture into the composer the way a drop does (an upscale, so the composer warns). */
const DROP = `(async () => {
  const c = document.createElement('canvas'); c.width = 96; c.height = 72
  const g = c.getContext('2d'); g.fillStyle = '#3a6'; g.fillRect(0, 0, 96, 72); g.fillStyle = '#fff'; g.fillRect(10, 10, 40, 20)
  const blob = await new Promise((r) => c.toBlob(r, 'image/png'))
  const dt = new DataTransfer(); dt.items.add(new File([blob], 'probe.png', { type: 'image/png' }))
  const zone = document.querySelector(${JSON.stringify(SEL)} + ' .sstv-tx-drop')
  zone.dispatchEvent(new DragEvent('drop', { dataTransfer: dt, bubbles: true, cancelable: true }))
  for (let i = 0; i < 60; i++) { await new Promise((r) => setTimeout(r, 100)); if (document.querySelector('.sstv-tx-notice.warn')) return true }
  return false
})()`

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-sstvlayout-probe-'))
const server = await createServer({
  root: UI, configFile: join(UI, 'vite.config.ts'), cacheDir, logLevel: 'warn', plugins: [swap],
  server: { host: '127.0.0.1', port: 0, strictPort: false, hmr: false },
})
await server.listen()
const origin = `http://127.0.0.1:${server.httpServer.address().port}`
const browser = await chrome()

async function run([cls, W, H, scale, D = 1], lang, plant = null) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text)
    return r.result.value
  }
  const key = (k, code, keyCode) => [['keyDown', 'rawKeyDown'], ['keyUp', 'keyUp']].reduce((p, [, type]) =>
    p.then(() => call('Input.dispatchKeyEvent', { type, key: k, code, windowsVirtualKeyCode: keyCode })), Promise.resolve())
  const settle = async () => {
    let last = null
    for (let i = 0; i < 20; i++) { await sleep(150); const now = await evaluate(STOPS); if (last && JSON.stringify(now) === JSON.stringify(last)) return now; last = now }
    return last
  }
  try {
    await call('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: D, mobile: false })
    await call('Page.navigate', { url: `${origin}/probe-blank` })
    await sleep(300)
    const ls = { 'nexus.workspace': 'dx', 'nexus.features.v1': FEATURES, 'tempo-theme': 'dark', 'nexus-ui-scale-mode': scale, 'nexus.locale': lang }
    await evaluate(`localStorage.clear(); ${Object.entries(ls).map(([k, v]) => `localStorage.setItem(${JSON.stringify(k)}, ${JSON.stringify(v)})`).join(';')}; 1`)
    await call('Page.navigate', { url: `${origin}/#sstv` })
    let drew = false
    for (let i = 0; i < 240 && !drew; i++) {
      await sleep(250)
      drew = await evaluate(`(() => { const s = document.querySelector(${JSON.stringify(SEL)}); return !!(s && s.getBoundingClientRect().height > 1 && s.querySelector('[data-pane="txcompose"]') && s.querySelector(':scope > .cockpit-txstrip .op-btn.stop') && document.documentElement.getAttribute('data-viewport')) })()`).catch(() => false)
    }
    if (!drew) return null
    if (plant) await evaluate(`(() => { const s = document.createElement('style'); s.textContent = ${JSON.stringify(plant)}; document.head.appendChild(s); return 1 })()`)
    const m = { vp: await evaluate('document.documentElement.getAttribute("data-viewport")'), moved: [] }
    const base = await settle()
    m.base = base
    // The large meters pane, ticked the operator's way: the strip's button from md up, ⊞ Panels below.
    const title = await label(lang)
    m.metersVia = await evaluate(`(async () => {
      const s = document.querySelector(${JSON.stringify(SEL)})
      const t = s.querySelector(':scope > .cockpit-txstrip .txmeters-pane-toggle')
      if (t) { t.click(); return 'strip' }
      s.querySelector('.panels-menu-btn').click()
      await new Promise((r) => setTimeout(r, 300))
      const box = [...document.querySelectorAll('.panels-menu-pop label')].find((l) => l.textContent.trim() === ${JSON.stringify(title)})?.querySelector('input')
      if (box) box.click()
      await new Promise((r) => setTimeout(r, 100))
      // Closed with its own button (Escape is also the app's halt key, which the stub api answers with no
      // snapshot), and checked closed: an open menu stands over the header and the strip.
      const btn = s.querySelector('.panels-menu-btn')
      if (btn.getAttribute('aria-expanded') === 'true') btn.click()
      await new Promise((r) => setTimeout(r, 200))
      if (btn.getAttribute('aria-expanded') === 'true') return 'stuck open'
      btn.blur()
      return box ? 'menu' : 'none'
    })()`)
    await sleep(300)
    m.meters = await evaluate(`!!document.querySelector(${JSON.stringify(SEL)} + ' [data-pane="txMetersPane"]')`)
    if (!(await evaluate(`!!document.querySelector(${JSON.stringify(SEL)})`))) {
      m.gone = await evaluate('document.body.innerText.slice(0, 400)')
      return m
    }
    const after = [['the meters pane', await settle()]]
    // Check 6, as the operator first sees the pane: ticked, before a picture or an alert grows the composer or the bar.
    m.metersWhole = await evaluate(METERS_WHOLE)
    m.dropped = await evaluate(DROP)
    after.push(['a picture loaded, with its warning', await settle()])
    for (const [what, now] of after) for (const k of ['stopTx', 'stop']) if (now?.[k] !== base?.[k]) m.moved.push(`${k === 'stop' ? 'Stop' : 'Stop TX'} moved after ${what} (${base?.[k]} → ${now?.[k]})`)
    m.order = await evaluate(ORDER)
    const n = await evaluate(REACH_SETUP)
    const unreached = []
    for (let i = 0; i < n; i++) { const miss = await evaluate(REACH_ONE(i)); if (miss) unreached.push(miss) }
    // The bar's alert: the engine's own sentence for a dropped picture.
    await evaluate(`(() => { window.__sstvOver = { txNotice: ${JSON.stringify(DROPPED)} }; return 1 })()`)
    for (let i = 0; i < 30; i++) { await sleep(100); if (await evaluate(`!!document.querySelector(${JSON.stringify(SEL)} + ' > .sstv-tx-bar [role=alert]')`)) break }
    m.reach = { n, unreached, ...(await evaluate(REACH_END)) }
    await evaluate('window.__sstvOver = undefined; 1')
    await sleep(1200)
    // Check 5: the waterfall's divider, by real keys.
    m.divider = await (async () => {
      const at = await evaluate(`(() => { const s = document.querySelector(${JSON.stringify(SEL)}); s.scrollTop = 0; const d = s.querySelector(':scope > .pane-splitter'); const st = s.querySelector(':scope > .sstv-canvas'); if (!d || !st) return null; if (getComputedStyle(d).display === 'none') return { hidden: true }; d.focus({ preventScroll: true }); return { h: Math.round(st.getBoundingClientRect().height), travel: Number(d.getAttribute('aria-valuemax')) - Number(d.getAttribute('aria-valuemin')) } })()`)
      if (at == null) return { none: true }
      if (at.hidden) return { hidden: true }
      const travel = at.travel
      // Up: a stock stage on a short window already stands at the most its neighbours' floors leave it.
      const h = () => evaluate(`Math.round(document.querySelector(${JSON.stringify(SEL)} + ' > .sstv-canvas').getBoundingClientRect().height)`)
      for (let i = 0; i < 3; i++) await key('ArrowUp', 'ArrowUp', 38)
      await sleep(200)
      let grown = await h()
      // …and down, where the stage already stood at the bottom of its range.
      if (grown === at.h) { for (let i = 0; i < 3; i++) await key('ArrowDown', 'ArrowDown', 40); await sleep(200); grown = await h() }
      await key('Backspace', 'Backspace', 8)
      await sleep(200)
      const back = await evaluate(`Math.round(document.querySelector(${JSON.stringify(SEL)} + ' > .sstv-canvas').getBoundingClientRect().height)`)
      return { at: at.h, grown, back, travel }
    })()
    return m
  } finally {
    await browser.call('Target.closeTarget', { targetId })
  }
}

function reds(name, m) {
  const out = []
  if (m.gone !== undefined) return [`${name}: the SSTV screen went away after the meters pane was ticked: ${JSON.stringify(m.gone)}`]
  if (m.metersVia === 'none' || !m.meters) out.push(`${name}: the large meters pane could not be ticked (${m.metersVia})`)
  if (!m.dropped) out.push(`${name}: the picture never loaded into the composer`)
  for (const r of m.moved) out.push(`${name}: ${r}`)
  for (const r of m.order.out) out.push(`${name}: ${r}`)
  if (m.reach.page > 0) out.push(`${name}: the page scrolls ${m.reach.page} px`)
  if (m.reach.unreached.length) out.push(`${name}: never on screen and topmost: ${m.reach.unreached.join(', ')}`)
  for (const r of m.reach.alarms) out.push(`${name}: ${r}`)
  if (m.reach.sideways) out.push(`${name}: ${m.reach.sideways}`)
  if (m.vp !== 'xs' && m.metersWhole?.pane && !m.metersWhole.found) out.push(`${name}: the meters pane (${m.metersWhole.h} px) is never on screen whole with its readouts, at any scroll (header pin ${m.metersWhole.pin})`)
  if (m.divider.none) out.push(`${name}: no waterfall divider`)
  // sm and xs hide every divider in the app (styles.css, the narrow-window rule), SSTV's included, as before.
  else if (m.divider.hidden) { if (!['xs', 'sm'].includes(m.vp)) out.push(`${name}: the waterfall divider is hidden at ${m.vp}`) }
  else {
    // A divider with no travel is the layout's own answer on a window with no room to give (its range is the
    // neighbours' floors); printed, and a red only where the divider offered travel and the waterfall did not move.
    if (m.divider.travel > 0 && m.divider.grown === m.divider.at) out.push(`${name}: the divider did not size the waterfall (${m.divider.at} → ${m.divider.grown})`)
    if (Math.abs(m.divider.back - m.divider.at) > 1) out.push(`${name}: Backspace did not give the waterfall back (${m.divider.at} → ${m.divider.back})`)
  }
  return out
}

const tagOf = ([cls, W, H, s], lang) => `${cls} ${W}×${H} ${s} % ${lang}`
let code = 0
const red = (msg) => {
  console.error(`sstvlayout-probe: RED — ${msg}`)
  code = 1
}
try {
  for (const size of SIZES.filter((z) => !ONLY || (z[0] === ONLY[0] && z[3] === ONLY[1]))) {
    for (const lang of LANGS.filter((l) => !ONLY || l === ONLY[2])) {
      const name = tagOf(size, lang)
      const m = await run(size, lang)
      if (!m) {
        console.error(`sstvlayout-probe: ${name} never drew`)
        process.exitCode = 2
        throw new Error('no screen')
      }
      if (m.vp !== size[0]) red(`${name}: the window classed as ${m.vp}, not ${size[0]} — the size table is wrong`)
      if (m.gone !== undefined) { for (const r of reds(name, m)) red(r); continue }
      console.log(`sstvlayout-probe: ${name.padEnd(26)} composer ${m.order.reads}, meters via ${m.metersVia} (whole at scroll ${JSON.stringify(m.metersWhole?.found ?? null)}, header pin ${m.metersWhole?.pin}), ${m.reach.n} controls, ` +
        `${m.reach.unreached.length} unreachable, ${m.moved.length} stop moves, ${m.reach.alarms.length} hidden alarms, ${m.divider.hidden ? 'divider hidden (sm/xs)' : `divider ${m.divider.at}→${m.divider.grown}→${m.divider.back} (travel ${m.divider.travel} px)`}`)
      for (const n of m.reach.notes) console.log(`sstvlayout-probe:   note: ${n}`)
      for (const r of reds(name, m)) red(r)
    }
  }
  // THE CONTROLS.
  const controls = [
    ['the composer ordered above the gallery', ['md', 1366, 768, '100'], '[data-pane="txcompose"] { order: -1 }', (r) => r.includes('not below the gallery')],
    ['a region sized by its content', ['md', 1366, 768, '100'], '.cockpit-panes { flex: 0 0 auto !important; min-height: 0 !important }', (r) => r.includes('moved after')],
    ['a composer body clipped to two lines', ['md', 1366, 768, '100'], '[data-pane="txcompose"] .pane-body { overflow: hidden !important; max-height: 2.5em !important }', (r) => r.includes('never on screen') || r.includes('hidden')],
    // Check 6's: an opaque band over all of the window but its bottom 120 px (a parked surface too tall for the pane, as
    // the pinned header was): no meters pane (186 px here since the half-size dials, 245 in 1.24.0) is whole in what is
    // left. Sized from the window's foot, not as a share of it: a 60 % band left 307 px and stopped failing once the
    // dials' pane shrank to 186 (main 932c1c882, review round 2).
    ['an opaque band over the window down to its last 120 px', ['sm', 1366, 768, '175'], 'body::after { content: ""; position: fixed; left: 0; right: 0; top: 0; height: calc(100% - 120px); z-index: 99999; background: #000 }', (r) => r.includes('meters pane')],
  ]
  for (const [what, size, css, hit] of ONLY && ONLY[0] !== 'controls' ? [] : controls) {
    const m = await run(size, 'en', css)
    const caught = m ? reds(`${tagOf(size, 'en')} (planted: ${what})`, m).filter(hit) : []
    if (caught.length === 0) red(`CONTROL FAILED — ${what} was not caught`)
    else console.log(`sstvlayout-probe: CONTROL — ${caught[0]} (caught)`)
  }
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
if (code) process.exitCode = code
else if (!process.exitCode) console.log('sstvlayout-probe: every check held')
