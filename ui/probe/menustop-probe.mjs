// ONE CLICK ON STOP TX WITH A MENU OPEN, in real Chrome: `scripts/browser-probe --desktop menustop`.
//
// A Radix menu is modal by default, and a modal menu locks the page's pointer events while it is open, so one click
// on Stop TX only closed the menu and stopped nothing (Help open: halts +0, measured). Every menu now opens through
// ui/src/components/ui/MenuParts.tsx, which is never modal; jsdom cannot hit-test, so this is where the click itself
// is proved. The probe serves the REAL app (index.html, main.tsx, App) with the app's own Vite config and src/api.ts
// swapped for probe/menustop-api.ts, which counts `haltTx`. On the Logbook (whose top bar carries Stop TX) and on
// Operate (whose strip does), at 700×800 (xs) and 2560×1440 (xl) at a pinned 100 %, it opens each menu with a REAL
// click — Help, and the band pill's menu where the screen draws one — and asserts:
//   1. THE MENU OPENED and leaves Stop TX clickable: on a 9 × 5 grid across the button, at least one point is the
//      button itself with the menu open;
//   2. ONE REAL CLICK on that point stops: halts +1, and the menu is closed by it;
//   3. THE KEYBOARD STAYS WHERE THE CLICK PUT IT: Stop TX is focused afterwards, not the menu's button.
// THE CONTROLS, run at every case: the same page with the modal menu's lock planted (`pointer-events: none` on
// <body>, the menu's own layer left live, as Radix's modal menu does it), where the same click must stop NOTHING
// (halts +0); and with the open menu planted over the whole window, where check 1 must see Stop TX covered. If
// either is not caught, that check cannot fail and the probe says so instead of passing. Both run with the stop layer
// taken away (StopLayer.tsx), which beats a lock and a covering menu alike by design.
//
// A MENU DRAWN OVER STOP TX. At 700×800 the Logbook's top bar wraps, its TX cluster stands under the band pill, and
// the pill's menu is drawn over Stop TX. That case was KNOWN here until Stop TX stood above every overlay
// (components/StopLayer.tsx): Stop TX's mirror in the stop layer now counts as Stop TX for check 1, and a click on
// the mirror stops, leaves the menu open and moves no focus (the stop layer's rule), so for that click checks 2 and 3
// read "stopped once, the menu still open, the keyboard where it was" instead.
//
// ANY PART OF STOP TX COVERED IS COVERED. A menu can stand over part of Stop TX and leave its centre free (the band
// menu over 41 % of it, the centre the button's own, was seen at 1366×768 @175 %), and a press on the covered part
// then lands on the menu. The rule the shell probes hold a stop control to is: covered when its centre is not its
// own OR under 90 % of the points of a 9 × 5 grid across it are its own. With the stop layer on, the points that are
// Stop TX's mirror count as its own. So, on FT (Operate) at 1366×768 @175 %, 1920×1080 and 2560×1440:
//   4. EVERY MENU AND POPOVER the screen offers (each `[aria-haspopup]` trigger, scrolled into sight as an operator
//      scrolls, step by step, and opened by a
//      real click) leaves every stop control on screen clear by that rule; the share of each one the overlay itself
//      covers is printed. A trigger that a click reaches and that opens nothing is a red: it measured nothing. One no
//      scroll position brings wholly into the window and on top is a counted skip, named in the result;
//   5. A MENU PLANTED OVER THE LEFT 40 % OF STOP TX, its centre left free, gets a mirror: Stop TX is clear by the rule,
//      and one real click on a covered point stops once and leaves the menu open.
// THE CONTROL: the same plant with the stop layer taken away must come out covered, or check 5 could not fail.
//
// exit: 0 every case clear · 1 a red, or a control that could not fail · 2 a page never drew Stop TX
import { createServer } from 'vite'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'
import { FHD, FLOOR, QHD, metrics, offMatrix } from './sizes.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const REAL_API = join(UI, 'src', 'api.ts')
const STUB_SRC = join(UI, 'probe', 'menustop-api.ts')
const STUB_API = join(UI, 'probe', '__menustop-api.ts')
/** [window (probe/sizes.mjs), the size class it must come out as]. 700×800 is below any supported screen — the xs
 *  layout lives only in a window dragged that small (no supported screen reaches xs at the app's 100 %). */
