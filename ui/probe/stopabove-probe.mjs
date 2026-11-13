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
// THE FILLED IOTA BOX, in the two log forms that have no box of their own: the prompt to log (`overlay=logconfirm`,
// opened with its box filled) and the log strip (`overlay=strip`, a call typed with real key input), each with the
// station's reference (EU-901, "from the spot") under a very long group name (`&iota=1`), at 1024×768 and 1920×1080
// at the smallest (65 %) and largest (175 %) UI scale. Asserted: the box holds the reference and is the topmost thing
// at its centre, the group name is drawn whole inside its form and covers the centre of no control, the source is
// shown, nothing scrolls sideways (the page none; the prompt none at all — its two columns of boxes once ran ~80 px
// past its card at every scale; the form's content runs no wider than the same form's with no box), and Log is inside
// the window and topmost (the prompt's once, the strip's once scrolled to). Its CONTROL, per form (`&iotacontrol=1`, 1024×768 at 175 %): the name may not wrap, and
// must come back NOT drawn whole.
//
// THE ISLAND PICKER beside that box (the group's islands answered, one with a 200-character name, which the probe
// picks). Asserted, at the same sizes, against the same form with NO picker (`&islands=0`, the layout before it):
//   (a) Log, Stop TX and every other control of the form stay inside the window and are no narrower than without the
//       picker (to 1 px), and the form's content runs no wider than without it;
//   (b) the picker is usable: inside its form, the topmost thing at its centre, at least 6em wide, holding the long
//       name, and that name in its title when the box cuts it short.
// Its CONTROL, per form (`&islandcontrol=1`, 1024×768 at 175 %): the picker's containment taken away, so the select
// is as wide as its longest option again; it must come back RED.
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

