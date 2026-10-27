// The station card in real Chrome, for what jsdom cannot see: `scripts/browser-probe --desktop callcard`.
//
// The card (ui/src/components/CallCard.tsx) must never cover a transmit control. The probe serves
// src/probe/callcard.tsx with the app's own Vite config at 1366×768 and 1024×768: a call just above the
// TX strip with the TX dock under it, and a call with FT's QSO strip and its Stop TX directly under it.
// It clicks the call with a real mouse press and asserts the card is drawn (placed, and the topmost thing
// at its own centre) and that at nine points across every transmit box — the strip, Stop TX, the dock,
// the QSO strip — and nine across any part of it the card's box overlaps, the topmost thing is never
// the card.
//
// THE CONTROL, run every time: the same pages with the boxes stripped of their transmit classes, where
// the card is free to land on them. There the probe must see the card covering one; if it does not, the
// check cannot fail and the probe says so instead of passing.
//
// PHONE'S CARD BUTTON, the operator's report of 2026-10-26: the card opened from the log strip's Card
// button for a call the strip had already looked up showed the callsign's initials instead of the photo
// and squeezed every line of text to the callsign's width. At 1366×768, 1024×768 (and there at a 175%
// UI scale) and 2560×1440 the probe opens it for a long callbook answer with a photo, presses nothing
// else, and asserts the photo is drawn, there is no Look up to press, and the text column takes most
// of the card: wider than twice the callsign and at least 60% of the card's body, with the card drawn
// whole (it scrolls neither way: its box once came out its border narrower and shorter than its body).
//
// exit: 0 every case clear · 1 a red, or a control that could not fail · 2 the page never drew the card
import { createServer } from 'vite'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const SIZES = [{ w: 1366, h: 768 }, { w: 1024, h: 768 }]
const CASES = ['strip', 'qso']
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

/** The card's box and whether it is drawn, and for each transmit box whether the card covers any of nine
 *  points across it (the topmost thing there is inside the card). */
const MEASURE = `(() => {
  const card = document.querySelector('.callcard')
  if (!card || card.dataset.placed !== 'yes') return null
  const c = card.getBoundingClientRect()
  const mid = document.elementFromPoint(c.left + c.width / 2, c.top + c.height / 2)
  const covered = {}
  for (const el of document.querySelectorAll('[data-tx]')) {
    const r = el.getBoundingClientRect()
    // Nine points across the box, and nine across the part of it the card's box overlaps, if any.
    const areas = [r]
    const o = { left: Math.max(r.left, c.left), top: Math.max(r.top, c.top), right: Math.min(r.right, c.right), bottom: Math.min(r.bottom, c.bottom) }
    if (o.right > o.left && o.bottom > o.top) areas.push({ left: o.left, top: o.top, width: o.right - o.left, height: o.bottom - o.top })
    let hit = false
    for (const a of areas) for (const fx of [0.05, 0.5, 0.95]) for (const fy of [0.1, 0.5, 0.9]) {
      const top = document.elementFromPoint(a.left + a.width * fx, a.top + a.height * fy)
      if (top && card.contains(top)) hit = true
    }
    covered[el.dataset.tx] = hit
  }
  return {
    card: { left: Math.round(c.left), top: Math.round(c.top), w: Math.round(c.width), h: Math.round(c.height) },
    drawn: mid != null && card.contains(mid),
    covered,
    refused: window.__refused(),
  }
})()`

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-callcard-probe-'))
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

/** Phone's card: the photo, the Look up button, and how wide the text column is against the card. */
const MEASURE_PHONE = `(() => {
  const card = document.querySelector('.callcard')
  if (!card || card.dataset.placed !== 'yes' || document.documentElement.dataset.lookedUp !== 'yes') return null
  const w = (sel) => { const e = card.querySelector(sel); return e ? Math.round(e.getBoundingClientRect().width) : 0 }
  const img = card.querySelector('img.callcard-photo')
  return {
    card: w('.callcard-body'),
    text: w('.callcard-id'),
    call: w('.callcard-call'),
    photo: !!img && img.complete && img.naturalWidth > 0 && getComputedStyle(img).display !== 'none' && img.getBoundingClientRect().width > 0,
    lookup: !!card.querySelector('.callcard-lookup'),
    sideways: card.scrollWidth > card.clientWidth + 1,
    // Placed whole (the room is ample at every size here): it must not scroll either way.
    clipped: card.scrollHeight > card.clientHeight + 1,
    refused: window.__refused(),
  }
})()`