const SIZES = [[offMatrix(700, 800), 'xs'], [QHD, 'xl']]
const SCREENS = ['logbook', 'operate']
/** [name, the trigger's selector]: Help is on every screen; the band pill only where the screen draws it. */
const MENUS = [['Help', `button.theme-chip[title="Help"]`], ['band', 'button.freq-channel']]
// Each control takes the stop layer away too: with it, Stop TX beats a lock or a covering menu, which is its job
// (stopabove-probe.mjs proves that half), and the control would prove nothing about this probe's checks.
const NO_LAYER = ' .stop-layer { display: none !important }'
const LOCK = 'body { pointer-events: none !important } [data-radix-popper-content-wrapper] { pointer-events: auto !important }' + NO_LAYER
const OVER_ALL = '[data-radix-popper-content-wrapper] { position: fixed !important; inset: 0 !important; transform: none !important } [data-radix-popper-content-wrapper] > [role="menu"] { width: 100vw; height: 100vh; max-height: none !important }' + NO_LAYER
const STOP = '.op-btn.stop'
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

/** The centre of the first visible element matching `sel`, or null. */
const centre = (sel) => `(() => {
  const el = [...document.querySelectorAll(${JSON.stringify(sel)})].find((e) => e.getBoundingClientRect().width > 0 && e.closest('[hidden]') == null)
  if (!el) return null
  const q = el.getBoundingClientRect()
  return [q.left + q.width / 2, q.top + q.height / 2]
})()`
/** A point of the visible Stop TX that is the topmost thing where it stands, searched on a 9 × 5 grid. */
const STOP_POINT = `(() => {
  const b = [...document.querySelectorAll(${JSON.stringify(STOP)})].find((e) => e.getBoundingClientRect().width > 0 && e.closest('[hidden]') == null)
  if (!b) return null
  const q = b.getBoundingClientRect()
  for (let j = 1; j <= 5; j++) for (let i = 1; i <= 9; i++) {
    const x = q.left + (q.width * i) / 10, y = q.top + (q.height * j) / 6
    if (x < 0 || y < 0 || x > innerWidth || y > innerHeight) continue
    const t = document.elementFromPoint(x, y)
    // Stop TX's mirror in the stop layer is Stop TX, where an overlay covers the button (StopLayer.tsx).
    if (t && (t === b || b.contains(t) || t.closest('.stop-layer [data-stop-mirror]'))) return [x, y]
  }
  return 'covered'
})()`

const realSrc = await readFile(REAL_API, 'utf8')
const stubSrc = await readFile(STUB_SRC, 'utf8')
const names = [...realSrc.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1])
const own = new Set([...stubSrc.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1]))
const stubCode =
  stubSrc +
  `\nexport * from ${JSON.stringify(REAL_API)}\n` +
  names.filter((n) => !own.has(n)).map((n) => `export async function ${n}(..._a: unknown[]): Promise<any> { return [] }`).join('\n')
