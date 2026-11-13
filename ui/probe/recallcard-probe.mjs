// FT's callsign (recall) card in the real app, in real Chrome: `scripts/browser-probe --desktop recallcard`.
//
// THE REPORT (tester, 2026-11-13, with a screenshot): the card was squeezed narrow. The name broke one word a line
// ("Barry / N / Evans"), the call and "Indianapolis, IN" sat below it, the avatar was cut off at the bottom left,
// and the card showed a vertical scrollbar. MEASURED on main: in Classic the card stands in the Stations column
// (cockpit-lower.classic's third track, minmax(260px, 72fr)), 178-363 px wide across the laptop screens, and its
// head gave the name what was left after a 52 px avatar, the badges (flex:none, up to 42 % of the head) and the
// pane's ✕: 11-68 px. The head grew to ~300 px of one-word lines, the card's ceiling (cockpit-panes.css
// `.cockpit-recall`, min(14em, 33%)) cut it, and the avatar, centred in that tall head, fell below the cut.
//
// The probe serves the REAL app (index.html, main.tsx, App) with the app's own Vite config and src/api.ts swapped
// for probe/waterfalls-api.ts with a contact in progress with N9BNE: a callbook answer (Barry N Evans, Indianapolis,
// IN), two prior contacts with a note, and the ✓ and New mode badges. Classic and Roster, at the app's own fit and at
// its 100 %, over screens of probe/sizes.mjs: CORE, 1366×768 unscaled and at 150 %, 1080p at 150 % and 1440×900 at
// 125 % by default; RECALL_FULL=1 runs the whole MATRIX (1366×768 to 4K, 100-175 % and 200 % on 4K).
//   THE CARD, at every size where it is drawn: the name is on ONE line; it is cut short (an ellipsis) only on a
//     card under NAME_ROOM px; on a rail of RAIL_ROOM px or more the name, the call and the avatar are each wholly
//     inside the card and topmost at their centres with nothing scrolled (asserted at every such size); and wherever the card's ceiling (min(14em, a third of its rail): cockpit-panes.css `.cockpit-recall`)
//     is tall enough for the identity and the rail gives the card that much, the whole identity row is inside the
//     card and a station with no history (K9NEW, the ordinary first contact) shows its card with no scrollbar at
//     all. Where the ceiling or the rail's sharing is shorter than the identity, that is printed as a NOTE: the share
//     is the operator's ruling of 2026-09-15, and the ⤢ is the way to the whole card there.
//   THE ⤢, at the CORE screens: a real click opens the whole card in a floating box, drawn and topmost at its own
//     centre; Stop TX and every transmit surface (features/txSurfaces TX_AVOID) stand exactly where they stood and
//     are never under the box at nine points across each; the avatar's QRZ button in it still opens QRZ; Esc
//     closes it, and so does a click outside it.
// THE CONTROLS, run every time: the name forced back to wrapping (the squeeze's look) must be caught, so must the
// name, the call and the avatar each covered by a planted box, and so must the box planted over Stop TX. If either is not, its check cannot fail and the probe says so instead of passing.
//
// exit: 0 every check held · 1 a red, or a control that could not fail · 2 the card never drew
import { createServer } from 'vite'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'
import { CORE, FHD_150, FLOOR_150, LAPTOP, MATRIX, at, css } from './sizes.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const REAL_API = join(UI, 'src', 'api.ts')
const STUB_SRC = join(UI, 'probe', 'waterfalls-api.ts')
const STUB_API = join(UI, 'probe', '__recallcard-api.ts')
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
/** A card narrower than this may cut a 13-letter name short (with the whole name in its tooltip); wider, never. */
const NAME_ROOM = 300
/** A rail at least this tall has room for the card's identity under its ceiling: there the identity is whole. */
const RAIL_ROOM = 220
const LAYOUTS = (process.env.RECALL_LAYOUTS ?? 'classic,roster').split(',')
const SCALES = (process.env.RECALL_SCALES ?? 'auto,100').split(',')
/** The default set: CORE, the floor screen unscaled and at 150 %, 1080p at 150 % and 1440×900 at 125 % (the Classic
 *  rail's narrowest cards, where main squeezed the name hardest). RECALL_FULL=1 sweeps the whole MATRIX. */
