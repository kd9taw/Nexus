// The radio switch notice and the Radios card in real Chrome, for what jsdom cannot see:
// `scripts/browser-probe --desktop radioswitch`.
//
// The probe serves the REAL app (index.html, main.tsx, App) with the app's own Vite config and src/api.ts
// swapped for probe/radioswitch-api.ts: a three-radio station with two routing rules and the notice on. At
// the representative screens (probe/sizes.mjs CORE: 1366×768 at 175 %, the floor, to 4K at 200 %), in the
// dark theme, it has Nexus switch radio by itself and
// asserts:
//   1. THE NOTICE: "Switched to IC-9700 …" is drawn inside the window, nothing in it or the page scrolls
//      sideways, and Undo and Why? are each the topmost thing where they stand;
//   2. UNDO selects the radio it left (FTdx10, through the radio pick) and takes the notice away;
//   3. WHY? opens Settings at Radio ▸ Radios, where each of the three cards says what Nexus uses it for, the
//      plain-words lines stand above the rules (one per radio), and the "When Nexus switches radio by itself"
//      choice is on screen and reachable, with nothing in the Radios section or the page scrolling sideways.
// THE CONTROL, run at every size: the same pages with a 2400 px unbreakable line planted in the notice and in
// the first card's routing line. The probe must see both overflow; if it does not, the check cannot fail and
// the probe says so instead of passing.
//
// exit: 0 every size clear · 1 a red, or a control that could not fail · 2 a page never drew the notice
import { createServer } from 'vite'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'
import { CORE, metrics } from './sizes.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const REAL_API = join(UI, 'src', 'api.ts')
const STUB_SRC = join(UI, 'probe', 'radioswitch-api.ts')
// Served under a name with no file behind it, beside the source, as the waterfalls probe does.
const STUB_API = join(UI, 'probe', '__radioswitch-api.ts')
const SIZES = CORE
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

const HELPERS = `
  const reach = (el) => {
    if (!el) return false
    el.scrollIntoView({ block: 'nearest' })
    const q = el.getBoundingClientRect()
    if (q.width < 1 || q.height < 1 || q.top < 0 || q.left < 0 || q.bottom > innerHeight || q.right > innerWidth) return false
    const t = document.elementFromPoint(q.left + q.width / 2, q.top + q.height / 2)
    return t != null && (t === el || el.contains(t))
  }
  const wider = (el) => (el ? el.scrollWidth - el.clientWidth : null)
  const plant = (el) => {
    if (!el || el.querySelector('.probe-wide')) return
    const s = document.createElement('span')
    s.className = 'probe-wide'
    s.textContent = 'W'.repeat(240)
    Object.assign(s.style, { whiteSpace: 'nowrap', overflowWrap: 'normal', display: 'inline-block', fontSize: '16px' })
    el.appendChild(s)
  }
  const buttonNamed = (root, re) => [...(root?.querySelectorAll('button') ?? [])].find((b) => re.test(b.textContent.trim()))
`

const NOTICE = (control) => `(() => { ${HELPERS}
  const toast = [...document.querySelectorAll('.ui-toast')].find((t) => /Switched to IC-9700/.test(t.textContent))
  if (!toast) return null
  const msg = toast.querySelector('.ui-toast-msg')
  if (${control}) plant(msg)
  const r = toast.getBoundingClientRect()
  return {
    text: msg.textContent,
    inside: r.left >= 0 && r.top >= 0 && r.right <= innerWidth && r.bottom <= innerHeight,
    box: [Math.round(r.left), Math.round(r.top), Math.round(r.width), Math.round(r.height)],
    page: wider(document.scrollingElement),
    toast: wider(toast),
    msg: wider(msg),
    undoOn: reach(buttonNamed(toast, /^Undo/)),
    whyOn: reach(buttonNamed(toast, /^Why\\?/)),
  }
})()`

const CLICK = (re) => `(() => { ${HELPERS}
  const toast = [...document.querySelectorAll('.ui-toast')].find((t) => /Switched to IC-9700/.test(t.textContent))
  const b = buttonNamed(toast, ${re}); if (!b) return false; b.click(); return true })()`