/** The filled IOTA box of `form` ('logconfirm' | 'strip'), as drawn. */
const IOTA_MEASURE = (form) => `(() => {
  const host = document.querySelector(${JSON.stringify(form === 'logconfirm' ? '.logconfirm' : '.probe-below')})
  const field = host && host.querySelector('.iota-offer')
  const input = field && field.querySelector('input')
  const name = field && field.querySelector('.iota-name-text')
  if (!input || input.value !== 'EU-901' || !name) return { waiting: true, value: input ? input.value : null }
  field.scrollIntoView({ block: 'nearest' })
  const r = (el) => el.getBoundingClientRect()
  const top = (el) => {
    const b = r(el)
    if (b.width === 0 || b.height === 0) return false
    const hit = document.elementFromPoint(b.left + b.width / 2, b.top + b.height / 2)
    return !!hit && (hit === el || el.contains(hit))
  }
  const h = r(host), n = r(name)
  const controls = [...host.querySelectorAll('input, button, select, textarea')].filter((el) => r(el).width > 0)
  const covers = controls.filter((el) => { const b = r(el); const x = b.left + b.width / 2, y = b.top + b.height / 2; return x >= n.left && x <= n.right && y >= n.top && y <= n.bottom })
    .map((el) => el.getAttribute('aria-label') || el.textContent.trim() || el.tagName)
  const zoom = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--ui-zoom')) || 1
  const line = parseFloat(getComputedStyle(name).lineHeight) || parseFloat(getComputedStyle(name).fontSize) * 1.2 * zoom
  const inputTop = top(input)
  const source = field.querySelector('.iota-offer-from')?.textContent.trim() ?? null
  // The island picker, when the station answered islands: pick the island with the longest name.
  const pick = field.querySelector('.iota-island-pick')
  let island = null
  if (pick) {
    const long = [...pick.options].reduce((a, o) => (o.textContent.length > a.textContent.length ? o : a))
    if (pick.value !== long.value) {
      pick.value = long.value
      pick.dispatchEvent(new Event('change', { bubbles: true }))
    }
    pick.scrollIntoView({ block: 'nearest' })
    const p = r(pick)
    const em = parseFloat(getComputedStyle(pick).fontSize)
    island = {
      picked: pick.value === long.value,
      label: long.textContent.length,
      width: Math.round(p.width),
      wideEnough: p.width >= 6 * em,
      inside: p.left >= h.left - 0.5 && p.right <= h.right + 0.5,
      top: top(pick),
      cut: pick.scrollWidth > pick.clientWidth + 1 || long.textContent.length * em * 0.4 > p.width,
      titled: pick.title === long.textContent,
    }
  }
  // Every control of the form outside the IOTA row, and Stop TX, by a name: where each is and how wide.
  const name_ = (el, i) => (el.getAttribute('aria-label') || el.getAttribute('title') || el.textContent.trim().slice(0, 24) || el.tagName) + '#' + i
  const placed = (el) => { const b = r(el); return { w: Math.round(b.width), in: b.left >= -0.5 && b.top >= -0.5 && b.right <= innerWidth + 0.5 && b.bottom <= innerHeight + 0.5 } }
  const widths = {}
  ;[...host.querySelectorAll('input, button, select, textarea')].filter((el) => !field.contains(el) && r(el).width > 0)
    .forEach((el, i) => { el.scrollIntoView({ block: 'nearest' }); widths[name_(el, i)] = placed(el) })
  const stop = document.querySelector('.topbar .op-btn.stop')
  if (stop) {
    stop.scrollIntoView({ block: 'nearest' })
    widths['Stop TX'] = placed(stop)
  }
  const log = ${form === 'logconfirm' ? `host.querySelector('.logconfirm-log')` : `[...host.querySelectorAll('button')].find((b) => /^log$/i.test(b.textContent.trim()))`}
  if (log) log.scrollIntoView({ block: 'nearest' })
  const l = log && r(log)
  return {
    zoom,
    page: document.scrollingElement.scrollWidth - document.scrollingElement.clientWidth,
    overflow: host.scrollWidth - host.clientWidth,
    extent: host.scrollWidth,
    form: { left: Math.round(h.left), right: Math.round(h.right) },
    name: { left: Math.round(n.left), right: Math.round(n.right), top: Math.round(n.top), bottom: Math.round(n.bottom) },
    nameLines: Math.round(n.height / line),
    nameWhole: n.width > 0 && n.left >= h.left - 0.5 && n.right <= h.right + 0.5 && name.scrollWidth <= name.clientWidth + 1,
    covers, inputTop, source,
    // To half a pixel: a scroll offset is whole pixels and a box at a fractional UI scale is not, so a Log scrolled
    // into view can sit a fraction of a pixel past the edge (0.17 px at 1024×768 175 % once the picker's line moves it).
    logInWindow: !!l && l.left >= -0.5 && l.top >= -0.5 && l.right <= innerWidth + 0.5 && l.bottom <= innerHeight + 0.5,
    logTop: !!log && top(log),
    logWidth: l ? Math.round(l.width) : null,
    logBox: l ? [l.left, l.top, l.right, l.bottom, innerWidth, innerHeight].map((v) => Math.round(v * 100) / 100) : null,
    island, widths,
  }
})()`

/** How wide `form`'s content runs with NO IOTA box (no `&iota=1`, its `scrollWidth`): the baseline the box may add
 *  nothing to. Its content's extent is compared, not `scrollWidth - clientWidth`: a box that makes the prompt taller
 *  brings in its vertical scrollbar (the internal scroll it is given), which narrows the client box by the bar's width
 *  without anything drawn any wider. */
const BARE = (form) => `(() => {
  const host = document.querySelector(${JSON.stringify(form === 'logconfirm' ? '.logconfirm' : '.probe-below')})
  return host ? host.scrollWidth : null
})()`

