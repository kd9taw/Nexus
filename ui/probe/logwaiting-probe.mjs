// The Operate strip's Log button and its count of contacts waiting for Log QSO in real Chrome, for what jsdom
// cannot see: `scripts/browser-probe --desktop logwaiting`.
//
// The probe serves src/probe/logwaiting.tsx with the app's own Vite config at 1024×768 (the supported floor) and
// 1920×1080, in the dark and the light theme, mid-contact and with no station on screen (after Call CQ), and
// asserts:
//   · nothing scrolls sideways;
//   · every control in the strip stands exactly where it stands with no count (1 and 12 waiting): the count never
//     moves or wraps Log or anything beside it;
//   · the count is drawn whole inside the strip (the strip clips), shows the number, and covers the centre of no
//     control; Log is the topmost thing at its centre;
//   · with no station on screen, Log is live while contacts wait, and a real click on it reaches Log once.
//
// THE CONTROL, run every time: the same page with the count put back in the flow, wide. The probe must see the
// controls move; if it does not, the check cannot fail and the probe says so instead.
//
// exit: 0 every size clear · 1 a red, or a control that could not fail · 2 a page never drew the strip
import { createServer } from 'vite'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const SIZES = [
  { w: 1024, h: 768 },
  { w: 1920, h: 1080 },
]
const THEMES = ['dark', 'light']
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

const MEASURE = `(() => {
  const strip = document.querySelector('.cockpit-qso')
  const log = strip && strip.querySelector('.cq-log')
  if (!strip || !log) return null
  const box = (el) => { const r = el.getBoundingClientRect(); return { x: Math.round(r.left), y: Math.round(r.top), w: Math.round(r.width), h: Math.round(r.height) } }
  const topmostAt = (el, x, y) => { const hit = document.elementFromPoint(x, y); return hit != null && el.contains(hit) }
  const centre = (el) => { const r = el.getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2] }
  const controls = [...strip.querySelectorAll('button, input')].filter((el) => el.getBoundingClientRect().width > 0)
  const badge = strip.querySelector('.cq-log-waiting')
  const s = strip.getBoundingClientRect()
  let badgeInside = null, covers = []
  if (badge) {
    const b = badge.getBoundingClientRect()
    badgeInside = b.width > 0 && b.left >= s.left && b.right <= s.right && b.top >= s.top && b.bottom <= s.bottom
    covers = controls.filter((el) => { const [x, y] = centre(el); return x >= b.left && x <= b.right && y >= b.top && y <= b.bottom }).map((el) => el.textContent.trim() || el.getAttribute('aria-label'))
  }
  return {
    page: document.scrollingElement.scrollWidth - document.scrollingElement.clientWidth,
    controls: controls.map((el) => (el.textContent.trim() || el.getAttribute('aria-label') || el.tagName) + '@' + JSON.stringify(box(el))),
    log: box(log),
    logOn: topmostAt(log, ...centre(log)),
    logDisabled: log.disabled,
    badge: badge ? { text: badge.textContent, box: box(badge), inside: badgeInside, covers } : null,
  }
})()`

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-logwaiting-probe-'))
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

async function page(w, h, q) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true })
    return r.exceptionDetails ? { error: r.exceptionDetails.exception?.description ?? r.exceptionDetails.text } : r.result.value
  }
  await call('Emulation.setDeviceMetricsOverride', { width: w, height: h, deviceScaleFactor: 1, mobile: false })
  await call('Page.navigate', { url: `${origin}/probe/logwaiting.html?${q}` })
  let last = null
  for (let i = 0; i < 80; i++) {
    await sleep(150)
    const now = await evaluate(MEASURE)
    if (now && !now.error && last && JSON.stringify(now) === JSON.stringify(last)) break
    last = now
  }
  const click = async (x, y) => {
    for (const type of ['mousePressed', 'mouseReleased'])
      await call('Input.dispatchMouseEvent', { type, x, y, button: 'left', buttons: 1, clickCount: 1 })
    await sleep(200)
  }
  return { targetId, evaluate, click, m: last }
}

