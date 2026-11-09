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
//
// HOW "ON SCREEN AND TOPMOST" IS READ: the button's whole box is inside the window, and at each point of a 5 × 3
// grid across its face (inset past its round corners) the topmost element is the button. While it reads, every
// element takes pointer events, so a notice that lets clicks through still counts as covering what it hides.
//
// AND THE TAB WALK (FT, plain, 150/175 % up to 1366×768): real Tab through the shell; no focused control may have its
// centre behind the QSO strip's parked line (it covered 36 Tab stops at 900×600 / 175 % before the shell's scroll
// padding read the parked line's height).
//
// AND THE HEADER-CHANGE GUARD (FT at 100 % and fit, 1024×768 and 1366×768, en/ru, CAT lost and the alarms): Stop TX stands
// where a fresh layout (the window resized away and back) puts it, so the waterfall's divider re-fit to the header.
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

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const REAL_API = join(UI, 'src', 'api.ts')
const STUB_SRC = join(UI, 'probe', 'waterfalls-api.ts')
const STUB_API = join(UI, 'probe', '__stopvisible-api.ts')
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

/** [label, route]. Tempo is the `chat` view. */
const VIEWS = [['FT', 'operate'], ['Tempo', 'chat'], ['Phone', 'phone'], ['CW', 'cw'], ['RTTY', 'rtty'], ['PSK', 'psk'],
  ['JTTY', 'jtty'], ['SSTV', 'sstv'], ['APRS', 'aprs'], ['JS8', 'js8']]
const ALL_LOCS = ['en', 'de', 'es', 'fr', 'ja', 'ru', 'zh', 'zh-hant']
const SMALL = [[700, 800], [900, 600], [1024, 768], [1280, 720], [1366, 768]]
const LARGE = [[1600, 900], [1920, 1080], [2560, 1440], [3440, 1440]]
/** [scale ('auto' = the app's own fit), sizes, languages]: one page per view × scale × language, resized through its sizes. */
const CI = [
  ['175', [[700, 800], [900, 600], [1024, 768], [1366, 768]], ['en', 'ru']],
  ['150', [[900, 600], [1366, 768]], ['en']],
  ['100', [[1024, 768], [3440, 1440]], ['en']],
  ['auto', [[1200, 750]], ['en']],
]
const FULL = [
  ['175', [...SMALL, ...LARGE], ALL_LOCS], ['150', [...SMALL, ...LARGE], ALL_LOCS], ['125', [...SMALL, ...LARGE], ALL_LOCS],
  ['100', [...SMALL, ...LARGE], ALL_LOCS], ['auto', [...SMALL, ...LARGE], ALL_LOCS],
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
const PLAIN = { radio: { catOk: true, catDetail: '14.0740 MHz', transmitting: false, txEnabled: false, txWatchdog: false }, top: { txAlarms: [], qso: null }, lane: null, mode: null }
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
}