const SIZES = process.env.RECALL_FULL === '1' ? MATRIX : [...CORE, LAPTOP, FLOOR_150, FHD_150, at(1440, 900, 125)]

// THE API SWAP — the waterfalls probe's stub, with the snapshot's subscription made real (`__push`), the log
// answering the card's two questions, the callbook answering, and QRZ's opener counted.
const realSrc = await readFile(REAL_API, 'utf8')
let stub = (await readFile(STUB_SRC, 'utf8'))
  .replace('export function subscribeSnapshot(_fn: unknown): () => void { return () => {} }',
    `const __subs: any[] = []
;(globalThis as any).__push = (radio: any, top: any) => { snapshot.radio = { ...snapshot.radio, ...(radio || {}) }; Object.assign(snapshot, top || {}); const s = { ...snapshot, radio: { ...snapshot.radio } }; __subs.forEach((f) => f(s)); return __subs.length }
export function subscribeSnapshot(fn: any): () => void { __subs.push(fn); return () => { const i = __subs.indexOf(fn); if (i >= 0) __subs.splice(i, 1) } }`)
  .replace(/^export async function askLog\(\).*$/m, 'export async function askLog(q: any): Promise<any> { return (globalThis as any).__answer(q) }')
for (const seam of ['__push', '__answer']) if (!stub.includes(seam)) throw new Error(`recallcard: the stub seam ${seam} did not apply; probe/waterfalls-api.ts changed shape`)
stub += `
const NOW = Math.floor(Date.now() / 1000)
const QSOS = [
  { id: 'a', call: 'N9BNE', whenUnix: NOW - 86400 * 40, band: '20m', mode: 'FT8', rstSent: '-07', rstRcvd: '-12', comment: 'tnx QSO', notes: 'Barry, Indy club', confirmed: true },
  { id: 'b', call: 'N9BNE', whenUnix: NOW - 86400 * 400, band: '40m', mode: 'FT8', rstSent: '-10', rstRcvd: '-15', comment: '', notes: '', confirmed: false },
]
;(globalThis as any).__answer = (q: any) => {
  if (q.kind === 'callHistory') {
    const qsos = q.call === 'N9BNE' ? QSOS : []
    return { qsos, count: qsos.length, workedBefore: qsos.length > 0, dupeThisBand: false, lastUnix: qsos[0]?.whenUnix ?? null, confirmedCount: qsos.length ? 1 : 0, bands: [], modes: [] }
  }
  if (q.kind === 'entity') return { newEntity: false, slots: { workedEver: true, bandUnknown: false, bandsWorked: ['20m', '40m'], modesWorked: ['SSB'] } }
  throw new Error('no log')
}
export async function resolveEntity(): Promise<any> { return 'United States' }
const BOOK: Record<string, any> = {
  N9BNE: { name: 'Barry N Evans', qth: 'Indianapolis', grid: 'EM69vu', state: 'IN' },
  K9NEW: { name: 'Dale R Hughes', qth: 'Fort Wayne', grid: 'EN71', state: 'IN' },
}
export async function qrzLookup(call: string): Promise<any> { const b = BOOK[call]; return b ? { call, nickname: null, country: 'United States', dxcc: 291, cqZone: 4, ituZone: 8, image: null, ...b } : null }
let __qrz: string[] = []
;(globalThis as any).__qrzOpened = () => __qrz
export async function openQrzPage(call: string): Promise<void> { __qrz.push(call) }
// Esc is the stop key too (useEscStop): the halt answers with the snapshot, as the station does, so the contact stays.
export async function haltTx(): Promise<any> { return snapshot }
`
const names = [...realSrc.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1])
const own = new Set([...stub.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1]))
const stubCode = stub + `\nexport * from ${JSON.stringify(REAL_API)}\n` +
  names.filter((n) => !own.has(n)).map((n) => `export async function ${n}(..._a: unknown[]): Promise<any> { return {} }`).join('\n')
