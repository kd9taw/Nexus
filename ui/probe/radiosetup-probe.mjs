// The radio setup ("Set up radios…") in real Chrome, for what jsdom cannot see:
// `scripts/browser-probe --desktop radiosetup`.
//
// The probe serves the REAL app with the app's own Vite config and src/api.ts swapped for
// probe/radiosetup-api.ts: the radio switch probe's three-radio station. At 1024×768 (the supported floor)
// and 1920×1080, in the dark theme, it reaches Settings ▸ Radio ▸ Radios the way an operator does after a
// switch (the notice's Why?), presses "Set up radios…", and on EVERY step of the setup asserts:
//   1. the dialog is inside the window and nothing in it or the page scrolls sideways (the band grid
//      included: three radios fit);
//   2. Next (Finish on the last step), Back and Cancel are each the topmost thing where they stand;
//   3. on Audio and keying, Test CAT on the IC-9700 (not in use) answers on screen; on Check, the
//      "Where would this go?" answer and the rules Finish saves are on screen.
// Then Finish, untouched, closes the setup and wrote nothing (the stub counts the writers).
// THE CONTROL, run at every size: the same pages with a 2400 px unbreakable line planted in the dialog's
// heading. The probe must see it overflow; if it does not, the check cannot fail and the probe says so.
//
// exit: 0 every size clear · 1 a red, or a control that could not fail · 2 a page never drew the setup
import { createServer } from 'vite'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const REAL_API = join(UI, 'src', 'api.ts')
const STUB_SRC = join(UI, 'probe', 'radiosetup-api.ts')
// Served under a name with no file behind it, beside the source, as the waterfalls probe does.
const STUB_API = join(UI, 'probe', '__radiosetup-api.ts')
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

const CLICK_WHY = `(() => { ${HELPERS}
  const toast = [...document.querySelectorAll('.ui-toast')].find((t) => /Switched to IC-9700/.test(t.textContent))
  const b = buttonNamed(toast, /^Why\\?/); if (!b) return false; b.click(); return true })()`

const OPEN_SETUP = `(() => { ${HELPERS}
  const sec = document.getElementById('settings-radios')
  const b = buttonNamed(sec, /^Set up radios/)
  if (!b) return null
  const on = reach(b)
  b.click()
  return { on }
})()`

const STEP = (control) => `(() => { ${HELPERS}
  const d = document.querySelector('.ui-dialog.radio-setup')
  if (!d) return null
  const title = d.querySelector('.wizard-title')
  if (${control}) plant(title)
  const r = d.getBoundingClientRect()
  const grid = d.querySelector('.radio-setup-grid')
  const go = buttonNamed(d, /^(Next|Finish)$/)
  return {
    title: title?.textContent ?? null,
    inside: r.left >= 0 && r.top >= 0 && r.right <= innerWidth && r.bottom <= innerHeight,
    box: [Math.round(r.left), Math.round(r.top), Math.round(r.width), Math.round(r.height)],
    page: wider(document.scrollingElement),
    dialog: wider(d),
    grid: grid ? wider(grid) : 0,
    go: go?.textContent ?? null,
    goOn: reach(go),
    cancelOn: reach(buttonNamed(d, /^Cancel$/)),
    backOn: buttonNamed(d, /^Back$/) ? reach(buttonNamed(d, /^Back$/)) : null,
    answer: d.querySelector('.radio-setup-answer')?.textContent ?? null,
    rules: d.querySelectorAll('.radio-setup-rules li').length,
    cat: [...d.querySelectorAll('.radio-setup-test [role=status]')].map((s) => s.textContent),
  }
})()`

