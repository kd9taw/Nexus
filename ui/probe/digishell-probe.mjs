// The SSTV, RTTY and PSK shells in real Chrome, for what jsdom cannot see: `scripts/browser-probe --desktop digishell`.
//
// The three screens pin their cockpit header (the mode, the dial, the band, ⊞ Panels, power, CAT) so it stays in the
// window when the screen scrolls; the TX strip parks under it and the dock at the bottom. On a small window at a large
// zoom the three parked surfaces are taller than the window, and a header pinned there sat over Stop TX, Send and
// Esc/Stop at every scroll position (900×600 and 700×800 at 175 %, measured), so the strip unpins the header where
// the three do not fit. This probe serves the REAL app (index.html, main.tsx, App) with the app's own Vite config and
// src/api.ts swapped for probe/waterfalls-api.ts, opens each screen with fresh storage at the sizes below, and at the
// TOP of the screen's scroll and at its END asserts:
//   1. NOTHING IN THE HEADER COVERS A TRANSMIT CONTROL: every control of the TX strip and of the dock that is on
//      screen but not the topmost thing where it stands is covered by something other than the header.
//   2. A STOP CONTROL IS REACHABLE: at least one of Stop TX (strip), Esc/Stop (RTTY/PSK dock), Stop (SSTV's bar) is
//      on screen and topmost. (Which ones, and any transmit control covered by something else, is printed: on a
//      small window at 175 % RTTY and PSK already lost the strip's Stop TX under the dock before the header was
//      pinned, which is that shell's own matter, not this probe's.)
//   3. WHERE THE HEADER IS PINNED, ITS CONTROLS ARE ON SCREEN AT THE END of the scroll (#457), and the page itself
//      never scrolls.
// THE CONTROL, run every time: the header forced to stick on top (z 9) at 900×600 / 175 % must make check 1 fail.
//
// exit: 0 every check held · 1 a red, or a control that could not fail · 2 the page never drew a screen
import { createServer } from 'vite'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const REAL_API = join(UI, 'src', 'api.ts')
const STUB_SRC = join(UI, 'probe', 'waterfalls-api.ts')
const STUB_API = join(UI, 'probe', '__digishell-api.ts')
/** [window W, H, pinned UI scale or null for the app's own fit]. */
const SIZES = [
  [900, 600, '175'], [700, 800, '175'], [1366, 768, '175'], [1366, 768, '150'], [1920, 1080, '150'], [1200, 750, null],
]
const VIEWS = [['SSTV', 'sstv', 'main.sstv-view'], ['RTTY', 'rtty', 'main.rtty-cockpit'], ['PSK', 'psk', 'main.psk-cockpit']]
const FEATURES = JSON.stringify({ profile: 'custom', enabled: { rtty: true, psk: true, sstv: true } })
const PLANT = ".cockpit-header-pinned, .cockpit-header-pinned[data-pin='off'] { position: sticky !important; top: 0 !important; z-index: 9 !important }"
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

const measure = (sel) => `(() => {
  const shell = document.querySelector(${JSON.stringify(sel)})
  if (!shell || shell.getBoundingClientRect().height < 1 || !document.documentElement.getAttribute('data-viewport')) return null
  const head = shell.querySelector(':scope > .cockpit-header')
  const strip = shell.querySelector(':scope > .cockpit-txstrip')
  const dock = shell.querySelector(':scope > .cockpit-txdock, :scope > .sstv-tx-bar')
  if (!head || !strip || !dock) return null
  const vis = (el) => el.closest('[hidden]') == null && el.getBoundingClientRect().width > 0 && el.getBoundingClientRect().height > 0
  const label = (b) => (b.textContent || b.getAttribute('aria-label') || b.type || '').replace(/\\s+/g, ' ').trim().slice(0, 20)
  const at = (b) => {
    const q = b.getBoundingClientRect()
    if (q.bottom > innerHeight + 0.5 || q.top < -0.5 || q.right > innerWidth + 0.5 || q.left < -0.5) return { on: false }
    const t = document.elementFromPoint(q.left + q.width / 2, q.top + Math.min(q.height / 2, 6))
    return { on: true, top: t != null && (t === b || b.contains(t)), byHeader: t != null && head.contains(t) }
  }
  const ctls = [...strip.querySelectorAll('button, select, input'), ...dock.querySelectorAll('button, select, input')].filter(vis)
  const pos = () => {
    const rows = ctls.map((b) => ({ b, ...at(b) }))
    const stops = rows.filter((r) => /stop/i.test(label(r.b)))
    const hc = [...head.querySelectorAll('button, select, input')].filter(vis)
    return {
      coveredByHeader: rows.filter((r) => r.on && !r.top && r.byHeader).map((r) => label(r.b)),
      coveredOther: rows.filter((r) => r.on && !r.top && !r.byHeader).map((r) => label(r.b)),
      stopsReachable: stops.filter((r) => r.on && r.top).map((r) => label(r.b)),
      stops: stops.map((r) => label(r.b)),
      headReach: hc.filter((b) => { const a = at(b); return a.on && a.top }).length + '/' + hc.length,
    }
  }
  shell.scrollTop = 0
  const top = pos()
  shell.scrollTop = shell.scrollHeight
  const end = pos()
  shell.scrollTop = 0
  const se = document.scrollingElement
  return {
    vp: document.documentElement.getAttribute('data-viewport'),
    pin: head.getAttribute('data-pin'),
    valve: Math.max(0, Math.round(shell.scrollHeight - shell.clientHeight)),
    page: Math.max(0, se.scrollHeight - se.clientHeight),
    top, end,
  }
})()`