async function measureBare(form, w, h, zoom) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  try {
    await call('Emulation.setDeviceMetricsOverride', { width: w, height: h, deviceScaleFactor: 1, mobile: false })
    await call('Page.navigate', { url: `${origin}/probe/stopabove.html?overlay=${form}&zoom=${zoom}` })
    let last = null
    for (let i = 0; i < 60; i++) {
      await sleep(150)
      const r = await call('Runtime.evaluate', { expression: BARE(form), returnByValue: true })
      const now = r.exceptionDetails ? null : r.result.value
      if (now != null && now === last) return now
      last = now
    }
    return last
  } finally {
    await browser.call('Target.closeTarget', { targetId })
  }
}

async function measureIota(form, w, h, zoom, control, extra = '') {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true })
    return r.exceptionDetails ? null : r.result.value
  }
  try {
    await call('Emulation.setDeviceMetricsOverride', { width: w, height: h, deviceScaleFactor: 1, mobile: false })
    await call('Page.navigate', { url: `${origin}/probe/stopabove.html?overlay=${form}&iota=1&zoom=${zoom}${control ? '&iotacontrol=1' : ''}${extra}` })
    if (form === 'strip') {
      let focused = false
      for (let i = 0; i < 100 && !focused; i++) {
        await sleep(100)
        focused = await evaluate(`(() => { const i = document.querySelector('input.le-call'); if (!i) return false; i.focus(); return document.activeElement === i })()`)
      }
      if (!focused) return null
      await call('Input.insertText', { text: 'IA5X' })
    }
    let last = null
    for (let i = 0; i < 100; i++) {
      await sleep(150)
      const now = await evaluate(IOTA_MEASURE(form))
      if (now && !now.waiting && last && !last.waiting && JSON.stringify(now) === JSON.stringify(last)) break
      last = now
    }
    return last
  } finally {
    await browser.call('Target.closeTarget', { targetId })
  }
}

const IOTA_SIZES = [
  { w: 1024, h: 768, zoom: 0.65 },
  { w: 1024, h: 768, zoom: 1.75 },
  { w: 1920, h: 1080, zoom: 0.65 },
  { w: 1920, h: 1080, zoom: 1.75 },
]
const IOTA_FORMS = ['logconfirm', 'strip']

/** What the island picker breaks, measured `m` against the same form with no picker (`base`): (a) the form's
 *  controls and Stop TX inside the window and no narrower, no more sideways scroll; (b) the picker usable. */
function islandReds(m, base, at) {
  const red = []
  if (!m.island) return ['the island picker never drew']
  if (!base || base.waiting) return ['the form never drew without the picker']
  console.log(`stopabove-probe: island picker, ${at}: ${JSON.stringify(m.island)}; widths with/without it: ${JSON.stringify(
    Object.fromEntries(Object.entries(base.widths).map(([k, b]) => [k, [m.widths[k]?.w ?? null, b.w]])),
  )}; content extent ${m.extent}/${base.extent}, sideways scroll ${m.overflow}/${base.overflow}`)
  for (const [k, b] of Object.entries(base.widths)) {
    const now = m.widths[k]
    if (!now) red.push(`${k} is gone`)
    else if (!now.in) red.push(`${k} is outside the window`)
    else if (now.w < b.w - 1) red.push(`${k} is ${now.w} px wide, ${b.w} without the picker`)
  }
  if (!m.logInWindow) red.push('Log is outside the window')
  if (m.extent > base.extent + 1) red.push(`the picker makes the form run wider: ${m.extent} px, ${base.extent} without it`)
  const i = m.island
  if (!i.picked) red.push('the long-named island was not picked')
  if (!i.inside) red.push('the picker is not inside its form')
  if (!i.top) red.push('the picker is covered')
  if (!i.wideEnough) red.push(`the picker is ${i.width} px wide, under 6em`)
  if (i.cut && !i.titled) red.push('the picked name is cut short with no title')
  return red
}

