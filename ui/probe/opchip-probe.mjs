// THE TOP BAR'S OPERATOR CHIP, in the real app in real Chrome: `scripts/browser-probe --desktop opchip`.
//
// Turning contest mode on puts the top bar's operator chip up ("Set operator", or "OP <call>" once one is set). On FT,
// Field Day, the Logbook and CW at the floor (1366×768 at the OS's 175 %), at 100 % and the app's fit, in English, French
// and Russian, contest mode off vs on with nobody set and with an operator set, this asserts:
//   1. EVERY STOP TX STANDS WHERE IT STOOD (the TX strip's, and the bar's own on the Logbook and Field Day);
//   2. THE CHIP IS READABLE: with an operator set, the call is whole on screen; with none, at least half of its words show.
// WHY. The chip took its whole text width in the bar's line from inside the Help group, and in French and Russian at the
// floor's fit that wrapped FT's Tx-cycle group onto a second line: the bar grew a row and Stop TX moved down 31 px (fr) and
// 33 px (ru). Taken out of the line beside Help, it was cut to a sliver on the Logbook and Field Day ("S." in English at
// the fit, the call hidden in Russian at 100 %), whose Help line is full while the bar's free room is on the line above.
// THE CONTROLS, run every time: the chip's old sizing put back (its own text width in the line) must move FT's Stop TX in
// French at the floor's fit, and the chip held to 2em must read as cut on Field Day in English at the fit. If one is not
// caught, its check cannot fail and the probe says so instead of passing.
// OPCHIP_LOCS ("en,de,…") names the languages; every one is the full sweep (about 108 rows).
//
// exit: 0 every check held · 1 a red, or a control that could not fail · 2 a screen never drew
import { createServer } from 'vite'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'
import { FLOOR } from './sizes.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const REAL_API = join(UI, 'src', 'api.ts')
const STUB_API = join(UI, 'probe', '__opchip-api.ts')
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const LOCS = (process.env.OPCHIP_LOCS || 'en,fr,ru').split(',').map((s) => s.trim())
const px = (s) => [s.w, s.h, s.scale]
const lab = ([W, H, D = 1]) => `${W}×${H}${D === 1 ? '' : ` (OS ${Math.round(D * 100)} %)`}`

// THE API SWAP — the waterfalls probe's stub, with the settings App reads at start taken from `probe.settings` in
// localStorage over the fixture: contest mode is turned on through it (settings.fdActive), as the operator's own switch does.
let stub = await readFile(join(UI, 'probe', 'waterfalls-api.ts'), 'utf8')
const SETTINGS_STUB = 'export async function getSettings(): Promise<any> { return settingsFixture }'
if (!stub.includes(SETTINGS_STUB)) throw new Error('opchip: the stub seam getSettings did not apply; probe/waterfalls-api.ts changed shape')
stub = stub.replace(SETTINGS_STUB, 'export async function getSettings(): Promise<any> { return { ...settingsFixture, ...JSON.parse(localStorage.getItem("probe.settings") ?? "{}") } }')
// APRS's answers, as stopvisible appends them (the rail's screens mount their reads), and the partner lookups an FT screen makes.
stub += '\n' + (await readFile(join(UI, 'probe', 'aprsmap-api.ts'), 'utf8'))
stub += "\nexport async function resolveEntity(): Promise<any> { return 'United States' }\nexport async function qrzLookup(): Promise<any> { return null }\n"
const realSrc = await readFile(REAL_API, 'utf8')
const names = [...realSrc.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1])
const own = new Set([...stub.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1]))
const stubCode = stub + `\nexport * from ${JSON.stringify(REAL_API)}\n` +
  names.filter((n) => !own.has(n)).map((n) => `export async function ${n}(..._a: unknown[]): Promise<any> { return (globalThis as any).__DEFAULT_ARR ? [] : {} }`).join('\n')
const swap = {
  name: 'opchip-probe-api', enforce: 'pre',
  async resolveId(source, importer, opts) {
    if (opts?.scan) return null
    if (source === STUB_API) return STUB_API
    if (!importer || importer === STUB_API || !/api(\.ts)?$/.test(source)) return null
    const r = await this.resolve(source, importer, { ...opts, skipSelf: true })
    return r && r.id.split('?')[0] === REAL_API ? STUB_API : null
  },
  load(id) { return id === STUB_API ? stubCode : null },
}

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-opchip-probe-'))
const server = await createServer({ root: UI, configFile: join(UI, 'vite.config.ts'), cacheDir, logLevel: 'warn', plugins: [swap],
  server: { host: '127.0.0.1', port: 0, strictPort: false, hmr: false } })
