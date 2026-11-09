// STOP TX ABOVE EVERY OVERLAY, in real Chrome: `scripts/browser-probe --desktop stopabove`.
//
// The operator's ruling, "always clickable on top": Stop TX and the docks' stop controls stand above every overlay
// and stay clickable, a click on them stops, and the overlay stays open (components/StopLayer.tsx). jsdom cannot
// stack or hit-test, so this is where the click itself is proved. The page is probe/stopabove.html: the real TopBar
// as the Logbook gives it, inside the real `.app`, with the real StopLayer and one overlay over it (src/probe/
// stopabove.tsx). For every overlay kind — a ui/Dialog, a ConfirmHost question, the prompt to log, the launch radio
// picker, an aria-modal Logbook scrim, and the band pill's menu (opened with a real click, and drawn over Stop TX
// at xs) — at 700×800 (xs) and 2560×1440 (xl), it asserts:
//   1. STOP TX IS ON TOP WHERE IT IS DRAWN: on a 5 × 3 grid across the button, the topmost thing at some point is
//      Stop TX or its mirror in the stop layer, and the mirror (when there is one) stands exactly where the button
//      does, to the pixel — Stop TX does not move, and it is where it was before the overlay opened;
//   2. ONE REAL CLICK THERE STOPS ONCE: halts +1;
//   3. THE OVERLAY STAYS OPEN, and the keyboard stays where it was (in the dialog, for the ones that had it) — for a
//      press on a mirror. A menu that does not cover Stop TX (the band pill's at xl) leaves it no mirror: that click
//      is Stop TX's own, and the menu closes and lets it through.
//   4. THE MIRROR IS A SECOND STOP TX AND IS HELD TO IT: one TAP stops once too; no node of the mirror is exposed in
//      Chrome's accessibility tree; and it follows its button before the next frame (disabled at once, enabled again
//      at once, gone at once when the button goes behind [hidden]).
// THE CONTROL, run at every case where a mirror took the click: the same page with the stop layer taken away (`.stop-layer { display: none }`).
// The same click must stop NOTHING; if it stops, the check cannot fail and the probe says so instead of passing.
//
// exit: 0 every case clear · 1 a red, or a control that could not fail · 2 the page never drew
import { createServer } from 'vite'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
/** [window W, H, the size class it must come out as]. */
const SIZES = [[700, 800, 'xs'], [2560, 1440, 'xl']]
/** [overlay, how to tell it is open]. */
const OVERLAYS = [
  ['dialog', `document.querySelector('.ui-dialog') != null`],
  ['confirm', `document.querySelector('.ui-dialog') != null`],
  ['logconfirm', `document.querySelector('.logconfirm-backdrop') != null`],
  ['radiopicker', `document.querySelector('.radio-picker-overlay') != null`],
  ['scrim', `document.querySelector('.logconfirm-backdrop .logconfirm') != null`],
  ['bandmenu', `document.querySelector('[role="menu"]') != null`],
]
const NO_LAYER = '.stop-layer { display: none !important }'
const STOP = '.topbar .op-btn.stop'
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

const BOX = `(() => {
  const b = document.querySelector(${JSON.stringify(STOP)})
  if (!b) return null
  const q = b.getBoundingClientRect()
  return [q.left, q.top, q.width, q.height].map((v) => Math.round(v * 10) / 10)
})()`
/** A point of Stop TX where it, or its mirror, is the topmost thing; and the mirror's box. */
const ON_TOP = `(() => {
  const b = document.querySelector(${JSON.stringify(STOP)})
  if (!b) return null
  const q = b.getBoundingClientRect()
  const m = document.querySelector('.stop-layer [data-stop-mirror]')
  const mq = m?.getBoundingClientRect()
  let point = null, onTop = null
  for (let j = 1; j <= 3 && !point; j++) for (let i = 1; i <= 5 && !point; i++) {
    const x = q.left + (q.width * i) / 6, y = q.top + (q.height * j) / 4
    const t = document.elementFromPoint(x, y)
    if (t && (t === b || b.contains(t) || (m && (t === m || m.contains(t))))) point = [x, y]
    else if (!onTop) onTop = t ? (t.className || t.tagName) : null
  }
  return { point, onTop, mirror: mq ? [mq.left, mq.top, mq.width, mq.height].map((v) => Math.round(v * 10) / 10) : null }
})()`

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-stopabove-probe-'))
const server = await createServer({
  root: UI, configFile: join(UI, 'vite.config.ts'), cacheDir, logLevel: 'warn',
  server: { host: '127.0.0.1', port: 0, strictPort: false, hmr: false },
})
await server.listen()
const origin = `http://127.0.0.1:${server.httpServer.address().port}`
const browser = await chrome()

