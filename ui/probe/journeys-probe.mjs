// The first-run journeys in real Chrome, for what jsdom cannot see: `scripts/browser-probe --desktop journeys`.
//
// The probe serves the REAL app (index.html, main.tsx, App) with the app's own Vite config and src/api.ts
// swapped for probe/journeys-api.ts: a fresh install, so the wizard opens by itself. At 1024×768 (the
// supported floor) and 1920×1080, in the dark and the light theme, it walks the two journeys this page
// adds and asserts, each where it stands:
//   1. ABOUT YOU: the three experience answers, the two radio-count answers and Next each the topmost thing
//      where they stand, the wizard inside the window, nothing sideways;
//   2. YOUR RIG, "Two or more": the hint on screen; the radio found picked, "＋ I have a second radio"
//      pressed, the radio setup opens by itself inside the window with its Cancel on top; Cancel closes it;
//   3. THE LOG, "Experienced": "Where your contacts go" lists QRZ Logbook, LoTW, ClubLog and eQSL (never the
//      QRZ callbook), QRZ's Test on top and its answer on screen, the open-Settings offer on top; a log
//      picked opens the import preview inside the window with Import on top, and nothing is imported
//      before Import; after Import and Close the result line is on screen; nothing sideways;
//   4. FINISH: Settings ▸ Confirmations opens (the offer was ticked), and over the whole walk no call
//      uploaded a contact or stored a login.
// THE CONTROL, run at every size and theme: a 2400 px unbreakable line planted in the services' sentence
// and in the preview. The probe must see both overflow; if it does not, the check cannot fail and the
// probe says so instead of passing.
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
const STUB_SRC = join(UI, 'probe', 'journeys-api.ts')
const STUB_API = join(UI, 'probe', '__journeys-api.ts')
const SIZES = [[1024, 768], [1920, 1080]]
const THEMES = ['dark', 'light']
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
// Every api verb that sends a contact out, fetches from a logbook service, or stores a login.
const SENDS = /push|upload|^sync(Qrz|Lotw|Clublog|Eqsl)|^download(Lotw|Eqsl)|^set\w*(Password|Key|Code|Token)$|^setUploadToggle|^markLotwUploaded/i

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
  const sideways = (d) => Math.max(wider(d) ?? 0, ...[...d.querySelectorAll('*:not(.sr-only)')].map((e) => (e.scrollWidth > e.clientWidth + 1 && getComputedStyle(e).overflowX !== 'visible' ? e.scrollWidth - e.clientWidth : 0)))
  const inside = (d) => { const r = d.getBoundingClientRect(); return r.left >= 0 && r.top >= 0 && r.right <= innerWidth + 1 && r.bottom <= innerHeight + 1 }
  const plant = (el) => {
    if (!el || el.querySelector('.probe-wide')) return
    const s = document.createElement('span')
    s.className = 'probe-wide'
    s.textContent = 'W'.repeat(240)
    Object.assign(s.style, { whiteSpace: 'nowrap', overflowWrap: 'normal', display: 'inline-block', fontSize: '16px' })
    el.appendChild(s)
  }
  const buttonNamed = (root, re) => [...(root?.querySelectorAll('button') ?? [])].find((b) => re.test(b.textContent.trim()))
  const dialogNamed = (re) => [...document.querySelectorAll('.ui-dialog')].reverse().find((d) => re.test(d.textContent))
  const wizard = () => dialogNamed(/I’ll set it up myself/)
