// The TX strip's ATU button and its "Switch tuner out" menu in real Chrome, for what jsdom cannot see:
// `scripts/browser-probe --desktop atu`.
//
// The probe serves src/probe/atu.tsx with the app's own Vite config at 1024×768 (the supported floor) and
// 1920×1080, in the dark and the light theme, and asserts:
//   · nothing scrolls sideways: the page, the strip and its control group are no wider than they show;
//   · TX On, Tune, ATU and Stop TX are each the topmost thing where they stand;
//   · Stop TX does not move when the button reads ATU ON instead of ATU (the label's width floor);
//   · ATU ON is drawn lit: its background is not the unlit button's;
//   · with real input events: a left click on ATU ON runs a tune-up (`atu_tune`) and nothing else; a
//     right-click opens the menu, whose "Switch tuner out" is on screen and topmost while Stop TX stays
//     topmost (the menu never covers the stop); choosing it switches the tuner out (`atu_bypass`) and runs
//     no tune-up, so React's bubbling out of the menu's portal does not reach the button; and a long touch
//     press opens the same menu without a tune-up.
//
// THE CONTROL, run every time: the same page with a 2400 px label planted in the strip and a box laid over
// everything. The probe must see the sideways overflow and Stop TX covered; if it does not, the check cannot
// fail and the probe says so instead.
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

const BTN = (re) => `[...document.querySelectorAll('.cockpit-txstrip button')].find((b) => ${re}.test(b.textContent.trim()))`
const MEASURE = `(() => {
  const strip = document.querySelector('.cockpit-txstrip')
  if (!strip) return null
  const topmost = (el) => {
    if (!el) return false
    const r = el.getBoundingClientRect()
    if (r.width <= 0 || r.height <= 0) return false
    const x = r.left + r.width / 2, y = r.top + r.height / 2
    if (x < 0 || y < 0 || x > innerWidth || y > innerHeight) return false
    const hit = document.elementFromPoint(x, y)
    return hit != null && el.contains(hit)
  }
  const wider = (el) => el ? el.scrollWidth - el.clientWidth : null
  const atu = ${BTN('/^ATU( ON)?$/')}
  const stop = ${BTN('/^Stop TX$/')}
  const box = (el) => { const r = el.getBoundingClientRect(); return { x: Math.round(r.left), y: Math.round(r.top), w: Math.round(r.width), h: Math.round(r.height) } }
  return {
    page: wider(document.scrollingElement),
    strip: wider(strip),
    group: wider(strip.querySelector('.cq-txctl')),
    atuLabel: atu?.textContent.trim() ?? null,
    atuBg: atu ? getComputedStyle(atu).backgroundColor : null,
    atu: atu ? box(atu) : null,
    stop: stop ? box(stop) : null,
    txOn: topmost(${BTN('/^TX (On|Off)$/')}),
    tuneOn: topmost(${BTN('/^Tune$/')}),
    atuOn: topmost(atu),
    stopOn: topmost(stop),
  }
})()`
const MENU = `(() => {
  const item = [...document.querySelectorAll('[role=menuitem]')].find((m) => m.textContent.trim() === 'Switch tuner out')
  const stop = ${BTN('/^Stop TX$/')}
  const topmost = (el) => {
    if (!el) return false
    const r = el.getBoundingClientRect()
    const x = r.left + r.width / 2, y = r.top + r.height / 2
    if (r.width <= 0 || x < 0 || y < 0 || x > innerWidth || y > innerHeight) return false
    const hit = document.elementFromPoint(x, y)
    return hit != null && el.contains(hit)
  }
  if (!item) return { open: false }
  const r = item.getBoundingClientRect()
  // Under Radix's modal menu the page behind takes no pointer events, so "topmost" for Stop TX is asked of
  // the layout: the menu's box must not overlap it.
  const m = item.closest('[role=menu]').getBoundingClientRect(), s = stop.getBoundingClientRect()
  const overlap = !(m.right <= s.left || m.left >= s.right || m.bottom <= s.top || m.top >= s.bottom)
  return { open: true, itemOn: topmost(item), item: { x: r.left + r.width / 2, y: r.top + r.height / 2 }, coversStop: overlap }
})()`

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-atu-probe-'))
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
  await call('Page.navigate', { url: `${origin}/probe/atu.html?${q}` })
  let last = null
  for (let i = 0; i < 80; i++) {
    await sleep(150)
    const now = await evaluate(MEASURE)
    const ready = now && !now.error && now.atu && (!q.includes('control=1') || now.group > 0 || now.page > 0)
    if (ready && last && JSON.stringify(now) === JSON.stringify(last)) break
    last = now
  }
  const mouse = async (x, y, button, type) =>
    call('Input.dispatchMouseEvent', { type, x, y, button, buttons: button === 'left' ? 1 : 2, clickCount: 1 })
  const click = async (x, y, button = 'left') => {
    await mouse(x, y, button, 'mousePressed')
    await mouse(x, y, button, 'mouseReleased')
    await sleep(200)
  }
  return { targetId, evaluate, call, click, m: last }
}