const swap = {
  name: 'menustop-probe-api',
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

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-menustop-probe-'))
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

/** One case: open `menu` on `screen` at `size` with a real click, then one real click on Stop TX. `plant` is
 *  null, 'lock' (the modal menu's pointer lock) or 'cover' (the menu over the whole window). */
async function run([win], screen, [, trigger], plant) {
  const style = (css) => `(() => { const s = document.createElement('style'); s.textContent = ${JSON.stringify(css)}; document.head.appendChild(s); return 1 })()`
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text)
    return r.result.value
  }
  const until = async (expression, tries = 80) => {
    for (let i = 0; i < tries; i++) {
      await sleep(200)
      const v = await evaluate(expression).catch(() => null)
      if (v) return v
    }
    return null
  }
  const click = async ([x, y]) => {
    await call('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y })
    await call('Input.dispatchMouseEvent', { type: 'mousePressed', x, y, button: 'left', clickCount: 1 })
    await call('Input.dispatchMouseEvent', { type: 'mouseReleased', x, y, button: 'left', clickCount: 1 })
  }
  try {
    await call('Emulation.setDeviceMetricsOverride', metrics(win))
    await call('Page.navigate', { url: `${origin}/probe-blank` })
    await sleep(300)
    await evaluate(`localStorage.clear(); localStorage.setItem('nexus.workspace', 'dx'); localStorage.setItem('tempo-theme', 'dark'); localStorage.setItem('nexus.features.wizardSeen', '1'); localStorage.setItem('nexus-ui-scale-mode', '100'); 1`)
    await call('Page.navigate', { url: `${origin}/#${screen}` })
    if (!(await until(`(${STOP_POINT}) != null && document.documentElement.getAttribute('data-viewport') != null`))) return { error: 'Stop TX never drew' }
    await sleep(800)
    const vp = await evaluate(`document.documentElement.getAttribute('data-viewport')`)
    const at = await evaluate(centre(trigger))
    if (!at) return { vp, absent: true }
    await click(at)
    const opened = await until(`document.querySelector('[role="menu"]') != null`, 20)
    if (!opened) return { vp, error: 'the menu did not open' }
    await sleep(300)
    if (plant === 'cover') {
      await evaluate(style(OVER_ALL))
      await sleep(200)
    }
    const found = await evaluate(STOP_POINT)
    if (found == null) return { vp, opened: true, error: 'Stop TX is gone with the menu open' }
    // Where the menu and the button stand, and what is on top of the button's centre: printed for a red, and the
    // centre is where a covered button is clicked, so the red also says what that click did.
    const boxes = await evaluate(`(() => {
      const box = (el) => { if (!el) return null; const q = el.getBoundingClientRect(); return [q.left, q.top, q.width, q.height].map(Math.round) }
      const b = [...document.querySelectorAll(${JSON.stringify(STOP)})].find((e) => e.getBoundingClientRect().width > 0 && e.closest('[hidden]') == null)
      const q = b.getBoundingClientRect()
      const top = document.elementFromPoint(q.left + q.width / 2, q.top + q.height / 2)
      return { stop: box(b), menu: box(document.querySelector('[role="menu"]')), onTop: top ? top.className || top.tagName : null, mid: [q.left + q.width / 2, q.top + q.height / 2] }
    })()`)
    const covered = found === 'covered'
    const point = covered ? boxes.mid : found
    if (plant === 'lock') await evaluate(style(LOCK))
    const mirrored = await evaluate(`(() => { const t = document.elementFromPoint(${point[0]}, ${point[1]}); return t != null && t.closest('.stop-layer [data-stop-mirror]') != null })()`)
    const before = await evaluate('window.__probeHalts()')
    await click(point)
    await sleep(500)
    return {
      vp,
      opened: true,
      covered,
      mirrored,
      ...(covered ? { stop: boxes.stop, menu: boxes.menu, onTop: boxes.onTop } : {}),
      point: point.map(Math.round),
      halts: (await evaluate('window.__probeHalts()')) - before,
      closed: await evaluate(`document.querySelector('[role="menu"]') == null`),
      focusOnStop: await evaluate(`document.activeElement != null && document.activeElement.matches(${JSON.stringify(STOP)})`),
    }
  } finally {
    await browser.call('Target.closeTarget', { targetId })
  }
}


/** COVERAGE: the sizes and the screen of checks 4 and 5. */
const COVER_SIZES = [FLOOR, FHD, QHD]
/** Every visible stop control against the 90 % rule: the share of a 9 × 5 grid across its visible part that is
 *  its own, under the overlay alone (`raw`, the stop layer's mirrors looked through) and as a click would find it
 *  (`eff`, a mirror counting as the button), and whether its centre is its own each way. */
const COVERAGE = `(() => {
  const layer = document.querySelector('.stop-layer')
  const inLayer = (e) => layer != null && layer.contains(e)
  return [...document.querySelectorAll('[data-stop-control]')]
    .filter((e) => {
      // On screen: a stop control scrolled out of the window is not one an overlay can cover.
      const q = e.getBoundingClientRect()
      return q.width > 0 && e.closest('[hidden]') == null && q.right > 0 && q.bottom > 0 && q.left < innerWidth && q.top < innerHeight
    })
    .map((b) => {
      const q = b.getBoundingClientRect()
      const l = Math.max(q.left, 0), t = Math.max(q.top, 0), r = Math.min(q.right, innerWidth), btm = Math.min(q.bottom, innerHeight)
      const at = (x, y) => {
        const all = document.elementsFromPoint(x, y)
        const under = all.find((e) => !inLayer(e))
        const raw = under != null && (under === b || b.contains(under))
        const top = all[0]
        const eff = top != null && (inLayer(top) ? top.closest('[data-stop-mirror]') != null : raw)
        return [raw, eff]
      }
      let raw = 0, eff = 0, n = 0
      for (let j = 1; j <= 5; j++) for (let i = 1; i <= 9; i++) {
        const [a, c] = at(l + ((r - l) * i) / 10, t + ((btm - t) * j) / 6)
        n += 1; raw += a ? 1 : 0; eff += c ? 1 : 0
      }
      const [cr, ce] = at((l + r) / 2, (t + btm) / 2)
      return { stop: b.getAttribute('data-stop-control'), box: [q.left, q.top, q.width, q.height].map(Math.round),
        raw: Math.round((raw / n) * 100), eff: Math.round((eff / n) * 100), centreRaw: cr, centreEff: ce,
        mirror: layer != null && layer.querySelector('[data-stop-mirror]') != null }
    })
})()`
/** The menu and popover triggers on screen, by their names. */
const TRIGGERS = `[...document.querySelectorAll('[aria-haspopup]:not([aria-haspopup="false"])')]
  .filter((e) => e.getBoundingClientRect().width > 0 && e.closest('[hidden]') == null && !e.disabled)`