function stepFaults(m) {
  const f = []
  if (!m.inside) f.push(`"${m.title}": the setup is not inside the window (${m.box})`)
  if (m.page > 0) f.push(`"${m.title}": the page scrolls sideways by ${m.page} px`)
  if (m.dialog > 0) f.push(`"${m.title}": the setup scrolls sideways by ${m.dialog} px`)
  if (m.grid > 0) f.push(`"${m.title}": the band grid scrolls sideways by ${m.grid} px`)
  if (!m.goOn) f.push(`"${m.title}": ${m.go ?? 'Next'} is not reachable`)
  if (!m.cancelOn) f.push(`"${m.title}": Cancel is not reachable`)
  if (m.backOn === false) f.push(`"${m.title}": Back is not reachable`)
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
  name: 'radiosetup-probe-api',
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

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-radiosetup-probe-'))
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
  try {
    await call('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: 1, mobile: false })
    await call('Page.navigate', { url: `${origin}/probe-blank` })
    await sleep(300)
    await evaluate(`localStorage.clear(); localStorage.setItem('nexus.workspace', 'dx'); localStorage.setItem('tempo-theme', 'dark'); localStorage.setItem('nexus.features.wizardSeen', '1'); 1`)
    // The first-run wizard is marked seen: this is a station that has been set up.
    await call('Page.navigate', { url: `${origin}/#operate` })
    // The app has drawn and read the settings once the radio switcher lists the three radios.
    if (!(await until(`document.querySelector('.app') != null && document.body.textContent.includes('FT-991A')`))) return { error: 'the app never drew' }
    await sleep(500)
    await evaluate('window.__probeSwitch()')
    if (!(await until(`[...document.querySelectorAll('.ui-toast')].some((t) => /Switched to IC-9700/.test(t.textContent))`))) return { error: 'the notice never drew' }
    await evaluate(CLICK_WHY)
    if (!(await until(`document.getElementById('settings-radios') != null`, 250))) return { error: 'Why? did not open the Radios cards' }
    await sleep(400)
    const opened = await until(OPEN_SETUP)
    if (!opened) return { error: 'no "Set up radios…" button on the Radios cards' }
    const steps = []
    for (let i = 0; i < 10; i++) {
      const m = await until(STEP(control))
      if (!m) return { opened, steps, error: 'the setup never drew' }
      if (/Audio and keying/.test(m.title ?? '') && !control) {
        await evaluate(`(() => { const d = document.querySelector('.ui-dialog.radio-setup'); [...d.querySelectorAll('button')].filter((b) => b.textContent.trim() === 'Test CAT')[1].click(); return 1 })()`)
        await until(`document.querySelectorAll('.ui-dialog.radio-setup .radio-setup-test [role=status]').length > 0`)
        Object.assign(m, await evaluate(STEP(false)))
      }
      if (/Where would this go/.test(m.title ?? '')) {
        await until(`document.querySelector('.ui-dialog.radio-setup .radio-setup-answer') != null`)
        Object.assign(m, await evaluate(STEP(control)))
      }
      steps.push(m)
      if (control) break
      if (m.go === 'Finish') break
      await evaluate(`(() => { const d = document.querySelector('.ui-dialog.radio-setup'); [...d.querySelectorAll('button')].find((b) => b.textContent.trim() === 'Next').click(); return 1 })()`)
      await sleep(250)
    }
    let finished = null
    if (!control) {
      await evaluate(`(() => { const d = document.querySelector('.ui-dialog.radio-setup'); [...d.querySelectorAll('button')].find((b) => b.textContent.trim() === 'Finish').click(); return 1 })()`)
      finished = await until(`document.querySelector('.ui-dialog.radio-setup') == null ? { writes: window.__probeWrites() } : null`, 40)
    }
    return { opened, steps, finished }
  } finally {
    await browser.call('Target.closeTarget', { targetId })
  }
}

let code = 0
try {
  for (const size of SIZES) {
    const tag = `${size[0]}×${size[1]}`
    const r = await run(size, false)
    console.log(`radiosetup-probe: ${tag}: ${JSON.stringify(r)}`)
    if (r.error) {
      console.error(`radiosetup-probe: at ${tag} ${r.error}`)
      code = 2
      continue
    }
    const red = []
    if (!r.opened.on) red.push('"Set up radios…" is not reachable on the Radios cards')
    if (r.steps.length !== 7) red.push(`${r.steps.length} steps drawn, not 7`)
    for (const m of r.steps) red.push(...stepFaults(m))
    const audio = r.steps.find((m) => /Audio and keying/.test(m.title ?? ''))
    if (!audio?.cat.includes('Answering: 144.174 MHz')) red.push(`Test CAT on the IC-9700 said ${JSON.stringify(audio?.cat)}`)
    const check = r.steps.at(-1)
    if (!check?.answer) red.push('the "Where would this go?" answer is not on screen')
    if (!(check?.rules > 0)) red.push('the rules Finish saves are not listed')
    if (!r.finished) red.push('Finish did not close the setup')
    else if (r.finished.writes.length) red.push(`an untouched Finish wrote ${r.finished.writes.join(', ')}`)
    if (red.length) {
      console.error(`radiosetup-probe: RED at ${tag} — ${red.join('; ')}`)
      code = code || 1
    }
    const c = await run(size, true)
    const caught = c.steps?.length ? stepFaults(c.steps[0]).filter((s) => s.includes('sideways')) : []
    console.log(`radiosetup-probe: CONTROL ${tag}: ${JSON.stringify(caught)}`)
    if (!caught.length) {
      console.error(`radiosetup-probe: CONTROL FAILED at ${tag} — a planted overflow was not seen; the check cannot fail`)
      code = code || 1
    }
  }
  if (code === 0) console.log(`radiosetup-probe: PASS — "Set up radios…" reachable, all seven steps inside the window with Next/Finish, Back and Cancel on top and nothing sideways, Test CAT and "Where would this go?" answered on screen, an untouched Finish wrote nothing, at ${SIZES.map(([w, h]) => `${w}×${h}`).join(', ')}; each control caught`)
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