await server.listen()
const origin = `http://127.0.0.1:${server.httpServer.address().port}`
const browser = await chrome()
const FEATURES = JSON.stringify({ profile: 'custom', enabled: { rtty: true, psk: true, sstv: true, js8: true, jtty: true, aprs: true, chat: true } })

async function openPage(route, [W, H, D = 1], scale, loc, settings) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (m, p) => browser.call(m, p, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text)
    return r.result.value
  }
  await call('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: D, mobile: false })
  await call('Page.navigate', { url: `${origin}/probe-blank` })
  await sleep(300)
  const ls = { 'nexus.workspace': 'dx', 'nexus.features.v1': FEATURES, 'tempo-theme': 'dark', 'nexus.locale': loc,
    ...(scale !== 'auto' ? { 'nexus-ui-scale-mode': scale } : {}), 'probe.settings': JSON.stringify(settings) }
  await evaluate(`localStorage.clear(); ${Object.entries(ls).map(([k, v]) => `localStorage.setItem(${JSON.stringify(k)}, ${JSON.stringify(v)})`).join(';')}; 1`)
  await call('Page.navigate', { url: `${origin}/#${route}` })
  return { evaluate, close: () => browser.call('Target.closeTarget', { targetId }) }
}
const frames = (p) => p.evaluate('new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(() => r(1), 30))))')
/** Read until two reads of every Stop TX, the bar and the chip agree. */
async function settle(p, minMs = 0) {
  if (minMs) await sleep(minMs)
  let last = null
  for (let i = 0; i < 25; i++) {
    await frames(p)
    const k = JSON.stringify(await p.evaluate(stopBox).catch(() => null))
    if (k === last) break
    last = k
  }
}

// [label, route, scales]. The Logbook draws Field Day's bar, so it runs at 100 % only; CW (no digital chrome, the chip
// at the bar's end before this) at the fit only.
const OPCHIP_VIEWS = [['FT', 'operate', ['100', 'auto']], ['Field Day', 'fieldDay', ['100', 'auto']], ['Logbook', 'logbook', ['100']], ['CW', 'cw', ['auto']]]
const OPCHIP_CALL = 'N0CALL/P'
const OPCHIP_OLD = '.topbar-opchip{flex:none!important;min-width:auto!important;max-width:none!important}'
const OPCHIP_NARROW = '.topbar-opchip{min-width:0!important;max-width:2em!important}'
const stopBox = `(() => { const t = document.querySelector('.topbar'); if (!t) return null
  const stops = [...document.querySelectorAll('.op-btn.stop:not([data-stop-mirror])')].filter((b) => b.closest('[hidden]') == null && b.getBoundingClientRect().width > 0)
    .map((b) => { const q = b.getBoundingClientRect(); return (b.closest('.topbar') ? 'bar' : 'strip') + '@' + Math.round(q.left * 10) / 10 + ',' + Math.round(q.top * 10) / 10 })
  const c = [...document.querySelectorAll('.topbar .op-chip')].find((e) => e.getBoundingClientRect().width > 0)
  // How much of the chip's words (or its call) is on screen: the text's own extent, against every clipping box from its
  // node up to the chip. Class-free, so it reads any version of the chip.
  const seen = (want) => { if (!c) return null; const w = document.createTreeWalker(c, NodeFilter.SHOW_TEXT); let n
    while ((n = w.nextNode())) { const k = n.textContent.indexOf(want); if (k < 0) continue
      const r = document.createRange(); r.setStart(n, k); r.setEnd(n, k + want.length); const q = r.getBoundingClientRect()
      let right = innerWidth, left = 0; for (let e = n.parentElement; e; e = e.parentElement) { const s = getComputedStyle(e); if (s.overflowX !== 'visible') { const b = e.getBoundingClientRect(); right = Math.min(right, b.right); left = Math.max(left, b.left) } if (e === c) break }
      return Math.round(Math.max(0, Math.min(q.right, right) - Math.max(q.left, left)) / q.width * 100) / 100 }
    return null }
  return { stops, bar: Math.round(t.getBoundingClientRect().height * 10) / 10, chip: c ? c.textContent : null,
    w: c ? Math.round(c.getBoundingClientRect().width * 10) / 10 : null, call: seen(${JSON.stringify(OPCHIP_CALL)}), words: c ? seen(c.textContent.trim()) ?? seen(c.lastElementChild?.textContent ?? '') : null } })()`
