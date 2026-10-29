// The Logbook's width and its column moves in real Chrome, for what jsdom cannot see:
// `scripts/browser-probe --desktop logbook`.
//
// The probe serves src/probe/logbook.tsx (the real Logbook in the app's `.app` > `main.layout single`, at
// the zoom the app picks for the window) at 1024×768, 1920×1080 and 2560×1440 and asserts, at each:
//   · the Logbook's panel spans the content box of the window it is in — no gutter beside it wider than
//     the layout's own padding — and its table spans the panel;
//   · nothing scrolls sideways at the page: not the document, not the view's host, not the panel;
//   · with the default columns the table fits and does not scroll sideways; with every column the
//     chooser offers (`?cols=wide`) it is wider than the window, and it is the table's OWN scroller that
//     scrolls sideways, carrying the sticky header with the rows;
//   · a real mouse drag of the Notes header onto the Call header's left half puts Notes first, saves
//     that order, and sorts nothing; a press with no travel on Country then still sorts by Country.
//
//   · every row control (the widest cluster: station control held, satellite names known) is a target
//     of at least 32×32 px in the app's own px, at least 24 px on screen at any zoom, with at least 6 px
//     between neighbours, wholly inside its cell and the one thing under its own centre; and no row
//     text is cut short without a tooltip to read it in full — at 1024×768 and 2560×1440 at the
//     window's own zoom and at 1280×800 and 1920×1080 at 100% and 150%;
//   · a 10,000-contact log renders only the rows in view (it stays virtualised), measured while
//     scrolling.
//
// THE CONTROL, run every time: at 1920×1080 the old centred measure put back on the panel
// (`?control=cap`). There the panel must come back NARROWER than the content box; if it does not, the
// width check cannot fail and the probe says so instead of passing. And the row sweep's own: the old
// 22 px row buttons put back (`?control=thin`), which the target-size check must catch.
//
// exit: 0 every size clear · 1 a red, or a control that could not fail · 2 the page never drew the table
import { createServer } from 'vite'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const SIZES = [{ w: 1024, h: 768 }, { w: 1920, h: 1080 }, { w: 2560, h: 1440 }]
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

/** The view's boxes, in window px, and every sideways scroll that could be the page's or the table's. */
const MEASURE = `(() => {
  const main = document.querySelector('main.layout.single')
  const panel = main?.querySelector(':scope > .panel.log-view')
  const table = panel?.querySelector('.logbook-table')
  const scroll = table?.querySelector('.log-scroll')
  const head = table?.querySelector('.logbook-row.head')
  if (!main || !panel || !table || !scroll || !head || !table.querySelector('.logbook-row:not(.head):not(.placeholder)')) return null
  const z = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--ui-zoom')) || 1
  const box = (el) => { const r = el.getBoundingClientRect(); return { left: Math.round(r.left), right: Math.round(r.right), width: Math.round(r.width) } }
  const m = box(main), p = box(panel), t = box(table)
  const pad = parseFloat(getComputedStyle(main).paddingLeft) * z
  const side = (el) => el.scrollWidth > el.clientWidth + 1
  return {
    vp: document.documentElement.getAttribute('data-viewport') ?? 'UNSET',
    zoom: z,
    window: innerWidth,
    main: m, panel: p, table: t,
    pad: Math.round(pad),
    gutter: { left: p.left - m.left, right: m.right - p.right },
    tableInset: { left: t.left - p.left, right: p.right - t.right },
    panelMaxWidth: getComputedStyle(panel).maxWidth,
    pageSideways: document.scrollingElement.scrollWidth > innerWidth + 1,
    hostSideways: side(main),
    panelSideways: side(panel),
    tableSideways: side(scroll),
    tableOverflowX: getComputedStyle(scroll).overflowX,
    headers: [...head.querySelectorAll(':scope > [data-col]')].map((h) => h.dataset.col),
    sorted: [...head.querySelectorAll(':scope > [aria-sort]')].filter((h) => h.getAttribute('aria-sort') !== 'none').map((h) => h.dataset.col + ':' + h.getAttribute('aria-sort')),
    saved: window.__saved.map((s) => (s && s.order ? s.order.slice(0, 3).join(',') : JSON.stringify(s))),
  }
})()`

