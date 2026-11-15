// Stop TX on screen and topmost on every cockpit, in every state, at the default scroll position, in real Chrome:
// `scripts/browser-probe --desktop stopvisible`.
//
// THE GUARANTEE. On every cockpit (FT, Tempo, Phone, CW, RTTY, PSK, JTTY, SSTV, APRS, JS8), at every size class
// xs-xl, at 100-175 % and the app's own fit, in every language, at least one Stop TX (`.op-btn.stop`, halt_tx) is
// FULLY inside the window and the topmost thing over its whole face, with nothing scrolled: the screen as it opens,
// whatever the radio is doing. Not "reachable by scrolling": an operator who needs Stop TX has no time to look for it.
// digishell holds the stops at every scroll position of the screens it opens in the plain state; this probe adds the
// screens it does not open (FT, Tempo, APRS), the states that grow the chrome, and every language.
//
// WHY. Two cases were measured on main before the fix:
//   · SSTV at 900×600 / 175 % in Russian: the engine's dropped-picture sentence grew the Send/Stop bar from 133 to
//     225 px, and the bar, sticky at the bottom, then covered the TX strip's Stop TX.
//   · FT at 150 / 175 % on a small window: the QSO strip, Stop TX in it, sat below the waterfall and the window's
//     bottom edge, so Stop TX was not on screen until the operator scrolled.
//
// THE STATES, each raised on the open screen and cleared again (the snapshot pushed through the app's own
// subscription; the lane through src/status.ts; the mode engines' state through their polls):
//   plain · catDown (CAT lost) · alarms (two transmitter alarms, the TX watchdog, a critical lane message) ·
//   keyed (on the air: the strip lit, an FT contact in progress, SSTV sending with its progress bar) ·
//   keyedAlarm (keyed under the alarms: SSTV's own Stop live while the shell is at its shortest) ·
//   worst (CAT lost + the alarms + an FT contact + SSTV's dropped-picture sentence + JTTY's longest refusal).
//   firstRun (no callsign yet, so the first-run nudge to set one is up, keyed under the alarms: what a new operator
//   meets; the probe's own station has a callsign, so no other state ever showed the nudge).
//
// HOW "ON SCREEN AND TOPMOST" IS READ: the button's whole box is inside the window, and at each point of a 5 × 3
// grid across its face (inset past its round corners) the topmost element is the button. While it reads, every
// element takes pointer events, so a notice that lets clicks through still counts as covering what it hides.
//
// AND THE TAB WALK (FT, plain, 150/175 % up to 1366×768): real Tab through the shell; no focused control may have its
// centre behind the QSO strip's parked line (it covered 36 Tab stops at 900×600 / 175 % before the shell's scroll
// padding read the parked line's height).
//
// AND THE HEADER-CHANGE GUARD (FT at 100 % and fit, the floor (1366×768 at 175 %) and 1366×768, en/ru, CAT lost and the alarms): Stop TX stands
// where a fresh layout (the window resized away and back) puts it, so the waterfall's divider re-fit to the header.
//
// AND THE STOP LAYER (a modal open): one real click at Stop TX on FT's parked line, and at Stop TX and SSTV's own Stop in its
// squeezed bar while sending, stops exactly once.
//
// THE CONTROLS, run every time: a planted box over Stop TX on FT at 900×600 / 175 % must be caught, and so must the
// shell's scroll padding taken away there (a Tab stop behind the parked line). If one is not, its check cannot fail and
// the probe says so instead of passing.
//
// SCOPE. The default is the CI matrix below. STOPVIS_FULL=1 runs every size × scale × language; STOPVIS_VIEWS,
// STOPVIS_LOCS, STOPVIS_SCALES ("100,175,auto") and STOPVIS_SIZES ("900x600,700x800") narrow either one, so the full sweep can be run in parts.
//
// exit: 0 every check held · 1 a red, or a control that could not fail · 2 a screen never drew
import { createServer } from 'vite'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'
import { FLOOR, FLOOR_150, LAPTOP, MATRIX, UHD, UHD_200 } from './sizes.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const REAL_API = join(UI, 'src', 'api.ts')
const STUB_SRC = join(UI, 'probe', 'waterfalls-api.ts')
const STUB_API = join(UI, 'probe', '__stopvisible-api.ts')
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

/** [label, route]. Tempo is the `chat` view. */
const VIEWS = [['FT', 'operate'], ['Tempo', 'chat'], ['Phone', 'phone'], ['CW', 'cw'], ['RTTY', 'rtty'], ['PSK', 'psk'],
  ['JTTY', 'jtty'], ['SSTV', 'sstv'], ['APRS', 'aprs'], ['JS8', 'js8']]
const ALL_LOCS = ['en', 'de', 'es', 'fr', 'it', 'ja', 'ru', 'zh', 'zh-hant']
/** A window as [CSS width, CSS height, the OS's scaling as a device pixel ratio] — a screen of probe/sizes.mjs, or a
 *  bare [w, h] window off the matrix (dragged below any supported screen: 700×800, 900×600, 1200×750 hold xs). */
const px = (s) => [s.w, s.h, s.scale]
const lab = ([W, H, D = 1]) => `${W}×${H}${D === 1 ? '' : ` (OS ${Math.round(D * 100)} %)`}`
const OFF = [[700, 800], [900, 600], [1200, 750]]
/** [scale ('auto' = the app's own fit), sizes, languages]: one page per view × scale × language, resized through its sizes.
 *  The scale is the app's own; the floor's row is 1366×768 at the OS's 175 % (a 780×438 window at ratio 1.75), the same
 *  effective window as 1366×768 at the app's 175 %, and 4K is there at 100 % (the widest) and at the OS's 200 %. */
const CI = [
  ['175', [[700, 800], [900, 600], px(LAPTOP)], ['en', 'ru']],
  ['150', [[900, 600], px(LAPTOP)], ['en']],
  ['100', [px(FLOOR), px(UHD), px(UHD_200)], ['en']],
  ['auto', [[1200, 750], px(FLOOR)], ['en']],
]
const FULL_SIZES = [...OFF, ...MATRIX.map(px)]
const FULL = [
  ['175', FULL_SIZES, ALL_LOCS], ['150', FULL_SIZES, ALL_LOCS], ['125', FULL_SIZES, ALL_LOCS],
  ['100', FULL_SIZES, ALL_LOCS], ['auto', FULL_SIZES, ALL_LOCS],
]
const pick = (env, all) => (process.env[env] ? process.env[env].split(',').map((s) => s.trim()) : all)
const wantViews = pick('STOPVIS_VIEWS', VIEWS.map((v) => v[0]))
const wantLocs = process.env.STOPVIS_LOCS ? pick('STOPVIS_LOCS', ALL_LOCS) : null
const wantSizes = process.env.STOPVIS_SIZES ? process.env.STOPVIS_SIZES.split(',').map((x) => x.split('x').map(Number)) : null
const wantScales = process.env.STOPVIS_SCALES ? pick('STOPVIS_SCALES', []) : null
const PLAN = (process.env.STOPVIS_FULL === '1' ? FULL : CI)
  .filter(([s]) => !wantScales || wantScales.includes(s))
  .map(([s, sizes, locs]) => [s, wantSizes ?? sizes, wantLocs ?? locs])
const FEATURES = JSON.stringify({ profile: 'custom', enabled: { rtty: true, psk: true, sstv: true, js8: true, jtty: true, aprs: true, chat: true } })

