// The waterfall and scope dividers under a REAL pointer drag in real Chrome, for what jsdom cannot see:
// `scripts/browser-probe --desktop seamdrag`.
//
// THE REPORT (the maintainer on 1.25.0-beta.2, and #549): "I was able to resize the height of the waterfall by
// dragging the bar before, why suddenly not working or so slow?" On FT, since 1.24.0, the divider stood still under
// the pointer for the whole drag and jumped on release: the strip divider's ResizeObserver also watches the strip's
// neighbours past a box-less wrapper, the lower region under the waterfall resized on every move, and its re-fit
// painted the STORED height back over the drag's (components/PaneSeam `dragging`). SSTV's divider had done the same
// since before 1.23, with 30 "ResizeObserver loop" errors a drag. Measured with this probe's own drag: FT and SSTV
// frozen on 1.24.0, 1.25.0-beta.2 and main; Tempo, RTTY and CW following.
//
// The probe serves the REAL app with src/api.ts swapped for probe/waterfalls-api.ts and opens each screen with fresh
// storage at 1366×768 and 1920×1080. Real CDP mouse events (press, 12 moves of 8 px, a frame each, release) and
// asserts:
//   1. THE DIVIDER FOLLOWS THE POINTER: after each move it stands where the pointer is (±1.5 px), or, once it has
//      stopped, it stopped at its range's end: it never moves again, and after the release it is still there and
//      announces its maximum (aria-valuenow = aria-valuemax). FT, Tempo, SSTV, RTTY, CW, Phone, PSK, JTTY and JS8.
//   2. NO "ResizeObserver loop" error during the drag.
//   3. A HEADER CHANGE UNDER A HELD PRESS IS NOT LOST (the review of the fix): FT's waterfall held at its ceiling by a
//      stored 85 %, the cockpit header 28 px taller; the press held without a move while the header goes back, then
//      released. Stop TX must stand where it stands when the header goes back with no press at all (±1 px). A re-fit
//      skipped during the press and never made up left it 4-9 px off.
// THE CONTROLS, run every time: (a) the regression planted on FT at 1366×768 (every style the drag paints put back
// after each move, as the stale re-fit did) must make check 1 fail; (b) check 3 with every ResizeObserver callback
// DROPPED while the press is held (a re-fit skipped and never made up, the review's finding) must make check 3 fail.
// If either holds, the check cannot fail and the probe says so instead of passing.
//
// exit: 0 held everywhere · 1 a red, or a control that could not fail · 2 a screen never drew its divider
import { createServer } from 'vite'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'
import { FHD, LAPTOP, metrics } from './sizes.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const REAL_API = join(UI, 'src', 'api.ts')
const STUB_API = join(UI, 'probe', '__seamdrag-api.ts')
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const SIZES = [LAPTOP, FHD]
/** [label, route, the divider's accessible name]. Tempo is the MSG workspace's `chat` view. */
const SCREENS = [
  ['FT', 'operate', 'waterfall height'],
  ['Tempo', 'chat', 'waterfall height'],
  ['SSTV', 'sstv', 'waterfall and picture height'],
  ['RTTY', 'rtty', 'waterfall height'],
  ['CW', 'cw', 'scope height'],
  // Every other cockpit with a strip divider (PaneSeam): the same drag, since a change to the primitive reaches them all.
  ['Phone', 'phone', 'scope height'],
  ['PSK', 'psk', 'waterfall height'],
  ['JTTY', 'jtty', 'waterfall height'],
  ['JS8', 'js8', 'waterfall height'],
]
const FEATURES = JSON.stringify({ profile: 'custom', enabled: { rtty: true, psk: true, sstv: true, js8: true, jtty: true, aprs: true, chat: true } })
const MOVES = 12
const STEP = 8

const stub = await readFile(join(UI, 'probe', 'waterfalls-api.ts'), 'utf8')
const names = [...(await readFile(REAL_API, 'utf8')).matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1])
const own = new Set([...stub.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1]))
const stubCode = stub + `\nexport * from ${JSON.stringify(REAL_API)}\n` +
  names.filter((n) => !own.has(n)).map((n) => `export async function ${n}(..._a: unknown[]): Promise<any> { return (globalThis as any).__DEFAULT_ARR ? [] : {} }`).join('\n')
const swap = {
  name: 'seamdrag-api', enforce: 'pre',
  async resolveId(source, importer, opts) {
    if (opts?.scan) return null
    if (source === STUB_API) return STUB_API
    if (!importer || importer === STUB_API || !/api(\.ts)?$/.test(source)) return null
    const r = await this.resolve(source, importer, { ...opts, skipSelf: true })
    return r && r.id.split('?')[0] === REAL_API ? STUB_API : null
  },
  load(id) { return id === STUB_API ? stubCode : null },
}
const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-seamdrag-probe-'))
const server = await createServer({ root: UI, configFile: join(UI, 'vite.config.ts'), cacheDir, logLevel: 'warn', plugins: [swap],
  server: { host: '127.0.0.1', port: 0, strictPort: false, hmr: false } })