/** What differs between a strip with no count and one with a count, as words; empty when nothing does. */
function moved(none, some) {
  const f = []
  if (none.controls.length !== some.controls.length) f.push(`${none.controls.length} controls became ${some.controls.length}`)
  none.controls.forEach((c, i) => {
    if (some.controls[i] !== c) f.push(`moved: ${c} → ${some.controls[i]}`)
  })
  return f
}

let code = 0
try {
  for (const { w, h } of SIZES) {
    for (const theme of THEMES) {
      for (const dx of ['1', '0']) {
        const at = `${w}×${h} ${theme}${dx === '0' ? ' (no station on screen)' : ''}`
        const base = await page(w, h, `waiting=0&dx=${dx}&theme=${theme}`)
        await browser.call('Target.closeTarget', { targetId: base.targetId })
        if (!base.m || base.m.error) {
          console.error(`logwaiting-probe: the strip never drew at ${at}: ${JSON.stringify(base.m)}`)
          code = 2
          continue
        }
        const red = []
        if (base.m.badge) red.push('a count shows with none waiting')
        if (dx === '0' && !base.m.logDisabled) red.push('Log is live with no station and none waiting')
        for (const n of [1, 12]) {
          const p = await page(w, h, `waiting=${n}&dx=${dx}&theme=${theme}`)
          const m = p.m
          if (!m || m.error) {
            red.push(`${n} waiting: the strip never drew`)
            await browser.call('Target.closeTarget', { targetId: p.targetId })
            continue
          }
          if (m.page > 0) red.push(`${n} waiting: the page scrolls sideways by ${m.page} px`)
          red.push(...moved(base.m, m).map((s) => `${n} waiting: ${s}`))
          if (!m.badge) red.push(`${n} waiting: no count`)
          else {
            if (m.badge.text !== String(n)) red.push(`${n} waiting: the count reads ${m.badge.text}`)
            if (!m.badge.inside) red.push(`${n} waiting: the count is clipped by the strip ${JSON.stringify(m.badge.box)}`)
            if (m.badge.covers.length) red.push(`${n} waiting: the count covers ${m.badge.covers.join(', ')}`)
          }
          if (!m.logOn) red.push(`${n} waiting: Log is covered`)
          if (m.logDisabled) red.push(`${n} waiting: Log is off`)
          if (!m.logDisabled) {
            await p.click(m.log.x + m.log.w / 2, m.log.y + m.log.h / 2)
            const calls = await p.evaluate('window.__calls()')
            if (JSON.stringify(calls) !== '["log"]') red.push(`${n} waiting: a click on Log asked ${JSON.stringify(calls)}`)
          }
          console.log(`logwaiting-probe: window ${at}, ${n} waiting: Log ${JSON.stringify(m.log)} count ${JSON.stringify(m.badge)}`)
          await browser.call('Target.closeTarget', { targetId: p.targetId })
        }
        if (red.length) {
          console.error(`logwaiting-probe: RED at ${at} — ${red.join('; ')}`)
          code = code || 1
        }
        const c = await page(w, h, `waiting=2&dx=${dx}&theme=${theme}&control=1`)
        await browser.call('Target.closeTarget', { targetId: c.targetId })
        const caught = c.m && !c.m.error ? moved(base.m, c.m) : []
        console.log(`logwaiting-probe: CONTROL window ${at}: caught ${caught.length} moves`)
        if (!caught.length) {
          console.error(`logwaiting-probe: CONTROL FAILED at ${at} — the count in the flow moved nothing; the check cannot fail`)
          code = code || 1
        }
      }
    }
  }
  if (code === 0) console.log(`logwaiting-probe: PASS — the count of contacts waiting for Log QSO moves no control, is drawn whole and covers none, Log stays topmost and live and a click reaches it, at ${SIZES.map((s) => `${s.w}×${s.h}`).join(', ')} in ${THEMES.join(' and ')}, with and without a station on screen; each control caught`)
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
