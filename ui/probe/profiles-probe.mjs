// The "My station" picker in the top bar, in real Chrome, for what jsdom cannot see:
// `scripts/browser-probe --desktop profiles`.
//
// The probe serves the REAL app (index.html, main.tsx, App) with the app's own Vite config and src/api.ts
// swapped for probe/profiles-api.ts: the radio switch probe's three-radio station with three station
// profiles, the third with a long name. On the FT screen (Operate, whose TX cluster is its own strip) and on
// the Logbook (where the top bar carries the TX cluster: TX, Tune, Stop TX), at 1024×768 (the supported floor)
// and 1920×1080, in the dark theme, it measures the bar WITHOUT profiles and then WITH them, and asserts:
//   1. THE PICKER is drawn inside the window, is the topmost thing where it stands, offers the three
//      profiles, and a pick reaches the switch;
//   2. THE BAR keeps its height, nothing in it or the page scrolls sideways, and no part of it runs past
//      the window's right edge;
//   3. THE TX CONTROLS in the bar (Stop TX and Tune, on the Logbook) are where they were without the
//      picker, to the pixel, and each is the topmost thing where it stands.
// THE CONTROL, run at every size: the same page with a 2400 px unbreakable line planted beside the
// callsign. The probe must see the bar overflow; if it does not, the check cannot fail and the probe says
// so instead of passing.
//
// exit: 0 every case clear · 1 a red, or a control that could not fail · 2 a page never drew the bar
import { createServer } from 'vite'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const REAL_API = join(UI, 'src', 'api.ts')
const STUB_SRC = join(UI, 'probe', 'profiles-api.ts')
const STUB_API = join(UI, 'probe', '__profiles-api.ts')
const SIZES = [[1024, 768], [1920, 1080]]
const SCREENS = ['operate', 'logbook']
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

const MEASURE = (control) => `(() => {
  const bar = document.querySelector('header.topbar')
  if (!bar) return null
  if (${control}) {
    const id = bar.querySelector('.mycall')
    if (id && !id.querySelector('.probe-wide')) {
      const s = document.createElement('span')
      s.className = 'probe-wide'
      s.textContent = 'W'.repeat(240)
      Object.assign(s.style, { whiteSpace: 'nowrap', display: 'inline-block', fontSize: '16px' })
      id.appendChild(s)
    }
  }
  const top = (el) => {
    if (!el) return false
    const q = el.getBoundingClientRect()
    if (q.width < 1 || q.height < 1 || q.left < 0 || q.top < 0 || q.right > innerWidth || q.bottom > innerHeight) return false
    const t = document.elementFromPoint(q.left + q.width / 2, q.top + q.height / 2)
    return t != null && (t === el || el.contains(t))
  }
  const box = (el) => { if (!el) return null; const q = el.getBoundingClientRect(); return [Math.round(q.left), Math.round(q.top), Math.round(q.width), Math.round(q.height)] }
  const named = (re) => [...bar.querySelectorAll('button')].find((b) => re.test(b.textContent.trim()))
  const picker = bar.querySelector('select.station-profile-picker')
  const r = bar.getBoundingClientRect()
  const rightmost = Math.max(...[...bar.querySelectorAll('*')].map((e) => e.getBoundingClientRect().right))
  return {
    height: Math.round(r.height),
    barSideways: bar.scrollWidth - bar.clientWidth,
    pageSideways: document.scrollingElement.scrollWidth - document.scrollingElement.clientWidth,
    pastEdge: Math.round(rightmost - innerWidth),
    picker: picker ? { on: top(picker), box: box(picker), options: [...picker.options].map((o) => o.textContent) } : null,
    stop: named(/^Stop TX$/) ? { on: top(named(/^Stop TX$/)), box: box(named(/^Stop TX$/)) } : null,
    tune: named(/^Tune/i) ? { on: top(named(/^Tune/i)), box: box(named(/^Tune/i)) } : null,
  }
})()`

const realSrc = await readFile(REAL_API, 'utf8')
const stubSrc = await readFile(STUB_SRC, 'utf8')
const names = [...realSrc.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1])
const own = new Set([...stubSrc.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1]))
const stubCode =
  stubSrc +
  `\nexport * from ${JSON.stringify(REAL_API)}\n` +
  names.filter((n) => !own.has(n)).map((n) => `export async function ${n}(..._a: unknown[]): Promise<any> { return [] }`).join('\n')
const swap = {
  name: 'profiles-probe-api',
  enforce: 'pre',
  async resolveId(source, importer, opts) {
    if (opts?.scan) return null
    if (source === STUB_API) return STUB_API
    if (!importer || importer === STUB_API || !/api(\.ts)?$/.test(source)) return null
    const r = await this.resolve(source, importer, { ...opts, skipSelf: true })
    return r && r.id.split('?')[0] === REAL_API ? STUB_API : null
  },
  load(id) {
    return id === STUB_API ? stubCode : null
  },
}

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-profiles-probe-'))
const server = await createServer({
  root: UI,
  configFile: join(UI, 'vite.config.ts'),
  cacheDir,
  logLevel: 'warn',
  plugins: [swap],
  server: { host: '127.0.0.1', port: 0, strictPort: false, hmr: false },
})
await server.listen()
const origin = `http://127.0.0.1:${server.httpServer.address().port}`
const browser = await chrome()

