// The transmit meters with the finals temperature and the COMP row's controls, in real Chrome, for
// what jsdom cannot see: `scripts/browser-probe --desktop txmeters`.
//
// The probe serves src/probe/txmeters.tsx with the app's own Vite config at 1024×768 (the supported
// floor) and 1920×1080, in °C and in °F, and asserts, in Phone's dock, the TX strip's meters cell (the
// compact dock, md and up; there the strip must also stay one line, no taller than its own buttons, with
// Stop TX on top) and Operate's telemetry cell:
//   · nothing scrolls sideways: the page, the panels and every meter row are no wider than they show;
//   · the COMP row's level bar is a usable width beside its PROC and bandwidth chips, and the row is
//     no taller than a meter row without them;
//   · the level bar, PROC, the bandwidth chip and both temperature rows are the topmost thing where
//     they stand, and the temperatures read in the chosen units;
//   · a real mouse drag on the bar sets the level where the pointer is, and real clicks on PROC and
//     the chip ask for the processor off and the radio's next step;
//   · all of that with the needle faces too, whose dials must be as high as the readout and, on the
//     1920 window, about twice the old face's arc, with the strip still no taller. (The 1024 window is
//     checked for a dial at all: at 100 % it is below the app's md, where the strip is not drawn.)
//
// THE CONTROL, run every time: the same page with a 400-character bandwidth step. There the probe
// must see the COMP row overflow; if it does not, the check cannot fail and the probe says so.
//
// exit: 0 every size clear · 1 a red, or a control that could not fail · 2 a page never drew
import { createServer } from 'vite'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const SIZES = [
  { w: 1024, h: 768, units: 'metric' },
  { w: 1920, h: 1080, units: 'imperial' },
]
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

const MEASURE = `(() => {
  const topmost = (el) => {
    if (!el) return false
    const r = el.getBoundingClientRect()
    if (r.width <= 0 || r.height <= 0) return false
    const x = r.left + r.width / 2, y = r.top + r.height / 2
    if (x < 0 || y < 0 || x > innerWidth || y > innerHeight) return false
    const hit = document.elementFromPoint(x, y)
    return hit != null && el.contains(hit)
  }
  const wider = (el) => (el ? el.scrollWidth - el.clientWidth : null)
  const dock = document.querySelector('[data-probe="dock"] .ph-txmeters')
  const cell = document.querySelector('[data-probe="cell"]')
  if (!dock || !cell) return null
  const rows = [...dock.querySelectorAll('.ph-txmeter')]
  const comp = dock.querySelector('.ph-txmeter.has-ctl')
  const swr = rows[0]
  const track = comp?.querySelector('[role="slider"]')
  const box = (el) => { const r = el.getBoundingClientRect(); return { x: r.left, y: r.top, w: r.width, h: r.height } }
  const temps = (root) => [...root.querySelectorAll('.ph-txmeter.temp')].map((r) => ({
    label: r.querySelector('.ph-txmeter-label').textContent,
    value: r.querySelector('.ph-txmeter-value').textContent,
    on: topmost(r.querySelector('.ph-txmeter-value')),
  }))
  return {
    page: wider(document.scrollingElement),
    dock: wider(dock),
    rowOverflow: Math.max(...rows.map(wider)),
    cellOverflow: cell.querySelector('.ph-txmeters').scrollWidth - cell.clientWidth,
    compH: comp ? Math.round(comp.getBoundingClientRect().height) : null,
    swrH: Math.round(swr.getBoundingClientRect().height),
    trackW: track ? Math.round(track.getBoundingClientRect().width) : null,
    track: track ? box(track) : null,
    trackOn: topmost(track),
    procOn: topmost(comp?.querySelector('[data-ctl="proc"]')),
    proc: comp?.querySelector('[data-ctl="proc"]') ? box(comp.querySelector('[data-ctl="proc"]')) : null,
    bwOn: topmost(comp?.querySelector('[data-ctl="txbw"]')),
    bw: comp?.querySelector('[data-ctl="txbw"]') ? box(comp.querySelector('[data-ctl="txbw"]')) : null,
    dockTemps: temps(dock),
    cellTemps: temps(cell),
    // The needle faces, where they are drawn: the narrowest dial in the dock or strip host, in em, so
    // a window's zoom cannot pass for a size, and how high a dial stands.
    dialEm: (() => {
      const dials = [...dock.querySelectorAll('.ph-txneedle-dial')]
      if (!dials.length) return null
      return Math.min(...dials.map((d) => d.getBoundingClientRect().width)) / parseFloat(getComputedStyle(dock).fontSize)
    })(),
    dialH: (() => {
      const d = dock.querySelector('.ph-txneedle-dial')
      return d ? Math.round(d.getBoundingClientRect().height) : null
    })(),
    // The strip host only: the strip's height against its own buttons. The meters must not raise it.
    strip: (() => {
      const s = document.querySelector('[data-probe="strip"]')
      if (!s) return null
      const stop = s.querySelector('.op-btn.stop').getBoundingClientRect()
      const cs = getComputedStyle(s)
      const chrome = parseFloat(cs.paddingTop) + parseFloat(cs.paddingBottom) + parseFloat(cs.borderTopWidth) + parseFloat(cs.borderBottomWidth)
      const cluster = s.querySelector('.cq-txctl').getBoundingClientRect()
      return { h: Math.round(s.getBoundingClientRect().height), button: Math.round(stop.height), cluster: Math.round(cluster.height), chrome: Math.round(chrome), stopOn: topmost(s.querySelector('.op-btn.stop')) }
    })(),
  }
})()`

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-txmeters-probe-'))
const server = await createServer({
  root: UI,
  configFile: join(UI, 'vite.config.ts'),
  cacheDir,
  logLevel: 'warn',
  server: { host: '127.0.0.1', port: 0, strictPort: false },
})
await server.listen()
const origin = `http://127.0.0.1:${server.httpServer.address().port}`
const browser = await chrome()

