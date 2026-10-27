// RTTY's cockpit fitting the window in real Chrome, for what jsdom cannot see: `scripts/browser-probe --desktop rttyfit`.
//
// When the tuning scope moved beside the waterfall, RTTY's cockpit started to scroll inside the window where it
// had fit before (1.21.0): 117 px at 1920×1080 with the UI pinned at 150 %, and more than before at 1366×768 at
// 125 % and 150 %. The probe serves the REAL app (index.html, main.tsx, App) with the app's own Vite config and src/api.ts
// swapped for probe/waterfalls-api.ts (the waterfalls probe's answers), opens RTTY with fresh storage at the
// common full-screen sizes (at the zoom the app fits there) and at 125 % and 150 % pinned on 1920×1080 and
// 1366×768, with the tuning scope shown (its default) and hidden, and asserts:
//   1. THE COCKPIT SCROLLS NO MORE THAN 1.21.0 DID: `main.rtty-cockpit`'s own scroll range (the deficit valve,
//      which scrolls the header away) is at most BEFORE's for that size, measured with this probe on 1.21.0;
//      0 wherever it was 0. The page itself never scrolls.
//   2. Wherever the cockpit does not scroll, Stop TX, Tune, the dock's Esc/Stop macro and the TX latch are on
//      screen and the topmost thing where they stand (Esc/Stop is disabled while nothing is sending; it is
//      still there).
//   3. The tuning scope's nudges and words are on screen and its column does not scroll, wherever the cockpit
//      does not scroll.
// THE CONTROL, run every time: a planted 120 px margin under the waterfall strip at 1920×1080 / 150 % must
// make check 1 fail. If it passes, the check cannot fail and the probe says so instead of passing.
//
// exit: 0 every check held · 1 a red, or a control that could not fail · 2 the page never drew RTTY
import { createServer } from 'vite'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const REAL_API = join(UI, 'src', 'api.ts')
const STUB_SRC = join(UI, 'probe', 'waterfalls-api.ts')
const STUB_API = join(UI, 'probe', '__rttyfit-api.ts')
/** [window W, H, pinned UI scale or null for the app's own fit]. */
const SIZES = [
  [1024, 768, null], [1280, 720, null], [1366, 768, null], [1536, 864, null], [1920, 1080, null], [2560, 1440, null],
  [1920, 1080, '125'], [1920, 1080, '150'], [1366, 768, '125'], [1366, 768, '150'],
]
/** The most the cockpit may scroll at each size, its own CSS px (`scrollHeight - clientHeight` of the shell), with
 *  the tuning scope shown or hidden. Measured with this probe on 1.21.0, in its default layout (scope shown):
 *  0 at every size the app fits by itself and at 1920×1080 / 125 %; 141 and 239 at 1366×768 / 125 % and 150 %,
 *  where even 1.21.0 had to scroll; and 28 at 1920×1080 / 150 %, which is held to 0 here, the size the report
 *  was about (the app's own fixtures measured 0 there on 1.21.0). With the scope hidden 1.21.0 scrolled more
 *  (117 / 281 / 379); the scope no longer changes RTTY's height, so both are held to the same ceiling. */
const BEFORE = {
  '1024×768 fit': 0, '1280×720 fit': 0, '1366×768 fit': 0, '1536×864 fit': 0, '1920×1080 fit': 0, '2560×1440 fit': 0,
  '1920×1080 125 %': 0, '1920×1080 150 %': 0, '1366×768 125 %': 141, '1366×768 150 %': 239,
}
const FEATURES = JSON.stringify({ profile: 'custom', enabled: { rtty: true } })
const NO_TUNING = { 'nexus.panels.rtty.main': JSON.stringify({ v: 1, state: { tuning: 'removed' }, share: {} }) }
const PLANT = '.rtty-cockpit > .rtty-wf-row, .rtty-cockpit > .waterfall-wrap { margin-bottom: 120px !important }'
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