// THE API SWAP — the waterfalls probe's stub (with the APRS map probe's answers appended), with three seams for the states: the snapshot's subscription is real
// (`__push`), and the SSTV / JTTY engines' polled state reads an override (`__modeOver`).
const realSrc = await readFile(REAL_API, 'utf8')
let stubPatched = (await readFile(STUB_SRC, 'utf8'))
  .replace('export function subscribeSnapshot(_fn: unknown): () => void { return () => {} }',
    `const __subs: any[] = []
;(globalThis as any).__push = (radio: any, top: any) => { snapshot.radio = { ...snapshot.radio, ...(radio || {}) }; Object.assign(snapshot, top || {}); const s = { ...snapshot, radio: { ...snapshot.radio } }; __subs.forEach((f) => f(s)); return __subs.length }
export function subscribeSnapshot(fn: any): () => void { __subs.push(fn); return () => { const i = __subs.indexOf(fn); if (i >= 0) __subs.splice(i, 1) } }`)
  // Whole lines, whatever another probe's own override on them says.
  .replace(/^export async function getSstvState\(\).*$/m,
    'export async function getSstvState(): Promise<any> { return { ...sstv, ...(globalThis as any).__modeOver?.sstv } }')
  .replace(/^export async function getJttyState\(\).*$/m,
    'export async function getJttyState(): Promise<any> { return { ...jtty, ...(globalThis as any).__jttyOver, ...(globalThis as any).__modeOver?.jtty } }')
// APRS reads its station list and health on mount: the APRS map probe's answers (a small local net), appended as it does.
stubPatched += '\n' + (await readFile(join(UI, 'probe', 'aprsmap-api.ts'), 'utf8'))
// The stops are COUNTED, for the stop layer's check: Stop TX's halt and SSTV's own Stop.
stubPatched += "\nlet __halts = 0, __sstvStops = 0\n;(globalThis as any).__probeStops = () => ({ halts: __halts, sstv: __sstvStops })\nexport async function haltTx(): Promise<any> { __halts += 1; return snapshot }\nexport async function sstvStop(): Promise<any> { __sstvStops += 1; return sstv }\n"
// An FT contact in progress has the recall card look its partner up; the generated `{}` answers crash it.
stubPatched += "\nexport async function resolveEntity(): Promise<any> { return 'United States' }\nexport async function qrzLookup(): Promise<any> { return null }\n"
for (const seam of ['__push', '__modeOver?.sstv', '__modeOver?.jtty']) if (!stubPatched.includes(seam)) throw new Error(`stopvisible: the stub seam ${seam} did not apply; probe/waterfalls-api.ts changed shape`)
const names = [...realSrc.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1])
const own = new Set([...stubPatched.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1]))
const stubCode = stubPatched + `\nexport * from ${JSON.stringify(REAL_API)}\n` +
  names.filter((n) => !own.has(n)).map((n) => `export async function ${n}(..._a: unknown[]): Promise<any> { return (globalThis as any).__DEFAULT_ARR ? [] : {} }`).join('\n')
const swap = {
  name: 'stopvisible-probe-api', enforce: 'pre',
  async resolveId(source, importer, opts) {
    if (opts?.scan) return null
    if (source === STUB_API) return STUB_API
    if (!importer || importer === STUB_API || !/api(\.ts)?$/.test(source)) return null
    const r = await this.resolve(source, importer, { ...opts, skipSelf: true })
    return r && r.id.split('?')[0] === REAL_API ? STUB_API : null
  },
  load(id) { return id === STUB_API ? stubCode : null },
}

// THE STATES. Text as the engine words it, at its longest.
const ALARMS = [
  { id: 1, text: 'High SWR: 3.2:1 — transmit was stopped', radioId: 1, radioName: 'IC-7300', atMs: 0 },
  { id: 2, text: 'PA temperature 82 C — transmit was stopped', radioId: 1, radioName: 'IC-7300', atMs: 0 },
]
const LANE = { tier: 'critical', message: 'PTT OFF FAILED — TX STOPPED', detail: 'probe' }
const QSO = { state: 'AwaitRr73', dxcall: 'VP2V/W1AW', rxReport: -12, running: false, cqRunning: false, txNow: 'VP2V/W1AW KD9TAW R-07', txCount: 3 }
const DROPPED = 'Picture dropped: transmit went off while it waited for the transmitter, so nothing was sent. Turn TX on and send it again.'
const REFUSAL = "JTTY keyer: the rig didn't accept PTT, so nothing was sent and what was queued was dropped. Check your PTT method + that Nexus's audio output is routed to the rig (like FT8)."
const CAT_LOST = { catOk: false, catDetail: '14.0740 MHz CAT link unreachable (1/3)' }
const PLAIN = { radio: { catOk: true, catDetail: '14.0740 MHz', transmitting: false, txEnabled: false, txWatchdog: false }, top: { txAlarms: [], qso: null, mycall: 'KD9TAW' }, lane: null, mode: null }
const STATES = {
  plain: PLAIN,
  catDown: { ...PLAIN, radio: { ...PLAIN.radio, ...CAT_LOST } },
  alarms: { ...PLAIN, radio: { ...PLAIN.radio, txWatchdog: true }, top: { txAlarms: ALARMS, qso: null }, lane: LANE },
  keyed: { ...PLAIN, radio: { ...PLAIN.radio, transmitting: true, txEnabled: true }, top: { txAlarms: [], qso: { ...QSO, running: true } },
    mode: { sstv: { sending: true, txMode: 'Martin M1', txProgress: 0.4, txElapsedSecs: 46, txTotalSecs: 114 } } },
  // On the air under an alarm: SSTV sending (its own Stop live) while the shell is at its shortest.
  keyedAlarm: { radio: { ...PLAIN.radio, transmitting: true, txEnabled: true, txWatchdog: true }, top: { txAlarms: ALARMS, qso: { ...QSO, running: true } }, lane: LANE,
    mode: { sstv: { sending: true, txMode: 'Martin M1', txProgress: 0.4, txElapsedSecs: 46, txTotalSecs: 114 } } },
  worst: { radio: { ...PLAIN.radio, ...CAT_LOST, txWatchdog: true }, top: { txAlarms: ALARMS, qso: QSO }, lane: LANE,
    mode: { sstv: { txNotice: DROPPED }, jtty: { tx: { refusal: REFUSAL, txHz: 1500, serial: 1, sending: false, live: false, left: 0, pending: '', notice: null } } } },
  // A fresh install: no callsign yet, so the first-run nudge to set one (OnboardingBanner) is up, as a new operator
  // meets it, and on the air under the alarms besides. Every other state puts the callsign back.
  firstRun: { radio: { ...PLAIN.radio, transmitting: true, txEnabled: true, txWatchdog: true }, top: { txAlarms: ALARMS, qso: { ...QSO, running: true }, mycall: '' }, lane: LANE,
    mode: { sstv: { sending: true, txMode: 'Martin M1', txProgress: 0.4, txElapsedSecs: 46, txTotalSecs: 114 } } },
}

/** What lifts every element into hit-testing while the probe reads (a notice that lets clicks through still hides what
 *  is under it), except the stop layer (components/StopLayer): a transparent, click-through layer over the whole window
 *  that draws nothing while no overlay is open, so it covers nothing an operator sees or clicks. */
