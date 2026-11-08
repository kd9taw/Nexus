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
// either is not caught, that check cannot fail and the probe says so instead of passing.
//
// ONE KNOWN CASE (`KNOWN` below), found by this probe and on main before it: at 700×800 the Logbook's top bar wraps,
// its TX cluster stands under the band pill, and the pill's menu is DRAWN over every point of Stop TX, so the click
// lands on the menu. That is stacking, not modality: it waits for Stop TX to stand above every overlay (the
// stop-above-overlays work), which must turn it green. Until then the case is still run and printed as KNOWN, and it
// fails the probe if anything else is wrong with it (a locked page, a menu that never opened). The day it comes out
// clear the probe says so, so it can be taken off the list.
//
// exit: 0 every case clear · 1 a red, or a control that could not fail · 2 a page never drew Stop TX
import { createServer } from 'vite'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const REAL_API = join(UI, 'src', 'api.ts')
const STUB_SRC = join(UI, 'probe', 'menustop-api.ts')
const STUB_API = join(UI, 'probe', '__menustop-api.ts')
/** [window W, H, the size class it must come out as]. */
const SIZES = [[700, 800, 'xs'], [2560, 1440, 'xl']]
const SCREENS = ['logbook', 'operate']
/** [name, the trigger's selector]: Help is on every screen; the band pill only where the screen draws it. */
const MENUS = [['Help', `button.theme-chip[title="Help"]`], ['band', 'button.freq-channel']]
const LOCK = 'body { pointer-events: none !important } [data-radix-popper-content-wrapper] { pointer-events: auto !important }'
const OVER_ALL = '[data-radix-popper-content-wrapper] { position: fixed !important; inset: 0 !important; transform: none !important } [data-radix-popper-content-wrapper] > [role="menu"] { width: 100vw; height: 100vh; max-height: none !important }'
const STOP = '.op-btn.stop'
/** Cases where an open menu is drawn over Stop TX, known and owned by the stop-above-overlays work (see the header). */
const KNOWN = new Set(['700×800 logbook band'])
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
    if (t && (t === b || b.contains(t))) return [x, y]
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
async function run([W, H], screen, [, trigger], plant) {
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
    await call('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: 1, mobile: false })
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
    const before = await evaluate('window.__probeHalts()')
    await click(point)
    await sleep(500)
    return {
      vp,
      opened: true,
      covered,
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

let code = 0
let ran = 0
try {
  for (const size of SIZES) {
    for (const screen of SCREENS) {
      for (const menu of MENUS) {
        const tag = `${size[0]}×${size[1]} ${screen} ${menu[0]}`
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
        if (KNOWN.has(tag)) {
          if (r.covered && r.onTop !== 'HTML' && r.vp === size[2]) {
            // Its controls are not run: with the menu drawn over the button, every click lands on the menu anyway.
            console.log(`menustop-probe: KNOWN at ${tag} — the open menu is drawn over every point of Stop TX (${r.onTop}); stacking, owned by the stop-above-overlays work`)
            continue
          }
          if (!r.covered) console.log(`menustop-probe: KNOWN CASE NOW CLEAR at ${tag} — take it off KNOWN`)
        }
        const red = []
        if (r.vp !== size[2]) red.push(`the window came out ${r.vp}, not ${size[2]}`)
        // An <html> on top of the button is a page whose pointer events are locked (a modal menu), not a menu over it.
        if (r.covered) red.push(r.onTop === 'HTML' ? 'Stop TX takes no pointer events with the menu open (the page is locked)' : `the open menu covers every point of Stop TX (${r.onTop} is on top)`)
        if (r.halts !== 1) red.push(`one click on Stop TX stopped ${r.halts} times, not once`)
        if (!r.closed) red.push('the menu is still open after the click')
        if (!r.focusOnStop) red.push('the keyboard is not on Stop TX after the click')
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
  if (ran === 0) {
    console.error('menustop-probe: no case ran')
    code = code || 1
  }
  if (code === 0) console.log(`menustop-probe: PASS — ${ran} cases (known: ${[...KNOWN].join(', ')}): with Help or the band menu open, one real click on Stop TX stopped once, closed the menu and left the keyboard on Stop TX, at ${SIZES.map(([w, h, v]) => `${w}×${h} (${v})`).join(' and ')} on ${SCREENS.join(' and ')}; each planted modal lock and covering menu caught`)
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