/** One row's controls and text, in the app's own px (window px ÷ zoom) and in window px. */
const ROW = `(() => {
  const table = document.querySelector('main.layout.single .logbook-table')
  const row = table?.querySelector('.logbook-row:not(.head):not(.placeholder)')
  const cell = row?.querySelector('.log-rowactions')
  if (!row || !cell) return null
  const z = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--ui-zoom')) || 1
  const cr = cell.getBoundingClientRect()
  const ctrls = [...cell.children].map((el) => {
    const r = el.getBoundingClientRect()
    const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2)
    return {
      label: el.getAttribute('aria-label') || el.textContent,
      text: el.tagName === 'SELECT' ? el.options[0]?.textContent : el.textContent,
      left: r.left, right: r.right,
      w: +(r.width / z).toFixed(1), h: +(r.height / z).toFixed(1),
      screenW: +r.width.toFixed(1), screenH: +r.height.toFixed(1),
      inCell: r.left >= cr.left - 0.5 && r.right <= cr.right + 0.5 && r.top >= cr.top - 0.5 && r.bottom <= cr.bottom + 0.5,
      hit: !!hit && (hit === el || el.contains(hit)),
    }
  })
  const gaps = ctrls.slice(1).map((c, i) => +((c.left - ctrls[i].right) / z).toFixed(1))
  // Text cut short: an element whose content is wider than its box, with no tooltip on it or on a
  // cell around it (the row's own "double-click to open" title does not count — it names no text).
  const cut = [...row.querySelectorAll('.log-cell, .log-cell *')]
    .filter((el) => el.scrollWidth > el.clientWidth + 1 && getComputedStyle(el).overflowX === 'hidden')
    .filter((el) => { const t = el.closest('[title]'); return !t || t === row || !t.getAttribute('title') })
    .map((el) => el.className + ': ' + el.textContent.slice(0, 40))
  return {
    vp: document.documentElement.getAttribute('data-viewport') ?? 'UNSET',
    zoom: z,
    rowH: +(row.getBoundingClientRect().height / z).toFixed(1),
    controls: ctrls.map(({ left, right, ...c }) => c),
    gaps,
    cut,
    tableSideways: table.querySelector('.log-scroll').scrollWidth > table.querySelector('.log-scroll').clientWidth + 1,
  }
})()`
/** The row sweep: the window's own zoom at the floor and the top, 100% and 150% between. */
const ROW_SIZES = [
  { w: 1024, h: 768, zoom: null }, { w: 1280, h: 800, zoom: 1 }, { w: 1280, h: 800, zoom: 1.5 },
  { w: 1920, h: 1080, zoom: 1 }, { w: 1920, h: 1080, zoom: 1.5 }, { w: 2560, h: 1440, zoom: null },
]
/** What the row sweep demands, as one list of reasons (empty when clear). */
function rowFaults(m) {
  const out = []
  for (const c of m.controls) {
    if (c.w < 32 || c.h < 32) out.push(`"${c.text}" is ${c.w}×${c.h} px, under 32×32`)
    if (c.screenW < 24 || c.screenH < 24) out.push(`"${c.text}" is ${c.screenW}×${c.screenH} on screen, under 24×24`)
    if (!c.inCell) out.push(`"${c.text}" is cut by its cell`)
    if (!c.hit) out.push(`"${c.text}" is not what is under its own centre`)
  }
  m.gaps.forEach((g, i) => { if (g < 6) out.push(`${g} px between "${m.controls[i].text}" and "${m.controls[i + 1].text}"`) })
  for (const c of m.cut) out.push(`row text cut short with no tooltip: ${c}`)
  if (m.tableSideways) out.push('the default table scrolls sideways')
  return out
}

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-logbook-probe-'))
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

async function open(w, h, query) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })
    return r.exceptionDetails ? null : r.result.value
  }
  await call('Emulation.setDeviceMetricsOverride', { width: w, height: h, deviceScaleFactor: 1, mobile: false })
  await call('Page.navigate', { url: `${origin}/probe/logbook.html${query}` })
  // Settled: the table drawn, the size class published, two reads the same.
  let last = null
  for (let i = 0; i < 150; i++) {
    await sleep(100)
    const now = await evaluate(MEASURE)
    if (now && now.vp !== 'UNSET' && last && JSON.stringify(now) === JSON.stringify(last)) break
    last = now
  }
  const close = () => browser.call('Target.closeTarget', { targetId })
  const mouse = async (type, x, y) => call('Input.dispatchMouseEvent', { type, x, y, button: 'left', buttons: type === 'mouseReleased' ? 0 : 1, clickCount: 1 })
  /** The centre of header `id`, or a point `frac` across it. */
  const at = (id, frac = 0.5) =>
    evaluate(`(() => { const h = document.querySelector('.logbook-row.head > [data-col="${id}"]'); if (!h) return null; const r = h.getBoundingClientRect(); return { x: r.left + r.width * ${frac}, y: r.top + r.height / 2 } })()`)
  return { evaluate, close, mouse, at, first: last }
}