const LIFT_CSS = '*:not(.stop-layer) { pointer-events: auto !important }'
/** Every Stop TX on the page: where it is, and whether its whole face is on screen and topmost. */
const MEASURE = `(() => {
  if (!document.documentElement.getAttribute('data-viewport')) return null
  // The REAL button only: a stop layer's mirror (a clone, data-stop-mirror) is proven by the clicking check, never counted here.
  const stops = [...document.querySelectorAll('.op-btn.stop:not([data-stop-mirror])')].filter((b) => b.closest('[hidden]') == null && b.getBoundingClientRect().width > 0)
  if (!stops.length) return { vp: document.documentElement.getAttribute('data-viewport'), stops: [] }
  const lift = document.createElement('style'); lift.textContent = ${JSON.stringify(LIFT_CSS)}; document.head.appendChild(lift)
  const name = (t) => !t ? 'nothing' : (t.closest('.sstv-tx-bar') ? 'sstv-tx-bar>' : t.closest('.cockpit-txdock, .cockpit-txdock-keep') ? 'dock>' : '') + (String(t.className || '').split(' ')[0] || t.tagName).slice(0, 32)
  const rows = stops.map((b) => {
    const q = b.getBoundingClientRect()
    const inside = q.left >= -0.5 && q.top >= -0.5 && q.right <= innerWidth + 0.5 && q.bottom <= innerHeight + 0.5
    let covered = 0, by = null
    for (let i = 0; i < 5; i++) for (let j = 0; j < 3; j++) {
      const x = q.left + q.width * (0.12 + 0.19 * i), y = q.top + q.height * (0.2 + 0.3 * j)
      if (x < 0 || y < 0 || x >= innerWidth || y >= innerHeight) { covered++; continue }
      const t = document.elementFromPoint(x, y)
      if (!(t === b || b.contains(t))) { covered++; by ??= name(t) }
    }
    return { where: b.closest('.cockpit-qso') ? 'strip' : b.closest('.cockpit-txstrip') ? 'strip' : b.closest('.topbar, header') ? 'bar' : 'other',
      box: [q.left, q.top, q.width, q.height].map((v) => Math.round(v)), inside, covered, by, ok: inside && covered === 0 }
  })
  lift.remove()
  const shells = [...document.querySelectorAll('main')].filter((m) => m.closest('[hidden]') == null && m.getBoundingClientRect().height > 0)
  const head = [...document.querySelectorAll('.cockpit-header')].find((h) => h.closest('[hidden]') == null && h.getBoundingClientRect().height > 0)
  // Printed, not failed: whether the strip's transmitter alarm cell (where a screen draws one) is on screen.
  const cell = [...document.querySelectorAll('.ch-txalarm-text')].find((x) => x.closest('[hidden]') == null && x.getBoundingClientRect().width > 0)
  const cq = cell?.getBoundingClientRect()
  const alarmSeen = !cell || !cell.textContent.trim() ? null : (() => { const x = cq.left + cq.width / 2, y = cq.top + cq.height / 2
    if (x < 0 || y < 0 || x >= innerWidth || y >= innerHeight) return false; const t = document.elementFromPoint(x, y); return t != null && (t === cell || cell.contains(t)) })()
  // Printed, not failed: a squeezed bottom surface's own stop (SSTV's Stop, Esc/Stop, PTT), whether it is in view.
  const sq = [...document.querySelectorAll("[data-squeeze='on']")].find((x) => x.closest('[hidden]') == null)
  const own = sq?.querySelector('.sstv-tx-stop, .rtty-stop, .ph-ptt')
  const squeezed = !sq ? null : (() => { const z = sq.getBoundingClientRect().height / (sq.offsetHeight || 1); const cap = Math.round(sq.clientHeight * z)
    // Any control of it whole inside its box, as its own scroll stands.
    const box = sq.getBoundingClientRect()
    const ctlWhole = [...sq.querySelectorAll('button, input, select, textarea')].some((e) => { const q = e.getBoundingClientRect(); return q.height > 0 && q.top >= box.top - 0.5 && q.bottom <= box.bottom + 0.5 })
    const st0 = [...document.querySelectorAll('.cockpit-txstrip')].find((x) => x.closest('[hidden]') == null && x.getBoundingClientRect().height > 0)
    const overStrip0 = !!st0 && box.top < st0.getBoundingClientRect().bottom - 0.5
    if (!own) return { cap, ctlWhole, overStrip: overStrip0, stop: null }; const q = own.getBoundingClientRect(); const x = q.left + q.width / 2, y = q.top + q.height / 2
    const t = x >= 0 && y >= 0 && x < innerWidth && y < innerHeight ? document.elementFromPoint(x, y) : null
    const st = [...document.querySelectorAll('.cockpit-txstrip')].find((x) => x.closest('[hidden]') == null && x.getBoundingClientRect().height > 0)
    const overStrip = !!st && box.top < st.getBoundingClientRect().bottom - 0.5
    return { cap, ctlWhole, overStrip, stop: { seen: t != null && (t === own || own.contains(t)), live: !own.disabled, h: Math.round(q.height) } } })()
  const span = (sel) => { const e = [...document.querySelectorAll(sel)].find((x) => x.closest('[hidden]') == null && x.getBoundingClientRect().height > 0); if (!e) return null; const q = e.getBoundingClientRect(); return Math.round(q.top) + '–' + Math.round(q.bottom) }
  const nudge = [...document.querySelectorAll('.onboarding-banner')].some((x) => x.getBoundingClientRect().height > 0)
  return { vp: document.documentElement.getAttribute('data-viewport'), stops: rows, nudge, headBottom: head ? Math.round(head.getBoundingClientRect().bottom) : null,
    alarmSeen, squeezed, boxes: { shell: span('main'), strip: span('.cockpit-txstrip, .cockpit-qso'), dock: span('.cockpit-txdock, .cockpit-txdock-keep[data-fold], .sstv-tx-bar') },
    scrolled: shells.map((m) => Math.round(m.scrollTop)).concat(Math.round(document.scrollingElement.scrollTop)).some((v) => v > 0),
    key: rows.map((r) => r.box.join(',') + r.covered).join('|') + '|' + document.body.scrollHeight }
})()`

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-stopvisible-probe-'))
const server = await createServer({ root: UI, configFile: join(UI, 'vite.config.ts'), cacheDir, logLevel: 'warn', plugins: [swap],
  server: { host: '127.0.0.1', port: 0, strictPort: false, hmr: false } })
await server.listen()
const origin = `http://127.0.0.1:${server.httpServer.address().port}`
const browser = await chrome()

async function openPage(route, [W, H, D = 1], scale, loc) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (m, p) => browser.call(m, p, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text)
    return r.result.value
  }
  const size = (w, h, d = 1) => call('Emulation.setDeviceMetricsOverride', { width: w, height: h, deviceScaleFactor: d, mobile: false })
  await size(W, H, D)
  // What broke, if a screen stops drawing: the page's own errors, kept for the report.
  await call('Page.addScriptToEvaluateOnNewDocument', { source: "window.__errs = []; addEventListener('error', (e) => __errs.push(String(e.message))); addEventListener('unhandledrejection', (e) => __errs.push(String(e.reason))); { const ce = console.error; console.error = (...a) => { __errs.push(a.map(String).join(' ').slice(0, 300)); ce(...a) } }" })
  await call('Page.navigate', { url: `${origin}/probe-blank` })
  await sleep(300)
  // Tempo is the MSG workspace's screen; on 'dx' its route opens FT.
  const ls = { 'nexus.workspace': route === 'chat' ? 'msg' : 'dx', 'nexus.features.v1': FEATURES, 'tempo-theme': 'dark', 'nexus.locale': loc, ...(scale !== 'auto' ? { 'nexus-ui-scale-mode': scale } : {}) }
  await evaluate(`localStorage.clear(); ${Object.entries(ls).map(([k, v]) => `localStorage.setItem(${JSON.stringify(k)}, ${JSON.stringify(v)})`).join(';')}; 1`)
  await call('Page.navigate', { url: `${origin}/#${route}` })
  return { told: false, call, evaluate, size, close: () => browser.call('Target.closeTarget', { targetId }) }
}