const swap = {
  name: 'recallcard-probe-api', enforce: 'pre',
  async resolveId(source, importer, opts) {
    if (opts?.scan) return null
    if (source === STUB_API) return STUB_API
    if (!importer || importer === STUB_API || !/api(\.ts)?$/.test(source)) return null
    const r = await this.resolve(source, importer, { ...opts, skipSelf: true })
    return r && r.id.split('?')[0] === REAL_API ? STUB_API : null
  },
  load(id) { return id === STUB_API ? stubCode : null },
}

/** The card as it stands, nothing scrolled: its box, the name's lines and whether it is cut short, whether the
 *  avatar and the identity row are inside the card's visible box, and whether the card scrolls. */
const MEASURE = `(() => {
  const card = [...document.querySelectorAll('main.operate-cockpit .recall-card')].find((c) => c.closest('[hidden]') == null && c.getBoundingClientRect().width > 0)
  if (!card) return null
  card.scrollTop = 0
  const r = card.getBoundingClientRect()
  const nm = card.querySelector('.recall-name'), av = card.querySelector('.recall-avatar'), head = card.querySelector('.recall-head')
  const range = document.createRange(); range.selectNodeContents(nm)
  const lines = new Set([...range.getClientRects()].filter((q) => q.width > 0).map((q) => Math.round(q.top))).size
  const inside = (el) => { const q = el.getBoundingClientRect(); return q.top >= r.top - 0.5 && q.bottom <= r.bottom + 0.5 && q.left >= r.left - 0.5 && q.right <= r.right + 0.5 }
  // Wholly inside the card's visible box AND the topmost thing at its own centre: seen, not merely laid out.
  const seen = (el) => { if (!el || !inside(el)) return false; const q = el.getBoundingClientRect(); const t = document.elementFromPoint(q.left + q.width / 2, q.top + q.height / 2); return !!t && (t === el || el.contains(t)) }
  const callEl = card.querySelector('.recall-call')
  const rail = card.parentElement
  // The card's ceiling as the cascade resolves it here (its max-height holds a share of the rail, so the computed
  // value is not px): the height it takes with content far taller than any ceiling and nothing shrinking it.
  // Read and undone in one task, so nothing paints it.
  const cap = (() => { const sp = document.createElement('div'); sp.style.height = '5000px'; const fs = card.style.flexShrink
    card.style.flexShrink = '0'; card.appendChild(sp); const h = card.getBoundingClientRect().height; sp.remove(); card.style.flexShrink = fs; return Math.round(h) })()
  return { vp: document.documentElement.getAttribute('data-viewport'), zoom: getComputedStyle(document.documentElement).getPropertyValue('--ui-zoom').trim() || '1',
    w: Math.round(r.width), h: Math.round(r.height), name: nm.textContent, lines, cut: nm.scrollWidth > nm.clientWidth + 1,
    avatar: seen(av), nameSeen: seen(nm), callSeen: callEl ? seen(callEl) : null, head: inside(head), headH: Math.round(head.getBoundingClientRect().height),
    scrolls: card.scrollHeight > card.clientHeight + 1, rail: Math.round(rail.clientHeight), expand: !!card.querySelector('.recall-expand'),
    // The card's ceiling (above), its padding and border above and below the identity, and what the rail's other
    // panes got.
    cap,
    // In window px, as every box here is (the app's zoom applies to all of them alike): the card's padding and
    // border above the identity, taken again below it.
    pad: Math.round(2 * (head.getBoundingClientRect().top - r.top)),
    feeds: [...rail.children].filter((e) => e !== card && e.getBoundingClientRect().height > 0 && !e.classList.contains('pane-splitter')).map((e) => Math.round(e.getBoundingClientRect().height)) }
})()`