/** One page: the case at `w`×`h`, the call clicked, the card measured once it has stopped moving. */
async function measure(w, h, which, avoid, extra = '', expr = MEASURE) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true })
    return r.exceptionDetails ? null : r.result.value
  }
  await call('Emulation.setDeviceMetricsOverride', { width: w, height: h, deviceScaleFactor: 1, mobile: false })
  await call('Page.navigate', { url: `${origin}/probe/callcard.html?case=${which}${avoid ? '' : '&avoid=off'}${extra}` })
  let at = null
  for (let i = 0; i < 100 && !at; i++) {
    await sleep(100)
    at = await evaluate(`(() => { const b = document.querySelector('.probe-call')?.getBoundingClientRect(); return b ? { x: b.left + b.width / 2, y: b.top + b.height / 2 } : null })()`)
  }
  if (!at) return null
  for (const type of ['mousePressed', 'mouseReleased']) {
    await call('Input.dispatchMouseEvent', { type, x: at.x, y: at.y, button: 'left', clickCount: 1 })
  }
  let last = null
  for (let i = 0; i < 60; i++) {
    await sleep(150)
    const now = await evaluate(expr)
    if (now && last && JSON.stringify(now) === JSON.stringify(last)) break
    last = now
  }
  await browser.call('Target.closeTarget', { targetId })
  return last
}

let code = 0
try {
  for (const { w, h } of SIZES) {
    for (const which of CASES) {
      const m = await measure(w, h, which, true)
      console.log(`callcard-probe: ${which}, window ${w}×${h}: ${JSON.stringify(m)}`)
      if (!m || !m.drawn) {
        console.error(`callcard-probe: the card was never drawn for the ${which} case at ${w}×${h}`)
        code = 2
      } else if (Object.keys(m.covered).length === 0 || Object.values(m.covered).some(Boolean)) {
        console.error(`callcard-probe: RED ${which} at ${w}×${h} — the card covers a transmit control (see "covered")`)
        code = code || 1
      }
      const c = await measure(w, h, which, false)
      console.log(`callcard-probe: CONTROL ${which}, window ${w}×${h}, transmit classes removed: ${JSON.stringify(c)}`)
      if (!c || !Object.values(c.covered).some(Boolean)) {
        console.error(`callcard-probe: CONTROL FAILED ${which} at ${w}×${h} — with nothing to keep off, the card did not land on the boxes; the check cannot fail`)
        code = code || 1
      }
    }
  }
  for (const { w, h, zoom } of [{ w: 1366, h: 768 }, { w: 1024, h: 768 }, { w: 1024, h: 768, zoom: 1.75 }, { w: 2560, h: 1440 }]) {
    const m = await measure(w, h, 'phone', true, zoom ? `&zoom=${zoom}` : '', MEASURE_PHONE)
    const at = `${w}×${h}${zoom ? ` at ${zoom * 100}%` : ''}`
    console.log(`callcard-probe: phone Card button, window ${at}: ${JSON.stringify(m)}`)
    if (!m) {
      console.error(`callcard-probe: the card was never drawn from Phone's Card button at ${at}`)
      code = 2
      continue
    }
    if (!m.photo || m.lookup) {
      console.error(`callcard-probe: RED phone at ${at} — the strip's callbook answer is not on the card (photo drawn: ${m.photo}, Look up still offered: ${m.lookup})`)
      code = code || 1
    }
    if (m.text < 2 * m.call || m.text < 0.6 * m.card || m.sideways || m.clipped) {
      console.error(`callcard-probe: RED phone at ${at} — the text column is ${m.text}px of a ${m.card}px card (callsign ${m.call}px; scrolls sideways: ${m.sideways}; scrolls down: ${m.clipped})`)
      code = code || 1
    }
  }
  if (code === 0) console.log(`callcard-probe: PASS — the card clear of the TX strip, Stop TX, the TX dock and the QSO strip at ${SIZES.map((s) => `${s.w}×${s.h}`).join(', ')}; each control covered with the transmit classes removed; from Phone's Card button the photo shows and the text takes the card's width`)
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