const frames = (p) => p.evaluate('new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(() => r(1), 30))))')
/** Where every Stop TX and every shell child stands: cheap, read until two reads agree (a strip re-measures its parked
 *  neighbours a frame after they change), and only then the full read. */
const LAYOUT_KEY = `[...document.querySelectorAll('.op-btn.stop, main > *, .sstv-tx-bar > *')].map((e) => { const q = e.getBoundingClientRect(); return Math.round(q.top) + ':' + Math.round(q.height) }).join(',')`
async function settle(p, minMs = 0) {
  if (minMs) await sleep(minMs)
  let last = null
  for (let i = 0; i < 25; i++) {
    await frames(p)
    const k = await p.evaluate(LAYOUT_KEY).catch(() => null)
    if (k != null && k === last) break
    last = k
  }
  return p.evaluate(MEASURE).catch(() => null)
}
async function apply(p, st, route) {
  await p.evaluate(`import('/src/status.ts').then((m) => { m.setStatus('stopvisible', ${JSON.stringify(st.lane)}); return 1 })`)
  await p.evaluate(`(() => { globalThis.__modeOver = ${JSON.stringify(st.mode)}; return globalThis.__push(${JSON.stringify(st.radio)}, ${JSON.stringify(st.top)}) })()`)
  // The SSTV and JTTY engines are polled once a second: on their screens a state that changes them waits out one poll.
  return st.mode && POLLED.has(route) ? 1300 : 0
}

/** THE TAB WALK (FT, plain state, sizes where its strip parks): real Tab from the start of the shell, and every control it
 *  focuses in the shell must not have its centre behind the parked QSO-strip line. Parked at the foot, the line covered
 *  36 Tab stops at 900×600 / 175 % (measured) until the shell's scroll padding read its height. Returns the names hidden. */
async function tabWalk(p) {
  await p.evaluate(`(() => { document.activeElement?.blur?.(); const m = document.querySelector('main.operate-cockpit'); m.scrollTop = 0; m.querySelector('button, input, select')?.focus({ preventScroll: true }); m.scrollTop = 0; return 1 })()`)
  const hidden = []
  let first = null
  for (let i = 0; i < 160; i++) {
    await p.call('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Tab', code: 'Tab', windowsVirtualKeyCode: 9 })
    await p.call('Input.dispatchKeyEvent', { type: 'keyUp', key: 'Tab', code: 'Tab', windowsVirtualKeyCode: 9 })
    await frames(p)
    const f = await p.evaluate(`(() => { const e = document.activeElement; if (!e || !e.closest('main.operate-cockpit')) return { out: true }
      const r = e.getBoundingClientRect(); const x = r.left + r.width / 2, y = r.top + r.height / 2
      const name = ((e.getAttribute('aria-label') || e.textContent || e.tagName) + '').replace(/\\s+/g, ' ').trim().slice(0, 20) + '@' + Math.round(y)
      if (!r.width || !r.height || x < 0 || y < 0 || x >= innerWidth || y >= innerHeight) return { name }
      const lift = document.createElement('style'); lift.textContent = ${JSON.stringify(LIFT_CSS)}; document.head.appendChild(lift)
      const t = document.elementFromPoint(x, y); lift.remove()
      return { name, behind: !(t === e || e.contains(t)) && !!t?.closest('.cockpit-ftstrip') && !e.closest('.cockpit-ftstrip') } })()`).catch(() => null)
    if (!f) continue
    if (f.out) { if (first) break; continue }
    if (f.name === first) break
    first ??= f.name
    if (f.behind) hidden.push(f.name)
  }
  await p.evaluate(`(() => { document.activeElement?.blur?.(); document.querySelector('main.operate-cockpit').scrollTop = 0; return 1 })()`)
  return hidden
}
const TAB_SIZES = (W, H) => W <= 1366 && H <= 800

const POLLED = new Set(['sstv', 'jtty'])
const rows = []
const reds = []
const nudgeRows = []
let errors = 0
const t0 = Date.now()
for (const [label, route] of VIEWS.filter(([l]) => wantViews.includes(l))) {
  for (const [scale, sizes, locs] of PLAN) {
    for (const loc of locs) {
      const p = await openPage(route, sizes[0], scale, loc)
      try {
        let first = null
        for (let i = 0; i < 160 && !first?.stops.length; i++) { await sleep(250); first = await p.evaluate(MEASURE).catch(() => null) }
        if (!first?.stops.length) {
          errors++
          console.log(`${label} ${scale} ${loc}: ERROR no Stop TX ever drew — page errors: ${JSON.stringify(await p.evaluate('(window.__errs || []).slice(-3)').catch((e) => String(e))).slice(0, 900)}; page: ${await p.evaluate(`JSON.stringify({ hash: location.hash, mains: [...document.querySelectorAll('main')].map((m) => m.className.slice(0, 40) + (m.closest('[hidden]') ? ' [hidden]' : '')), hosts: [...document.querySelectorAll('[class$=host]')].map((h) => h.className + (h.hidden ? ' [hidden]' : '')) })`).catch((e) => String(e))}`)
          continue
        }
        let mode = false
        for (const [W, H, D = 1] of sizes) {
          await p.size(W, H, D)
          const sz = lab([W, H, D])
          for (const [state, st] of Object.entries(STATES)) {
            const wait = await apply(p, st, route)
            const m = await settle(p, wait || (mode ? 1300 : 0))
            mode = !!wait
            const row = { view: label, scale, loc, W, H, D, state, vp: m?.vp, stops: m?.stops ?? [], scrolled: m?.scrolled, alarmSeen: m?.alarmSeen ?? null, squeezed: m?.squeezed ?? null }
            rows.push(row)
            const ok = row.stops.some((s) => s.ok)
            // The first-run state is only a test while the nudge is drawn: printed per row, so a nudge that moved off a
            // screen reads as such, and a page that never drew it anywhere is a red below.
            if (state === 'firstRun') { row.nudge = !!m?.nudge; nudgeRows.push(row) }
            if (route === 'operate' && state === 'plain' && scale !== 'auto' && Number(scale) >= 150 && TAB_SIZES(W, H)) {
              const hidden = await tabWalk(p)
              if (hidden.length) {
                reds.push(`${label} ${sz} ${scale} ${loc} plain: Tab focuses ${hidden.length} controls behind the parked QSO-strip line (${hidden.slice(0, 8).join(', ')})`)
                console.log('RED ' + reds[reds.length - 1])
              }
            }
            if (!row.stops.length && !p.told) { p.told = true; console.log(`${label} ${sz} ${scale} ${loc} ${state}: NO STOP TX — page errors: ${JSON.stringify(await p.evaluate('(window.__errs || []).slice(-4)').catch((e) => String(e)))}; page: ${await p.evaluate(`JSON.stringify({ hash: location.hash, mains: [...document.querySelectorAll('main')].map((m) => m.className.slice(0, 40) + (m.closest('[hidden]') ? ' [hidden]' : '') + ' ' + Math.round(m.getBoundingClientRect().height)), stops: [...document.querySelectorAll('.op-btn.stop')].map((b) => Math.round(b.getBoundingClientRect().width) + (b.closest('[hidden]') ? 'H' : '')), text: document.body.innerText.slice(-500) })`).catch((e) => String(e))}`) }
            if (!ok) {
              const why = row.stops.map((s) => `${s.where} Stop TX at y ${s.box[1]}–${s.box[1] + s.box[3]} of ${H}${s.inside ? '' : ' (not inside the window)'}${s.covered ? `, covered at ${s.covered}/15 by ${s.by}` : ''}`).join('; ') || 'no Stop TX drawn'
              reds.push(`${label} ${sz} ${scale} ${loc} [${row.vp}] ${state}: no Stop TX fully on screen and topmost — ${why}; header ends at y ${m?.headBottom}; shell ${m?.boxes?.shell}, strip ${m?.boxes?.strip}, dock ${m?.boxes?.dock}${row.scrolled ? ' (a scroller moved)' : ''}`)
              // As it is found: a part of the full sweep cut short by its timeout keeps what it saw.
              console.log('RED ' + reds[reds.length - 1])
            }
          }
        }
        console.log(`${label} ${scale} ${loc}: ${sizes.length * Object.keys(STATES).length} rows, ${reds.length} red so far, ${Math.round((Date.now() - t0) / 1000)} s`)
      } finally {
        await p.close()
      }
    }
  }
}

