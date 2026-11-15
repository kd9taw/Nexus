// The cockpit header's lines in real Chrome, for what jsdom cannot see: `scripts/browser-probe --desktop header`.
//
// The header's actions (⊞ Panels, the amplifier, power, CAT) must never take a line of their own while the
// mode controls' last line has room for them. As a header item placed after a cluster that had already
// taken a whole line, they did exactly that whenever the cluster wrapped: ⊞ Panels and CAT alone at the
// right under a REC alone at the left, a band of empty header above the scope (operator photo, CW on a
// 1080p screen above 100 %). The probe serves src/probe/header.tsx with the app's own Vite config, in a
// CW shell with a CW header's controls, across window widths from the floor's 780 px (1366×768 at 175 %,
// probe/sizes.mjs) to 2560 at 100 %, at the app's own 110-175 % on 1920×1080, and at the OS's scaling on the
// floor, 1080p and 4K (its fractional CSS pixel moves a wrap by a sub-pixel), and asserts at each: if the actions sit on a line below the last mode control,
// the room left on that control's line is less than the actions' width; and nothing runs past the window.
//
// THE CONTROL, run every time: the same controls in the header's previous shape (`&old=on`). There the
// probe must find at least one width where the actions took a line they did not need; if it does not,
// the check cannot fail and the probe says so instead of passing.
//
// AND THE CONTEST SWITCH (the real app, FT): with the contest switch on, FT's header carries a "Contest: …"
// chip. Its first build was an ordinary item of the header's wrapping row, and wherever that row was nearly
// full it broke onto a new line: at 8 of the supported screens Stop TX dropped 21-36 px when the switch went
// on, and no probe saw it, because none turned the switch on. So the real app (its api swapped for the
// waterfalls probe's stub, the station in plain FT8 with the New York QSO Party picked) is drawn with the
// switch off and on at every screen of probe/sizes.mjs MATRIX, in English, German, French and Russian, and at
// each the header must be exactly as tall and Stop TX exactly as far below the header's top: the switch adds
// no header line. (Measured from the header's top because the TOP BAR's "Set operator", which the switch also
// shows, grows the top bar in French and Russian at the floor: a separate matter, printed, not failed.)
// THE CONTROL: the chip's zero-cost slot taken away (its old sizing planted over it) must move the header at one
// screen at least, or the check cannot fail and the probe says so.
//
// exit: 0 every case holds · 1 a red, or a control that could not fail · 2 the page never drew the header
import { createServer } from 'vite'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'
import { FHD_125, FHD_150, FLOOR, MATRIX, UHD_150, UHD_200, metrics } from './sizes.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const CASES = []
for (let w = FLOOR.w; w <= 2560; w += 64) CASES.push({ w, h: 900, scale: 1, name: `${w}×900`, z: 1 })
for (const z of [1.1, 1.25, 1.5, 1.75]) CASES.push({ w: 1920, h: 1080, scale: 1, name: '1920×1080', z })
for (const s of [FLOOR, FHD_125, FHD_150, UHD_150, UHD_200]) CASES.push({ ...s, z: 1 })
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

const MEASURE = `(() => {
  const h = document.querySelector('.cockpit-header')
  const a = h?.querySelector('.ch-actions')
  const ctl = [...(h?.querySelectorAll('.probe-control') ?? [])]
  if (!h || !a || ctl.length === 0 || !document.documentElement.getAttribute('data-viewport')) return null
  const z = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--ui-zoom')) || 1
  const r = (e) => e.getBoundingClientRect()
  const last = r(ctl[ctl.length - 1]), act = r(a), hr = r(h)
  const gap = parseFloat(getComputedStyle(ctl[0].parentElement).columnGap) * z || 0
  return {
    below: act.top >= last.bottom - 1,
    room: Math.round(hr.right - last.right - gap),
    actW: Math.round(act.width),
    headerH: Math.round(hr.height),
    past: Math.max(0, Math.round(Math.max(...[...h.querySelectorAll('*')].map((e) => r(e).right)) - innerWidth)),
  }
})()`

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-header-probe-'))
const server = await createServer({
  root: UI, configFile: join(UI, 'vite.config.ts'), cacheDir, logLevel: 'warn',
  server: { host: '127.0.0.1', port: 0, strictPort: false },
})
await server.listen()
const origin = `http://127.0.0.1:${server.httpServer.address().port}`
const browser = await chrome()

async function measure(size, old) {
  const { z } = size
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true })
    return r.exceptionDetails ? null : r.result.value
  }
  await call('Emulation.setDeviceMetricsOverride', metrics(size))
  await call('Page.navigate', { url: `${origin}/probe/header.html?zoom=${z}${old ? '&old=on' : ''}` })
  let last = null
  for (let i = 0; i < 60; i++) {
    await sleep(120)
    const now = await evaluate(MEASURE)
    if (i > 6 && now && last && JSON.stringify(now) === JSON.stringify(last)) break
    last = now
  }
  await browser.call('Target.closeTarget', { targetId })
  return last
}

