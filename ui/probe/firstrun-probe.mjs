// The first-run wizard's About-you question, the first-contact walk and Settings' folding in real Chrome,
// for what jsdom cannot see: `scripts/browser-probe --desktop firstrun`.
//
// The probe serves the REAL app (index.html, main.tsx, App) with the app's own Vite config and src/api.ts
// swapped for probe/firstrun-api.ts: a fresh install, so the wizard opens by itself. At 1024×768 (the
// supported floor) and 1920×1080, in the dark theme, it asserts:
//   1. ABOUT YOU: the wizard opens on "How much have you done?", inside the window, with the three answers
//      and Next each the topmost thing where they stand, and nothing in the dialog or page sideways;
//   2. FINISH: after "New to the hobby" and Next through the steps, the guide and walk offers are ticked and
//      Finish is reachable; Finish writes the answer (newToHobby) through its writer;
//   3. GUIDE, THEN WALK: the Getting started guide opens first; closed, the first-contact walk opens, its
//      four lines and Got it on screen, nothing sideways;
//   4. SETTINGS FOLDED: reopened (as a station reloads), Settings ▸ Appearance shows the fold note with
//      Show all reachable and its sections folded to their headings; Show all opens them.
// THE CONTROL, run at every size: the same pages with a 2400 px unbreakable line planted in an About-you
// answer and in the walk's list. The probe must see both overflow; if it does not, the check cannot fail
// and the probe says so instead of passing.
//
// exit: 0 every size clear · 1 a red, or a control that could not fail · 2 a page never drew
import { createServer } from 'vite'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const REAL_API = join(UI, 'src', 'api.ts')
const STUB_SRC = join(UI, 'probe', 'firstrun-api.ts')
// Served under a name with no file behind it, beside the source, as the waterfalls probe does.
const STUB_API = join(UI, 'probe', '__firstrun-api.ts')
const SIZES = [[1024, 768], [1920, 1080]]
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


const dialogNamed = (re) => `[...document.querySelectorAll('.ui-dialog')].find((d) => ${re}.test(d.textContent))`

const ABOUT = (control) => `(() => { ${HELPERS}
  const d = ${dialogNamed('/How much have you done\\?/')}
  if (!d) return null
  // The experience question's answers: the first group (the radio count below it is the journeys probe's).
  const answers = [...(d.querySelector('.wizard-modes')?.querySelectorAll('.wizard-mode') ?? [])]
  if (${control}) plant(answers[0]?.querySelector('.wizard-mode-blurb'))
  const r = d.getBoundingClientRect()
  return {
    inside: r.left >= 0 && r.top >= 0 && r.right <= innerWidth + 1 && r.bottom <= innerHeight + 1,
    box: [Math.round(r.left), Math.round(r.top), Math.round(r.width), Math.round(r.height)],
    answers: answers.map((b) => [b.querySelector('.wizard-mode-label')?.textContent, reach(b)]),
    next: reach(buttonNamed(d, /^Next/)),
    // The dialog's screen-reader title is a 1 px clipped box (.sr-only): wider by design, never seen.
    dialog: Math.max(wider(d) ?? 0, ...[...d.querySelectorAll('*:not(.sr-only)')].map((e) => (e.scrollWidth > e.clientWidth + 1 && getComputedStyle(e).overflowX !== 'visible' ? e.scrollWidth - e.clientWidth : 0))),
    page: wider(document.scrollingElement),
  }
})()`

const CLICK = (re) => `(() => { ${HELPERS}
  const d = document.querySelector('.ui-dialog'); const b = buttonNamed(d, ${re}); if (!b) return false; b.click(); return true })()`

const FINISH = `(() => { ${HELPERS}
  const d = document.querySelector('.ui-dialog')
  const f = buttonNamed(d, /^Finish/)
  if (!f) return null
  const pressed = (re) => buttonNamed(d, re)?.getAttribute('aria-pressed')
  return { finish: reach(f), label: f.textContent, guide: pressed(/^Show me Getting started|Getting started/), walk: pressed(/^Walk me through/) }
})()`

const WALK = (control) => `(() => { ${HELPERS}
  const d = ${dialogNamed('/Your first FT8 contact/')}
  if (!d) return null
  const list = d.querySelector('.first-contact-steps')
  if (${control}) plant(list?.querySelector('li'))
  const r = d.getBoundingClientRect()
  return {
    inside: r.left >= 0 && r.top >= 0 && r.right <= innerWidth + 1 && r.bottom <= innerHeight + 1,
    box: [Math.round(r.left), Math.round(r.top), Math.round(r.width), Math.round(r.height)],
    lines: [...(list?.querySelectorAll('li') ?? [])].map((li) => reach(li)),
    gotIt: reach(buttonNamed(d, /^Got it/)),
    dialog: Math.max(wider(d) ?? 0, wider(list) ?? 0, ...[...(list?.querySelectorAll('li') ?? [])].map((e) => e.scrollWidth - e.clientWidth)),
    page: wider(document.scrollingElement),
    guideOpen: !!document.querySelector('.gsg-dialog'),
  }
})()`