// THE HEADER-CHANGE GUARD (FT, 100 % and the app's fit, the floor and 1366×768, en and ru): CAT lost, and the alarms (a
// transmitter alarm, the watchdog and a critical status message), change the cockpit header's height or the window's. The
// waterfall's divider must re-fit to that, as a fresh layout would: Stop TX is measured as the state leaves it, the window
// is then resized away and back (every divider re-fits on that), and Stop TX must stand where it stood. A divider that
// re-fit only on the shell's own resize left the waterfall at its old height and Stop TX 28 px off on CAT lost.
const FRESH = [['100', px(FLOOR)], ['100', px(LAPTOP)], ['auto', px(FLOOR)], ['auto', px(LAPTOP)]]
const stopY = `(() => { const b = document.querySelector('main.operate-cockpit .cockpit-qso .op-btn.stop'); return b ? Math.round(b.getBoundingClientRect().top * 10) / 10 : null })()`
async function freshCheck(p, [W, H, D = 1], plant = null) {
  await settle(p)
  const before = plant ? await p.evaluate(plant) : await p.evaluate(stopY)
  await p.size(W + 40, H + 40, D)
  await settle(p)
  await p.size(W, H, D)
  await settle(p)
  const after = await p.evaluate(stopY)
  return before == null || after == null ? null : Math.round((after - before) * 10) / 10
}
const freshReds = []
if (wantViews.includes('FT') && process.env.STOPVIS_FRESH !== '0') {
  for (const [scale, size] of FRESH) for (const loc of ['en', 'ru']) {
    const p = await openPage('operate', size, scale, loc)
    try {
      let m = null
      for (let i = 0; i < 160 && !m?.stops.length; i++) { await sleep(250); m = await p.evaluate(MEASURE).catch(() => null) }
      for (const state of ['catDown', 'alarms']) {
        await apply(p, PLAIN, 'operate')
        await settle(p)
        await apply(p, STATES[state], 'operate')
        const dy = await freshCheck(p, size)
        if (dy == null || Math.abs(dy) > 1) {
          freshReds.push(`FT ${lab(size)} ${scale} ${loc} ${state}: Stop TX stood ${dy} px from where a fresh layout puts it (the waterfall's divider did not re-fit)`)
          console.log('RED ' + freshReds[freshReds.length - 1])
        }
      }
    } finally {
      await p.close()
    }
  }
  console.log(`fresh-layout guard: ${FRESH.length * 2 * 2} rows, ${freshReds.length} red`)
}
// ITS CONTROL: a divider value left stale (written and read in one task, before any observer can re-fit it) must be caught.
// On 1366×768 at 100 %: at the floor the waterfall already stands on its own floor, so half the value lays out the same.
let freshControl = null
if (wantViews.includes('FT') && process.env.STOPVIS_FRESH !== '0') {
  const p = await openPage('operate', px(LAPTOP), '100', 'en')
  try {
    let m = null
    for (let i = 0; i < 160 && !m?.stops.length; i++) { await sleep(250); m = await p.evaluate(MEASURE).catch(() => null) }
    await apply(p, PLAIN, 'operate')
    const dy = await freshCheck(p, px(LAPTOP), `(() => { const body = document.querySelector('main.operate-cockpit .cockpit-body'); const v = parseFloat(body.style.getPropertyValue('--cockpit-wf-h')) || 25
      body.style.setProperty('--cockpit-wf-h', (v / 2) + '%'); return ${stopY} })()`)
    freshControl = dy != null && Math.abs(dy) > 1
    console.log(`CONTROL — FT ${lab(px(LAPTOP))} 100 % with the divider's value left stale: Stop TX ${dy} px from a fresh layout${freshControl ? ' (caught)' : ''}`)
  } finally {
    await p.close()
  }
}

// THE CONTROL: a box planted over FT's Stop TX must be caught.
let control = null
{
  const p = await openPage('operate', [900, 600], '175', 'en')
  try {
    let m = null
    for (let i = 0; i < 160 && !m?.stops.length; i++) { await sleep(250); m = await p.evaluate(MEASURE).catch(() => null) }
    await apply(p, PLAIN, 'operate')
    await p.evaluate(`(() => { const b = document.querySelector('.op-btn.stop'); const q = b.getBoundingClientRect(); const d = document.createElement('div')
      d.style.cssText = 'position:fixed;z-index:99999;background:#000;left:' + q.left + 'px;top:' + (q.top + q.height / 2) + 'px;width:' + q.width + 'px;height:' + q.height + 'px'
      document.body.appendChild(d); return 1 })()`)
    m = await settle(p)
    control = m ? !m.stops.some((s) => s.ok) : null
  } finally {
    await p.close()
  }
}

// THE TAB CONTROL: the shell's scroll padding taken away must leave a Tab stop behind FT's parked line.
let tabControl = null
{
  const p = await openPage('operate', [900, 600], '175', 'en')
  try {
    let m = null
    for (let i = 0; i < 160 && !m?.stops.length; i++) { await sleep(250); m = await p.evaluate(MEASURE).catch(() => null) }
    await apply(p, PLAIN, 'operate')
    await settle(p)
    await p.evaluate(`(() => { const s = document.createElement('style'); s.textContent = '.layout.single { scroll-padding-bottom: 0 !important }'; document.head.appendChild(s); return 1 })()`)
    const hidden = await tabWalk(p)
    tabControl = hidden.length > 0
    console.log(`CONTROL — FT 900×600 175 % with the shell's scroll padding taken away: ${hidden.length} Tab stops behind the parked line${hidden.length ? ' (caught)' : ''}`)
  } finally {
    await p.close()
  }
}