/** Every Stop TX on the page: where it is, and whether its whole face is on screen and topmost. */
const MEASURE = `(() => {
  if (!document.documentElement.getAttribute('data-viewport')) return null
  const stops = [...document.querySelectorAll('.op-btn.stop')].filter((b) => b.closest('[hidden]') == null && b.getBoundingClientRect().width > 0)
  if (!stops.length) return { vp: document.documentElement.getAttribute('data-viewport'), stops: [] }
  const lift = document.createElement('style'); lift.textContent = '* { pointer-events: auto !important }'; document.head.appendChild(lift)
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
    const box = sq.getBoundingClientRect(); const first = [...sq.querySelectorAll('button, input, select, textarea')].find((e) => e.offsetHeight > 0)
    const fq = first?.getBoundingClientRect(); const ctlWhole = !!fq && fq.top >= box.top - 0.5 && fq.bottom <= box.bottom + 0.5
    if (!own) return { cap, ctlWhole, stop: null }; const q = own.getBoundingClientRect(); const x = q.left + q.width / 2, y = q.top + q.height / 2
    const t = x >= 0 && y >= 0 && x < innerWidth && y < innerHeight ? document.elementFromPoint(x, y) : null
    return { cap, ctlWhole, stop: { seen: t != null && (t === own || own.contains(t)), live: !own.disabled, h: Math.round(q.height) } } })()
  const span = (sel) => { const e = [...document.querySelectorAll(sel)].find((x) => x.closest('[hidden]') == null && x.getBoundingClientRect().height > 0); if (!e) return null; const q = e.getBoundingClientRect(); return Math.round(q.top) + '–' + Math.round(q.bottom) }
  return { vp: document.documentElement.getAttribute('data-viewport'), stops: rows, headBottom: head ? Math.round(head.getBoundingClientRect().bottom) : null,
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

async function openPage(route, [W, H], scale, loc) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (m, p) => browser.call(m, p, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text)
    return r.result.value
  }
  const size = (w, h) => call('Emulation.setDeviceMetricsOverride', { width: w, height: h, deviceScaleFactor: 1, mobile: false })
  await size(W, H)
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
      const lift = document.createElement('style'); lift.textContent = '* { pointer-events: auto !important }'; document.head.appendChild(lift)
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
        for (const [W, H] of sizes) {
          await p.size(W, H)
          for (const [state, st] of Object.entries(STATES)) {
            const wait = await apply(p, st, route)
            const m = await settle(p, wait || (mode ? 1300 : 0))
            mode = !!wait
            const row = { view: label, scale, loc, W, H, state, vp: m?.vp, stops: m?.stops ?? [], scrolled: m?.scrolled, alarmSeen: m?.alarmSeen ?? null, squeezed: m?.squeezed ?? null }
            rows.push(row)
            const ok = row.stops.some((s) => s.ok)
            if (route === 'operate' && state === 'plain' && scale !== 'auto' && Number(scale) >= 150 && TAB_SIZES(W, H)) {
              const hidden = await tabWalk(p)
              if (hidden.length) {
                reds.push(`${label} ${W}×${H} ${scale} ${loc} plain: Tab focuses ${hidden.length} controls behind the parked QSO-strip line (${hidden.slice(0, 8).join(', ')})`)
                console.log('RED ' + reds[reds.length - 1])
              }
            }
            if (!row.stops.length && !p.told) { p.told = true; console.log(`${label} ${W}×${H} ${scale} ${loc} ${state}: NO STOP TX — page errors: ${JSON.stringify(await p.evaluate('(window.__errs || []).slice(-4)').catch((e) => String(e)))}; page: ${await p.evaluate(`JSON.stringify({ hash: location.hash, mains: [...document.querySelectorAll('main')].map((m) => m.className.slice(0, 40) + (m.closest('[hidden]') ? ' [hidden]' : '') + ' ' + Math.round(m.getBoundingClientRect().height)), stops: [...document.querySelectorAll('.op-btn.stop')].map((b) => Math.round(b.getBoundingClientRect().width) + (b.closest('[hidden]') ? 'H' : '')), text: document.body.innerText.slice(-500) })`).catch((e) => String(e))}`) }
            if (!ok) {
              const why = row.stops.map((s) => `${s.where} Stop TX at y ${s.box[1]}–${s.box[1] + s.box[3]} of ${H}${s.inside ? '' : ' (not inside the window)'}${s.covered ? `, covered at ${s.covered}/15 by ${s.by}` : ''}`).join('; ') || 'no Stop TX drawn'
              reds.push(`${label} ${W}×${H} ${scale} ${loc} [${row.vp}] ${state}: no Stop TX fully on screen and topmost — ${why}; header ends at y ${m?.headBottom}; shell ${m?.boxes?.shell}, strip ${m?.boxes?.strip}, dock ${m?.boxes?.dock}${row.scrolled ? ' (a scroller moved)' : ''}`)
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

// THE HEADER-CHANGE GUARD (FT, 100 % and the app's fit, 1024×768 and 1366×768, en and ru): CAT lost, and the alarms (a
// transmitter alarm, the watchdog and a critical status message), change the cockpit header's height or the window's. The
// waterfall's divider must re-fit to that, as a fresh layout would: Stop TX is measured as the state leaves it, the window
// is then resized away and back (every divider re-fits on that), and Stop TX must stand where it stood. A divider that
// re-fit only on the shell's own resize left the waterfall at its old height and Stop TX 28 px off on CAT lost.
const FRESH = [['100', [1024, 768]], ['100', [1366, 768]], ['auto', [1024, 768]], ['auto', [1366, 768]]]
const stopY = `(() => { const b = document.querySelector('main.operate-cockpit .cockpit-qso .op-btn.stop'); return b ? Math.round(b.getBoundingClientRect().top * 10) / 10 : null })()`
async function freshCheck(p, [W, H], plant = null) {
  await settle(p)
  const before = plant ? await p.evaluate(plant) : await p.evaluate(stopY)
  await p.size(W + 40, H + 40)
  await settle(p)
  await p.size(W, H)
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
          freshReds.push(`FT ${size.join('×')} ${scale} ${loc} ${state}: Stop TX stood ${dy} px from where a fresh layout puts it (the waterfall's divider did not re-fit)`)
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
let freshControl = null
if (wantViews.includes('FT') && process.env.STOPVIS_FRESH !== '0') {
  const p = await openPage('operate', [1024, 768], '100', 'en')
  try {
    let m = null
    for (let i = 0; i < 160 && !m?.stops.length; i++) { await sleep(250); m = await p.evaluate(MEASURE).catch(() => null) }
    await apply(p, PLAIN, 'operate')
    const dy = await freshCheck(p, [1024, 768], `(() => { const body = document.querySelector('main.operate-cockpit .cockpit-body'); const v = parseFloat(body.style.getPropertyValue('--cockpit-wf-h')) || 25
      body.style.setProperty('--cockpit-wf-h', (v / 2) + '%'); return ${stopY} })()`)
    freshControl = dy != null && Math.abs(dy) > 1
    console.log(`CONTROL — FT 1024×768 100 % with the divider's value left stale: Stop TX ${dy} px from a fresh layout${freshControl ? ' (caught)' : ''}`)
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

await browser.stop?.()
await server.close()
await rm(cacheDir, { recursive: true, force: true })

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
  const sq = rows.filter((r) => r.squeezed?.stop)
  if (sq.length) {
    const hidden = sq.filter((r) => !r.squeezed.stop.seen)
    console.log(`note: a squeezed bottom surface shows its own stop in ${sq.length - hidden.length} of ${sq.length} rows (live: ${sq.filter((r) => r.squeezed.stop.live && r.squeezed.stop.seen).length} of ${sq.filter((r) => r.squeezed.stop.live).length})` +
      (hidden.length ? `; in its scroll: ${hidden.slice(0, 16).map((r) => `${r.view} ${r.W}×${r.H} ${r.scale} ${r.loc} ${r.state} (cap ${r.squeezed.cap} px, stop ${r.squeezed.stop.h} px${r.squeezed.stop.live ? ', live' : ''})`).join('; ')}` : ''))
  }
}
if (tabControl !== true) console.log('CONTROL FAILED: with the scroll padding taken away, no Tab stop landed behind FT’s parked line: the Tab check cannot fail')
if (freshControl === false) console.log('CONTROL FAILED: a stale divider value was not caught: the fresh-layout guard cannot fail')
if (freshReds.length) console.log(`stopvisible: RED — ${freshReds.length} rows where Stop TX stood off a fresh layout`)
if (reds.length || freshReds.length || freshControl === false || control !== true || tabControl !== true) { console.log(`stopvisible: RED — ${reds.length} rows with no Stop TX on screen and topmost`); process.exit(1) }
console.log('stopvisible: PASS — a Stop TX fully on screen and topmost in every row, no Tab stop behind FT’s parked line, FT’s Stop TX where a fresh layout puts it after a header change; every control was caught')
process.exit(0)