const AFTER_UNDO = `({ selected: window.__probeSelected(),
  gone: ![...document.querySelectorAll('.ui-toast')].some((t) => /Switched to IC-9700/.test(t.textContent)) })`

const RADIOS = (control) => `(() => { ${HELPERS}
  const sec = document.getElementById('settings-radios')
  if (!sec || sec.querySelectorAll('.radio-card-routes').length < 3) return null
  const routes = [...sec.querySelectorAll('.radio-card-routes')]
  if (${control}) plant(routes[0])
  const plain = [...sec.querySelectorAll('.routing-plain li')]
  const policy = sec.querySelector('.routing-policy select')
  const scroller = sec.closest('.settings-body, .settings-content, .settings-panel, main') ?? document.scrollingElement
  return {
    tab: document.querySelector('.settings-tab.active')?.textContent.trim() ?? null,
    routes: routes.map((r) => [r.textContent, reach(r), wider(r)]),
    plain: plain.map((li) => [li.textContent, reach(li)]),
    policy: policy ? { on: reach(policy), value: policy.value } : null,
    section: wider(sec),
    scroller: wider(scroller),
    page: wider(document.scrollingElement),
  }
})()`

function noticeFaults(m) {
  const f = []
  if (!m.inside) f.push(`the notice is not inside the window (${m.box})`)
  if (m.page > 0) f.push(`the page scrolls sideways by ${m.page} px`)
  if (m.toast > 0 || m.msg > 0) f.push(`the notice is ${Math.max(m.toast, m.msg)} px wider than it shows (sideways)`)
  if (!m.undoOn) f.push('Undo is not reachable')
  if (!m.whyOn) f.push('Why? is not reachable')
  return f
}
function radiosFaults(m) {
  const f = []
  if (m.tab !== 'Radio') f.push(`Why? opened the ${m.tab} tab, not Radio`)
  if (m.routes.length !== 3) f.push(`${m.routes.length} cards say what the radio is used for, not 3`)
  for (const [text, on, w] of m.routes) {
    if (!/^Nexus uses it for: /.test(text)) f.push(`a card's routing line reads "${text.slice(0, 60)}"`)
    if (!on) f.push(`the routing line "${text.slice(0, 40)}" is not on screen`)
    if (w > 0) f.push(`a card's routing line is ${w} px wider than it shows (sideways)`)
  }
  if (m.plain.length !== 3) f.push(`${m.plain.length} plain-words lines above the rules, not 3`)
  for (const [text, on] of m.plain) if (!on) f.push(`the line "${text.slice(0, 40)}" is not on screen`)
  if (!m.policy?.on) f.push('the switch choice is not reachable')
  if (m.policy && m.policy.value !== 'tell') f.push(`the switch choice reads ${m.policy.value}, not tell`)
  if (m.section > 0 || m.scroller > 0) f.push(`the Radios section scrolls sideways by ${Math.max(m.section, m.scroller)} px`)
  if (m.page > 0) f.push(`the page scrolls sideways by ${m.page} px`)
  return f
}

const realSrc = await readFile(REAL_API, 'utf8')
const stubSrc = await readFile(STUB_SRC, 'utf8')
const names = [...realSrc.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1])
const own = new Set([...stubSrc.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1]))
const stubCode =
  stubSrc +
  `\nexport * from ${JSON.stringify(REAL_API)}\n` +
  names.filter((n) => !own.has(n)).map((n) => `export async function ${n}(..._a: unknown[]): Promise<any> { return [] }`).join('\n')