/** Every transmit surface: where it stands, and how many of nine points across it have the full card on top. */
const TX = `(() => {
  const full = document.querySelector('.recall-full')
  const sel = '.cockpit-txstrip, .cockpit-txdock, .cockpit-qso, .sstv-tx-bar, .op-controls, .op-btn.stop'
  return [...document.querySelectorAll(sel)].filter((e) => e.closest('[hidden]') == null && e.getBoundingClientRect().width > 0).map((e) => {
    const q = e.getBoundingClientRect()
    let under = 0
    for (const fx of [0.05, 0.5, 0.95]) for (const fy of [0.1, 0.5, 0.9]) {
      const x = q.left + q.width * fx, y = q.top + q.height * fy
      if (x < 0 || y < 0 || x >= innerWidth || y >= innerHeight) continue
      const t = document.elementFromPoint(x, y)
      if (full && t && full.contains(t)) under++
    }
    return { what: (e.className || e.tagName).toString().split(' ').slice(0, 2).join('.'), box: [q.left, q.top, q.width, q.height].map((v) => Math.round(v * 10) / 10).join(','), under }
  })
})()`
const FULL = `(() => {
  const d = document.querySelector('.recall-full')
  if (!d) return null
  const q = d.getBoundingClientRect()
  const t = document.elementFromPoint(q.left + q.width / 2, q.top + q.height / 2)
  return { placed: d.dataset.placed, top: !!t && d.contains(t), box: [q.left, q.top, q.width, q.height].map(Math.round), scrolls: d.scrollHeight > d.clientHeight + 1,
    whole: q.left >= 0 && q.top >= 0 && q.right <= innerWidth && q.bottom <= innerHeight, name: d.querySelector('.recall-name')?.textContent }
})()`
const centre = (sel) => `(() => { const e = [...document.querySelectorAll(${JSON.stringify(sel)})].find((x) => x.closest('[hidden]') == null && x.getBoundingClientRect().width > 0); if (!e) return null; e.scrollIntoView({ block: 'nearest' }); const q = e.getBoundingClientRect(); return { x: q.left + q.width / 2, y: q.top + q.height / 2 } })()`

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-recallcard-probe-'))
const server = await createServer({ root: UI, configFile: join(UI, 'vite.config.ts'), cacheDir, logLevel: 'warn', plugins: [swap],
  server: { host: '127.0.0.1', port: 0, strictPort: false, hmr: false } })
await server.listen()
const origin = `http://127.0.0.1:${server.httpServer.address().port}`
const browser = await chrome()

async function openPage(layout, s, scale) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (m, p) => browser.call(m, p, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text)
    return r.result.value
  }
  const size = (x) => call('Emulation.setDeviceMetricsOverride', { width: x.w, height: x.h, deviceScaleFactor: x.scale, mobile: false })
  await size(s)
  await call('Page.navigate', { url: `${origin}/probe-blank` })
  await sleep(300)
  const ls = { 'nexus.workspace': 'dx', 'tempo-theme': 'dark', 'nexus.locale': 'en', 'nexus.operateLayout': layout, ...(scale !== 'auto' ? { 'nexus-ui-scale-mode': scale } : {}) }
  await evaluate(`localStorage.clear(); ${Object.entries(ls).map(([k, v]) => `localStorage.setItem(${JSON.stringify(k)}, ${JSON.stringify(v)})`).join(';')}; 1`)
  await call('Page.navigate', { url: `${origin}/#operate` })
  const click = async (at) => {
    for (const type of ['mouseMoved', 'mousePressed', 'mouseReleased']) await call('Input.dispatchMouseEvent', { type, x: at.x, y: at.y, button: 'left', clickCount: 1 })
  }
  const key = async (k, code, vk) => {
    await call('Input.dispatchKeyEvent', { type: 'keyDown', key: k, code, windowsVirtualKeyCode: vk })
    await call('Input.dispatchKeyEvent', { type: 'keyUp', key: k, code, windowsVirtualKeyCode: vk })
  }
  return { call, evaluate, size, click, key, close: () => browser.call('Target.closeTarget', { targetId }) }
}
const qso = (dxcall) => ({ state: 'AwaitReport', dxcall, rxReport: -12, running: false, cqRunning: false, txNow: `${dxcall} KD9TAW EM69`, txCount: 1 })
const frames = (p) => p.evaluate('new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(() => r(1), 30))))')
/** Until the card reads the same twice (and has the callbook's name, after the lookup's debounce). */
async function settle(p, want) {
  let last = null
  for (let i = 0; i < 40; i++) {
    await frames(p)
    const m = await p.evaluate(MEASURE).catch(() => null)
    const k = JSON.stringify(m)
    if (m && m.name === want && k === last) return m
    last = k
    await sleep(60)
  }
  return p.evaluate(MEASURE).catch(() => null)
}
async function showCall(p, call, want) {
  await p.evaluate(`globalThis.__push({}, { qso: ${JSON.stringify(qso(call))} })`)
  await sleep(500)
  return settle(p, want)
}

