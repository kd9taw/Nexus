// The Logbook contact card in real Chrome, for what jsdom cannot see: `scripts/browser-probe --desktop
// contactcard`.
//
// The probe serves src/probe/contactcard.tsx (the real Logbook) with the app's own Vite config at
// 1024×768, 1366×768 and 1920×1080, double-clicks a contact's callsign cell with a real mouse — the
// gesture must reach the row through whatever the cell holds — and asserts the card is drawn whole: its
// box inside the window, scrolling nowhere sideways, the callbook photo drawn, and Save and Cancel the
// topmost thing at their centres. Then it scrolls the card to its foot and asserts the last of the ten
// recent contacts and Save are both on screen and topmost.
//
// The call cell is a station-card trigger; with no card host on the probe page its click falls back to
// opening the QRZ page, so `open_qrz_page` shows among the refused commands — the gesture as the app's
// own call cells take it, not a fault.
//
// THE CONTROL, run every time: at 1024×768 the card with its Save bar put back into the flow
// (`?footer=static`). There Save starts below the fold and must come back NOT on screen; if it does
// not, the card is too short for the check to fail and the probe says so instead of passing.
//
// THE FILLED IOTA BOX (`?iota=1`): the Logbook's Add form opened with a real click, a call typed, and the
// form's IOTA box filled by the station (EU-901, "from the spot") under a very long group name, at
// 1024×768 and 1920×1080 at the smallest (65 %) and largest (175 %) UI scale. Asserted: the box holds
// the reference and is the topmost thing at its centre, the group name is drawn whole inside the form
// and covers the centre of no control, the source is shown, nothing scrolls sideways (page or form), and
// Log is topmost once scrolled to. Its CONTROL (`&iotacontrol=1`, 1024×768 at 175 %): the name may not
// wrap, and must come back NOT drawn whole.
//
// exit: 0 every size clear · 1 a red, or a control that could not fail · 2 the page never drew the card
import { createServer } from 'vite'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const SIZES = [{ w: 1024, h: 768 }, { w: 1366, h: 768 }, { w: 1920, h: 1080 }]
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

/** The card as drawn: its box against the window, the photo, and whether Save, Cancel and the last
 *  recent contact are each the topmost thing at their own centre. */
const MEASURE = `(() => {
  const box = document.querySelector('.contact-card-dialog')
  if (!box) return null
  const r = box.getBoundingClientRect()
  const top = (el) => {
    if (!el) return false
    const b = el.getBoundingClientRect()
    if (b.width === 0 || b.height === 0) return false
    const hit = document.elementFromPoint(b.left + b.width / 2, b.top + b.height / 2)
    return !!hit && (hit === el || el.contains(hit))
  }
  const button = (name) => [...box.querySelectorAll('button')].find((b) => b.textContent.trim() === name)
  const rows = box.querySelectorAll('.contact-card-recent tbody tr')
  const img = box.querySelector('img.callcard-photo')
  return {
    box: { left: Math.round(r.left), top: Math.round(r.top), right: Math.round(r.right), bottom: Math.round(r.bottom) },
    inside: r.left >= 0 && r.top >= 0 && r.right <= innerWidth && r.bottom <= innerHeight,
    sideways: box.scrollWidth > box.clientWidth + 1,
    photo: !!img && img.complete && img.naturalWidth > 0 && getComputedStyle(img).display !== 'none',
    save: top(button('Save')),
    cancel: top(button('Cancel')),
    recent: rows.length,
    lastRecent: top(rows[rows.length - 1]),
    call: box.querySelector('.logbook-field input')?.value ?? null,
    refused: window.__refused(),
  }
})()`

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-contactcard-probe-'))
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

/** The card opened at `w`×`h` by a real double-click on the newest EC1DD row's call: measured as it
 *  opens, and again scrolled to its foot. */
