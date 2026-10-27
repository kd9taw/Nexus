// The Activation pane, its window and its Stop checklist in real Chrome, for what jsdom cannot see:
// `scripts/browser-probe --desktop activation`.
//
// The probe serves src/probe/activation.tsx with the app's own Vite config at 1024×768 (the supported
// floor) and 1920×1080, in three cases — the Activation box in the dashboard rail beside a cockpit's TX
// strip and dock, the pane in its own window, and the "Activation over?" checklist — and asserts:
//   · nothing scrolls sideways: the page, the pane's box (or window) and the checklist are no wider
//     than they show;
//   · the first park's count is on screen before any scroll, and the head, every park line, the rate and
//     the first contact are the topmost thing where they stand once the box's own scroll reaches them;
//   · Stop TX, Tune, PTT, the TX strip and the dock are the topmost thing at nine points across each:
//     the pane covers none of them;
//   · the checklist's box fits the window, and Keep activating and End activation are on screen once
//     the dialog's own scroll reaches them.
//
// THE CONTROL, run every time: the same pages with a 2400 px unbreakable line in the pane (and in the
// checklist), and the TX strip and dock drawn over the rail. There the probe must see the sideways
// overflow and the cover; if it does not, the check cannot fail and the probe says so instead of passing.
//
// exit: 0 every case clear · 1 a red, or a control that could not fail · 2 a page never drew
import { createServer } from 'vite'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const SIZES = [{ w: 1024, h: 768 }, { w: 1920, h: 1080 }]
const CASES = ['rail', 'window', 'over']
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

const MEASURE = `(() => {
  const topmostIn = (el) => {
    const r = el.getBoundingClientRect()
    if (r.width <= 0 || r.height <= 0) return false
    const x = r.left + r.width / 2, y = r.top + r.height / 2
    if (x < 0 || y < 0 || x > innerWidth || y > innerHeight) return false
    const hit = document.elementFromPoint(x, y)
    return hit != null && el.contains(hit)
  }
  const wider = (el) => el ? el.scrollWidth - el.clientWidth : null
  const pane = document.querySelector('.act-pane')
  const over = document.querySelector('.ui-dialog')
  if (!pane && !over) return null
  const out = { page: wider(document.scrollingElement), refused: window.__refused() }
  if (pane) {
    const body = pane.closest('.pane-body') ?? pane.closest('.detached-activation')
    out.paneBody = wider(body)
    out.paneOwn = wider(pane)
    out.rows = pane.querySelectorAll('.act-pane-row:not(.empty)').length
    const parts = [pane.querySelector('.act-pane-head'), ...pane.querySelectorAll('.act-pane-park'), pane.querySelector('.act-pane-rate'), pane.querySelector('.act-pane-row')]
    // The counts must show before any scroll; the rest may need the box's own scroll, so each is asked
    // after scrolling it into view the way a roving list does.
    // From the top each time: an earlier measure's scrolls are not the operator's.
    for (let a = pane.parentElement; a; a = a.parentElement) a.scrollTop = 0
    out.countsFirst = [...pane.querySelectorAll('.act-pane-park')].slice(0, 1).every(topmostIn)
    out.parts = parts.map((el) => { if (!el) return false; el.scrollIntoView({ block: 'nearest' }); return topmostIn(el) })
    out.paneBox = (() => { const r = pane.getBoundingClientRect(); return { left: Math.round(r.left), top: Math.round(r.top), w: Math.round(r.width), h: Math.round(r.height) } })()
  }
  const covered = {}
  for (const el of document.querySelectorAll('[data-tx]')) {
    const r = el.getBoundingClientRect()
    let clear = true
    for (const fx of [0.05, 0.5, 0.95]) for (const fy of [0.1, 0.5, 0.9]) {
      const top = document.elementFromPoint(r.left + r.width * fx, r.top + r.height * fy)
      if (!top || !(el.contains(top) || top.contains(el) || top.closest('[data-tx]'))) clear = false
      if (top && pane && pane.contains(top)) clear = false
    }
    covered[el.dataset.tx] = !clear
  }
  out.covered = covered
  if (over) {
    const r = over.getBoundingClientRect()
    out.dialog = { left: Math.round(r.left), right: Math.round(r.right), top: Math.round(r.top), bottom: Math.round(r.bottom) }
    out.dialogFits = r.left >= 0 && r.right <= innerWidth && r.top >= 0 && r.bottom <= innerHeight
    out.dialogWider = wider(over)
    out.buttons = [...over.querySelectorAll('.confirm-actions button')].map((b) => { b.scrollIntoView({ block: 'nearest' }); return topmostIn(b) })
    out.ticks = [...over.querySelectorAll('input[type=checkbox]')].map((b) => { b.scrollIntoView({ block: 'nearest' }); return topmostIn(b) })
  }
  return out
})()`

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-activation-probe-'))
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