const t0 = Date.now()
const reds = []
const notes = []
const redAt = (s) => { reds.push(s); console.log('RED ' + s) }
const red = redAt
let drew = 0
const isCore = (s) => CORE.some((c) => c.name === s.name) || s.name === LAPTOP.name

/** The card's checks for one measure. */
function judge(tag, m, ordinary, red = redAt) {
  if (m.lines !== 1) red(`${tag}: the name "${m.name}" breaks over ${m.lines} lines on a ${m.w} px card`)
  if (m.cut && m.w >= NAME_ROOM) red(`${tag}: the name "${m.name}" is cut short on a ${m.w} px card (room for it from ${NAME_ROOM} px)`)
  // Name, call and avatar: ASSERTED wherever the rail is RAIL_ROOM px or more (every such size, never a note).
  if (m.rail >= RAIL_ROOM) {
    if (!m.avatar) red(`${tag}: the avatar is not wholly inside the ${m.w}×${m.h} card, or not on top there (rail ${m.rail} px)`)
    if (!m.nameSeen) red(`${tag}: the name is not wholly inside the ${m.w}×${m.h} card, or not on top there (rail ${m.rail} px)`)
    if (m.callSeen === false) red(`${tag}: the call is not wholly inside the ${m.w}×${m.h} card, or not on top there (rail ${m.rail} px)`)
  }
  // The identity whole, and a first contact unscrolled, wherever the card's share of its rail (a third,
  // cockpit-panes.css `.cockpit-recall`) is tall enough to hold them; elsewhere a NOTE, never a pass.
  // Held to it where the card's ceiling is tall enough AND the rail gave the card that much (a rail whose other
  // panes hold floors can give it less: that is the rail's sharing, not the card's layout).
  const need = m.headH + m.pad
  const fits = m.cap >= need && m.h >= Math.min(m.cap, need) - 1
  if (fits) {
    if (!m.head) red(`${tag}: the identity row (${m.headH} px) is not wholly inside the ${m.w}×${m.h} card (rail ${m.rail} px)`)
    if (ordinary && m.scrolls) red(`${tag}: a first contact's card scrolls (${m.w}×${m.h}, identity ${m.headH} px, rail ${m.rail} px)`)
  } else if (red === redAt && (!m.head || (ordinary && m.scrolls))) notes.push(`${tag}: identity ${need} px, the card's ceiling ${m.cap} px, given ${m.h} px of a ${m.rail} px rail — ${m.head ? 'whole' : 'cut'}${m.scrolls ? ', scrolls' : ''}`)
}