// THE API SWAP — the waterfalls probe's, verbatim in shape.
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
  name: 'digishell-probe-api',
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

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-digishell-probe-'))
const server = await createServer({
  root: UI, configFile: join(UI, 'vite.config.ts'), cacheDir, logLevel: 'warn', plugins: [swap],
  server: { host: '127.0.0.1', port: 0, strictPort: false, hmr: false },
})
await server.listen()
const origin = `http://127.0.0.1:${server.httpServer.address().port}`
const browser = await chrome()

async function measureAt(route, sel, [W, H, scale], css = null) {
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
    const ls = { 'nexus.workspace': 'dx', 'nexus.features.v1': FEATURES, 'tempo-theme': 'dark', ...(scale ? { 'nexus-ui-scale-mode': scale } : {}) }
    await evaluate(`localStorage.clear(); ${Object.entries(ls).map(([k, v]) => `localStorage.setItem(${JSON.stringify(k)}, ${JSON.stringify(v)})`).join(';')}; 1`)
    await call('Page.navigate', { url: `${origin}/#${route}` })
    let m = null
    for (let i = 0; i < 240 && !m; i++) {
      await sleep(250)
      m = await evaluate(measure(sel)).catch(() => null)
    }
    if (!m) return null
    if (css) await evaluate(`(() => { const s = document.createElement('style'); s.textContent = ${JSON.stringify(css)}; document.head.appendChild(s); return 1 })()`)
    let last = null
    for (let i = 0; i < 40; i++) {
      await sleep(250)
      m = await evaluate(measure(sel))
      if (last && JSON.stringify(m) === JSON.stringify(last)) break
      last = m
    }
    return m
  } finally {
    await browser.call('Target.closeTarget', { targetId })
  }
}

const tagOf = ([W, H, s]) => `${W}×${H} ${s ? `${s} %` : 'fit'}`
function reds(name, m) {
  const out = []
  if (m.page > 0) out.push(`${name}: the page scrolls ${m.page} px`)
  for (const [where, p] of [['top', m.top], ['end', m.end]]) {
    if (p.coveredByHeader.length) out.push(`${name}, scrolled to the ${where}: the header covers ${p.coveredByHeader.join(', ')}`)
    if (p.stopsReachable.length === 0) out.push(`${name}, scrolled to the ${where}: no stop control reachable (${p.stops.join(', ') || 'none found'})`)
  }
  if (m.pin === 'on') {
    const [r, n] = m.end.headReach.split('/').map(Number)
    if (r < n) out.push(`${name}: pinned, but only ${m.end.headReach} header controls on screen at the end of the scroll`)
  }
  return out
}

let code = 0
const red = (msg) => {
  console.error(`digishell-probe: RED — ${msg}`)
  code = 1
}
try {
  for (const [view, route, sel] of VIEWS) {
    for (const size of SIZES) {
      const m = await measureAt(route, sel, size)
      const name = `${view} at ${tagOf(size)}`
      if (!m) {
        console.error(`digishell-probe: ${name} never drew`)
        process.exitCode = 2
        throw new Error('no screen')
      }
      const info = (p) => `stops ${p.stopsReachable.join('+') || '—'}${p.coveredOther.length ? `, covered by other than the header: ${p.coveredOther.join(', ')}` : ''}`
      console.log(`digishell-probe: ${name.padEnd(22)} header ${m.pin === 'on' ? 'pinned' : 'unpinned'} (${m.vp}), scrolls ${m.valve} px · top: ${info(m.top)} · end: ${info(m.end)}, header ${m.end.headReach}`)
      for (const r of reds(name, m)) red(r)
    }
  }
  // THE CONTROL.
  const size = [900, 600, '175']
  const m = await measureAt('sstv', 'main.sstv-view', size, PLANT)
  const caught = m ? reds(`SSTV at ${tagOf(size)} (planted)`, m).filter((r) => r.includes('the header covers')) : []
  if (caught.length === 0) red('CONTROL FAILED — a header forced to stick on top covered nothing the probe could see; check 1 cannot fail')
  else console.log(`digishell-probe: CONTROL — ${caught[0]} (caught)`)
  if (code === 0) console.log('digishell-probe: PASS — no header covers a transmit control, a stop control is reachable at the top and the end of every screen at every size, a pinned header stays on screen, and the control was caught')
  process.exitCode = process.exitCode || code
} catch (e) {
  if (!process.exitCode) {
    console.error(`digishell-probe: ${e?.stack ?? e}`)
    process.exitCode = 2
  }
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