const wasted = (m) => m.below && m.room >= m.actW
let code = 0
try {
  let controlCaught = 0
  for (const c of CASES) {
    const at = `${c.name} ${Math.round(c.z * 100)}%`
    const m = await measure(c, false)
    const o = await measure(c, true)
    console.log(`header-probe: ${at}: ${JSON.stringify(m)} · previous shape ${JSON.stringify(o)}`)
    if (!m || !o) {
      console.error(`header-probe: the header was never drawn at ${at}`)
      code = 2
      continue
    }
    if (wasted(o)) controlCaught++
    if (wasted(m)) {
      console.error(`header-probe: RED ${at} — the actions (${m.actW}px) took a line of their own with ${m.room}px free on the line above`)
      code = code || 1
    }
    if (m.past > 0) {
      console.error(`header-probe: RED ${at} — the header runs ${m.past}px past the window`)
      code = code || 1
    }
  }
  console.log(`header-probe: CONTROL — the previous shape wasted a line at ${controlCaught} of ${CASES.length} sizes`)
  if (controlCaught === 0) {
    console.error('header-probe: CONTROL FAILED — the previous shape never wasted a line here; the check cannot fail')
    code = code || 1
  }
  if (code === 0) console.log(`header-probe: PASS — the header's actions never take a line they do not need, ${FLOOR.w} to 2560 wide, 110-175 % on 1920×1080 and the OS's 125-200 %`)
  code = Math.max(code, await contestSwitch())
  if (code === 0) console.log('header-probe: PASS — the contest switch adds no line to FT\'s header at any screen of the matrix, in en, de, fr and ru')
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}