/** What is wrong with a layout measure, as words; empty when nothing is. */
function faults(m) {
  const f = []
  if (m.page > 0) f.push(`the page scrolls sideways by ${m.page} px`)
  if (m.strip > 0) f.push(`the strip is ${m.strip} px wider than it shows`)
  if (m.group > 0) f.push(`the strip's controls are ${m.group} px wider than they show`)
  if (!m.txOn) f.push('TX On is not on screen')
  if (!m.tuneOn) f.push('Tune is not on screen')
  if (!m.atuOn) f.push('ATU is not on screen')
  if (!m.stopOn) f.push('Stop TX is covered or off screen')
  return f
}

let code = 0
try {
  for (const { w, h } of SIZES) {
    for (const theme of THEMES) {
      const at = `${w}×${h} ${theme}`
      const off = await page(w, h, `atu=off&theme=${theme}`)
      await browser.call('Target.closeTarget', { targetId: off.targetId })
      const on = await page(w, h, `atu=on&theme=${theme}`)
      const red = []
      if (!on.m || on.m.error || !off.m || off.m.error) {
        console.error(`atu-probe: the strip never drew at ${at}: ${JSON.stringify(on.m ?? off.m)}`)
        code = 2
        await browser.call('Target.closeTarget', { targetId: on.targetId })
        continue
      }
      red.push(...faults(on.m), ...faults(off.m).map((f) => `(ATU) ${f}`))
      if (on.m.atuLabel !== 'ATU ON' || off.m.atuLabel !== 'ATU') red.push(`labels ${on.m.atuLabel} / ${off.m.atuLabel}`)
      if (Math.abs(on.m.stop.x - off.m.stop.x) > 1) red.push(`Stop TX moves ${on.m.stop.x - off.m.stop.x} px when ATU reads ATU ON`)
      if (on.m.atuBg === off.m.atuBg) red.push(`ATU ON is not drawn lit (${on.m.atuBg})`)

      // A left click on ATU ON: a tune-up, nothing else.
      const cx = on.m.atu.x + on.m.atu.w / 2, cy = on.m.atu.y + on.m.atu.h / 2
      await on.click(cx, cy)
      const afterClick = await on.evaluate('window.__calls()')
      if (JSON.stringify(afterClick) !== '["atu_tune"]') red.push(`a click on ATU ON asked ${JSON.stringify(afterClick)}, not one tune-up`)
      // A right-click: the menu, on screen, clear of Stop TX; its item switches the tuner out, and only that.
      await on.click(cx, cy, 'right')
      const menu = await on.evaluate(MENU)
      if (!menu.open) red.push('a right-click opened no menu')
      else {
        if (!menu.itemOn) red.push('"Switch tuner out" is not on screen')
        if (menu.coversStop) red.push('the menu covers Stop TX')
        await on.click(menu.item.x, menu.item.y)
        const afterMenu = await on.evaluate('window.__calls()')
        if (JSON.stringify(afterMenu) !== '["atu_tune","atu_bypass"]') red.push(`the menu item asked ${JSON.stringify(afterMenu)}`)
      }
      // A long touch press: the same menu, no tune-up.
      await on.call('Emulation.setTouchEmulationEnabled', { enabled: true, maxTouchPoints: 1 })
      await on.call('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x: cx, y: cy }] })
      await sleep(800)
      await on.call('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] })
      await sleep(300)
      const touch = await on.evaluate(MENU)
      const afterTouch = await on.evaluate('window.__calls()')
      if (!touch.open) red.push('a long touch press opened no menu')
      if (afterTouch.filter((c) => c === 'atu_tune').length !== 1) red.push(`a long touch press tuned: ${JSON.stringify(afterTouch)}`)
      await browser.call('Target.closeTarget', { targetId: on.targetId })

      console.log(`atu-probe: window ${at}: ATU ON ${JSON.stringify(on.m)} ATU ${JSON.stringify(off.m)} menu ${JSON.stringify(menu)} touch ${JSON.stringify(touch)} calls ${JSON.stringify(afterTouch)}`)
      if (red.length) {
        console.error(`atu-probe: RED at ${at} — ${red.join('; ')}`)
        code = code || 1
      }
      const c = await page(w, h, `atu=on&theme=${theme}&control=1`)
      await browser.call('Target.closeTarget', { targetId: c.targetId })
      const caught = c.m && !c.m.error ? faults(c.m) : []
      console.log(`atu-probe: CONTROL window ${at}: caught ${JSON.stringify(caught)}`)
      if (!caught.some((s) => s.includes('wider') || s.includes('sideways')) || !caught.some((s) => s.startsWith('Stop TX'))) {
        console.error(`atu-probe: CONTROL FAILED at ${at} — the planted overflow or cover was not seen; the check cannot fail`)
        code = code || 1
      }
    }
  }
  if (code === 0) console.log(`atu-probe: PASS — TX On, Tune, ATU and Stop TX on screen with nothing sideways, Stop TX still as ATU reads ATU ON, ATU ON lit, a click tunes, the right-click and touch menu switches the tuner out clear of Stop TX, at ${SIZES.map((s) => `${s.w}×${s.h}`).join(', ')} in ${THEMES.join(' and ')}; each control caught`)
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