`

const ABOUT = `(() => { ${HELPERS}
  const d = wizard()
  if (!d || !/How much have you done\\?/.test(d.textContent)) return null
  const answers = [...d.querySelectorAll('.wizard-mode')]
  return {
    inside: inside(d),
    answers: answers.map((b) => [b.querySelector('.wizard-mode-label')?.textContent, reach(b)]),
    next: reach(buttonNamed(d, /^Next/)),
    sideways: sideways(d),
    page: wider(document.scrollingElement),
  }
})()`

const CLICK = (re, scope = 'wizard()') => `(() => { ${HELPERS}
  const b = buttonNamed(${scope}, ${re}); if (!b) return false; b.click(); return true })()`

const RIG = `(() => { ${HELPERS}
  const d = wizard()
  const hint = d?.querySelector('.wizard-several-hint')
  if (!hint) return null
  return { hint: reach(hint), sideways: sideways(d), page: wider(document.scrollingElement) }
})()`

const SETUP = `(() => { ${HELPERS}
  const d = dialogNamed(/Your radios/)
  if (!d || d === wizard()) return null
  return { inside: inside(d), cancel: reach(buttonNamed(d, /^Cancel$/)), sideways: sideways(d) }
})()`

const LOG = (control) => `(() => { ${HELPERS}
  const d = wizard()
  const panel = d?.querySelector('.wizard-services')
  if (!panel || panel.querySelectorAll('.conn-status-row').length === 0) return null
  if (${control}) plant(panel.querySelector('.wizard-license-sub'))
  return {
    inside: inside(d),
    rows: [...panel.querySelectorAll('.conn-status-row')].map((r) => [r.querySelector('.conn-name')?.textContent, reach(r)]),
    test: reach(buttonNamed(panel, /^Test$/)),
    offer: reach(buttonNamed(panel, /^Open Settings/)),
    importBtn: reach(buttonNamed(d, /^Import my/)),
    sideways: sideways(d),
    page: wider(document.scrollingElement),
  }
})()`

const TESTED = `(() => { ${HELPERS}
  const r = wizard()?.querySelector('.wizard-services .conn-test-result')
  return r ? { text: r.textContent, on: reach(r) } : null
})()`

const PICK = `(() => {
  const input = document.querySelector('.wizard-log-import input[type=file]')
  if (!input) return false
  const dt = new DataTransfer()
  dt.items.add(new File(['<CALL:4>K1AB<QSO_DATE:8>20240101<EOR>'], 'old-log.adi', { type: 'text/plain' }))
  input.files = dt.files
  input.dispatchEvent(new Event('change', { bubbles: true }))
  return true
})()`

const PREVIEW = (control) => `(() => { ${HELPERS}
  const d = document.querySelector('.import-preview-dialog')
  if (!d || !buttonNamed(d, /^Import$/)) return null
  if (${control}) plant(d.querySelector('.import-preview-format'))
  return { inside: inside(d), import: reach(buttonNamed(d, /^Import$/)), cancel: reach(buttonNamed(d, /^Cancel$/)), sideways: sideways(d), imported: window.__probeCalls().includes('importLog') }
})()`

const RESULT = `(() => { ${HELPERS}
  if (document.querySelector('.import-preview-dialog')) return null
  const r = wizard()?.querySelector('.wizard-log-result')
  return r ? { text: r.textContent, on: reach(r) } : null
})()`

const SETTINGS = `(() => { ${HELPERS}
  const s = document.getElementById('settings-confirmations')
  if (!s || document.querySelector('.ui-dialog')) return null
  return { on: reach(s.querySelector('legend')), page: wider(document.scrollingElement) }
})()`

function faults(tag, m, f, what) {
  if (!m) return f.push(`${what}: never on screen`)
  if (m.inside === false) f.push(`${what}: not inside the window`)
  if (m.sideways > 0) f.push(`${what}: scrolls sideways by ${m.sideways} px`)
  if (m.page > 0) f.push(`${what}: the page scrolls sideways by ${m.page} px`)
}

const realSrc = await readFile(REAL_API, 'utf8')
const stubSrc = await readFile(STUB_SRC, 'utf8')
const names = [...realSrc.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1])
const own = new Set([...stubSrc.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1]))
// Every other verb answers `[]` and is counted, so a call that would upload is seen even though it is not stubbed.
const stubCode =
  stubSrc +
  `\nexport * from ${JSON.stringify(REAL_API)}\n` +
  names
    .filter((n) => !own.has(n))
    .map((n) => `export async function ${n}(..._a: unknown[]): Promise<any> { ((window as any).__probeAny ??= []).push(${JSON.stringify(n)}); return [] }`)
    .join('\n')
const swap = {
  name: 'journeys-probe-api',
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

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-journeys-probe-'))
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

async function run([W, H], theme, control) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text)
    return r.result.value
  }
  const until = async (expression, tries = 60) => {
    for (let i = 0; i < tries; i++) {
      await sleep(200)
      const v = await evaluate(expression).catch(() => null)
      if (v) return v
    }
    return null
  }
  const next = async () => { await evaluate(CLICK('/^Next/')); await sleep(300) }
  try {
    await call('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: 1, mobile: false })
    await call('Page.navigate', { url: `${origin}/probe-blank` })
    await sleep(300)
    await evaluate(`localStorage.clear(); localStorage.setItem('tempo-theme', ${JSON.stringify(theme)}); 1`)
    await call('Page.navigate', { url: `${origin}/#operate` })
    if (!(await until(ABOUT))) return { error: 'the wizard never opened on About you' }
    await sleep(800)
    const painted = await evaluate(`document.documentElement.dataset.theme ?? null`)
    const about = await evaluate(ABOUT)
    await evaluate(CLICK('/^Experienced/'))
    await evaluate(CLICK('/^Two or more/'))
    await next() // → Your station
    await next() // → Your rig
    const rig = await until(RIG)
    // The radio found on USB, then the second radio: the radio setup opens by itself.
    await until(CLICK('/Yaesu FTDX-10/'))
    await until(CLICK('/second radio/'))
    const setup = await until(SETUP)
    if (setup) {
      await evaluate(CLICK('/^Cancel$/', `dialogNamed(/Your radios/)`))
      await until(`(() => { ${HELPERS} return !dialogNamed(/Your radios/) || dialogNamed(/Your radios/) === wizard() })()`, 20)
    }
    await next() // → Your log
    const log = await until(LOG(control))
    if (control) {
      await evaluate(PICK)
      const preview = await until(PREVIEW(true))
      return { about, log, preview }
    }
    await evaluate(CLICK('/^Test$/', `wizard()?.querySelector('.wizard-services')`))
    const tested = await until(TESTED)
    await evaluate(CLICK('/^Open Settings/', `wizard()?.querySelector('.wizard-services')`))
    await evaluate(PICK)
    const preview = await until(PREVIEW(false))
    await evaluate(CLICK('/^Import$/', `document.querySelector('.import-preview-dialog')`))
    await until(CLICK('/^Close$/', `document.querySelector('.import-preview-dialog')`))
    const result = await until(RESULT)
    await next() // → Finish
    await evaluate(CLICK('/^Finish/'))
    const settings = await until(SETTINGS)
    const calls = await evaluate(`[...window.__probeCalls(), ...(window.__probeAny ?? [])]`)
    return { painted, about, rig, setup, log, tested, preview, result, settings, calls }
  } finally {
    await browser.call('Target.closeTarget', { targetId })
  }
}