/** Clear by the rule: centre its own and at least 90 % of the grid. */
const clear = (s) => s.centreEff && s.eff >= 90

/** One coverage case on Operate at `win`: open trigger number `k` (or, with `plant`, the band menu and plant it over
 *  the left 40 % of Stop TX) and measure. `plant` is null, 'part' or 'part-nolayer' (the control). */
async function cover(win, k, plant) {
  const style = (css) => `(() => { const s = document.createElement('style'); s.textContent = ${JSON.stringify(css)}; document.head.appendChild(s); return 1 })()`
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text)
    return r.result.value
  }
  const until = async (expression, tries = 80) => {
    for (let i = 0; i < tries; i++) {
      await sleep(200)
      const v = await evaluate(expression).catch(() => null)
      if (v) return v
    }
    return null
  }
  const click = async ([x, y]) => {
    await call('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y })
    await call('Input.dispatchMouseEvent', { type: 'mousePressed', x, y, button: 'left', clickCount: 1 })
    await call('Input.dispatchMouseEvent', { type: 'mouseReleased', x, y, button: 'left', clickCount: 1 })
  }
  try {
    await call('Emulation.setDeviceMetricsOverride', metrics(win))
    await call('Page.navigate', { url: `${origin}/probe-blank` })
    await sleep(300)
    await evaluate(`localStorage.clear(); localStorage.setItem('nexus.workspace', 'dx'); localStorage.setItem('tempo-theme', 'dark'); localStorage.setItem('nexus.features.wizardSeen', '1'); localStorage.setItem('nexus-ui-scale-mode', '100'); 1`)
    await call('Page.navigate', { url: `${origin}/#operate` })
    if (!(await until(`(${STOP_POINT}) != null && document.documentElement.getAttribute('data-viewport') != null`))) return { error: 'Stop TX never drew' }
    await sleep(800)
    const names = await evaluate(`${TRIGGERS}.map((e) => (e.getAttribute('aria-label') || e.getAttribute('title') || e.textContent || e.className).trim().slice(0, 40))`)
    const sel = plant ? `[...document.querySelectorAll('button.freq-channel')].filter((e) => e.getBoundingClientRect().width > 0)` : TRIGGERS
    // The trigger is brought into sight first, the way the operator does it: by scrolling, not by scrollIntoView or
    // focus(). FT's TX strip is sticky (only its first line, with Stop TX, is parked at the window's foot), so a
    // scrollIntoView from the top measures the parked strip and lands short. Each scroll container the trigger sits in,
    // innermost first, is stepped from its top to its end, a frame at each step, until the trigger is wholly in the
    // window and the topmost thing at its centre; it is clicked there.
    const at = await evaluate(`(async () => {
      const e = ${sel}[${plant ? 0 : k}]
      if (!e) return null
      const frame = () => new Promise((r) => requestAnimationFrame(() => r()))
      const seen = () => {
        const q = e.getBoundingClientRect()
        if (q.width <= 0 || q.top < 0 || q.left < 0 || q.bottom > innerHeight || q.right > innerWidth) return null
        const x = q.left + q.width / 2, y = q.top + q.height / 2
        const t = document.elementFromPoint(x, y)
        return t != null && (t === e || e.contains(t)) ? [x, y] : null
      }
      let point = seen()
      for (let n = e.parentElement; !point && n; n = n.parentElement) {
        const oy = getComputedStyle(n).overflowY
        if (!/auto|scroll/.test(oy) || n.scrollHeight <= n.clientHeight) continue
        const max = n.scrollHeight - n.clientHeight
        for (let y = 0; !point && y <= max + 8; y += 8) {
          n.scrollTop = Math.min(y, max)
          await frame()
          point = seen()
        }
      }
      return { point, onTop: point != null }
    })()`)
    await sleep(300)
    if (!at) return { names, absent: true }
    if (!at.onTop) return { names, name: names[k], unreachable: true }
    await click(at.point)
    await sleep(500)
    const opened = await evaluate(`document.querySelector('[role="menu"], [role="dialog"], [role="group"].panels-menu-pop') != null`)
    if (plant) {
      if (!opened) return { names, error: 'the band menu did not open' }
      // The open menu, moved over the left 40 % of Stop TX (and 4 px round it), its centre left free.
      const box = await evaluate(`(() => { const b = [...document.querySelectorAll(${JSON.stringify(STOP)})].find((e) => e.getBoundingClientRect().width > 0 && e.closest('[hidden]') == null); const q = b.getBoundingClientRect(); return [q.left, q.top, q.width, q.height] })()`)
      const [l, t, w, h] = box
      await evaluate(style(`[data-radix-popper-content-wrapper] { position: fixed !important; left: ${l - 4}px !important; top: ${t - 4}px !important; width: ${w * 0.4 + 4}px !important; height: ${h + 8}px !important; min-width: 0 !important; transform: none !important; overflow: hidden !important } [data-radix-popper-content-wrapper] > [role="menu"] { width: 100% !important; height: 100% !important; min-width: 0 !important; max-height: none !important; overflow: hidden !important }` + (plant === 'part-nolayer' ? NO_LAYER : '')))
      await sleep(400)
      const stops = await evaluate(COVERAGE)
      const before = await evaluate('window.__probeHalts()')
      await click([l + w * 0.15, t + h / 2])
      await sleep(500)
      return { names, stops, halts: (await evaluate('window.__probeHalts()')) - before, open: await evaluate(`document.querySelector('[role="menu"]') != null`) }
    }
    return { names, name: names[k], opened, stops: await evaluate(COVERAGE) }
  } finally {
    await browser.call('Target.closeTarget', { targetId })
  }
}

