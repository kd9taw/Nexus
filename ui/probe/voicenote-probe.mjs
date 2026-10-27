// The voice keyer's Flex DAX note in real Chrome, for what jsdom cannot see:
// `scripts/browser-probe --desktop voicenote`.
//
// The probe serves src/probe/voicenote.tsx with the app's own Vite config at 1024×768 (the supported
// floor), 1366×768 and 1920×1080 with the keyer's column beside the log, and at 1920×1080 with three
// columns and the keyer's dragged to its 16em floor (the narrowest it gets), presses F1, and asserts:
//   · the note appears, and nothing scrolls sideways: the page, the pane's box and the note are no wider
//     than they show;
//   · the note, its Got it button and the F1 slot are the topmost thing where they stand once the pane's
//     own scroll reaches them, and the TX dock is not covered;
//   · Got it takes the note away and records the dismissal.
//
// THE CONTROL, run every time: the same page with a 2400 px unbreakable line in the note. There the probe
// must see the sideways overflow; if it does not, the check cannot fail and the probe says so instead.
//
// exit: 0 every size clear · 1 a red, or a control that could not fail · 2 a page never drew the note
import { createServer } from 'vite'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const SIZES = [
  { w: 1024, h: 768, q: 'cols=2' },
  { w: 1366, h: 768, q: 'cols=2' },
  { w: 1920, h: 1080, q: 'cols=2' },
  { w: 1920, h: 1080, q: 'cols=3&narrow=1' },
]
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

const PRESS = `(() => { if (document.querySelectorAll('.vk-slot').length !== 7) return false
  window.dispatchEvent(new KeyboardEvent('keydown', { key: 'F1', bubbles: true })); return true })()`

const MEASURE = `(() => {
  const topmostIn = (el) => {
    if (!el) return false
    el.scrollIntoView({ block: 'nearest' })
    const r = el.getBoundingClientRect()
    if (r.width <= 0 || r.height <= 0) return false
    const x = r.left + r.width / 2, y = r.top + r.height / 2
    if (x < 0 || y < 0 || x > innerWidth || y > innerHeight) return false
    const hit = document.elementFromPoint(x, y)
    return hit != null && el.contains(hit)
  }
  const wider = (el) => el ? el.scrollWidth - el.clientWidth : null
  const note = document.querySelector('.vk-flex-dax-note')
  if (!note) return null
  const pane = note.closest('.pane-body') ?? note.closest('.pane-frame')
  const dock = document.querySelector('[data-tx]')
  const r = note.getBoundingClientRect()
  return {
    page: wider(document.scrollingElement),
    paneBody: wider(pane),
    note: wider(note),
    noteBox: { left: Math.round(r.left), top: Math.round(r.top), w: Math.round(r.width), h: Math.round(r.height) },
    noteOn: topmostIn(note),
    dismissOn: topmostIn(note.querySelector('button')),
    f1On: topmostIn(document.querySelector('.vk-slot .vk-play')),
    dockOn: topmostIn(dock),
    refused: window.__refused(),
  }
})()`

const DISMISS = `(() => { const b = document.querySelector('.vk-flex-dax-note button'); if (!b) return null; b.click()
  return true })()`
const AFTER = `({ gone: document.querySelector('.vk-flex-dax-note') === null, stored: localStorage.getItem('nexus.phone.flexDaxNote') })`

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-voicenote-probe-'))
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

async function measure(w, h, q, control) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true })
    return r.exceptionDetails ? { error: r.exceptionDetails.exception?.description ?? r.exceptionDetails.text } : r.result.value
  }
  await call('Emulation.setDeviceMetricsOverride', { width: w, height: h, deviceScaleFactor: 1, mobile: false })
  await call('Page.navigate', { url: `${origin}/probe/voicenote.html?${q}${control ? '&control=1' : ''}` })
  let pressed = false
  for (let i = 0; i < 80 && !pressed; i++) {
    await sleep(150)
    pressed = (await evaluate(PRESS)) === true
  }
  let last = null
  for (let i = 0; pressed && i < 80; i++) {
    await sleep(150)
    const now = await evaluate(MEASURE)
    const ready = now && !now.error && (!control || now.note > 0 || now.paneBody > 0 || now.page > 0)
    if (ready && last && JSON.stringify(now) === JSON.stringify(last)) break
    last = now
  }
  let after = null
  if (last && !last.error && !control) {
    await evaluate(DISMISS)
    await sleep(300)
    after = await evaluate(AFTER)
  }
  await browser.call('Target.closeTarget', { targetId })
  return { m: last, after }
}

/** What is wrong with a measure, as words; empty when nothing is. */
function faults(m) {
  const f = []
  if (m.page > 0) f.push(`the page scrolls sideways by ${m.page} px`)
  if (m.paneBody > 0) f.push(`the pane's box scrolls sideways by ${m.paneBody} px`)
  if (m.note > 0) f.push(`the note is ${m.note} px wider than it shows`)
  if (!m.noteOn) f.push('the note is not on screen')
  if (!m.dismissOn) f.push('Got it is not reachable')
  if (!m.f1On) f.push('the F1 slot is not reachable')
  if (!m.dockOn) f.push('the TX dock is covered')
  return f
}

let code = 0
try {
  for (const { w, h, q } of SIZES) {
    const { m, after } = await measure(w, h, q, false)
    console.log(`voicenote-probe: window ${w}×${h} ${q}: ${JSON.stringify(m)} after Got it: ${JSON.stringify(after)}`)
    if (!m || m.error) {
      console.error(`voicenote-probe: the note never drew at ${w}×${h} ${q}`)
      code = 2
      continue
    }
    const red = faults(m)
    if (!after?.gone) red.push('Got it did not take the note away')
    if (after?.stored !== '1') red.push('Got it did not record the dismissal')
    if (red.length) {
      console.error(`voicenote-probe: RED at ${w}×${h} ${q} — ${red.join('; ')}`)
      code = code || 1
    }
    const c = await measure(w, h, q, true)
    const caught = c.m && !c.m.error ? faults(c.m) : []
    console.log(`voicenote-probe: CONTROL window ${w}×${h} ${q}: caught ${JSON.stringify(caught)}`)
    if (!caught.some((s) => s.includes('sideways') || s.includes('wider'))) {
      console.error(`voicenote-probe: CONTROL FAILED at ${w}×${h} ${q} — the planted overflow was not seen; the check cannot fail`)
      code = code || 1
    }
  }
  if (code === 0) console.log(`voicenote-probe: PASS — the note, Got it and F1 on screen with nothing sideways and the dock clear, dismissed by Got it, at ${SIZES.map((s) => `${s.w}×${s.h} ${s.q}`).join(', ')}; each control caught`)
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