async function measure(w, h, extra = '') {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true })
    return r.exceptionDetails ? null : r.result.value
  }
  await call('Emulation.setDeviceMetricsOverride', { width: w, height: h, deviceScaleFactor: 1, mobile: false })
  await call('Page.navigate', { url: `${origin}/probe/contactcard.html${extra}` })
  let at = null
  for (let i = 0; i < 150 && !at; i++) {
    await sleep(100)
    at = await evaluate(`(() => {
      if (document.documentElement.dataset.lookedUp !== 'yes') return null
      const row = [...document.querySelectorAll('.logbook-row:not(.head)')].find((r) => r.textContent.includes('EC1DD'))
      const cell = row && [...row.querySelectorAll('*')].find((e) => e.children.length === 0 && e.textContent.trim() === 'EC1DD')
      const b = cell?.getBoundingClientRect()
      return b && b.width > 0 ? { x: b.left + b.width / 2, y: b.top + b.height / 2 } : null
    })()`)
  }
  if (!at) {
    await browser.call('Target.closeTarget', { targetId })
    return null
  }
  for (const clickCount of [1, 2]) {
    for (const type of ['mousePressed', 'mouseReleased']) {
      await call('Input.dispatchMouseEvent', { type, x: at.x, y: at.y, button: 'left', clickCount })
    }
  }
  const settle = async () => {
    let last = null
    for (let i = 0; i < 60; i++) {
      await sleep(150)
      const now = await evaluate(MEASURE)
      if (now && last && now.recent > 0 && JSON.stringify(now) === JSON.stringify(last)) return now
      last = now
    }
    return last
  }
  const opened = await settle()
  await evaluate(`(() => { const b = document.querySelector('.contact-card-dialog'); if (b) b.scrollTop = b.scrollHeight })()`)
  const foot = opened ? await settle() : null
  await browser.call('Target.closeTarget', { targetId })
  return opened && { opened, foot }
}

/** The Add form's filled IOTA box at `w`×`h` and UI scale `zoom`, after a real click on Log QSO and a
 *  call typed. */
const IOTA_MEASURE = `(() => {
  const form = document.querySelector('.logbook-form')
  if (!form) return null
  const field = [...form.querySelectorAll('.logbook-field')].find((l) => l.querySelector(':scope > span')?.textContent.trim() === 'IOTA')
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
  const f = r(form), n = r(name)
  const controls = [...form.querySelectorAll('input, button, select, textarea')].filter((el) => r(el).width > 0)
  const covers = controls.filter((el) => { const b = r(el); const x = b.left + b.width / 2, y = b.top + b.height / 2; return x >= n.left && x <= n.right && y >= n.top && y <= n.bottom })
    .map((el) => el.getAttribute('aria-label') || el.textContent.trim() || el.tagName)
  const box = { left: Math.round(n.left), right: Math.round(n.right), top: Math.round(n.top), bottom: Math.round(n.bottom) }
  const nameWhole = n.width > 0 && n.left >= f.left - 0.5 && n.right <= f.right + 0.5 && name.scrollWidth <= name.clientWidth + 1
  const inputTop = top(input)
  const source = field.querySelector('.iota-offer-from')?.textContent.trim() ?? null
  const log = [...form.querySelectorAll('button[type="submit"]')][0]
  log.scrollIntoView({ block: 'nearest' })
  return {
    zoom: parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--ui-zoom')) || 1,
    page: document.scrollingElement.scrollWidth - document.scrollingElement.clientWidth,
    formSideways: form.scrollWidth > form.clientWidth + 1,
    form: { left: Math.round(f.left), right: Math.round(f.right) },
    name: box, nameLines: Math.round(n.height / (parseFloat(getComputedStyle(name).lineHeight) || parseFloat(getComputedStyle(name).fontSize) * 1.2 * (parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--ui-zoom')) || 1))),
    nameWhole, covers, inputTop, source, logTop: top(log),
  }
})()`

async function measureIota(w, h, zoom, control) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true })
    return r.exceptionDetails ? null : r.result.value
  }
  const click = async (x, y) => {
    for (const type of ['mousePressed', 'mouseReleased'])
      await call('Input.dispatchMouseEvent', { type, x, y, button: 'left', buttons: 1, clickCount: 1 })
  }
  await call('Emulation.setDeviceMetricsOverride', { width: w, height: h, deviceScaleFactor: 1, mobile: false })
  await call('Page.navigate', { url: `${origin}/probe/contactcard.html?iota=1&zoom=${zoom}${control ? '&iotacontrol=1' : ''}` })
  let open = null
  for (let i = 0; i < 150 && !open; i++) {
    await sleep(100)
    open = await evaluate(`(() => {
      const b = [...document.querySelectorAll('button')].find((e) => e.textContent.trim() === 'Log QSO')
      if (!b) return null
      b.scrollIntoView({ block: 'nearest' })
      const r = b.getBoundingClientRect()
      return r.width > 0 ? { x: r.left + r.width / 2, y: r.top + r.height / 2 } : null
    })()`)
  }
  if (!open) {
    await browser.call('Target.closeTarget', { targetId })
    return null
  }
  await click(open.x, open.y)
  let typed = false
  for (let i = 0; i < 50 && !typed; i++) {
    await sleep(100)
    typed = await evaluate(`(() => { const i = document.querySelector('.logbook-form .logbook-field-call input'); if (!i) return false; i.focus(); return document.activeElement === i })()`)
  }
  if (!typed) {
    await browser.call('Target.closeTarget', { targetId })
    return null
  }
  await call('Input.insertText', { text: 'EC1DD' })
  let last = null
  for (let i = 0; i < 80; i++) {
    await sleep(150)
    const now = await evaluate(IOTA_MEASURE)
    if (now && !now.waiting && last && !last.waiting && JSON.stringify(now) === JSON.stringify(last)) break
    last = now
  }
  await browser.call('Target.closeTarget', { targetId })
  return last
}