// ── THE CONTEST SWITCH, in the real app ────────────────────────────────────────────────────────────────────
async function contestSwitch() {
  const REAL_API = join(UI, 'src', 'api.ts')
  const STUB_API = join(UI, 'probe', '__header-contest-api.ts')
  let stub = await readFile(join(UI, 'probe', 'waterfalls-api.ts'), 'utf8')
  const SETTINGS = 'export async function getSettings(): Promise<any> { return settingsFixture }'
  if (!stub.includes(SETTINGS) || !stub.includes("mode: 'Normal'")) throw new Error('header-probe: probe/waterfalls-api.ts changed shape')
  // The probe's settings over the fixture (`probe.settings`), and the station in plain QSO mode.
  stub = stub.replace(SETTINGS, 'export async function getSettings(): Promise<any> { return { ...settingsFixture, ...JSON.parse(localStorage.getItem("probe.settings") ?? "{}") } }')
    .replace("mode: 'Normal'", "mode: 'qso'")
  stub += "\nexport async function resolveEntity(): Promise<any> { return 'United States' }\nexport async function qrzLookup(): Promise<any> { return null }\n"
  const names = [...(await readFile(REAL_API, 'utf8')).matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1])
  const own = new Set([...stub.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1]))
  const stubCode = stub + `\nexport * from ${JSON.stringify(REAL_API)}\n` +
    names.filter((n) => !own.has(n)).map((n) => `export async function ${n}(..._a: unknown[]): Promise<any> { return (globalThis as any).__DEFAULT_ARR ? [] : {} }`).join('\n')
  const swap = {
    name: 'header-contest-api', enforce: 'pre',
    async resolveId(source, importer, opts) {
      if (opts?.scan) return null
      if (source === STUB_API) return STUB_API
      if (!importer || importer === STUB_API || !/api(\.ts)?$/.test(source)) return null
      const r = await this.resolve(source, importer, { ...opts, skipSelf: true })
      return r && r.id.split('?')[0] === REAL_API ? STUB_API : null
    },
    load(id) { return id === STUB_API ? stubCode : null },
  }
  const dir = await mkdtemp(join(tmpdir(), 'nexus-header-contest-'))
  const app = await createServer({ root: UI, configFile: join(UI, 'vite.config.ts'), cacheDir: dir, logLevel: 'warn', plugins: [swap],
    server: { host: '127.0.0.1', port: 0, strictPort: false, hmr: false } })
  await app.listen()
  const at = `http://127.0.0.1:${app.httpServer.address().port}`
  // The first build's sizing, planted over the slot: an ordinary item that counts toward the actions' width.
  const NO_CHIP = '.cockpit-contest-slot, .cockpit-contest-chip { display: none !important } .ch-actions { flex-grow: 0 !important }'
  const OLD = '.cockpit-contest-slot { container-type: normal !important; flex: 0 0 auto !important; margin-right: 0 !important } .ch-actions:has(> .cockpit-contest-slot) { flex-grow: 0 !important } .cockpit-contest-chip { display: inline-block !important }'
  const READ = `(() => {
    const vis = (s) => [...document.querySelectorAll(s)].find((e) => e.closest('[hidden]') == null && e.getBoundingClientRect().height > 0)
    const stop = vis('.op-btn.stop'), head = vis('main.operate-cockpit .cockpit-header')
    if (!stop || !head || !document.documentElement.getAttribute('data-viewport')) return null
    const chip = vis('.cockpit-contest-chip'), q = stop.getBoundingClientRect(), h = head.getBoundingClientRect()
    return { stopY: Math.round(q.top), below: Math.round(q.top - h.top), headH: Math.round(h.height), chipW: chip ? Math.round(chip.getBoundingClientRect().width) : 0, slotW: Math.round(document.querySelector('main.operate-cockpit .cockpit-contest-slot')?.getBoundingClientRect().width ?? -1) }
  })()`
  const KEY = `[...document.querySelectorAll('.op-btn.stop, main.operate-cockpit > *, .cockpit-header, .ch-actions')].map((e) => { const q = e.getBoundingClientRect(); return Math.round(q.top) + ':' + Math.round(q.height) + ':' + Math.round(q.width) }).join(',')`
  /** `plant`: CSS laid over the page once it draws. */
  async function sweep(loc, on, plant) {
    const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
    const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
    const call = (m, p) => browser.call(m, p, sessionId)
    const ev = async (expression) => { const r = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true }); return r.exceptionDetails ? null : r.result.value }
    await call('Emulation.setDeviceMetricsOverride', metrics(MATRIX[0]))
    await call('Page.navigate', { url: `${at}/probe-blank` })
    await sleep(300)
    const ls = { 'nexus.workspace': 'dx', 'tempo-theme': 'dark', 'nexus.locale': loc, 'probe.settings': JSON.stringify({ fdActive: on, fdEvent: 'nyqp' }) }
    await ev(`localStorage.clear(); ${Object.entries(ls).map(([k, v]) => `localStorage.setItem(${JSON.stringify(k)}, ${JSON.stringify(v)})`).join(';')}; 1`)
    await call('Page.navigate', { url: `${at}/#operate` })
    const rows = []
    for (let i = 0; i < 160 && !(await ev(READ)); i++) await sleep(250)
    if (plant) await ev(`(() => { const s = document.createElement('style'); s.textContent = ${JSON.stringify(plant)}; document.head.appendChild(s); return 1 })()`)
    for (const size of MATRIX) {
      await call('Emulation.setDeviceMetricsOverride', metrics(size))
      let last = null
      for (let i = 0; i < 40; i++) {
        await ev('new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(() => r(1), 30))))')
        const k = await ev(KEY)
        if (i > 2 && k === last) break
        last = k
      }
      rows.push(await ev(READ))
    }
    await browser.call('Target.closeTarget', { targetId })
    return rows
  }
  let code = 0, caught = 0, hidden = 0
  try {
    for (const loc of ['en', 'de', 'fr', 'ru']) {
      // Off, on, and on with the chip's slot taken out: the switch also shows the TOP BAR's "Set operator",
      // which grows the top bar in French and Russian at the floor, so the chip is judged against the same
      // page without it, and the move from off is printed.
      const off = await sweep(loc, false, null), on = await sweep(loc, true, null)
      const bare = await sweep(loc, true, NO_CHIP)
      const old = loc === 'en' ? await sweep(loc, true, OLD) : null
      MATRIX.forEach((size, i) => {
        const a = off[i], b = on[i], n = bare[i], c = old?.[i]
        const where = `${size.name} ${loc}`
        if (!a || !b || !n) { console.error(`header-probe: contest switch: FT never drew at ${where}`); code = 2; return }
        if (c && (c.headH !== n.headH || c.stopY !== n.stopY)) caught++
        if (!b.chipW) hidden++
        const moved = b.stopY - a.stopY
        console.log(`header-probe: contest switch ${where}: header ${n.headH}→${b.headH} px, Stop TX ${n.stopY}→${b.stopY} px with the chip; ${moved ? `${moved > 0 ? '+' : ''}${moved} px from off (the top bar)` : 'not moved from off'}; chip ${b.chipW ? b.chipW + ' px' : 'not drawn'} in a ${b.slotW} px slot`)
        if (b.headH !== n.headH || b.stopY !== n.stopY) {
          console.error(`header-probe: RED contest switch ${where} — the chip took the header ${n.headH}→${b.headH} px and Stop TX ${n.stopY}→${b.stopY} px`)
          code = code || 1
        }
      })
    }
    console.log(`header-probe: contest switch: the chip is drawn at ${MATRIX.length * 4 - hidden} of ${MATRIX.length * 4} cases; CONTROL — the slot's old sizing moved the header at ${caught} of ${MATRIX.length} screens`)
    if (caught === 0) { console.error('header-probe: CONTROL FAILED — the chip\'s old sizing never moved the header; the contest check cannot fail'); code = code || 1 }
  } finally {
    await app.close()
    await rm(dir, { recursive: true, force: true })
  }
  return code
}
