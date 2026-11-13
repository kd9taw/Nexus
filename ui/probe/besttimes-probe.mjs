// The Best times box in real Chrome, for what jsdom cannot see: `scripts/browser-probe --desktop besttimes`.
//
// The box (ui/src/components/prop/BestTimesPane.tsx) grows when its answer lands: the answer, five
// windows, the week and a ten-band chart. In the dashboard rail beside a cockpit's TX strip and dock
// (src/probe/besttimes.tsx), that growth must move no transmit control and cover none. At the
// representative screens (probe/sizes.mjs CORE: 1366×768 at 175 %, the floor, to 4K at 200 %) and at
// 1366×768 and 900×600 (xs) with the app's own UI at 175 % the probe reads every transmit box before the answer and
// after it, and asserts each is where it was, that the topmost thing at its centre is itself, and that
// the box did draw its answer.
//
// THE CONTROL, run every time: the same box drawn bare above the TX strip, outside any frame, where its
// growth must push Stop TX down. If the probe does not see that move, the check cannot fail and it says so.
//
// exit: 0 every case clear · 1 a red, or a control that could not fail · 2 the page never drew the answer
import { createServer } from 'vite'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'
import { CORE, LAPTOP, metrics, offMatrix } from './sizes.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
// xs: 900×600 (the shell's own minimum window, off the matrix) at the app's 175 % is 514×343 effective — the
// row that keeps the xs layout here; 1366×768 at 175 % is 780 px, sm.
const SIZES = [...CORE, { ...LAPTOP, zoom: 1.75 }, { ...offMatrix(900, 600), zoom: 1.75 }]
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

/** Every transmit box: its rounded rect, and whether the topmost thing at its centre is itself. */
const RECTS = `JSON.stringify([...document.querySelectorAll('[data-tx]')].map((e) => {
  const r = e.getBoundingClientRect()
  const top = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2)
  return [e.dataset.tx, Math.round(r.left), Math.round(r.top), Math.round(r.width), Math.round(r.height), !!top && e.contains(top)]
}))`
const ANSWERED = `document.documentElement.dataset.answered === 'yes' && !!document.querySelector('.bt-now-head')`

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-besttimes-probe-'))
const server = await createServer({ root: UI, configFile: join(UI, 'vite.config.ts'), cacheDir, logLevel: 'warn', server: { host: '127.0.0.1', port: 0, strictPort: false } })
await server.listen()
const origin = `http://127.0.0.1:${server.httpServer.address().port}`
const browser = await chrome()

async function measure(size, zoom, control) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true })
    return r.exceptionDetails ? null : r.result.value
  }
  await call('Emulation.setDeviceMetricsOverride', metrics(size))
  if (zoom) await call('Page.addScriptToEvaluateOnNewDocument', { source: `document.documentElement.style.setProperty('--ui-zoom', '${zoom}')` })
  await call('Page.navigate', { url: `${origin}/probe/besttimes.html${control ? '?control=1' : ''}` })
  // The reading before the answer: once the web fonts are in (a font swap moves a button's edge by a
  // pixel or two, which is not the box) and two readings agree, and while the answer is still held back.
  let before = null
  for (let i = 0, last = null; i < 100 && !before; i++) {
    await sleep(100)
    const r = (await evaluate(`document.fonts.status === 'loaded' && !document.documentElement.dataset.answered`)) ? await evaluate(RECTS) : null
    if (r && r !== '[]' && r === last) before = r
    last = r
  }
  let answered = false
  for (let i = 0; i < 60 && !answered; i++) {
    await sleep(150)
    answered = (await evaluate(ANSWERED)) === true
  }
  await sleep(300)
  const after = await evaluate(RECTS)
  const refused = await evaluate('window.__refused()')
  await browser.call('Target.closeTarget', { targetId })
  return { before: JSON.parse(before ?? '[]'), after: JSON.parse(after ?? '[]'), answered, refused }
}

const moved = (m) => m.before.length === 0 || JSON.stringify(m.before.map((r) => r.slice(0, 5))) !== JSON.stringify(m.after.map((r) => r.slice(0, 5)))

let code = 0
try {
  for (const size of SIZES) {
    const { zoom } = size
    const at = `${size.name}${zoom ? ` at ${zoom * 100}%` : ''}`
    const m = await measure(size, zoom, false)
    console.log(`besttimes-probe: rail, window ${at}: ${JSON.stringify(m)}`)
    if (!m.answered) {
      console.error(`besttimes-probe: the box never drew its answer at ${at}`)
      code = 2
    } else if (moved(m) || m.after.some((r) => !r[5])) {
      console.error(`besttimes-probe: RED at ${at} — a transmit control moved or was covered when the answer landed`)
      code = code || 1
    }
    const c = await measure(size, zoom, true)
    console.log(`besttimes-probe: CONTROL (the box bare above the TX strip), window ${at}: ${JSON.stringify(c)}`)
    if (!c.answered || !moved(c)) {
      console.error(`besttimes-probe: CONTROL FAILED at ${at} — the bare box grew and nothing moved; the check cannot fail`)
      code = code || 1
    }
  }
  if (code === 0) console.log(`besttimes-probe: PASS — no transmit control moved or was covered when the Best times answer landed, at ${SIZES.map((s) => `${s.name}${s.zoom ? ` ${s.zoom * 100}%` : ''}`).join(', ')}; the control moved Stop TX each time`)
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