const FOLD = `(() => { ${HELPERS}
  const note = document.querySelector('.settings-fold-note')
  if (!note) return null
  const folded = [...document.querySelectorAll('.settings-section[data-folded]')]
  return {
    tab: document.querySelector('.settings-tab.active')?.textContent.trim() ?? null,
    note: reach(note),
    showAll: reach(buttonNamed(note, /^Show all/)),
    folded: folded.map((f) => [f.id, Math.round(f.getBoundingClientRect().height), reach(f.querySelector('legend'))]),
    page: wider(document.scrollingElement),
  }
})()`

function aboutFaults(m) {
  const f = []
  if (!m.inside) f.push(`the wizard is not inside the window (${m.box})`)
  if (m.answers.length !== 3) f.push(`${m.answers.length} answers, not 3`)
  for (const [label, on] of m.answers) if (!on) f.push(`the answer "${label}" is not reachable`)
  if (!m.next) f.push('Next is not reachable')
  if (m.dialog > 0) f.push(`the wizard scrolls sideways by ${m.dialog} px`)
  if (m.page > 0) f.push(`the page scrolls sideways by ${m.page} px`)
  return f
}
function walkFaults(m) {
  const f = []
  if (!m.inside) f.push(`the walk is not inside the window (${m.box})`)
  if (m.lines.length !== 4) f.push(`${m.lines.length} lines, not 4`)
  m.lines.forEach((on, i) => { if (!on) f.push(`line ${i + 1} is not on screen`) })
  if (!m.gotIt) f.push('Got it is not reachable')
  if (m.dialog > 0) f.push(`the walk scrolls sideways by ${m.dialog} px`)
  if (m.page > 0) f.push(`the page scrolls sideways by ${m.page} px`)
  if (m.guideOpen) f.push('the walk opened over the guide')
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
  name: 'firstrun-probe-api',
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

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-firstrun-probe-'))
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


async function run([W, H], control) {
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
  // Escape where Radix listens for it (the dismissable layer's document listener), from the open dialog.
  const escape = () =>
    evaluate(`(() => { const d = document.querySelector('.gsg-dialog') ?? document.querySelector('.ui-dialog'); (d ?? document).dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); return 1 })()`)
  try {
    await call('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: 1, mobile: false })
    await call('Page.navigate', { url: `${origin}/probe-blank` })
    await sleep(300)
    // A fresh install: nothing stored, so the wizard opens by itself.
    await evaluate(`localStorage.clear(); localStorage.setItem('tempo-theme', 'dark'); 1`)
    await call('Page.navigate', { url: `${origin}/#operate` })
    if (!(await until(ABOUT(false)))) return { error: 'the wizard never opened on About you' }
    // Measured once the dialog's open animation has settled.
    await sleep(800)
    const about = await evaluate(ABOUT(control))
    if (control) {
      await evaluate(CLICK('/^New to the hobby/'))
      for (let i = 0; i < 4; i++) { await evaluate(CLICK('/^Next/')); await sleep(250) }
      await evaluate(CLICK('/^Finish/'))
      await until(`!!document.querySelector('.gsg-dialog')`)
      await sleep(500)
      await escape()
      const walk = await until(WALK(true))
      return { about, walk }
    }
    await evaluate(CLICK('/^New to the hobby/'))
    for (let i = 0; i < 4; i++) { await evaluate(CLICK('/^Next/')); await sleep(250) }
    const finish = await until(FINISH)
    await evaluate(`window.__errs = []; addEventListener('error', (e) => __errs.push('E ' + e.message + ' ' + String(e.error?.stack).slice(0, 600))); addEventListener('unhandledrejection', (e) => __errs.push('R ' + String(e.reason))); const ce = console.error; console.error = (...a) => { __errs.push('C ' + a.map(String).join(' ').slice(0, 300)); ce(...a) }; 1`)
    await evaluate(CLICK('/^Finish/'))
    const written = await until(`(() => { const w = window.__probeWritten(); return w.length ? w : null })()`)
    const guide = await until(`!!document.querySelector('.gsg-dialog')`)
    await sleep(500)
    await escape()
    const errs = await evaluate('window.__errs')
    const walk = await until(WALK(false))
    const walkWhy = walk ? null : await evaluate(`[...document.querySelectorAll('.ui-dialog')].map((d) => d.className + ': ' + d.textContent.slice(0, 80))`)
    if (walk) await evaluate(`(() => { ${HELPERS} const d = ${dialogNamed('/Your first FT8 contact/')}; buttonNamed(d, /^Got it/)?.click(); return 1 })()`)
    const walkGone = await until(`![...document.querySelectorAll('.ui-dialog')].some((d) => /Your first FT8 contact/.test(d.textContent))`, 20)
    // A station reopening: the answer is stored, the wizard seen. Settings, Appearance tab.
    await call('Page.navigate', { url: `${origin}/#operate` })
    await until(`document.querySelector('.app') != null && !document.querySelector('.app.loading')`, 100)
    await sleep(500)
    const navd = await evaluate(`(() => { const b = [...document.querySelectorAll('button, a')].find((x) => [x.getAttribute('aria-label'), x.title, x.textContent].some((v) => (v ?? '').trim() === 'Settings')); b?.click(); return !!b })()`)
    await until(`!!document.querySelector('.settings-tabs')`, 100)
    await evaluate(`(() => { const b = [...document.querySelectorAll('.settings-tab')].find((x) => x.textContent.trim() === 'Appearance'); b?.click(); return !!b })()`)
    const fold = await until(FOLD, 60)
    const foldWhy = fold ? null : await evaluate(`({ navd: ${navd}, hash: location.hash, tabs: [...document.querySelectorAll('.settings-tab')].map((t) => t.textContent.trim()), active: document.querySelector('.settings-tab.active')?.textContent, level: document.querySelector('select[aria-label="Your experience"]')?.value ?? null, stored: localStorage.getItem('probe.level'), dialogs: document.querySelectorAll('.ui-dialog').length, sections: document.querySelectorAll('.settings-section').length })`)
    let opened = null
    if (fold) {
      await evaluate(`(() => { const n = document.querySelector('.settings-fold-note'); [...n.querySelectorAll('button')].find((b) => /^Show all/.test(b.textContent.trim()))?.click(); return 1 })()`)
      opened = await until(`(() => document.querySelector('.settings-fold-note') ? null : { folded: document.querySelectorAll('.settings-section[data-folded]').length, theme: Math.round(document.getElementById('settings-theme')?.getBoundingClientRect().height ?? 0) })()`, 20)
    }
    return { about, finish, written, guide, errs, walk, walkWhy, walkGone, fold, foldWhy, opened }
  } finally {
    await browser.call('Target.closeTarget', { targetId })
  }
}

let code = 0
try {
  for (const size of SIZES) {
    const tag = `${size[0]}×${size[1]}`
    const r = await run(size, false)
    console.log(`firstrun-probe: ${tag}: ${JSON.stringify(r)}`)
    if (r.error) {
      console.error(`firstrun-probe: at ${tag} ${r.error}`)
      code = 2
      continue
    }
    const red = [...aboutFaults(r.about)]
    if (!r.finish?.finish) red.push('Finish is not reachable')
    if (r.finish && (r.finish.guide !== 'true' || r.finish.walk !== 'true')) red.push(`the guide/walk offers read ${r.finish.guide}/${r.finish.walk}, not ticked`)
    if (JSON.stringify(r.written) !== '["newToHobby"]') red.push(`the answer written was ${JSON.stringify(r.written)}`)
    if (!r.guide) red.push('the guide did not open after Finish')
    if (!r.walk) red.push('the walk did not open after the guide closed')
    else red.push(...walkFaults(r.walk))
    if (!r.walkGone) red.push('Got it did not close the walk')
    if (!r.fold) red.push('Settings ▸ Appearance showed no fold note')
    else {
      if (r.fold.tab !== 'Appearance') red.push(`the ${r.fold.tab} tab, not Appearance`)
      if (!r.fold.note || !r.fold.showAll) red.push('the fold note or Show all is not reachable')
      if (r.fold.folded.length < 3) red.push(`${r.fold.folded.length} sections folded on Appearance`)
      for (const [id, h, legend] of r.fold.folded) {
        if (h > 80) red.push(`${id} is ${h} px tall while folded`)
        if (!legend && h > 0) { /* off screen below: reach() needs it scrolled, which it does */ }
      }
      if (r.fold.page > 0) red.push(`the page scrolls sideways by ${r.fold.page} px`)
    }
    if (!r.opened) red.push('Show all did not open the tab')
    else if (r.opened.folded !== 0 || r.opened.theme <= 80) red.push(`after Show all: ${JSON.stringify(r.opened)}`)
    if (red.length) {
      console.error(`firstrun-probe: RED at ${tag} — ${red.join('; ')}`)
      code = code || 1
    }
    const c = await run(size, true)
    const caughtAbout = c.about ? aboutFaults(c.about).filter((s) => s.includes('sideways')) : []
    const caughtWalk = c.walk ? walkFaults(c.walk).filter((s) => s.includes('sideways')) : []
    console.log(`firstrun-probe: CONTROL ${tag}: about ${JSON.stringify(caughtAbout)} walk ${JSON.stringify(caughtWalk)}`)
    if (!caughtAbout.length || !caughtWalk.length) {
      console.error(`firstrun-probe: CONTROL FAILED at ${tag} — a planted overflow was not seen; the check cannot fail`)
      code = code || 1
    }
  }
  if (code === 0) console.log(`firstrun-probe: PASS — About you with its three answers and Next on screen, Finish writes the answer, the guide then the walk with its four lines and Got it on screen, Settings ▸ Appearance folded with Show all reachable and opening it, nothing sideways, at ${SIZES.map(([w, h]) => `${w}×${h}`).join(', ')}; each control caught`)
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