// THE STOP LAYER OVER A PARKED STRIP AND A SQUEEZED BAR (components/StopLayer): with a modal open over the whole window,
// a real click at Stop TX's own place on FT's parked line (900×600 / 175 %), and at Stop TX and at SSTV's own Stop in its
// squeezed bar while SSTV is sending under an alarm (900×600 / 175 %, ru), stops exactly once, through the layer's mirror.
// The control: the stop layer taken away, the same click stops nothing (the modal takes it).
async function mirrorCase(route, size, scale, loc, state, sel, kind, css = null) {
  const p = await openPage(route, size, scale, loc)
  try {
    let m = null
    for (let i = 0; i < 160 && !m?.stops.length; i++) { await sleep(250); m = await p.evaluate(MEASURE).catch(() => null) }
    const wait = await apply(p, STATES[state], route)
    await settle(p, wait)
    if (css) await p.evaluate(`(() => { const s = document.createElement('style'); s.textContent = ${JSON.stringify(css)}; document.head.appendChild(s); return 1 })()`)
    await p.evaluate(`(() => { const d = document.createElement('div'); d.setAttribute('role', 'dialog'); d.setAttribute('aria-modal', 'true'); d.className = 'probe-modal'
      d.style.cssText = 'position:fixed;inset:0;z-index:1000;background:rgba(0,0,0,0.35)'; document.body.appendChild(d); return 1 })()`)
    await frames(p); await frames(p)
    const at = await p.evaluate(`(() => { const b = document.querySelector(${JSON.stringify(sel)}); if (!b) return null; const q = b.getBoundingClientRect()
      return { x: q.left + q.width / 2, y: q.top + q.height / 2, disabled: !!b.disabled, mirrors: document.querySelectorAll('.stop-layer > *').length } })()`)
    if (!at) return { error: 'no ' + sel }
    const before = await p.evaluate('globalThis.__probeStops()')
    await p.call('Input.dispatchMouseEvent', { type: 'mouseMoved', x: at.x, y: at.y })
    await p.call('Input.dispatchMouseEvent', { type: 'mousePressed', x: at.x, y: at.y, button: 'left', clickCount: 1 })
    await p.call('Input.dispatchMouseEvent', { type: 'mouseReleased', x: at.x, y: at.y, button: 'left', clickCount: 1 })
    await sleep(300)
    const after = await p.evaluate('globalThis.__probeStops()')
    return { stops: after[kind] - before[kind], mirrors: at.mirrors, disabled: at.disabled, open: await p.evaluate("!!document.querySelector('.probe-modal')"),
      squeezed: await p.evaluate("[...document.querySelectorAll(\"[data-squeeze='on']\")].some((x) => x.closest('[hidden]') == null)") }
  } finally {
    await p.close()
  }
}
const MIRROR = [
  ['FT parked line, Stop TX', 'operate', [900, 600], '175', 'en', 'plain', 'main.operate-cockpit .cockpit-qso .op-btn.stop', 'halts'],
  ['SSTV squeezed bar, Stop TX', 'sstv', null, '175', null, 'keyedAlarm', 'main.sstv-view .cockpit-txstrip .op-btn.stop', 'halts'],
  ['SSTV squeezed bar, its own Stop (sending)', 'sstv', null, '175', null, 'keyedAlarm', 'main.sstv-view .sstv-tx-bar .sstv-tx-stop', 'sstv'],
]
/** Where SSTV's bar really squeezes while sending under an alarm, the first of these: the shell's room depends on the top
 *  bar and the header, which other changes move (a top bar that no longer grows for a status message squeezes far less),
 *  so the case is searched for, and asserted squeezed, never assumed. */
const SQUEEZE_AT = [[[900, 600], 'ru'], [[900, 600], 'ja'], [[800, 520], 'ru'], [[700, 500], 'ja'], [[640, 480], 'ru']]
async function squeezedSstv() {
  for (const [size, loc] of SQUEEZE_AT) {
    const p = await openPage('sstv', size, '175', loc)
    try {
      let m = null
      for (let i = 0; i < 160 && !m?.stops.length; i++) { await sleep(250); m = await p.evaluate(MEASURE).catch(() => null) }
      await settle(p, await apply(p, STATES.keyedAlarm, 'sstv'))
      if (await p.evaluate("!!document.querySelector(\"main.sstv-view .sstv-tx-bar[data-squeeze='on']\")")) return [size, loc]
    } finally {
      await p.close()
    }
  }
  return null
}
const mirrorReds = []
let mirrorControl = null
if (process.env.STOPVIS_MIRROR !== '0' && wantViews.includes('FT')) {
  const sqAt = await squeezedSstv()
  if (!sqAt) mirrorReds.push(`stop layer — SSTV: no size of ${SQUEEZE_AT.map(([[w, h], l]) => `${w}×${h} ${l}`).join(', ')} at 175 % squeezes the bar while sending under an alarm: the squeezed-bar cases cannot run`)
  else console.log(`stop layer — SSTV's bar squeezes at ${sqAt[0].join('×')} 175 % ${sqAt[1]}`)
  for (const [what, route, size, scale, loc, ...rest] of MIRROR) {
    const sstv = route === 'sstv'
    if (sstv && !sqAt) continue
    const r = await mirrorCase(route, sstv ? sqAt[0] : size, scale, sstv ? sqAt[1] : loc, ...rest)
    console.log(`stop layer — ${what}, a modal open: ${JSON.stringify(r)}`)
    if (r.error || r.stops !== 1 || r.disabled || !r.open || (sstv && !r.squeezed)) mirrorReds.push(`stop layer — ${what}: with a modal open, one real click stopped ${r.stops ?? 0} times (${r.error ?? (r.disabled ? 'the stop was disabled' : !r.open ? 'the modal closed' : sstv && !r.squeezed ? 'the bar was not squeezed' : 'not once')})`)
  }
  const c = await mirrorCase(...MIRROR[0].slice(1), '.stop-layer { display: none !important }')
  mirrorControl = c.stops === 0
  console.log(`CONTROL — FT parked line with the stop layer taken away: the click stopped ${c.stops} times${mirrorControl ? ' (caught)' : ''}`)
  for (const r of mirrorReds) console.log('RED ' + r)
}