const swap = {
  name: 'radioswitch-probe-api',
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

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-radioswitch-probe-'))
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

async function run(size, control) {
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
    await call('Emulation.setDeviceMetricsOverride', metrics(size))
    await call('Page.navigate', { url: `${origin}/probe-blank` })
    await sleep(300)
    await evaluate(`localStorage.clear(); localStorage.setItem('nexus.workspace', 'dx'); localStorage.setItem('tempo-theme', 'dark'); localStorage.setItem('nexus.features.wizardSeen', '1'); 1`)
    // The first-run wizard is marked seen: this is a station that has been set up.
    await call('Page.navigate', { url: `${origin}/#operate` })
    // The app has drawn and read the settings once the radio switcher lists the three radios.
    if (!(await until(`document.querySelector('.app') != null && document.body.textContent.includes('FT-991A')`))) return { error: 'the app never drew' }
    await sleep(500)
    await evaluate('window.__probeSwitch()')
    const notice = await until(NOTICE(control))
    if (!notice) return { error: 'the notice never drew' }
    let undo = null
    if (!control) {
      await evaluate(CLICK('/^Undo/'))
      undo = await until(`(() => { const a = ${AFTER_UNDO}; return a.gone ? a : null })()`, 40)
      await evaluate('window.__probeSwitch()')
      if (!(await until(NOTICE(false)))) return { notice, undo, error: 'the second notice never drew' }
    }
    // Page errors are collected so a Settings that never opens says why.
    await evaluate(`window.__errs = []; addEventListener('error', (e) => __errs.push(e.message)); addEventListener('unhandledrejection', (e) => __errs.push(String(e.reason))); 1`)
    const clicked = await evaluate(CLICK('/^Why\\?/'))
    const radios = await until(RADIOS(control), 250)
    if (!radios) return { notice, undo, error: 'no radios: ' + JSON.stringify(await evaluate(`({ clicked: ${clicked}, errs: window.__errs, hash: location.hash, settingsish: document.querySelectorAll('[class*=settings]').length, tab: document.querySelector('.settings-tab.active')?.textContent, sec: !!document.getElementById('settings-radios'), cards: document.querySelectorAll('.radio-card').length, routes: document.querySelectorAll('.radio-card-routes').length, toasts: [...document.querySelectorAll('.ui-toast')].map((t) => t.textContent), err: document.body.textContent.slice(0, 300) })`)) }
    if (radios) {
      await sleep(400)
      Object.assign(radios, await evaluate(RADIOS(control)))
    }
    return { notice, undo, radios }
  } finally {
    await browser.call('Target.closeTarget', { targetId })
  }
}

let code = 0
try {
  for (const size of SIZES) {
    const tag = size.name
    const r = await run(size, false)
    console.log(`radioswitch-probe: ${tag}: ${JSON.stringify(r)}`)
    if (r.error) {
      console.error(`radioswitch-probe: at ${tag} ${r.error}`)
      code = 2
      continue
    }
    const red = [...noticeFaults(r.notice)]
    if (!r.undo) red.push('Undo did not take the notice away')
    else if (r.undo.selected.at(-1) !== 0) red.push(`Undo selected ${JSON.stringify(r.undo.selected)}, not the FTdx10 (0)`)
    if (!r.radios) red.push('Why? did not open Settings at the Radios cards')
    else red.push(...radiosFaults(r.radios))
    if (red.length) {
      console.error(`radioswitch-probe: RED at ${tag} — ${red.join('; ')}`)
      code = code || 1
    }
    const c = await run(size, true)
    const caughtNotice = c.notice ? noticeFaults(c.notice).filter((s) => s.includes('sideways')) : []
    const caughtCard = c.radios ? radiosFaults(c.radios).filter((s) => s.includes('sideways')) : []
    console.log(`radioswitch-probe: CONTROL ${tag}: notice ${JSON.stringify(caughtNotice)} card ${JSON.stringify(caughtCard)}`)
    if (!caughtNotice.length || !caughtCard.length) {
      console.error(`radioswitch-probe: CONTROL FAILED at ${tag} — a planted overflow was not seen; the check cannot fail`)
      code = code || 1
    }
  }
  if (code === 0) console.log(`radioswitch-probe: PASS — the notice with Undo and Why? on screen, Undo back to the radio it left, Why? at the Radios cards with each radio's routing and the switch choice on screen, nothing sideways, at ${SIZES.map((s) => s.name).join(', ')}; each control caught`)
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