await server.listen()
const origin = `http://127.0.0.1:${server.httpServer.address().port}`
// Vite answers the first page only once it has bundled the app's dependencies: ask for it once before Chrome does.
await fetch(`${origin}/`).then((r) => r.text())
const browser = await chrome()

/** The plant for control (a): every inline style the drag changes is put back after each move, as the stale re-fit
 *  painted the stored height back over the drag's. */
const PLANT_FROZEN = `(() => {
  let snap = null
  addEventListener('pointerdown', () => { snap = new Map([...document.querySelectorAll('[style]')].map((e) => [e, e.getAttribute('style')])) }, true)
  addEventListener('pointermove', () => { if (snap) setTimeout(() => { for (const [e, s] of snap) if (e.getAttribute('style') !== s) e.setAttribute('style', s) }, 0) }, true)
  addEventListener('pointerup', () => { snap = null }, true)
})()`

/** The plant for control (b), before the app loads: while `__dropRO` is set, every ResizeObserver callback is
 *  dropped, as a re-fit skipped during a press and never made up is. */
const PLANT_DROP_RO = `(() => {
  const RO = window.ResizeObserver
  window.ResizeObserver = class extends RO { constructor(cb) { super((e, o) => { if (!window.__dropRO) cb(e, o) }) } }
})()`

async function openPage(size, route, extra = {}, before = null) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (m, p) => browser.call(m, p, sessionId)
  const ev = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text)
    return r.result.value
  }
  const go = (url) => call('Page.navigate', { url }).catch((e) => { if (!/timed out: Page\.navigate/.test(String(e))) throw e })
  await call('Emulation.setDeviceMetricsOverride', metrics(size))
  await call('Page.enable', {})
  if (before) await call('Page.addScriptToEvaluateOnNewDocument', { source: before })
  await call('Page.addScriptToEvaluateOnNewDocument', { source: "window.__roerr = 0; addEventListener('error', (e) => { if (/ResizeObserver/.test(String(e.message))) window.__roerr++ })" })
  await go(`${origin}/probe-blank`)
  await sleep(300)
  const ls = { 'nexus.workspace': route === 'chat' ? 'msg' : 'dx', 'nexus.features.v1': FEATURES, 'tempo-theme': 'dark', 'nexus.locale': 'en', ...extra }
  await ev(`localStorage.clear(); ${Object.entries(ls).map(([k, v]) => `localStorage.setItem(${JSON.stringify(k)}, ${JSON.stringify(v)})`).join(';')}; 1`)
  await go(`${origin}/#${route}`)
  const mouse = (type, x, y, buttons) => call('Input.dispatchMouseEvent', { type, x, y, button: 'left', buttons, clickCount: 1 })
  const frame = () => ev('new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => r(1))))')
  return { ev, mouse, frame, close: () => browser.call('Target.closeTarget', { targetId }) }
}

const SEP = (label) => `[...document.querySelectorAll('[role=separator][aria-label=${JSON.stringify(label)}]')].find((e) => e.closest('[hidden]') == null && e.getBoundingClientRect().height > 0)`
const seamAt = (p, label) => p.ev(`(() => { const s = ${SEP(label)}; if (!s) return null; const r = s.getBoundingClientRect(); return { x: Math.round(r.left + Math.min(r.width / 2, 200)), y: r.top + r.height / 2, now: s.getAttribute('aria-valuenow'), max: s.getAttribute('aria-valuemax') } })()`)
async function drawn(p, label) {
  for (let i = 0; i < 80; i++) {
    const s = await seamAt(p, label).catch(() => null)
    if (s) return s
    await sleep(250)
  }
  return null
}

/** Check 1 and 2 on one screen at one size; `plant` runs in the page first. */
async function drag(size, [name, route, label], plant = null) {
  const p = await openPage(size, route)
  try {
    if (plant) await p.ev(plant)
    const s0 = await drawn(p, label)
    if (!s0) return { error: `the ${label} divider never drew` }
    await sleep(800)
    const start = await seamAt(p, label)
    await p.ev('window.__roerr = 0; 1')
    await p.mouse('mouseMoved', start.x, start.y, 0)
    await p.mouse('mousePressed', start.x, start.y, 1)
    const trace = []
    let stoppedAt = null
    let ok = true
    for (let i = 1; i <= MOVES; i++) {
      const y = start.y + i * STEP
      await p.mouse('mouseMoved', start.x, y, 1)
      await p.frame()
      const at = (await seamAt(p, label)).y
      trace.push(`${Math.round(y)}>${Math.round(at)}`)
      if (stoppedAt == null) {
        if (Math.abs(at - y) > 1.5) stoppedAt = at
      } else if (Math.abs(at - stoppedAt) > 1.5) ok = false
    }
    await p.mouse('mouseReleased', start.x, start.y + MOVES * STEP, 0)
    await p.frame()
    await sleep(200)
    const end = await seamAt(p, label)
    // Stopped short of the pointer: only at the end of its range, where the release leaves it.
    if (stoppedAt != null && (Math.abs(end.y - stoppedAt) > 1.5 || end.now !== end.max)) ok = false
    const roerr = await p.ev('window.__roerr')
    return { follows: ok, roerr, trace: trace.join(' '), stoppedAt: stoppedAt == null ? null : Math.round(stoppedAt), end: Math.round(end.y), aria: [end.now, end.max] }
  } finally {
    await p.close()
  }
}