let code = 0
let ran = 0
try {
  for (const form of IOTA_FORMS) {
    for (const { w, h, zoom } of IOTA_SIZES) {
      const at = `${form} ${w}×${h} at ${Math.round(zoom * 100)}%`
      const m = await measureIota(form, w, h, zoom, false)
      const bare = await measureBare(form, w, h, zoom)
      console.log(`stopabove-probe: filled IOTA box, ${at}: ${JSON.stringify(m)} (no box: content extent ${bare})`)
      if (!m || m.waiting) {
        console.error(`stopabove-probe: the IOTA box never filled in the ${at}`)
        code = 2
        continue
      }
      const red = []
      if (Math.abs(m.zoom - zoom) > 0.001) red.push(`the UI scale is ${m.zoom}, not ${zoom}`)
      if (m.page > 0) red.push(`the page scrolls sideways by ${m.page} px`)
      if (bare == null) red.push('the form never drew without the box')
      else if (m.extent > bare + 1) red.push(`the box makes the form run wider: ${m.extent} px, ${bare} without it`)
      if (form === 'logconfirm' && m.overflow > 0) red.push(`the prompt to log scrolls sideways by ${m.overflow} px`)
      if (!m.nameWhole) red.push(`the group name is not drawn whole inside its form ${JSON.stringify(m.name)} vs ${JSON.stringify(m.form)}`)
      if (m.covers.length) red.push(`the group name covers ${m.covers.join(', ')}`)
      if (!m.inputTop) red.push('the IOTA box is covered')
      if (m.source !== 'from the spot') red.push(`the source reads ${JSON.stringify(m.source)}`)
      if (!m.logInWindow) red.push('Log is outside the window')
      if (!m.logTop) red.push('Log is covered')
      red.push(...islandReds(m, await measureIota(form, w, h, zoom, false, '&islands=0'), at))
      if (red.length) {
        console.error(`stopabove-probe: RED (filled IOTA box) at ${at} — ${red.join('; ')}`)
        code = code || 1
      }
    }
    const ic = await measureIota(form, 1024, 768, 1.75, false, '&islandcontrol=1')
    const icReds = ic && !ic.waiting ? islandReds(ic, await measureIota(form, 1024, 768, 1.75, false, '&islands=0'), 'control') : ['never drew']
    console.log(`stopabove-probe: CONTROL island picker uncontained, ${form} 1024×768 at 175%: ${JSON.stringify(icReds)}`)
    if (icReds.length === 0) {
      console.error(`stopabove-probe: CONTROL FAILED (${form}) — the uncontained island picker still read as clear; the check cannot fail`)
      code = code || 1
    }
    const c = await measureIota(form, 1024, 768, 1.75, true)
    console.log(`stopabove-probe: CONTROL group name kept on one line, ${form} 1024×768 at 175%: ${JSON.stringify(c)}`)
    if (!c || c.waiting || c.nameWhole) {
      console.error(`stopabove-probe: CONTROL FAILED (${form}) — the unwrapped group name still read as drawn whole; the check cannot fail`)
      code = code || 1
    }
  }
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
  if (code === 0) console.log(`stopabove-probe: PASS — ${ran} cases: with ${OVERLAYS.map(([o]) => o).join(', ')} open, Stop TX is on top where it is drawn, unmoved, one real click and one tap each stop once, the mirror is unseen by screen readers and follows its button at once, the overlay stays open and the keyboard stays where it was, at ${SIZES.map(([w, h, v]) => `${w}×${h} (${v})`).join(' and ')}; each control (no stop layer) caught. The filled IOTA box of the prompt to log and of the log strip at ${IOTA_SIZES.map((s) => `${s.w}×${s.h} ${Math.round(s.zoom * 100)}%`).join(', ')}: filled, its long group name whole and covering nothing, the source shown, no sideways scroll (the page none; the prompt none at all; the strip no more than without the box), Log in the window and on top; each unwrapped-name control caught. The island picker beside it, with a 200-character island picked: Log, Stop TX and every other control in the window and no narrower than without the picker, no sideways scroll added, the picker inside its form, on top, at least 6em wide and titled; each uncontained-picker control caught`)
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