const MEASURE = `(() => {
  const shell = document.querySelector('main.rtty-cockpit')
  if (!shell || shell.getBoundingClientRect().height < 1 || !document.documentElement.getAttribute('data-viewport')) return null
  if (!shell.querySelector('.waterfall-canvas')) return null
  const vis = (el) => el && el.closest('[hidden]') == null && el.getBoundingClientRect().width > 0 && el.getBoundingClientRect().height > 0
  const reach = (b) => {
    if (!vis(b)) return false
    const q = b.getBoundingClientRect()
    if (q.bottom > innerHeight + 0.5 || q.top < -0.5 || q.right > innerWidth + 0.5 || q.left < -0.5) return false
    const t = document.elementFromPoint(q.left + q.width / 2, q.top + q.height / 2)
    return t != null && (t === b || b.contains(t))
  }
  const btn = (re) => [...shell.querySelectorAll('button')].find((b) => vis(b) && re.test(b.textContent.trim()))
  const z = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--ui-zoom')) || 1
  const se = document.scrollingElement
  const col = shell.querySelector('.rtty-tuning')
  return {
    vp: document.documentElement.getAttribute('data-viewport'), z,
    // The valve, in the cockpit's own CSS px.
    valve: Math.max(0, Math.round(shell.scrollHeight - shell.clientHeight)),
    page: Math.max(0, se.scrollHeight - se.clientHeight),
    shellH: Math.round(shell.getBoundingClientRect().height),
    strip: (() => { const s = shell.querySelector(':scope > .rtty-wf-row, :scope > .waterfall-wrap'); return s ? Math.round(s.getBoundingClientRect().height) : null })(),
    stops: [['Stop TX', btn(/^stop tx$/i)], ['Tune', btn(/^tune$/i)], ['Esc/Stop macro', btn(/^(esc|stop)\\b/i)],
      ['TX latch', shell.querySelector('.cockpit-txstrip button[aria-pressed]') ?? btn(/^tx (on|off)$/i)]]
      .map(([n, b]) => [n, b ? reach(b) : null]),
    tuning: vis(col) ? {
      scrolls: col.scrollHeight > col.clientHeight + 1,
      controls: [...col.querySelectorAll('.rtty-tuning-btn, .rtty-tuning-status')].every((e) => {
        const r = e.getBoundingClientRect(), q = col.getBoundingClientRect()
        return r.height > 0 && r.top >= q.top - 1 && r.bottom <= q.bottom + 1 && r.bottom <= innerHeight
      }),
    } : null,
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
  name: 'rttyfit-probe-api',
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

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-rttyfit-probe-'))
const server = await createServer({
  root: UI, configFile: join(UI, 'vite.config.ts'), cacheDir, logLevel: 'warn', plugins: [swap],
  server: { host: '127.0.0.1', port: 0, strictPort: false, hmr: false },
})
await server.listen()
const origin = `http://127.0.0.1:${server.httpServer.address().port}`
const browser = await chrome()

async function measure([W, H, scale], extra = {}, css = null) {
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
    const ls = { 'nexus.workspace': 'dx', 'nexus.features.v1': FEATURES, 'tempo-theme': 'dark', ...(scale ? { 'nexus-ui-scale-mode': scale } : {}), ...extra }
    await evaluate(`localStorage.clear(); ${Object.entries(ls).map(([k, v]) => `localStorage.setItem(${JSON.stringify(k)}, ${JSON.stringify(v)})`).join(';')}; 1`)
    await call('Page.navigate', { url: `${origin}/#rtty` })
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
    return m
  } finally {
    await browser.call('Target.closeTarget', { targetId })
  }
}

const tagOf = ([W, H, s]) => `${W}×${H} ${s ? `${s} %` : 'fit'}`
/** Checks 1-3 for one page; `which` 0 = scope shown, 1 = hidden. */
function reds(tag, which, m) {
  const out = []
  const name = `RTTY${which ? ' (no tuning scope)' : ''} at ${tag}`
  const cap = BEFORE[tag]
  if (m.page > 0) out.push(`${name}: the page scrolls ${m.page} px`)
  if (m.valve > cap + 2) out.push(`${name}: the cockpit scrolls ${m.valve} px, at most ${cap} (1.21.0)`)
  if (m.valve === 0) {
    for (const [label, ok] of m.stops) if (ok === false || ok === null) out.push(`${name}: ${label} is ${ok === null ? 'missing' : 'not on screen or covered'}`)
    if (!which && !m.tuning) out.push(`${name}: no tuning scope on screen`)
    if (m.tuning?.scrolls) out.push(`${name}: the tuning scope's column scrolls`)
    if (m.tuning && !m.tuning.controls) out.push(`${name}: the tuning scope's nudges or words are cut off`)
  }
  return out
}

let code = 0
const red = (msg) => {
  console.error(`rttyfit-probe: RED — ${msg}`)
  code = 1
}
try {
  for (const size of SIZES) {
    const tag = tagOf(size)
    const row = []
    for (const [which, extra] of [[0, {}], [1, NO_TUNING]]) {
      const m = await measure(size, extra)
      if (!m) {
        console.error(`rttyfit-probe: RTTY at ${tag} never drew`)
        process.exitCode = 2
        throw new Error('no RTTY')
      }
      if (!/^(xs|sm|md|lg|xl)$/.test(m.vp)) red(`RTTY at ${tag}: no viewport stamp (${m.vp}) — the measurement raced the layout`)
      row.push(`${which ? 'hidden' : 'shown'}: cockpit scrolls ${m.valve} px (at most ${BEFORE[tag]}), strip ${m.strip} px, zoom ${m.z}`)
      for (const r of reds(tag, which, m)) red(r)
    }
    console.log(`rttyfit-probe: ${tag.padEnd(16)} tuning scope ${row.join(' · tuning scope ')}`)
  }
  // THE CONTROL.
  const size = [1920, 1080, '150']
  const m = await measure(size, {}, PLANT)
  const caught = m ? reds(tagOf(size), 0, m).filter((r) => r.includes('cockpit scrolls')) : []
  if (caught.length === 0) red(`CONTROL FAILED — a planted 120 px margin under the strip passed check 1 (cockpit scrolls ${m?.valve}); the check cannot fail`)
  else console.log(`rttyfit-probe: CONTROL — ${caught[0]} (caught)`)
  if (code === 0) console.log('rttyfit-probe: PASS — RTTY scrolls no more than 1.21.0 did, at every size, its stop controls on screen wherever it does not scroll, and the control was caught')
  process.exitCode = process.exitCode || code
} catch (e) {
  if (!process.exitCode) {
    console.error(`rttyfit-probe: ${e?.stack ?? e}`)
    process.exitCode = 2
  }
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