const IOTA_SIZES = [
  { w: 1024, h: 768, zoom: 0.65 },
  { w: 1024, h: 768, zoom: 1.75 },
  { w: 1920, h: 1080, zoom: 0.65 },
  { w: 1920, h: 1080, zoom: 1.75 },
]

let code = 0
try {
  for (const { w, h, zoom } of IOTA_SIZES) {
    const at = `${w}×${h} at ${Math.round(zoom * 100)}%`
    const m = await measureIota(w, h, zoom, false)
    console.log(`contactcard-probe: filled IOTA box, window ${at}: ${JSON.stringify(m)}`)
    if (!m || m.waiting) {
      console.error(`contactcard-probe: the Add form's IOTA box never filled at ${at}`)
      code = 2
      continue
    }
    const red = []
    if (Math.abs(m.zoom - zoom) > 0.001) red.push(`the UI scale is ${m.zoom}, not ${zoom}`)
    if (m.page > 0) red.push(`the page scrolls sideways by ${m.page} px`)
    if (m.formSideways) red.push('the form scrolls sideways')
    if (!m.nameWhole) red.push(`the group name is not drawn whole inside the form ${JSON.stringify(m.name)} vs ${JSON.stringify(m.form)}`)
    if (m.covers.length) red.push(`the group name covers ${m.covers.join(', ')}`)
    if (!m.inputTop) red.push('the IOTA box is covered')
    if (m.source !== 'from the spot') red.push(`the source reads ${JSON.stringify(m.source)}`)
    if (!m.logTop) red.push('Log is covered')
    if (red.length) {
      console.error(`contactcard-probe: RED (filled IOTA box) at ${at} — ${red.join('; ')}`)
      code = code || 1
    }
  }
  const ic = await measureIota(1024, 768, 1.75, true)
  console.log(`contactcard-probe: CONTROL group name kept on one line, window 1024×768 at 175%: ${JSON.stringify(ic)}`)
  if (!ic || ic.waiting || ic.nameWhole) {
    console.error('contactcard-probe: CONTROL FAILED — the unwrapped group name still read as drawn whole; the check cannot fail')
    code = code || 1
  }

  for (const { w, h } of SIZES) {
    const m = await measure(w, h)
    console.log(`contactcard-probe: window ${w}×${h}: ${JSON.stringify(m)}`)
    if (!m) {
      console.error(`contactcard-probe: the double-click never opened the card at ${w}×${h}`)
      code = 2
      continue
    }
    const { opened: o, foot: f } = m
    if (o.call !== 'EC1DD' || !o.inside || o.sideways || !o.photo || !o.save || !o.cancel) {
      console.error(`contactcard-probe: RED at ${w}×${h} — on opening: the card ${o.inside ? 'inside' : 'NOT inside'} the window, sideways scroll ${o.sideways}, photo ${o.photo}, Save on screen ${o.save}, Cancel on screen ${o.cancel}`)
      code = code || 1
    }
    if (f.recent !== 10 || !f.lastRecent || !f.save) {
      console.error(`contactcard-probe: RED at ${w}×${h} — at its foot: ${f.recent} recent contacts, the last on screen ${f.lastRecent}, Save on screen ${f.save}`)
      code = code || 1
    }
  }
  const c = await measure(1024, 768, '?footer=static')
  console.log(`contactcard-probe: CONTROL Save bar in the flow, window 1024×768: ${JSON.stringify(c)}`)
  if (!c || c.opened.save) {
    console.error('contactcard-probe: CONTROL FAILED — with the Save bar in the flow it was still on screen at the top of the card; the check cannot fail')
    code = code || 1
  }
  if (code === 0) console.log(`contactcard-probe: PASS — a double-click on the call opens the card whole at ${SIZES.map((s) => `${s.w}×${s.h}`).join(', ')}: inside the window, no sideways scroll, the photo drawn, Save and Cancel on screen at the top and the foot, ten recent contacts; the in-flow control hid Save. The Add form's filled IOTA box at ${IOTA_SIZES.map((s) => `${s.w}×${s.h} ${Math.round(s.zoom * 100)}%`).join(', ')}: filled, its long group name whole and covering nothing, the source shown, nothing sideways, Log reachable; the unwrapped-name control was caught`)
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