async function run([W, H], screen, control) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text)
    return r.result.value
  }
  const until = async (expression, tries = 80) => {
    for (let i = 0; i < tries; i++) {
      await sleep(200)
      const v = await evaluate(expression).catch(() => null)
      if (v) return v
    }
    return null
  }
  try {
    await call('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: 1, mobile: false })
    await call('Page.navigate', { url: `${origin}/probe-blank` })
    await sleep(300)
    await evaluate(`localStorage.clear(); localStorage.setItem('nexus.workspace', 'dx'); localStorage.setItem('tempo-theme', 'dark'); localStorage.setItem('nexus.features.wizardSeen', '1'); 1`)
    await call('Page.navigate', { url: `${origin}/#${screen}` })
    if (!(await until(`document.querySelector('select.station-profile-picker') != null`))) return { error: 'the picker never drew' }
    await sleep(600)
    await evaluate('window.__probeProfiles(false)')
    const without = await until(`(() => document.querySelector('select.station-profile-picker') ? null : ${MEASURE(false)})()`)
    await evaluate('window.__probeProfiles(true)')
    const withP = await until(`(() => document.querySelector('select.station-profile-picker') ? ${MEASURE(control)} : null)()`)
    if (!without || !withP) return { error: 'the bar never drew both ways' }
    let switched = null
    if (!control) {
      await evaluate(`(() => { const s = document.querySelector('select.station-profile-picker'); s.value = 'p1'; s.dispatchEvent(new Event('change', { bubbles: true })); return 1 })()`)
      switched = await until(`(() => { const s = window.__probeSwitched(); return s.length ? s : null })()`, 20)
    }
    return { without, with: withP, switched }
  } finally {
    await browser.call('Target.closeTarget', { targetId })
  }
}

function faults(r, screen) {
  const f = []
  const m = r.with
  if (!m.picker) f.push('no picker')
  else {
    if (!m.picker.on) f.push(`the picker is not topmost on screen (${m.picker.box})`)
    if (m.picker.options.length !== 3) f.push(`the picker offers ${m.picker.options.length} profiles, not 3`)
  }
  if (m.height !== r.without.height) f.push(`the bar is ${m.height} px tall with the picker, ${r.without.height} without`)
  if (m.barSideways > 0 || m.pageSideways > 0) f.push(`sideways: bar ${m.barSideways} px, page ${m.pageSideways} px`)
  if (m.pastEdge > 0) f.push(`the bar runs ${m.pastEdge} px past the window`)
  if (screen === 'logbook') {
    for (const k of ['stop', 'tune']) {
      if (!m[k] || !r.without[k]) { f.push(`no ${k} control in the bar`); continue }
      if (!m[k].on) f.push(`${k} is not topmost on screen`)
      if (JSON.stringify(m[k].box) !== JSON.stringify(r.without[k].box)) f.push(`${k} moved: ${r.without[k].box} → ${m[k].box}`)
    }
  }
  return f
}

let code = 0
try {
  for (const size of SIZES) {
    for (const screen of SCREENS) {
      const tag = `${size[0]}×${size[1]} ${screen}`
      const r = await run(size, screen, false)
      console.log(`profiles-probe: ${tag}: ${JSON.stringify(r)}`)
      if (r.error) {
        console.error(`profiles-probe: at ${tag} ${r.error}`)
        code = 2
        continue
      }
      const red = faults(r, screen)
      if (!r.switched || r.switched.at(-1) !== 'p1') red.push(`a pick did not reach the switch (${JSON.stringify(r.switched)})`)
      if (red.length) {
        console.error(`profiles-probe: RED at ${tag} — ${red.join('; ')}`)
        code = code || 1
      }
      const c = await run(size, screen, true)
      const caught = c.with ? faults(c, screen).filter((s) => /sideways|past the window/.test(s)) : []
      console.log(`profiles-probe: CONTROL ${tag}: ${JSON.stringify(caught)}`)
      if (!caught.length) {
        console.error(`profiles-probe: CONTROL FAILED at ${tag} — a planted overflow was not seen; the check cannot fail`)
        code = code || 1
      }
    }
  }
  if (code === 0) console.log(`profiles-probe: PASS — the picker on screen with its three profiles and a pick reaching the switch, the bar's height unchanged, nothing sideways or past the window, Stop TX and Tune unmoved and topmost on the Logbook, at ${SIZES.map(([w, h]) => `${w}×${h}`).join(', ')} on ${SCREENS.join(' and ')}; each control caught`)
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