async function run([W, H], [overlay, isOpen], control) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text)
    return r.result.value
  }
  const until = async (expression, tries = 60) => {
    for (let i = 0; i < tries; i++) {
      await sleep(150)
      const v = await evaluate(expression).catch(() => null)
      if (v) return v
    }
    return null
  }
  // The box once it holds still for three looks in a row (the bar settles as its fonts and chips come in).
  const settled = async () => {
    let last = null, same = 0
    for (let i = 0; i < 60; i++) {
      await sleep(150)
      const v = JSON.stringify(await evaluate(BOX).catch(() => null))
      same = v !== 'null' && v === last ? same + 1 : 0
      last = v
      if (same >= 2) return JSON.parse(v)
    }
    return null
  }
  const click = async ([x, y]) => {
    await call('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y })
    await call('Input.dispatchMouseEvent', { type: 'mousePressed', x, y, button: 'left', clickCount: 1 })
    await call('Input.dispatchMouseEvent', { type: 'mouseReleased', x, y, button: 'left', clickCount: 1 })
  }
  try {
    await call('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: 1, mobile: false })
    // The box Stop TX has with no overlay at all: the overlay must not move it.
    await call('Page.navigate', { url: `${origin}/probe/stopabove.html?overlay=none` })
    if (!(await until(`document.documentElement.getAttribute('data-viewport') && ${BOX}`))) return { error: 'the page never drew Stop TX' }
    const bare = await settled()
    await call('Page.navigate', { url: `${origin}/probe/stopabove.html?overlay=${overlay}` })
    if (!(await until(`document.documentElement.getAttribute('data-viewport') && ${BOX}`))) return { error: 'the page never drew Stop TX' }
    const vp = await evaluate(`document.documentElement.getAttribute('data-viewport')`)
    if (overlay === 'bandmenu') {
      const at = await evaluate(`(() => { const b = document.querySelector('.topbar button.freq-channel'); if (!b) return null; const q = b.getBoundingClientRect(); return [q.left + q.width / 2, q.top + q.height / 2] })()`)
      if (!at) return { vp, error: 'no band pill' }
      await click(at)
    }
    if (!(await until(isOpen, 30))) return { vp, error: 'the overlay never opened' }
    if (control) await evaluate(`(() => { const s = document.createElement('style'); s.textContent = ${JSON.stringify(NO_LAYER)}; document.head.appendChild(s); return 1 })()`)
    const box = await settled()
    const top = await evaluate(ON_TOP)
    const focusBefore = await evaluate(`document.activeElement ? (document.activeElement.className || document.activeElement.tagName) : null`)
    // A covered button is clicked at its centre, so a red also says what that click did.
    const point = top.point ?? [box[0] + box[2] / 2, box[1] + box[3] / 2]
    const before = await evaluate('window.__probeHalts()')
    await click(point)
    await sleep(400)
    const out = {
      vp,
      bare,
      box,
      ...top,
      reach: top.point != null,
      point: point.map(Math.round),
      halts: (await evaluate('window.__probeHalts()')) - before,
      stillOpen: await evaluate(isOpen),
      focusBefore,
      focusAfter: await evaluate(`document.activeElement ? (document.activeElement.className || document.activeElement.tagName) : null`),
    }
    if (control || !top.mirror) return out
    // THE MIRROR IS A SECOND STOP TX, so three more things, on the mirror the click just used:
    // (a) one TAP is one stop, as one click was;
    const tapBefore = await evaluate('window.__probeHalts()')
    await call('Emulation.setTouchEmulationEnabled', { enabled: true, maxTouchPoints: 1 })
    await call('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x: point[0], y: point[1] }] })
    await call('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] })
    await sleep(400)
    out.tapHalts = (await evaluate('window.__probeHalts()')) - tapBefore
    await call('Emulation.setTouchEmulationEnabled', { enabled: false })
    // (b) screen readers see no second Stop TX: no exposed node of the accessibility tree is the mirror or inside it;
    await call('Accessibility.enable', {})
    const { nodes } = await call('Accessibility.getFullAXTree', {})
    const { result } = await call('Runtime.evaluate', { expression: `document.querySelector('.stop-layer')` })
    const { node } = await call('DOM.describeNode', { objectId: result.objectId, depth: -1 })
    const inLayer = new Set()
    const walk = (n) => { inLayer.add(n.backendNodeId); for (const c of n.children ?? []) walk(c) }
    walk(node)
    out.mirrorExposed = nodes.filter((n) => !n.ignored && inLayer.has(n.backendDOMNodeId)).length
    // (c) the mirror follows its button before the next frame: disabled, it is disabled; behind [hidden], it is gone.
    out.follows = await evaluate(`(async () => {
      const b = document.querySelector(${JSON.stringify(STOP)})
      const mirror = () => document.querySelector('.stop-layer [data-stop-mirror]')
      b.disabled = true
      await Promise.resolve()
      const disabled = mirror()?.disabled === true
      b.disabled = false
      await Promise.resolve()
      const enabled = mirror()?.disabled === false
      b.closest('.app').hidden = true
      await Promise.resolve()
      return { disabled, enabled, goneHidden: mirror() == null }
    })()`)
    return out
  } finally {
    await browser.call('Target.closeTarget', { targetId })
  }
}

const same = (a, b) => a && b && a.every((v, i) => Math.abs(v - b[i]) <= 1)

let code = 0
let ran = 0
try {
  for (const size of SIZES) {
    for (const ov of OVERLAYS) {
      const tag = `${size[0]}×${size[1]} ${ov[0]}`
      const r = await run(size, ov, false)
      console.log(`stopabove-probe: ${tag}: ${JSON.stringify(r)}`)
      if (r.error) {
        console.error(`stopabove-probe: at ${tag} ${r.error}`)
        code = r.error === 'the page never drew Stop TX' ? 2 : code || 1
        continue
      }
      ran += 1
      const red = []
      if (r.vp !== size[2]) red.push(`the window came out ${r.vp}, not ${size[2]}`)
      if (!r.reach) red.push(`Stop TX is not on top anywhere (${r.onTop} is)`)
      if (!same(r.box, r.bare)) red.push(`Stop TX moved with the overlay open: ${r.bare} → ${r.box}`)
      if (r.mirror && !same(r.mirror, r.box)) red.push(`the mirror is not where Stop TX is: ${r.mirror} vs ${r.box}`)
      if (r.halts !== 1) red.push(`one click on Stop TX stopped ${r.halts} times, not once`)
      // A press on the mirror leaves the overlay and the keyboard alone. A menu that does not cover Stop TX gets no
      // mirror: the click is Stop TX's own, and the menu closes and lets it through (MenuParts.tsx), as it should.
      if (r.mirror && !r.stillOpen) red.push('the overlay closed')
      if (r.mirror && r.focusAfter !== r.focusBefore) red.push(`the keyboard moved: ${r.focusBefore} → ${r.focusAfter}`)
      if (!r.mirror && ov[0] !== 'bandmenu') red.push('no mirror under an overlay that covers the window')
      if (r.mirror) {
        if (r.tapHalts !== 1) red.push(`one tap on the mirror stopped ${r.tapHalts} times, not once`)
        if (r.mirrorExposed !== 0) red.push(`${r.mirrorExposed} node(s) of the mirror are exposed to screen readers`)
        if (!r.follows?.disabled || !r.follows?.enabled) red.push(`the mirror did not follow its button's disabled state at once (${JSON.stringify(r.follows)})`)
        if (!r.follows?.goneHidden) red.push('the mirror outlived its button behind [hidden]')
      }
      if (red.length) {
        console.error(`stopabove-probe: RED at ${tag} — ${red.join('; ')}`)
        code = code || 1
        continue
      }
      // A menu that does not cover Stop TX gave the click to Stop TX itself, which no stop layer is in the way of.
      if (!r.mirror) continue
      const c = await run(size, ov, true)
      console.log(`stopabove-probe: CONTROL ${tag}: ${JSON.stringify(c)}`)
      if (c.error || c.halts !== 0) {
        console.error(`stopabove-probe: CONTROL FAILED at ${tag} — with the stop layer taken away the click still stopped (${c.error ?? `halts ${c.halts}`}); the check cannot fail`)
        code = code || 1
      }
    }
  }
  if (ran === 0) {
    console.error('stopabove-probe: no case ran')
    code = code || 1
  }
  if (code === 0) console.log(`stopabove-probe: PASS — ${ran} cases: with ${OVERLAYS.map(([o]) => o).join(', ')} open, Stop TX is on top where it is drawn, unmoved, one real click and one tap each stop once, the mirror is unseen by screen readers and follows its button at once, the overlay stays open and the keyboard stays where it was, at ${SIZES.map(([w, h, v]) => `${w}×${h} (${v})`).join(' and ')}; each control (no stop layer) caught`)
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
