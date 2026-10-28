// The club log on a joined laptop in real Chrome, for what jsdom cannot see:
// `scripts/browser-probe --desktop clublog`.
//
// Every laptop at a club now lists the club's contacts under its club board, as the host does. The probe
// serves src/probe/clublog.tsx (a laptop joined to an Illinois QSO Party club of five positions with long
// names, sent the newest 100 of 4,321 contacts) with the app's own Vite config, on the Contest screen and in
// the club board pop-out, at 1024×768 (the supported floor) and 1920×1080, and asserts:
//   · the list is there, under the club board, with its head ("the newest 100 of 4321 contacts");
//   · nothing scrolls sideways: the page, the screen's scroller, the list and its rows are no wider than
//     they show;
//   · the newest contact's cells (time, call, exchange, band, mode and the position that logged it) are on
//     screen once the screen scrolls to them: inside the window, held by every box that clips them, and the
//     topmost thing where they stand;
//   · the hundredth contact is reached by scrolling, and its cells are then on screen too.
//
// THE CONTROL, run every time: the same page with the newest row made 2400 px wide. There the probe must see the sideways overflow; if it does not, the check cannot fail and
// the probe says so instead.
//
// exit: 0 every size clear · 1 a red, or a control that could not fail · 2 a page never drew the list
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
const VIEWS = ['contest', 'popout']
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

const PLANT = `(() => {
  const row = document.querySelector('[role=region][aria-label="Club log"] .fd-log-row')
  if (!row) return false
  row.style.minWidth = '2400px'
  return true
})()`

/** Whether the list is drawn, how far each box scrolls sideways, and which cells are on screen. */
const MEASURE = `(() => {
  const holds = (o, i) => i.left >= o.left - 0.5 && i.right <= o.right + 0.5 && i.top >= o.top - 0.5 && i.bottom <= o.bottom + 0.5
  const win = { left: 0, top: 0, right: innerWidth, bottom: innerHeight }
  const onScreen = (el) => {
    if (!el) return false
    el.scrollIntoView({ block: 'nearest', inline: 'nearest' })
    const r = el.getBoundingClientRect()
    if (r.width < 1 || r.height < 1 || !holds(win, r)) return false
    for (let a = el.parentElement; a; a = a.parentElement) {
      const s = getComputedStyle(a)
      if (/(hidden|auto|scroll|clip)/.test(s.overflowX + s.overflowY) && !holds(a.getBoundingClientRect(), r)) return false
    }
    const top = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2)
    return top != null && (top === el || el.contains(top))
  }
  const wider = (el) => (el ? el.scrollWidth - el.clientWidth : 0)
  const list = document.querySelector('[role=region][aria-label="Club log"]')
  if (!list) return null
  const rows = [...list.querySelectorAll('.fd-log-row')]
  if (rows.length < 100) return null
  const cells = (row) => [...row.querySelectorAll('.fd-col')].filter((c) => (c.textContent ?? '').trim() !== '')
  const head = list.firstElementChild
  const scroller = list.querySelector('.fd-log-scroll')
  const newest = cells(rows[0])
  const newestOn = newest.length >= 6 && newest.every(onScreen)
  const last = cells(rows[rows.length - 1])
  const lastOn = last.length >= 6 && last.every(onScreen)
  const club = list.closest('[aria-label="Club sync"]')
  let sideways = { page: wider(document.scrollingElement), list: wider(list), scroller: wider(scroller), row: wider(rows[0]) }
  for (let a = list.parentElement; a; a = a.parentElement) sideways.ancestors = Math.max(sideways.ancestors ?? 0, wider(a))
  return {
    rows: rows.length,
    head: (head?.textContent ?? '').trim(),
    headOn: onScreen(head),
    underBoard: club != null && club.contains(list),
    newestOn,
    lastOn,
    sideways,
    refused: window.__refused?.() ?? [],
  }
})()`

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-clublog-probe-'))
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

async function measure(w, h, view, control) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true })
    return r.exceptionDetails ? { error: r.exceptionDetails.exception?.description ?? r.exceptionDetails.text } : r.result.value
  }
  await call('Emulation.setDeviceMetricsOverride', { width: w, height: h, deviceScaleFactor: 1, mobile: false })
  await call('Page.navigate', { url: `${origin}/probe/clublog.html?view=${view}` })
  let last = null
  for (let i = 0; i < 100; i++) {
    await sleep(150)
    if (control) await evaluate(PLANT)
    const now = await evaluate(MEASURE)
    if (now && !now.error && last && JSON.stringify(now) === JSON.stringify(last)) break
    last = now
  }
  await browser.call('Target.closeTarget', { targetId })
  return last
}

/** What is wrong with a measure, as words; empty when nothing is. */
function faults(m) {
  const f = []
  for (const [box, px] of Object.entries(m.sideways)) if (px > 0) f.push(`${box} scrolls sideways by ${px} px`)
  if (!m.underBoard) f.push('the list is not in the club block')
  if (!m.headOn || !/newest 100 of 4321/.test(m.head)) f.push(`the head is not on screen as it should read: "${m.head}"`)
  if (!m.newestOn) f.push("the newest contact's cells are not all on screen")
  if (!m.lastOn) f.push("the hundredth contact's cells are not all on screen once scrolled to")
  return f
}

let code = 0
try {
  for (const view of VIEWS) {
    for (const { w, h } of SIZES) {
      const m = await measure(w, h, view, false)
      console.log(`clublog-probe: ${view} ${w}×${h}: ${JSON.stringify(m)}`)
      if (!m || m.error) {
        console.error(`clublog-probe: the list never drew in ${view} at ${w}×${h}`)
        code = 2
        continue
      }
      const red = faults(m)
      if (red.length) {
        console.error(`clublog-probe: RED in ${view} at ${w}×${h} — ${red.join('; ')}`)
        code = code || 1
      }
      const c = await measure(w, h, view, true)
      const caught = c && !c.error ? faults(c) : []
      console.log(`clublog-probe: CONTROL ${view} ${w}×${h}: caught ${JSON.stringify(caught)}`)
      if (!caught.some((s) => s.includes('sideways') || s.includes('not all on screen'))) {
        console.error(`clublog-probe: CONTROL FAILED in ${view} at ${w}×${h} — the planted wide row was not seen; the check cannot fail`)
        code = code || 1
      }
    }
  }
  if (code === 0)
    console.log(
      `clublog-probe: PASS — a joined laptop's club log under its club board, its head, newest and hundredth contact on screen with nothing sideways, on the Contest screen and in the pop-out at ${SIZES.map((s) => `${s.w}×${s.h}`).join(' and ')}; each control caught`,
    )
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