let code = 0
try {
  for (const size of SIZES) {
    for (const theme of THEMES) {
      const tag = `${size[0]}×${size[1]} ${theme}`
      const r = await run(size, theme, false)
      console.log(`journeys-probe: ${tag}: ${JSON.stringify(r)}`)
      if (r.error) {
        console.error(`journeys-probe: at ${tag} ${r.error}`)
        code = 2
        continue
      }
      const red = []
      if (r.painted !== theme) red.push(`the ${r.painted} theme painted, not ${theme}`)
      faults(tag, r.about, red, 'About you')
      if (r.about) {
        if (r.about.answers.length !== 5) red.push(`About you: ${r.about.answers.length} answers, not 3 + 2`)
        for (const [label, on] of r.about.answers) if (!on) red.push(`About you: "${label}" is not reachable`)
        if (!r.about.next) red.push('About you: Next is not reachable')
      }
      faults(tag, r.rig, red, 'Your rig')
      if (r.rig && !r.rig.hint) red.push('Your rig: the "Two or more" hint is not on screen')
      faults(tag, r.setup, red, 'the radio setup the second radio opened')
      if (r.setup && !r.setup.cancel) red.push('the radio setup: Cancel is not reachable')
      faults(tag, r.log, red, 'Your log')
      if (r.log) {
        const names = r.log.rows.map(([n]) => n)
        if (JSON.stringify(names) !== JSON.stringify(['QRZ Logbook', 'LoTW', 'ClubLog', 'eQSL'])) red.push(`the services listed: ${JSON.stringify(names)}`)
        for (const [n, on] of r.log.rows) if (!on) red.push(`the ${n} row is not on screen`)
        if (!r.log.test) red.push("QRZ's Test is not reachable")
        if (!r.log.offer) red.push('the open-Settings offer is not reachable')
        if (!r.log.importBtn) red.push('Import my log is not reachable')
      }
      if (!r.tested?.on || !/reachable/.test(r.tested?.text ?? '')) red.push(`QRZ's Test answer: ${JSON.stringify(r.tested)}`)
      faults(tag, r.preview, red, 'the import preview')
      if (r.preview) {
        if (!r.preview.import || !r.preview.cancel) red.push('the preview: Import or Cancel is not reachable')
        if (r.preview.imported) red.push('the preview: imported before Import was pressed')
      }
      if (!r.result?.on || !/Imported 3/.test(r.result?.text ?? '')) red.push(`the result line: ${JSON.stringify(r.result)}`)
      if (!r.settings?.on) red.push('Settings ▸ Confirmations did not open after Finish')
      const sent = (r.calls ?? []).filter((c) => SENDS.test(c))
      if (sent.length) red.push(`uploaded or stored a login: ${sent.join(', ')}`)
      if (!(r.calls ?? []).includes('importLog') || !(r.calls ?? []).includes('qrzTestConnection')) red.push(`the calls seen: ${JSON.stringify(r.calls)}`)
      if (red.length) {
        console.error(`journeys-probe: RED at ${tag} — ${red.join('; ')}`)
        code = code || 1
      }
      const c = await run(size, theme, true)
      const caught = [c.log?.sideways > 0, c.preview?.sideways > 0]
      console.log(`journeys-probe: CONTROL ${tag}: services ${c.log?.sideways ?? 'not seen'} px, preview ${c.preview?.sideways ?? 'not seen'} px`)
      if (!caught.every(Boolean)) {
        console.error(`journeys-probe: CONTROL FAILED at ${tag} — a planted overflow was not seen; the check cannot fail`)
        code = code || 1
      }
    }
  }
  if (code === 0) console.log(`journeys-probe: PASS — About you with its five answers, the "Two or more" hint and the radio setup it opens, where contacts go with QRZ's Test answered, the import preview with nothing imported before Import, the result line, and Settings ▸ Confirmations after Finish, all on screen and nothing sideways, no upload and no login stored, at ${SIZES.map(([w, h]) => `${w}×${h}`).join(', ')} in ${THEMES.join(' and ')}; each control caught`)
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