let code = 0
let ran = 0
try {
  for (const size of SIZES) {
    for (const screen of SCREENS) {
      for (const menu of MENUS) {
        const tag = `${size[0].name} ${screen} ${menu[0]}`
        const r = await run(size, screen, menu, false)
        console.log(`menustop-probe: ${tag}: ${JSON.stringify(r)}`)
        if (r.error) {
          console.error(`menustop-probe: at ${tag} ${r.error}`)
          code = r.error === 'Stop TX never drew' ? 2 : code || 1
          continue
        }
        if (r.absent) {
          // Help is on every screen; only the band pill may be missing.
          if (menu[0] === 'Help') {
            console.error(`menustop-probe: RED at ${tag} — no Help button`)
            code = code || 1
          }
          continue
        }
        ran += 1
        const red = []
        if (r.vp !== size[1]) red.push(`the window came out ${r.vp}, not ${size[1]}`)
        // An <html> on top of the button is a page whose pointer events are locked (a modal menu), not a menu over it.
        if (r.covered) red.push(r.onTop === 'HTML' ? 'Stop TX takes no pointer events with the menu open (the page is locked)' : `the open menu covers every point of Stop TX (${r.onTop} is on top)`)
        if (r.halts !== 1) red.push(`one click on Stop TX stopped ${r.halts} times, not once`)
        // A click on the mirror leaves the menu open and the keyboard where it was; a click on Stop TX itself closes it.
        if (!r.mirrored && !r.closed) red.push('the menu is still open after the click')
        if (!r.mirrored && !r.focusOnStop) red.push('the keyboard is not on Stop TX after the click')
        if (r.mirrored && r.closed) red.push('the click on the mirror closed the menu')
        if (red.length) {
          console.error(`menustop-probe: RED at ${tag} — ${red.join('; ')}`)
          code = code || 1
          continue
        }
        const c = await run(size, screen, menu, 'lock')
        console.log(`menustop-probe: CONTROL lock ${tag}: ${JSON.stringify(c)}`)
        if (c.halts !== 0) {
          console.error(`menustop-probe: CONTROL FAILED at ${tag} — the planted modal lock did not swallow the click (halts ${c.halts}); check 2 cannot fail`)
          code = code || 1
        }
        const o = await run(size, screen, menu, 'cover')
        console.log(`menustop-probe: CONTROL cover ${tag}: ${JSON.stringify(o)}`)
        if (!o.covered) {
          console.error(`menustop-probe: CONTROL FAILED at ${tag} — a menu planted over the window did not cover Stop TX; check 1 cannot fail`)
          code = code || 1
        }
      }
    }
  }
  // Checks 4 and 5, and check 5's control (see the header).
  let covered = 0
  const skipped = []
  for (const win of COVER_SIZES) {
    const first = await cover(win, 0, null)
    if (first.error) {
      console.error(`menustop-probe: coverage at ${win.name}: ${first.error}`)
      code = code || 1
      continue
    }
    for (let k = 0; k < first.names.length; k++) {
      const r = k === 0 ? first : await cover(win, k, null)
      // A trigger no scroll position brings wholly into the window and on top opens nothing an operator could open:
      // a counted skip.
      if (r.unreachable) {
        skipped.push(`${win.name} "${r.name}"`)
        console.log(`menustop-probe: SKIP ${win.name} "${r.name}": no scroll position brings it into the window, on top`)
        continue
      }
      if (r.error || r.absent) {
        console.error(`menustop-probe: coverage at ${win.name} trigger ${k}: ${r.error ?? 'absent'}`)
        code = code || 1
        continue
      }
      console.log(`menustop-probe: COVER ${win.name} "${r.name}" opened=${r.opened}: ${JSON.stringify(r.stops)}`)
      // A trigger that opened nothing measured nothing: that is a red, never a clear case.
      if (!r.opened) {
        console.error(`menustop-probe: RED at ${win.name} — "${r.name}" opened nothing, so its overlay was never measured`)
        code = code || 1
        continue
      }
      covered += 1
      const bad = r.stops.filter((s) => !clear(s))
      if (bad.length) {
        console.error(`menustop-probe: RED at ${win.name} with "${r.name}" open — ${bad.map((s) => `${s.stop} ${s.eff} % its own, centre ${s.centreEff ? 'its own' : 'lost'}`).join('; ')}`)
        code = code || 1
      }
    }
    const p = await cover(win, 0, 'part')
    console.log(`menustop-probe: PLANT part ${win.name}: ${JSON.stringify(p)}`)
    const tx = p.stops?.find((s) => s.stop === 'stop-tx' && s.raw < 90)
    if (p.error || !tx) {
      console.error(`menustop-probe: plant at ${win.name}: ${p.error ?? 'the planted menu covers no Stop TX'}`)
      code = code || 1
    } else {
      const red = []
      if (!clear(tx)) red.push(`Stop TX is ${tx.eff} % its own with the menu over ${100 - tx.raw} % of it (centre ${tx.centreRaw ? 'free' : 'covered'}): no mirror`)
      if (p.halts !== 1) red.push(`one click on the covered part stopped ${p.halts} times, not once`)
      if (!p.open) red.push('the click on the covered part closed the menu')
      if (red.length) {
        console.error(`menustop-probe: RED at ${win.name}, a menu over part of Stop TX — ${red.join('; ')}`)
        code = code || 1
      }
    }
    const c = await cover(win, 0, 'part-nolayer')
    console.log(`menustop-probe: CONTROL part-nolayer ${win.name}: ${JSON.stringify(c)}`)
    const ctx = c.stops?.find((s) => s.stop === 'stop-tx' && s.raw < 90)
    if (!ctx || clear(ctx)) {
      console.error(`menustop-probe: CONTROL FAILED at ${win.name} — without the stop layer, a menu over 40 % of Stop TX was not caught; check 5 cannot fail`)
      code = code || 1
    }
  }
  // Every case must run: a band pill the probe cannot find (a renamed trigger) would otherwise shrink it to Help.
  const all = SIZES.length * SCREENS.length * MENUS.length
  if (ran !== all) {
    console.error(`menustop-probe: ${ran} of ${all} cases ran`)
    code = code || 1
  }
  if (code === 0) console.log(`menustop-probe: PASS — ${covered} overlays opened on FT at ${COVER_SIZES.map((w) => w.name).join(', ')} left every stop control clear (${skipped.length} skipped, no scroll position reaches them: ${skipped.join(', ') || 'none'}), and a menu over part of Stop TX was mirrored at each; ${ran} cases: with Help or the band menu open, one real click on Stop TX stopped once, closed the menu and left the keyboard on Stop TX, at ${SIZES.map(([s, v]) => `${s.name} (${v})`).join(' and ')} on ${SCREENS.join(' and ')}; each planted modal lock and covering menu caught`)
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