// THE PROMPT TO LOG BESIDE THE STOPS (2026-11-13). The prompt to log (LogConfirm) is a card over the whole window, capped
// to the window's height. At 1366×768 at 175 % (the floor, a 780×438 window) that card stood over every cockpit's stop
// controls, so their stop-layer mirrors were drawn over the card's own boxes: a click meant for the Comment box was a Stop TX.
// On every cockpit, at the floor (the app's fit and 100 %) and 1366×768 at the app's 175 %, with a contact waiting to be
// logged: no stop control (`[data-stop-control]`, other than the card's own) may meet the card, the card is wholly inside the
// window, and Stop TX must be on screen and reachable — the topmost thing at its centre is Stop TX or its mirror. Whether Log
// shows without a scroll is printed. THE CONTROL: the card centred over the whole window again must meet one.
const PENDING = { call: 'IA5X', grid: 'JN52', band: '20m', mode: 'FT8', rstSent: '-12', rstRcvd: '-09', comment: null, qsoDate: '2026-11-13', timeOn: '14:22:33' }
const LOGCARD = `(() => {
  const card = document.querySelector('.logconfirm'); if (!card) return null
  const c = card.getBoundingClientRect()
  const stops = [...document.querySelectorAll('[data-stop-control]:not([data-stop-mirror])')].filter((b) => !card.contains(b) && b.closest('[hidden]') == null && b.getBoundingClientRect().width > 0)
  const rows = stops.map((b) => { const q = b.getBoundingClientRect(); const x = q.left + q.width / 2, y = q.top + q.height / 2
    const meets = q.left < c.right - 0.5 && q.right > c.left + 0.5 && q.top < c.bottom - 0.5 && q.bottom > c.top + 0.5
    const inside = q.left >= -0.5 && q.top >= -0.5 && q.right <= innerWidth + 0.5 && q.bottom <= innerHeight + 0.5
    const t = inside ? document.elementFromPoint(x, y) : null
    const m = t?.closest?.('[data-stop-mirror]')
    const reach = !!t && (t === b || b.contains(t) || (!!m && Math.abs(m.getBoundingClientRect().left - q.left) < 1 && Math.abs(m.getBoundingClientRect().top - q.top) < 1))
    return { kind: b.getAttribute('data-stop-control'), box: [q.left, q.top, q.width, q.height].map(Math.round), meets, inside, reach } })
  const lb = card.querySelector('.logconfirm-log'), log = lb?.getBoundingClientRect()
  // Log as the prompt opens, before any scroll: inside the card's visible box and the window, and topmost at its centre.
  const logSeen = !!log && log.top >= c.top - 0.5 && log.bottom <= c.bottom + 0.5 && log.bottom <= innerHeight + 0.5 && (() => {
    const t = document.elementFromPoint(log.left + log.width / 2, log.top + log.height / 2); return !!t && (t === lb || lb.contains(t)) })()
  const inWindow = c.left >= -0.5 && c.top >= -0.5 && c.right <= innerWidth + 0.5 && c.bottom <= innerHeight + 0.5
  return { card: [c.left, c.top, c.width, c.height].map(Math.round), rows, log: log ? [Math.round(log.top), Math.round(log.bottom)] : null, logSeen, inWindow }
})()`
async function logCard(route, size, scale, css = null) {
  const p = await openPage(route, size, scale, 'en')
  try {
    let m = null
    for (let i = 0; i < 160 && !m?.stops.length; i++) { await sleep(250); m = await p.evaluate(MEASURE).catch(() => null) }
    await settle(p, await apply(p, PLAIN, route))
    if (css) await p.evaluate(`(() => { const s = document.createElement('style'); s.textContent = ${JSON.stringify(css)}; document.head.appendChild(s); return 1 })()`)
    await p.evaluate(`globalThis.__push(null, { pendingLog: ${JSON.stringify(PENDING)} })`)
    let r = null
    for (let i = 0; i < 40 && !r; i++) { await frames(p); r = await p.evaluate(LOGCARD).catch(() => null) }
    await settle(p)
    return r ? await p.evaluate(LOGCARD) : null
  } finally {
    await p.close()
  }
}
const logReds = []
let logControl = null
if (process.env.STOPVIS_LOG !== '0') {
  const LOG_AT = [['auto', px(FLOOR)], ['100', px(FLOOR)], ['175', px(LAPTOP)]]
  for (const [label, route] of VIEWS.filter(([l]) => wantViews.includes(l))) for (const [scale, size] of LOG_AT) {
    const r = await logCard(route, size, scale)
    const at = `${label} ${lab(size)} ${scale}`
    if (!r) { logReds.push(`prompt to log — ${at}: the prompt never drew`); console.log('RED ' + logReds.at(-1)); continue }
    const meets = r.rows.filter((x) => x.meets), tx = r.rows.filter((x) => x.kind === 'stop-tx')
    console.log(`prompt to log — ${at}: card ${r.card.join(',')}${r.inWindow ? '' : ' (NOT inside the window)'}, Log ${r.log?.join('–')}${r.logSeen ? ' seen' : ' NOT seen without a scroll'}; stops ${r.rows.map((x) => `${x.kind}@${x.box.join(',')}${x.meets ? ' MEETS' : ''}${x.reach ? '' : ' unreachable'}`).join(' ')}`)
    if (meets.length) logReds.push(`prompt to log — ${at}: the card ${r.card.join(',')} meets ${meets.map((x) => `${x.kind} at ${x.box.join(',')}`).join(', ')}`)
    if (!tx.some((x) => x.inside && x.reach)) logReds.push(`prompt to log — ${at}: no Stop TX on screen and reachable with the prompt open`)
    if (!r.inWindow) logReds.push(`prompt to log — ${at}: the card ${r.card.join(',')} is not wholly inside the window`)
    if (meets.length || !tx.some((x) => x.inside && x.reach) || !r.inWindow) console.log('RED ' + logReds.at(-1))
  }
  // The control: the card's placement taken away, so it is centred over the whole window as before (base's layout).
  // The room is the backdrop's padding and the card's own width and height (LogConfirm), so all three are put back.
  const c = await logCard('operate', px(FLOOR), '100', '.logconfirm-backdrop { padding: 0 !important } .logconfirm { width: min(420px, calc(0.92 * var(--vw-eff))) !important; max-height: calc(0.9 * var(--vh-eff)) !important }')
  logControl = !!c && c.rows.some((x) => x.meets)
  console.log(`CONTROL — FT at the floor (100 %), the prompt centred over the whole window: ${c ? (logControl ? 'it meets a stop (caught)' : 'it meets none') : 'never drew'}`)
}

// THE PROMPT TO LOG WHEN A STOP MOVES WHILE IT IS OPEN (2026-11-13). The prompt measured its room only as it opened, so a
// cockpit that went on the air or under an alarm with the prompt open moved its stops and the card stayed where it was.
// Each case: the prompt opened with the cockpit plain, the Comment box focused and the card scrolled; then the cockpit
// is put in the state. Asserted: the card meets no stop control, it is inside the window, the Comment box still has the
// focus, and the card's scroll is where it was (or at its end, if the card got shorter). THE CONTROL, on the first
// case: the card held where it opened (its room pinned) must meet a stop after the same change.
const LOG_MOVES = [
  // [view, route, scale, size, state] — where that change moved a stop into the room the card opened in (measured on
  // the tree before the fix: Stop TX on the strip, and SSTV's own Stop as it starts to send).
  ['Phone', 'phone', '175', [900, 600], 'keyed'],
  ['CW', 'cw', '175', [900, 600], 'keyedAlarm'],
  ['JS8', 'js8', '175', [900, 600], 'keyed'],
  ['SSTV', 'sstv', '100', px(FLOOR_150), 'keyed'],
  ['SSTV', 'sstv', '175', [700, 800], 'worst'],
]
async function logMove(route, scale, size, state, pin) {
  const p = await openPage(route, size, scale, 'en')
  try {
    let m = null
    for (let i = 0; i < 160 && !m?.stops.length; i++) { await sleep(250); m = await p.evaluate(MEASURE).catch(() => null) }
    await settle(p, await apply(p, PLAIN, route))
    await p.evaluate(`globalThis.__push(null, { pendingLog: ${JSON.stringify(PENDING)} })`)
    let r = null
    for (let i = 0; i < 40 && !r; i++) { await frames(p); r = await p.evaluate(LOGCARD).catch(() => null) }
    await settle(p)
    const before = await p.evaluate(`(() => {
      const b = document.querySelector('.logconfirm-backdrop'), card = b.querySelector('.logconfirm')
      const box = [...card.querySelectorAll('.logconfirm-comment input')][0]
      box.focus({ preventScroll: true })
      card.scrollTop = Math.min(40, card.scrollHeight - card.clientHeight)
      if (${pin}) { const s = document.createElement('style'); s.textContent = '.logconfirm-backdrop { padding: ' + getComputedStyle(b).padding + ' !important } .logconfirm { width: ' + getComputedStyle(card).width + ' !important; max-height: ' + getComputedStyle(card).maxHeight + ' !important }'; document.head.appendChild(s) }
      return { scroll: card.scrollTop }
    })()`)
    const meetsAtOpen = (await p.evaluate(LOGCARD)).rows.filter((x) => x.meets).length
    await settle(p, await apply(p, STATES[state], route))
    await settle(p, 300)
    const after = await p.evaluate(LOGCARD)
    const kept = await p.evaluate(`(() => {
      const card = document.querySelector('.logconfirm'), e = document.activeElement
      return { focus: !!e && e.matches('.logconfirm-comment input'), scroll: card.scrollTop, end: card.scrollHeight - card.clientHeight }
    })()`)
    return { meetsAtOpen, after, before, kept }
  } finally {
    await p.close()
  }
}
const moveReds = []
let moveControl = null
if (process.env.STOPVIS_LOG !== '0') {
  for (const [label, route, scale, size, state] of LOG_MOVES.filter(([l]) => wantViews.includes(l))) {
    const at = `${label} ${lab(size)} ${scale} plain→${state}`
    const r = await logMove(route, scale, size, state, false)
    const meets = r.after?.rows.filter((x) => x.meets) ?? []
    const scrollOk = Math.abs(r.kept.scroll - Math.min(r.before.scroll, r.kept.end)) <= 1
    console.log(`prompt to log, a stop moves — ${at}: met ${r.meetsAtOpen} at open; card ${r.after?.card.join(',')} meets ${meets.map((x) => `${x.kind}@${x.box.join(',')}`).join(' ') || 'none'}; focus ${r.kept.focus ? 'kept' : 'LOST'}; scroll ${r.before.scroll}→${r.kept.scroll}`)
    const why = [r.meetsAtOpen ? `the card met a stop already at open` : null,
      meets.length ? `the card ${r.after.card.join(',')} meets ${meets.map((x) => `${x.kind} at ${x.box.join(',')}`).join(', ')}` : null,
      r.after?.inWindow ? null : 'the card is not wholly inside the window', r.kept.focus ? null : 'the Comment box lost the focus',
      scrollOk ? null : `the card's scroll went ${r.before.scroll}→${r.kept.scroll}`].filter(Boolean)
    if (why.length) { moveReds.push(`prompt to log, a stop moves — ${at}: ${why.join('; ')}`); console.log('RED ' + moveReds.at(-1)) }
  }
  const [label, route, scale, size, state] = LOG_MOVES[0]
  const c = await logMove(route, scale, size, state, true)
  moveControl = !!c.after?.rows.some((x) => x.meets)
  console.log(`CONTROL — ${label} ${lab(size)} ${scale} plain→${state}, the card held where it opened: ${moveControl ? 'it meets a stop (caught)' : 'it meets none'}`)
}