async function measure(w, h, which, control) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true })
    return r.exceptionDetails ? { error: r.exceptionDetails.exception?.description ?? r.exceptionDetails.text } : r.result.value
  }
  await call('Emulation.setDeviceMetricsOverride', { width: w, height: h, deviceScaleFactor: 1, mobile: false })
  await call('Page.navigate', { url: `${origin}/probe/activation.html?case=${which}${control ? '&control=1' : ''}` })
  let last = null
  for (let i = 0; i < 80; i++) {
    await sleep(150)
    const now = await evaluate(MEASURE)
    const ready = now && !now.error && (which === 'over' ? now.ticks?.length > 0 : now.rows > 0) && (!control || (now.page > 0 || now.paneBody > 0 || now.dialogWider > 0 || now.paneOwn > 0))
    if (ready && last && JSON.stringify(now) === JSON.stringify(last)) break
    last = now
  }
  await browser.call('Target.closeTarget', { targetId })
  return last
}

/** What is wrong with a measure, as words; empty when nothing is. */
function faults(m, which) {
  const f = []
  if (m.page > 0) f.push(`the page scrolls sideways by ${m.page} px`)
  if (which !== 'over') {
    if (m.paneBody > 0) f.push(`the pane's box scrolls sideways by ${m.paneBody} px`)
    if (m.paneOwn > 0) f.push(`the pane is ${m.paneOwn} px wider than it shows`)
    if (m.parts.some((p) => !p)) f.push(`a part of the pane is not on screen: ${JSON.stringify(m.parts)}`)
    if (!m.countsFirst) f.push('the first park\'s count is not on screen before a scroll')
  }
  for (const [name, cov] of Object.entries(m.covered)) if (cov) f.push(`${name} is covered`)
  if (which === 'over') {
    if (!m.dialogFits) f.push(`the checklist does not fit the window: ${JSON.stringify(m.dialog)}`)
    if (m.dialogWider > 0) f.push(`the checklist scrolls sideways by ${m.dialogWider} px`)
    if (m.buttons.length !== 2 || m.buttons.some((b) => !b)) f.push(`a checklist button is not reachable: ${JSON.stringify(m.buttons)}`)
    if (m.ticks.some((b) => !b)) f.push(`a checklist tick is not reachable: ${JSON.stringify(m.ticks)}`)
  }
  return f
}

let code = 0
try {
  for (const { w, h } of SIZES) {
    for (const which of CASES) {
      const m = await measure(w, h, which, false)
      console.log(`activation-probe: ${which}, window ${w}×${h}: ${JSON.stringify(m)}`)
      if (!m || m.error || (which === 'over' ? !m.ticks?.length : !m.rows)) {
        console.error(`activation-probe: the ${which} case never drew at ${w}×${h}`)
        code = 2
        continue
      }
      const red = faults(m, which)
      if (red.length) {
        console.error(`activation-probe: RED ${which} at ${w}×${h} — ${red.join('; ')}`)
        code = code || 1
      }
      const c = await measure(w, h, which, true)
      const caught = c && !c.error ? faults(c, which) : []
      console.log(`activation-probe: CONTROL ${which}, window ${w}×${h}: caught ${JSON.stringify(caught)}`)
      const wantCover = which === 'rail'
      if (!caught.some((s) => s.includes('sideways') || s.includes('wider')) || (wantCover && !caught.some((s) => s.includes('covered')))) {
        console.error(`activation-probe: CONTROL FAILED ${which} at ${w}×${h} — the planted overflow${wantCover ? ' or cover' : ''} was not seen; the check cannot fail`)
        code = code || 1
      }
    }
  }
  if (code === 0) console.log(`activation-probe: PASS — the pane in the rail and in its window, and the checklist, whole with nothing sideways and no transmit control covered, at ${SIZES.map((s) => `${s.w}×${s.h}`).join(', ')}; each control caught`)
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