/** THE ⤢: open, check the box and every transmit surface, the QRZ button, Esc and a click outside. */
async function expandChecks(p, tag) {
  const at = await p.evaluate(centre('main.operate-cockpit .recall-expand'))
  if (!at) return red(`${tag}: no ⤢ on the card`)
  const before = await p.evaluate(TX)
  await p.click(at)
  let f = null
  for (let i = 0; i < 20 && f?.placed !== 'yes'; i++) { await frames(p); f = await p.evaluate(FULL) }
  if (!f || f.placed !== 'yes') return red(`${tag}: the ⤢ opened no placed card (${JSON.stringify(f)})`)
  await frames(p)
  f = await p.evaluate(FULL)
  if (!f.top) red(`${tag}: the whole card is not the topmost thing at its own centre`)
  if (!f.whole) red(`${tag}: the whole card is not inside the window (${f.box})`)
  const after = await p.evaluate(TX)
  const moved = after.filter((a, i) => before[i]?.box !== a.box).map((a) => a.what)
  if (moved.length || after.length !== before.length) red(`${tag}: opening the card moved or resized ${moved.join(', ') || 'a transmit surface'}`)
  const covered = after.filter((a) => a.under > 0)
  if (covered.length) red(`${tag}: the whole card covers ${covered.map((c) => `${c.what} at ${c.under}/9`).join(', ')}`)
  console.log(`  ${tag}: whole card at ${f.box}${f.scrolls ? ' (scrolls inside)' : ''}; ${after.length} transmit surfaces clear and still`)
  // The avatar's QRZ button in the whole card.
  const qrz = await p.evaluate(centre('.recall-full .recall-avatar'))
  await p.click(qrz)
  await frames(p)
  const opened = await p.evaluate('globalThis.__qrzOpened()')
  if (!opened.includes('N9BNE')) red(`${tag}: the avatar's QRZ button in the whole card opened nothing`)
  if (!(await p.evaluate(FULL))) red(`${tag}: the QRZ button closed the whole card`)
  // Esc closes it.
  await p.key('Escape', 'Escape', 27)
  await frames(p)
  if (await p.evaluate(FULL)) red(`${tag}: Esc did not close the whole card`)
  // A click outside closes it: a corner of the window outside the box.
  const again = await p.evaluate(centre('main.operate-cockpit .recall-expand'))
  if (!again) return red(`${tag}: the ⤢ was gone after Esc`)
  await p.click(again)
  for (let i = 0; i < 10 && !(await p.evaluate(FULL)); i++) await frames(p)
  if (!(await p.evaluate(FULL))) return red(`${tag}: the ⤢ did not open the whole card a second time`)
  const out = await p.evaluate(`(() => { const d = document.querySelector('.recall-full').getBoundingClientRect(); for (const [x, y] of [[4, 4], [innerWidth - 4, 4], [4, innerHeight / 2]]) if (x < d.left || x > d.right || y < d.top || y > d.bottom) return { x, y }; return null })()`)
  await p.click(out)
  await frames(p)
  if (await p.evaluate(FULL)) red(`${tag}: a click outside did not close the whole card`)
}

for (const scale of SCALES) for (const layout of LAYOUTS) {
  const p = await openPage(layout, MATRIX[0], scale)
  try {
    for (let i = 0; i < 120; i++) { await sleep(250); if (await p.evaluate(`!!document.querySelector('main.operate-cockpit .op-btn.stop')`).catch(() => false)) break }
    for (const s of SIZES) {
      await p.size(s)
      const tag = `${layout} ${scale} ${s.name} (${css(s)})`
      const m = await showCall(p, 'N9BNE', 'Barry N Evans')
      if (!m) { console.log(`${tag}: no card drawn`); continue }
      drew++
      judge(tag, m, false)
      const n = await showCall(p, 'K9NEW', 'Dale R Hughes')
      if (n) judge(`${tag} first contact`, n, true)
      console.log(`${tag} [${m.vp} ×${m.zoom}]: card ${m.w}×${m.h}, name ${m.lines} line${m.cut ? ' (cut)' : ''}, identity ${m.headH} px, avatar ${m.avatar ? 'whole' : 'CUT'}, rail ${m.rail} (other panes ${m.feeds.join('+')})${m.scrolls ? ', scrolls' : ''}; first contact ${n ? `${n.w}×${n.h}${n.scrolls ? ' SCROLLS' : ''}` : 'none'}`)
      if (isCore(s)) {
        await showCall(p, 'N9BNE', 'Barry N Evans')
        await expandChecks(p, tag)
      }
    }
  } finally {
    await p.close()
  }
}