async function measure(w, h, units, control, host, needle = false) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true })
    return r.exceptionDetails ? { error: r.exceptionDetails.exception?.description ?? r.exceptionDetails.text } : r.result.value
  }
  await call('Emulation.setDeviceMetricsOverride', { width: w, height: h, deviceScaleFactor: 1, mobile: false })
  await call('Page.navigate', { url: `${origin}/probe/txmeters.html?units=${units}${control ? '&control=1' : ''}${host === 'strip' ? '&host=strip' : ''}${needle ? '&needle=1' : ''}` })
  let last = null
  for (let i = 0; i < 80; i++) {
    await sleep(150)
    const now = await evaluate(MEASURE)
    if (now && !now.error && last && JSON.stringify(now) === JSON.stringify(last)) break
    last = now
  }
  let asked = null
  if (last && !last.error && !control && last.track && last.proc && last.bw) {
    const mouse = (type, x, y, buttons = 1) =>
      call('Input.dispatchMouseEvent', { type, x, y, button: 'left', buttons, clickCount: 1 })
    const t = last.track
    const y = t.y + t.h / 2
    await mouse('mousePressed', t.x + t.w * 0.75, y)
    await mouse('mouseMoved', t.x + t.w * 0.25, y)
    await mouse('mouseReleased', t.x + t.w * 0.25, y, 0)
    for (const b of [last.proc, last.bw]) {
      await mouse('mousePressed', b.x + b.w / 2, b.y + b.h / 2)
      await mouse('mouseReleased', b.x + b.w / 2, b.y + b.h / 2, 0)
    }
    await sleep(200)
    asked = await evaluate('window.__asked')
  }
  await browser.call('Target.closeTarget', { targetId })
  return { m: last, asked }
}