await browser.stop?.()
await server.close()
await rm(cacheDir, { recursive: true, force: true })

if (nudgeRows.length) console.log(`note: the first-run nudge is drawn in ${nudgeRows.filter((r) => r.nudge).length} of ${nudgeRows.length} first-run rows` +
  ` (drawn on: ${[...new Set(nudgeRows.filter((r) => r.nudge).map((r) => r.view))].join(', ') || 'none'})`)
console.log(`\n${rows.length} rows (${VIEWS.filter(([l]) => wantViews.includes(l)).length} screens × ${Object.keys(STATES).length} states), ${Math.round((Date.now() - t0) / 1000)} s`)
{
  const withCell = rows.filter((r) => r.alarmSeen !== null)
  if (withCell.length) {
    const hidden = withCell.filter((r) => !r.alarmSeen)
    console.log(`note: the strip's alarm cell is on screen in ${withCell.length - hidden.length} of ${withCell.length} rows that show one` +
      (hidden.length ? `; not on screen: ${[...new Set(hidden.map((r) => `${r.view} ${r.W}×${r.H} ${r.scale} ${r.loc}`))].slice(0, 12).join(', ')}` : ''))
  }
}
if (control !== true) console.log(`CONTROL FAILED: a box planted over FT's Stop TX at 900×600 / 175 % was ${control === null ? 'never measured' : 'not caught'}`)
if (errors) { console.log(`stopvisible: ${errors} screens never drew`); process.exit(2) }
{
  const all = rows.filter((r) => r.squeezed)
  if (all.length) {
    const sliver = all.filter((r) => !r.squeezed.ctlWhole)
    console.log(`note: a squeezed bottom surface shows at least one whole control in ${all.length - sliver.length} of ${all.length} rows` +
      (sliver.length ? `; a sliver only: ${sliver.slice(0, 12).map((r) => `${r.view} ${r.W}×${r.H} ${r.scale} ${r.loc} ${r.state} (cap ${r.squeezed.cap} px)`).join('; ')}` : ''))
  }
  // The one-control cap (CockpitTxStrip capFor): the surface stands over the strip's lower part. Stop TX in those rows is
  // held by the matrix's own check; they are named here so a size that starts to use it is seen.
  const over = all.filter((r) => r.squeezed.overStrip)
  console.log(`note: the one-control cap (a squeezed surface standing over the strip below Stop TX's line) in ${over.length} rows` +
    (over.length ? `: ${over.slice(0, 12).map((r) => `${r.view} ${r.W}×${r.H} ${r.scale} ${r.loc} ${r.state}`).join('; ')}` : ''))
  const sq = rows.filter((r) => r.squeezed?.stop)
  if (sq.length) {
    const hidden = sq.filter((r) => !r.squeezed.stop.seen)
    console.log(`note: a squeezed bottom surface shows its own stop in ${sq.length - hidden.length} of ${sq.length} rows (live: ${sq.filter((r) => r.squeezed.stop.live && r.squeezed.stop.seen).length} of ${sq.filter((r) => r.squeezed.stop.live).length})` +
      (hidden.length ? `; in its scroll: ${hidden.slice(0, 16).map((r) => `${r.view} ${r.W}×${r.H} ${r.scale} ${r.loc} ${r.state} (cap ${r.squeezed.cap} px, stop ${r.squeezed.stop.h} px${r.squeezed.stop.live ? ', live' : ''})`).join('; ')}` : ''))
  }
}
if (tabControl !== true) console.log('CONTROL FAILED: with the scroll padding taken away, no Tab stop landed behind FT’s parked line: the Tab check cannot fail')
if (mirrorControl === false) console.log('CONTROL FAILED: with the stop layer taken away the click still stopped: the stop layer check cannot fail')
if (freshControl === false) console.log('CONTROL FAILED: a stale divider value was not caught: the fresh-layout guard cannot fail')
if (freshReds.length) console.log(`stopvisible: RED — ${freshReds.length} rows where Stop TX stood off a fresh layout`)
if (logControl === false) console.log('CONTROL FAILED: the prompt to log over the whole window met no stop: the prompt check cannot fail')
if (moveControl === false) console.log('CONTROL FAILED: the prompt to log held where it opened met no stop after the change: the moving-stop check cannot fail')
if (moveReds.length) console.log(`stopvisible: RED — ${moveReds.length} rows where the prompt to log met a stop that moved while it was open:\n  ${moveReds.join('\n  ')}`)
if (logReds.length) console.log(`stopvisible: RED — ${logReds.length} rows where the prompt to log met a stop control or hid Stop TX:\n  ${logReds.join('\n  ')}`)
if (reds.length || freshReds.length || mirrorReds.length || logReds.length || moveReds.length || logControl === false || moveControl === false || mirrorControl === false || freshControl === false || control !== true || tabControl !== true) { console.log(`stopvisible: RED — ${reds.length} rows with no Stop TX on screen and topmost`); process.exit(1) }
console.log('stopvisible: PASS — a Stop TX fully on screen and topmost in every row, no Tab stop behind FT’s parked line, FT’s Stop TX where a fresh layout puts it after a header change, one click through the stop layer stops once over a parked strip and a squeezed bar, the prompt to log meets no stop control and leaves Stop TX reachable, also after a stop moves while it is open; every control was caught')
process.exit(0)