// THE CONTROLS, at 1366×768 at the app's 100 % in Classic (the tester's look on main).
let squeezeCaught = null
let coverCaught = null
let seenCaught = null
{
  const p = await openPage('classic', LAPTOP, '100')
  try {
    for (let i = 0; i < 120; i++) { await sleep(250); if (await p.evaluate(`!!document.querySelector('main.operate-cockpit .op-btn.stop')`).catch(() => false)) break }
    await p.evaluate(`(() => { const s = document.createElement('style'); s.id = 'ctl'; s.textContent = '.recall-name{white-space:normal!important;max-width:3.5em!important}'; document.head.appendChild(s); return 1 })()`)
    const m = await showCall(p, 'N9BNE', 'Barry N Evans')
    const caught = []
    if (m) judge('CONTROL', m, false, (x) => caught.push(x))
    squeezeCaught = caught.some((x) => x.includes('breaks over'))
    console.log(`CONTROL — the name forced to wrap: ${m?.lines} lines${squeezeCaught ? ' (caught)' : ''}`)
    await p.evaluate(`document.getElementById('ctl').remove()`)
    // The visibility control: the name, the call and the avatar each covered by a planted box must all be caught.
    await p.evaluate(`(() => { for (const sel of ['.recall-name', '.recall-call', '.recall-avatar']) { const e = document.querySelector('main.operate-cockpit ' + sel); const q = e.getBoundingClientRect(); const d = document.createElement('div'); d.className = 'ctl-cover'
      d.style.cssText = 'position:fixed;z-index:99999;background:#000;left:' + q.left + 'px;top:' + q.top + 'px;width:' + q.width + 'px;height:' + q.height + 'px'; document.body.appendChild(d) } return 1 })()`)
    const v = await p.evaluate(MEASURE)
    const hid = []
    if (v) judge('CONTROL', v, false, (x) => hid.push(x))
    seenCaught = ['the name is not', 'the call is not', 'the avatar is not'].every((k) => hid.some((x) => x.includes(k)))
    console.log(`CONTROL — the name, call and avatar covered: ${seenCaught ? 'all three caught' : 'NOT all caught'}`)
    await p.evaluate(`(() => { document.querySelectorAll('.ctl-cover').forEach((d) => d.remove()); return 1 })()`)
    const at = await p.evaluate(centre('main.operate-cockpit .recall-expand'))
    if (at) {
      await p.click(at)
      for (let i = 0; i < 20 && (await p.evaluate(FULL))?.placed !== 'yes'; i++) await frames(p)
      await p.evaluate(`(() => { const b = document.querySelector('main.operate-cockpit .op-btn.stop').getBoundingClientRect(); const d = document.querySelector('.recall-full'); d.style.left = b.left + 'px'; d.style.top = b.top + 'px'; return 1 })()`)
      await frames(p)
      const tx = await p.evaluate(TX)
      coverCaught = tx.some((t) => t.under > 0)
      console.log(`CONTROL — the whole card planted over Stop TX: ${coverCaught ? 'caught' : 'NOT caught'}`)
    }
  } finally {
    await p.close()
  }
}

await browser.close?.()
await server.close()
await rm(cacheDir, { recursive: true, force: true }).catch(() => {})
if (!drew) { console.log('recallcard: ERROR the card never drew'); process.exit(2) }
if (squeezeCaught !== true) red('CONTROL: the name forced to wrap was not caught — the squeeze check cannot fail')
if (seenCaught !== true) red('CONTROL: the name, call and avatar covered were not all caught — the visibility checks cannot fail')
if (coverCaught !== true) red('CONTROL: the whole card planted over Stop TX was not caught — the cover check cannot fail')
for (const n of notes) console.log('NOTE ' + n)
console.log(reds.length ? `recallcard: FAIL — ${reds.length} red` : `recallcard: PASS — ${drew} sizes drawn, every check held, every control caught, in ${Math.round((Date.now() - t0) / 1000)} s`)
process.exit(reds.length ? 1 : 0)