let code = 0
const red = (msg) => {
  console.error(`logbook-probe: RED — ${msg}`)
  code = code || 1
}
/** The width checks: a gutter no wider than the layout's padding, the table across the panel. */
const widthOk = (m) => m.gutter.left <= m.pad + 2 && m.gutter.right <= m.pad + 2 && m.tableInset.left <= 2 && m.tableInset.right <= 2
try {
  for (const { w, h } of SIZES) {
    // The default columns: the width, no sideways scroll anywhere, then a real drag and a real click.
    const page = await open(w, h, '')
    const m = page.first
    console.log(`logbook-probe: window ${w}×${h} default columns: ${JSON.stringify(m)}`)
    if (!m) {
      console.error(`logbook-probe: the table never drew at ${w}×${h}`)
      code = 2
      await page.close()
      continue
    }
    if (m.vp === 'UNSET') red(`${w}×${h}: the size class never landed — the reading is of no layout`)
    if (!widthOk(m)) red(`${w}×${h}: the panel does not span the window — gutters ${m.gutter.left}/${m.gutter.right} px against the layout's ${m.pad} px padding (max-width ${m.panelMaxWidth}), table inset ${m.tableInset.left}/${m.tableInset.right}`)
    if (m.pageSideways || m.hostSideways || m.panelSideways) red(`${w}×${h}: the page scrolls sideways (document ${m.pageSideways}, host ${m.hostSideways}, panel ${m.panelSideways})`)
    if (m.tableSideways) red(`${w}×${h}: the default table does not fit the window and scrolls sideways`)

    const from = await page.at('notes')
    const onto = await page.at('call', 0.2)
    await page.mouse('mouseMoved', from.x, from.y)
    await page.mouse('mousePressed', from.x, from.y)
    for (let k = 1; k <= 8; k++) await page.mouse('mouseMoved', from.x + ((onto.x - from.x) * k) / 8, from.y)
    await page.mouse('mouseReleased', onto.x, onto.y)
    await sleep(400)
    const d = await page.evaluate(MEASURE)
    console.log(`logbook-probe: window ${w}×${h} after dragging Notes onto Call: headers ${d.headers.join(' ')} · sorted ${d.sorted.join(' ')} · saved ${d.saved.join(' | ')}`)
    if (d.headers[0] !== 'notes' || d.headers[1] !== 'call') red(`${w}×${h}: the drag did not put Notes first (${d.headers.slice(0, 3).join(' ')})`)
    if (d.saved.length !== 1 || d.saved[0] !== 'notes,call,country') red(`${w}×${h}: the drag saved ${JSON.stringify(d.saved)}, not one order starting Notes, Call, Country`)
    if (JSON.stringify(d.sorted) !== JSON.stringify(m.sorted)) red(`${w}×${h}: the drag changed the sort (${m.sorted.join(' ')} → ${d.sorted.join(' ')})`)
    const c = await page.at('country')
    await page.mouse('mouseMoved', c.x, c.y)
    await page.mouse('mousePressed', c.x, c.y)
    await page.mouse('mouseReleased', c.x + 2, c.y)
    await sleep(400)
    const s = await page.evaluate(MEASURE)
    if (!s.sorted.some((x) => x.startsWith('country:'))) red(`${w}×${h}: a click on Country no longer sorts (${s.sorted.join(' ')})`)
    if (s.saved.length !== 1) red(`${w}×${h}: the click saved a column choice`)
    await page.close()

    // Every column: wider than the window, so the TABLE scrolls sideways and the page does not.
    const wide = await open(w, h, '?cols=wide')
    const x = wide.first
    console.log(`logbook-probe: window ${w}×${h} every column: ${JSON.stringify(x)}`)
    if (!x) {
      console.error(`logbook-probe: the wide table never drew at ${w}×${h}`)
      code = 2
      await wide.close()
      continue
    }
    if (!widthOk(x)) red(`${w}×${h} wide: the panel does not span the window — gutters ${x.gutter.left}/${x.gutter.right} px`)
    if (x.pageSideways || x.hostSideways || x.panelSideways) red(`${w}×${h} wide: the page scrolls sideways (document ${x.pageSideways}, host ${x.hostSideways}, panel ${x.panelSideways})`)
    if (!x.tableSideways || !['auto', 'scroll'].includes(x.tableOverflowX)) red(`${w}×${h} wide: the table's own scroller does not scroll sideways (overflow-x ${x.tableOverflowX}, overflowing ${x.tableSideways})`)
    const before = await wide.at('call')
    await wide.evaluate(`document.querySelector('.log-scroll').scrollLeft = 300`)
    await sleep(200)
    const after = await wide.at('call')
    if (!(before.x - after.x > 100)) red(`${w}×${h} wide: the header did not scroll sideways with the rows (${Math.round(before.x)} → ${Math.round(after.x)})`)
    await wide.close()
  }

  // THE ROW SWEEP: the widest cluster, at every size and zoom the operator runs.
  for (const { w, h, zoom } of ROW_SIZES) {
    const page = await open(w, h, `?sat=1${zoom ? `&zoom=${zoom}` : ''}`)
    const m = await page.evaluate(ROW)
    const tag = `${w}×${h} @ ${zoom ? Math.round(zoom * 100) + '%' : 'fit'}`
    console.log(`logbook-probe: rows ${tag}: ${JSON.stringify(m)}`)
    if (!m || m.vp === 'UNSET') red(`${tag}: no row to measure, or the size class never landed`)
    else for (const f of rowFaults(m)) red(`${tag}: ${f}`)
    await page.close()
  }
  // A big log stays virtualised: only the rows in view (and the overscan) are in the page, however
  // far it scrolls.
  const big = await open(1920, 1080, '?n=10000&zoom=1')
  const perf = await big.evaluate(`new Promise((done) => {
    const s = document.querySelector('.log-scroll')
    const frames = [], counts = []
    let last = performance.now(), i = 0
    const step = (now) => {
      frames.push(now - last); last = now
      counts.push(document.querySelectorAll('.logbook-row:not(.head)').length)
      s.scrollTop += 120
      if (++i < 240) requestAnimationFrame(step)
      else done({ total: s.scrollHeight, maxRows: Math.max(...counts), meanFrameMs: +(frames.slice(1).reduce((a, b) => a + b, 0) / (frames.length - 1)).toFixed(1), maxFrameMs: +Math.max(...frames.slice(1)).toFixed(1) })
    }
    requestAnimationFrame(step)
  })`)
  console.log(`logbook-probe: 10,000 contacts while scrolling: ${JSON.stringify(perf)}`)
  if (!perf || perf.maxRows > 80) red(`a 10,000-contact log rendered ${perf?.maxRows} rows at once — it is not virtualised`)
  await big.close()
  const thin = await open(1920, 1080, '?sat=1&zoom=1&control=thin')
  const tm = await thin.evaluate(ROW)
  console.log(`logbook-probe: CONTROL the old 22 px row buttons, window 1920×1080: ${JSON.stringify(tm?.controls?.map((c) => `${c.text} ${c.w}×${c.h}`))}`)
  if (!tm || rowFaults(tm).length === 0) {
    console.error('logbook-probe: CONTROL FAILED — with the old 22 px row buttons back the target-size check still passed; it cannot fail')
    code = code || 1
  }
  await thin.close()

  const ctl = await open(1920, 1080, '?control=cap')
  console.log(`logbook-probe: CONTROL the old centred measure, window 1920×1080: ${JSON.stringify(ctl.first)}`)
  if (!ctl.first || widthOk(ctl.first)) {
    console.error('logbook-probe: CONTROL FAILED — with the old 1100 px measure back on the panel the width check still passed; it cannot fail')
    code = code || 1
  }
  await ctl.close()
  if (code === 0) console.log(`logbook-probe: PASS — at ${SIZES.map((s) => `${s.w}×${s.h}`).join(', ')} the Logbook spans the window, nothing scrolls sideways at the page, the default table fits, every column scrolls inside the table with its header, a drag moves Notes first without sorting and a click still sorts, every row control is a 32×32 target 6 px from its neighbours with no row text cut short untold, a 10,000-contact log stays virtualised; the old measure and the old 22 px buttons were caught`)
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