/** Check 3 on FT: where Stop TX stands after the header goes back under a held press, against no press at all. */
async function heldHeader(size, plant = false) {
  const p = await openPage(size, 'operate', { 'nexus.split.operate.waterfall': '85' }, plant ? PLANT_DROP_RO : null)
  try {
    const label = 'waterfall height'
    if (!(await drawn(p, label))) return { error: 'the waterfall divider never drew' }
    await sleep(800)
    const HEAD = `[...document.querySelectorAll('.cockpit-header')].find((h) => h.closest('[hidden]') == null && h.getBoundingClientRect().height > 0)`
    const settle = async () => { await p.frame(); await sleep(150); await p.frame() }
    const tall = async (px) => { await p.ev(`(() => { const h = ${HEAD}; h.style.minHeight = ''; if (${px}) h.style.minHeight = (h.getBoundingClientRect().height + ${px}) + 'px'; return 1 })()`); await settle() }
    const stop = () => p.ev(`(() => { const b = [...document.querySelectorAll('.op-btn.stop:not([data-stop-mirror])')].find((b) => b.closest('[hidden]') == null && b.getBoundingClientRect().width > 0); return b ? Math.round(b.getBoundingClientRect().top) : null })()`)
    await tall(28)
    await tall(0)
    const control = await stop()
    await tall(28)
    const s = await seamAt(p, label)
    await p.mouse('mouseMoved', s.x, s.y, 0)
    await p.mouse('mousePressed', s.x, s.y, 1)
    await p.frame()
    if (plant) await p.ev('window.__dropRO = true; 1')
    await tall(0)
    if (plant) await p.ev('window.__dropRO = false; 1')
    await p.mouse('mouseReleased', s.x, s.y, 0)
    await settle()
    await sleep(300)
    const after = await stop()
    return { control, after, holds: control != null && after != null && Math.abs(after - control) <= 1 }
  } finally {
    await p.close()
  }
}

let code = 0
try {
  for (const size of SIZES) {
    for (const screen of SCREENS) {
      const r = await drag(size, screen)
      const where = `${screen[0]} ${size.w}×${size.h}`
      if (r.error) {
        console.error(`seamdrag-probe: ${where}: ${r.error}`)
        code = Math.max(code, 2)
        continue
      }
      const ok = r.follows && r.roerr === 0
      console.log(`seamdrag-probe: ${ok ? 'ok ' : 'RED'} ${where}: ${r.follows ? 'follows' : 'DOES NOT FOLLOW'} the pointer${r.stoppedAt == null ? '' : `, stops at its end (${r.stoppedAt}, value ${r.aria.join('/')})`}, ${r.roerr} ResizeObserver-loop error(s)${ok ? '' : ` — pointer>divider ${r.trace}, released at ${r.end}`}`)
      if (!ok) code = Math.max(code, 1)
    }
    const h = await heldHeader(size)
    if (h.error) {
      console.error(`seamdrag-probe: FT ${size.w}×${size.h} header under a held press: ${h.error}`)
      code = Math.max(code, 2)
    } else {
      console.log(`seamdrag-probe: ${h.holds ? 'ok ' : 'RED'} FT ${size.w}×${size.h} header under a held press: Stop TX at ${h.after}, ${h.control} with no press`)
      if (!h.holds) code = Math.max(code, 1)
    }
  }
  const a = await drag(LAPTOP, SCREENS[0], PLANT_FROZEN)
  if (a.error || a.follows) {
    console.error(`seamdrag-probe: CONTROL FAILED — the frozen divider planted on FT at 1366×768 passed (${JSON.stringify(a)}); check 1 cannot fail`)
    code = Math.max(code, 1)
  } else console.log(`seamdrag-probe: control caught — the frozen divider on FT at 1366×768: ${a.trace}`)
  const b = await heldHeader(LAPTOP, true)
  if (b.error || b.holds) {
    console.error(`seamdrag-probe: CONTROL FAILED — the re-fits dropped during the press and check 3 held (${JSON.stringify(b)})`)
    code = Math.max(code, 1)
  } else console.log(`seamdrag-probe: control caught — re-fits dropped during the press left Stop TX at ${b.after}, ${b.control} with no press`)
  if (code === 0) console.log('seamdrag-probe: PASS — FT, Tempo, SSTV, RTTY, CW, Phone, PSK, JTTY and JS8 dividers follow the pointer with no ResizeObserver loop, and a header change under a held press is not lost')
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