/** What is wrong with a measure, as words; empty when nothing is. */
function faults(m, units, needle = false, w = 0) {
  const f = []
  if (needle) {
    if (!(m.dialEm > (w >= 1100 ? 8 : 1.5))) f.push(`the narrowest needle dial is ${m.dialEm}em wide`)
    if (!(m.dialH >= 24)) f.push(`a needle dial is ${m.dialH} px high`)
  }
  if (m.page > 0) f.push(`the page scrolls sideways by ${m.page} px`)
  if (m.dock > 0) f.push(`the dock's meters are ${m.dock} px wider than they show`)
  if (m.rowOverflow > 0) f.push(`a meter row overflows by ${m.rowOverflow} px`)
  if (m.cellOverflow > 0) f.push(`the Operate cell's meters overflow it by ${m.cellOverflow} px`)
  if (m.compH == null || m.compH > m.swrH) f.push(`the COMP row is ${m.compH} px high against a meter row's ${m.swrH}`)
  if (!(m.trackW >= 40)) f.push(`the level bar is ${m.trackW} px wide`)
  if (!m.trackOn) f.push('the level bar is not reachable')
  if (!m.procOn) f.push('PROC is not reachable')
  if (!m.bwOn) f.push('the bandwidth chip is not reachable')
  const unit = units === 'imperial' ? '°F' : '°C'
  const want = units === 'imperial' ? ['RIG 117°F', 'AMP 126°F'] : ['RIG 47°C', 'AMP 52°C']
  if (m.strip) {
    // One line: no taller than its own TX cluster and its padding and border, and Stop TX on top.
    if (m.strip.h > m.strip.cluster + m.strip.chrome + 1) f.push(`the meters raise the TX strip to ${m.strip.h} px (its TX cluster ${m.strip.cluster} + ${m.strip.chrome})`)
    if (!m.strip.stopOn) f.push('Stop TX in the strip is not reachable')
  }
  for (const [where, list] of [['dock', m.dockTemps], ['cell', m.cellTemps]]) {
    const got = list.map((r) => `${r.label} ${r.value}`)
    if (JSON.stringify(got) !== JSON.stringify(want)) f.push(`the ${where} temperatures read ${JSON.stringify(got)}, not ${unit} ${JSON.stringify(want)}`)
    if (!list.every((r) => r.on)) f.push(`a ${where} temperature is covered`)
  }
  return f
}

let code = 0
try {
  for (const { w, h, units } of SIZES) for (const host of ['dock', 'strip']) for (const needle of [false, true]) {
    const { m, asked } = await measure(w, h, units, false, host, needle)
    console.log(`txmeters-probe: window ${w}×${h} ${units} in the ${host}${needle ? ' with needles' : ''}: ${JSON.stringify(m)} asked: ${JSON.stringify(asked)}`)
    if (!m || m.error) {
      console.error(`txmeters-probe: the meters never drew at ${w}×${h}`)
      code = 2
      continue
    }
    const red = faults(m, units, needle, w)
    const comp = asked?.comp ?? []
    if (!(comp.length >= 2 && Math.abs(comp[0] - 0.75) <= 0.02 && Math.abs(comp[comp.length - 1] - 0.25) <= 0.02))
      red.push(`the drag asked for ${JSON.stringify(comp)}, not 0.75 then 0.25`)
    if (JSON.stringify(asked?.proc) !== '[false]') red.push(`PROC asked for ${JSON.stringify(asked?.proc)}`)
    if (JSON.stringify(asked?.bw) !== '["WIDE"]') red.push(`the chip asked for ${JSON.stringify(asked?.bw)}, not the next step`)
    if (red.length) {
      console.error(`txmeters-probe: RED at ${w}×${h} in the ${host} — ${red.join('; ')}`)
      code = code || 1
    }
    const c = await measure(w, h, units, true, host)
    const caught = c.m && !c.m.error ? faults(c.m, units) : []
    console.log(`txmeters-probe: CONTROL window ${w}×${h} in the ${host}: caught ${JSON.stringify(caught)}`)
    if (!caught.some((s) => s.includes('overflow') || s.includes('sideways') || s.includes('wider'))) {
      console.error(`txmeters-probe: CONTROL FAILED at ${w}×${h} in the ${host} — the planted overflow was not seen; the check cannot fail`)
      code = code || 1
    }
  }
  if (code === 0) console.log(`txmeters-probe: PASS — level bar, PROC, bandwidth chip and both temperatures on screen with nothing sideways, drag and clicks reach the radio's verbs, in the dock and in the TX strip (which they do not make taller), at ${SIZES.map((s) => `${s.w}×${s.h} ${s.units}`).join(', ')}; each control caught`)
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
