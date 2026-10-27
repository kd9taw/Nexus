// The cockpit header's lines in real Chrome, for what jsdom cannot see: `scripts/browser-probe --desktop header`.
//
// The header's actions (⊞ Panels, the amplifier, power, CAT) must never take a line of their own while the
// mode controls' last line has room for them. As a header item placed after a cluster that had already
// taken a whole line, they did exactly that whenever the cluster wrapped: ⊞ Panels and CAT alone at the
// right under a REC alone at the left, a band of empty header above the scope (operator photo, CW on a
// 1080p screen above 100 %). The probe serves src/probe/header.tsx with the app's own Vite config, in a
// CW shell with a CW header's controls, across window widths from the 1024 floor to 2560 at 100 % and at
// 110-175 % on 1920×1080, and asserts at each: if the actions sit on a line below the last mode control,
// the room left on that control's line is less than the actions' width; and nothing runs past the window.
//
// THE CONTROL, run every time: the same controls in the header's previous shape (`&old=on`). There the
// probe must find at least one width where the actions took a line they did not need; if it does not,
// the check cannot fail and the probe says so instead of passing.
//
// exit: 0 every case holds · 1 a red, or a control that could not fail · 2 the page never drew the header
import { createServer } from 'vite'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const CASES = []
for (let w = 1024; w <= 2560; w += 64) CASES.push({ w, h: 900, z: 1 })
for (const z of [1.1, 1.25, 1.5, 1.75]) CASES.push({ w: 1920, h: 1080, z })
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

const MEASURE = `(() => {
  const h = document.querySelector('.cockpit-header')
  const a = h?.querySelector('.ch-actions')
  const ctl = [...(h?.querySelectorAll('.probe-control') ?? [])]
  if (!h || !a || ctl.length === 0 || !document.documentElement.getAttribute('data-viewport')) return null
  const z = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--ui-zoom')) || 1
  const r = (e) => e.getBoundingClientRect()
  const last = r(ctl[ctl.length - 1]), act = r(a), hr = r(h)
  const gap = parseFloat(getComputedStyle(ctl[0].parentElement).columnGap) * z || 0
  return {
    below: act.top >= last.bottom - 1,
    room: Math.round(hr.right - last.right - gap),
    actW: Math.round(act.width),
    headerH: Math.round(hr.height),
    past: Math.max(0, Math.round(Math.max(...[...h.querySelectorAll('*')].map((e) => r(e).right)) - innerWidth)),
  }
})()`

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-header-probe-'))
const server = await createServer({
  root: UI, configFile: join(UI, 'vite.config.ts'), cacheDir, logLevel: 'warn',
  server: { host: '127.0.0.1', port: 0, strictPort: false },
})
await server.listen()
const origin = `http://127.0.0.1:${server.httpServer.address().port}`
const browser = await chrome()

async function measure({ w, h, z }, old) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true })
    return r.exceptionDetails ? null : r.result.value
  }
  await call('Emulation.setDeviceMetricsOverride', { width: w, height: h, deviceScaleFactor: 1, mobile: false })
  await call('Page.navigate', { url: `${origin}/probe/header.html?zoom=${z}${old ? '&old=on' : ''}` })
  let last = null
  for (let i = 0; i < 60; i++) {
    await sleep(120)
    const now = await evaluate(MEASURE)
    if (i > 6 && now && last && JSON.stringify(now) === JSON.stringify(last)) break
    last = now
  }
  await browser.call('Target.closeTarget', { targetId })
  return last
}

const wasted = (m) => m.below && m.room >= m.actW
let code = 0
try {
  let controlCaught = 0
  for (const c of CASES) {
    const at = `${c.w}×${c.h} ${Math.round(c.z * 100)}%`
    const m = await measure(c, false)
    const o = await measure(c, true)
    console.log(`header-probe: ${at}: ${JSON.stringify(m)} · previous shape ${JSON.stringify(o)}`)
    if (!m || !o) {
      console.error(`header-probe: the header was never drawn at ${at}`)
      code = 2
      continue
    }
    if (wasted(o)) controlCaught++
    if (wasted(m)) {
      console.error(`header-probe: RED ${at} — the actions (${m.actW}px) took a line of their own with ${m.room}px free on the line above`)
      code = code || 1
    }
    if (m.past > 0) {
      console.error(`header-probe: RED ${at} — the header runs ${m.past}px past the window`)
      code = code || 1
    }
  }
  console.log(`header-probe: CONTROL — the previous shape wasted a line at ${controlCaught} of ${CASES.length} sizes`)
  if (controlCaught === 0) {
    console.error('header-probe: CONTROL FAILED — the previous shape never wasted a line here; the check cannot fail')
    code = code || 1
  }
  if (code === 0) console.log(`header-probe: PASS — the header's actions never take a line they do not need, 1024 to 2560 wide and 110-175 % on 1920×1080`)
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