/** Contest mode off; on with nobody at the key yet ("Set operator"); on with an operator set ("OP <call>"). */
const OPCHIP_STATES = { off: { fdActive: false, fdOperator: '' }, set: { fdActive: true, fdOperator: '' }, op: { fdActive: true, fdOperator: OPCHIP_CALL } }
async function opchipCase(route, size, scale, loc, state, css = null) {
  const on = state !== 'off'
  const p = await openPage(route, size, scale, loc, OPCHIP_STATES[state])
  try {
    let m = null
    for (let i = 0; i < 160 && !m?.stops.length; i++) { await sleep(250); m = await p.evaluate(stopBox).catch(() => null) }
    if (css) await p.evaluate(`(() => { const s = document.createElement('style'); s.textContent = ${JSON.stringify(css)}; document.head.appendChild(s); return 1 })()`)
    // The settings arrive after the first paint: wait for the chip to come (on) or for the layout to hold (off).
    for (let i = 0; i < 40; i++) { const b = await p.evaluate(stopBox).catch(() => null); if (b && (!on || b.chip)) break; await sleep(250) }
    await settle(p, 500)
    return await p.evaluate(stopBox).catch(() => null)
  } finally {
    await p.close()
  }
}
const opchipReds = []
let opchipControl = null, opchipReadControl = null
{
  let n = 0
  const size = px(FLOOR)
  for (const [label, route, scales] of OPCHIP_VIEWS) for (const scale of scales) for (const loc of LOCS) {
    const off = await opchipCase(route, size, scale, loc, 'off')
    for (const state of ['set', 'op']) {
      n++
      const on = await opchipCase(route, size, scale, loc, state)
      const at = `${label} ${lab(size)} ${scale} ${loc} ${state}`
      console.log(`operator chip: ${at}: off ${JSON.stringify(off)} on ${JSON.stringify(on)}`)
      let red = null
      if (!off?.stops.length || !on?.stops.length || !on.chip) red = `${at}: never measured, or the chip never came (off ${JSON.stringify(off)}, on ${JSON.stringify(on)})`
      else if (JSON.stringify(on.stops) !== JSON.stringify(off.stops)) red = `${at}: Stop TX moved ${JSON.stringify(off.stops)} → ${JSON.stringify(on.stops)} when contest mode put the operator chip up (bar ${off.bar} → ${on.bar} px)`
      else if (state === 'op' && !(on.call >= 0.99)) red = `${at}: the operator's call is not whole on the chip (${Math.round((on.call ?? 0) * 100)} % of it shows, the chip ${on.w} px)`
      else if (state === 'set' && !(on.words >= 0.5)) red = `${at}: under half of the chip's words show (${Math.round((on.words ?? 0) * 100)} %, the chip ${on.w} px)`
      if (red) { opchipReds.push(red); console.log('RED ' + red) }
    }
  }
  console.log(`operator-chip guard: ${n} rows, ${opchipReds.length} red`)
  {
    const off = await opchipCase('operate', px(FLOOR), 'auto', 'fr', 'off', OPCHIP_OLD)
    const on = await opchipCase('operate', px(FLOOR), 'auto', 'fr', 'set', OPCHIP_OLD)
    opchipControl = off?.stops.length > 0 && on?.stops.length > 0 && JSON.stringify(on.stops) !== JSON.stringify(off.stops)
    console.log(`CONTROL — FT ${lab(px(FLOOR))} fit fr with the chip's old sizing: Stop TX ${JSON.stringify(off?.stops)} → ${JSON.stringify(on?.stops)}${opchipControl ? ' (caught)' : ''}`)
  }
  {
    const on = await opchipCase('fieldDay', px(FLOOR), 'auto', 'en', 'set', OPCHIP_NARROW)
    opchipReadControl = on?.chip != null && !(on.words >= 0.5)
    console.log(`CONTROL — Field Day ${lab(px(FLOOR))} fit en with the chip held to 2em: ${Math.round((on?.words ?? 0) * 100)} % of its words show${opchipReadControl ? ' (caught)' : ''}`)
  }
}


let code = 0
if (opchipReds.length) { console.log(`opchip: RED — ${opchipReds.length} rows where contest mode's operator chip moved Stop TX or could not be read:\n  ${opchipReds.join('\n  ')}`); code = 1 }
if (opchipControl !== true) { console.log('opchip: CONTROL FAILED — the operator chip at its old sizing did not move Stop TX: the Stop TX check cannot fail'); code = 1 }
if (opchipReadControl !== true) { console.log('opchip: CONTROL FAILED — the operator chip held to 2em read as readable: the readability check cannot fail'); code = 1 }
if (code === 0) console.log(`opchip: PASS — contest mode's operator chip leaves every Stop TX where it stood on FT, Field Day, the Logbook and CW at the floor, at 100 % and the fit, in ${LOCS.join(', ')}, with the call whole and at least half of the words shown; both controls were caught`)
await browser.stop()
await server.close()
await rm(cacheDir, { recursive: true, force: true })
process.exit(code)
