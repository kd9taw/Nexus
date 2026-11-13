// The left rail and the first-run wizard on laptop screens, in real Chrome: `scripts/browser-probe --desktop laptopfit`.
//
// WHY. On a laptop the rail drew about half its sections and hid the rest below a 4 px scrollbar, with
// nothing to say they were there: at 1366×768 a fresh install showed 11 of 23, and at 1920×1080 Stats,
// Contest, POTA/SOTA, Memories and Repeaters sat below the fold. And the first-run wizard's Back / Next
// sat at the foot of a box taller than the window, so on the radio step a new operator had to find a
// scrollbar to go on, while the step dots ran off the box in the longer languages.
//
// The probe serves the REAL app (index.html, main.tsx, App) with the app's own Vite config and src/api.ts
// swapped for probe/firstrun-api.ts (a fresh install: every section on, as shipped), over the supported
// screens (1366×768 to 3840×2160) at the scalings people run them at. Each screen is the window a
// maximised Nexus gets there: the screen in logical px, less the taskbar (48) and the title bar (31),
// at that device pixel ratio; the app's own auto-fit then picks its zoom, as it does on the station.
//   RAIL, at every size: every section is reachable (scrolled to, it is the topmost thing at its centre),
//     the bottom cluster (mode badge, Settings) is on screen, and EITHER every section is in view at rest
//     OR the "more below" cue is on screen and reachable — and pressing it brings the next hidden section
//     into view. Scrolled to the end, the cue is gone. (A section list wider than the rail is reported as a NOTE: it predates this probe.)
//   WIZARD, at every size in English (and in German and Russian at 1366×768 at 100 and 175 % and 1920×1080): on every step the dialog is inside the
//     window, every step dot is inside the dialog (none cut or run past its edge), nothing scrolls
//     sideways, and the step's go-on button (Next, or Finish) is reachable WITHOUT scrolling the dialog —
//     at the top of the step and at its foot.
// THE CONTROLS, run every time: the rail with the cue taken away (a size whose rail overflows must then
// go red), and the wizard with its step buttons put back in the flow (a step taller than the box must
// then hide Next). If neither control can fail, the probe says so instead of passing.
//
// exit: 0 every size clear · 1 a red, or a control that could not fail · 2 a page never drew
import { createServer } from 'vite'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'
import { at } from './sizes.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const REAL_API = join(UI, 'src', 'api.ts')
const STUB_SRC = join(UI, 'probe', 'firstrun-api.ts')
const STUB_API = join(UI, 'probe', '__laptopfit-api.ts')
// [screen w, screen h, scaling]. The supported screens and the scalings their owners run; each window is
// probe/sizes.mjs's `at()` for that screen (the CSS window of the screen at that scaling, floored as a real one is), less
// the taskbar and the title bar of a maximised window.
const SCREENS = [
  [1366, 768, 1], [1366, 768, 1.25], [1366, 768, 1.5], [1366, 768, 1.75],
  [1920, 1080, 1], [1920, 1080, 1.5],
  [2560, 1440, 1.5], [2560, 1440, 2],
  [3840, 2160, 1.5], [3840, 2160, 2],
]
const TASKBAR = 48
const TITLE = 31
const windowOf = ([W, H, s]) => { const z = at(W, H, Math.round(s * 100)); return { w: z.w, h: z.h - TASKBAR - TITLE, s: z.scale } }
const tagOf = ([W, H, s]) => `${W}×${H}@${Math.round(s * 100)}%`
const LOCALES = ['en', 'de', 'ru']
// The longer languages are measured where the wizard is tightest and at 1080p; English everywhere.
const LOCALE_SCREENS = new Set(['1366×768@100%', '1366×768@175%', '1920×1080@100%'])
// The controls. NO_CUE: the cue taken away. FLOW: the wizard's step buttons back in the flow.
const NO_CUE = '.mode-nav-more{display:none!important}'
const FLOW = '.ui-dialog .wizard-actions{position:static!important}'
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

const HELPERS = `
  const reach = (el) => {
    if (!el) return false
    el.scrollIntoView({ block: 'nearest' })
    return seen(el)
  }
  // On screen where it stands, now, with nothing over its centre: no scrolling.
  const seen = (el) => {
    if (!el) return false
    const q = el.getBoundingClientRect()
    if (q.width < 1 || q.height < 1 || q.top < -0.5 || q.left < -0.5 || q.bottom > innerHeight + 0.5 || q.right > innerWidth + 0.5) return false
    const t = document.elementFromPoint(q.left + q.width / 2, q.top + q.height / 2)
    return t != null && (t === el || el.contains(t))
  }
  const within = (el, box) => {
    const q = el.getBoundingClientRect(), b = box.getBoundingClientRect()
    return q.top >= b.top - 0.5 && q.bottom <= b.bottom + 0.5 && q.left >= b.left - 0.5 && q.right <= b.right + 0.5
  }
  const label = (b) => (b.querySelector('.mode-label')?.textContent ?? b.getAttribute('aria-label') ?? '').trim()
`

const RAIL = `(() => { ${HELPERS}
  const nav = document.querySelector('.mode-nav'), top = document.querySelector('.mode-nav-top')
  if (!nav || !top) return null
  top.scrollTop = 0
  const btns = [...top.querySelectorAll('button.mode-btn')]
  const atRest = btns.filter((b) => within(b, top) && seen(b))
  const hidden = btns.filter((b) => !atRest.includes(b)).map(label)
  const cue = document.querySelector('.mode-nav-more')
  const cueSeen = seen(cue)
  const bottom = [...document.querySelectorAll('.mode-nav-bottom > *')].map((e) => [e.textContent.trim(), seen(e)])
  const r = top.getBoundingClientRect()
  return {
    zoom: parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--ui-zoom')) || 1,
    viewport: document.documentElement.getAttribute('data-viewport'),
    total: btns.length, atRest: atRest.length, hidden, cueSeen,
    // CSS px of the window the scroller takes, and the 'more' it holds.
    scroller: [Math.round(r.height), Math.round(top.scrollHeight - top.clientHeight)],
    btnH: Math.round(Math.max(...btns.map((b) => b.getBoundingClientRect().height))),
    bottom, sideways: Math.max(nav.scrollWidth - nav.clientWidth, top.scrollWidth - top.clientWidth),
    // What runs past the list's right edge, when something does.
    wide: [...top.querySelectorAll('*')].filter((e) => e.getBoundingClientRect().right > r.left + top.clientLeft + top.clientWidth + 0.5)
      .map((e) => e.className + ':' + e.textContent.trim().slice(0, 12)).slice(0, 4),
  }
})()`

const REACH_ALL = `(async () => { ${HELPERS}
  const top = document.querySelector('.mode-nav-top')
  const out = [...top.querySelectorAll('button.mode-btn')].filter((b) => !reach(b)).map(label)
  // The scroll event (and so the cue's count) lands on the next frame: wait for it at each end.
  top.scrollTop = top.scrollHeight
  await new Promise((r) => setTimeout(r, 400))
  const endCue = seen(document.querySelector('.mode-nav-more'))
  top.scrollTop = 0
  await new Promise((r) => setTimeout(r, 400))
  return { unreachable: out, endCue }
})()`

// Press the cue, then: did a section that was hidden at rest come into view?
const PRESS_CUE = `(async () => { ${HELPERS}
  const top = document.querySelector('.mode-nav-top')
  top.scrollTop = 0
  const btns = [...top.querySelectorAll('button.mode-btn')]
  const firstHidden = btns.find((b) => !(within(b, top) && seen(b)))
  document.querySelector('.mode-nav-more')?.click()
  await new Promise((r) => setTimeout(r, 900))
  const ok = !!firstHidden && within(firstHidden, top) && seen(firstHidden)
  top.scrollTop = 0
  await new Promise((r) => setTimeout(r, 400))
  return ok
})()`

const WIZ = (where) => `(() => { ${HELPERS}
  const d = document.querySelector('.ui-dialog')
  if (!d) return null
  d.scrollTop = ${where === 'foot' ? 'd.scrollHeight' : '0'}
  const go = [...d.querySelectorAll('.wizard-actions .wizard-go')][0]
  const dots = [...d.querySelectorAll('.wizard-dot')]
  const r = d.getBoundingClientRect()
  const sideways = Math.max(d.scrollWidth - d.clientWidth, ...[...d.querySelectorAll('*:not(.sr-only)')].map((e) => (e.scrollWidth > e.clientWidth + 1 && getComputedStyle(e).overflowX !== 'visible' ? e.scrollWidth - e.clientWidth : 0)))
  const res = {
    step: d.querySelector('.wizard-dot.cur')?.textContent.trim() ?? '?',
    inside: r.left >= -0.5 && r.top >= -0.5 && r.right <= innerWidth + 0.5 && r.bottom <= innerHeight + 0.5,
    box: [Math.round(r.width), Math.round(r.height), Math.round(d.scrollHeight)],
    go: go ? [go.textContent.trim(), seen(go)] : null,
    dotsOut: dots.filter((x) => !within(x, d)).map((x) => x.textContent.trim()),
    dotRows: new Set(dots.map((x) => Math.round(x.getBoundingClientRect().top))).size,
    sideways,
  }
  d.scrollTop = 0
  return res
})()`

function railFaults(m, reachAll, pressed) {
  const f = []
  if (m.total < 20) f.push(`only ${m.total} sections drawn (expected every shipped one)`)
  for (const [t, on] of m.bottom) if (!on) f.push(`"${t}" at the rail's foot is not on screen`)
  if (reachAll.unreachable.length) f.push(`unreachable: ${reachAll.unreachable.join(', ')}`)
  if (m.hidden.length && !m.cueSeen) f.push(`${m.hidden.length} sections hidden with no cue (${m.hidden.join(', ')})`)
  if (m.hidden.length && m.cueSeen && !pressed) f.push('pressing the cue did not bring a hidden section into view')
  if (reachAll.endCue) f.push('the cue still shows with the list scrolled to its end')
  if (!m.hidden.length && m.cueSeen) f.push('the cue shows with nothing hidden')
  return f
}
function wizFaults(m, where) {
  const f = []
  if (!m.inside) f.push(`${m.step}: the wizard is not inside the window (${m.box})`)
  if (!m.go) f.push(`${m.step}: no Next/Finish`)
  else if (!m.go[1]) f.push(`${m.step}: ${m.go[0]} is not on screen at the step's ${where} without scrolling (box ${m.box})`)
  if (where === 'top' && m.dotsOut.length) f.push(`${m.step}: step dots cut (${m.dotsOut.join(', ')})`)
  if (m.sideways > 0) f.push(`${m.step}: scrolls sideways by ${m.sideways} px`)
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
  name: 'laptopfit-probe-api',
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

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-laptopfit-probe-'))
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

async function page(win) {
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
  await call('Emulation.setDeviceMetricsOverride', { width: win.w, height: win.h, deviceScaleFactor: win.s, mobile: false })
  await call('Page.navigate', { url: `${origin}/probe-blank` })
  await sleep(300)
  const css = (text) => evaluate(`(() => { const s = document.createElement('style'); s.textContent = ${JSON.stringify(text)}; document.head.appendChild(s); return 1 })()`)
  return { call, evaluate, until, css, close: () => browser.call('Target.closeTarget', { targetId }) }
}

async function rail(screen, control) {
  const p = await page(windowOf(screen))
  try {
    await p.evaluate(`localStorage.clear(); localStorage.setItem('tempo-theme', 'dark'); localStorage.setItem('probe.seen', '1'); localStorage.setItem('probe.level', 'experienced'); 1`)
    await p.call('Page.navigate', { url: `${origin}/#operate` })
    if (!(await p.until(`!!document.querySelector('.mode-nav-top button.mode-btn') && !!document.documentElement.getAttribute('data-viewport')`, 100))) return { error: 'the rail never drew' }
    await sleep(800)
    // Anything that opened over the app on a fresh profile (a guide, a walk) is closed first.
    for (let i = 0; i < 3; i++) {
      const open = await p.evaluate(`!!document.querySelector('.ui-dialog')`)
      if (!open) break
      await p.evaluate(`(() => { const d = document.querySelector('.gsg-dialog') ?? document.querySelector('.ui-dialog'); d.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); return 1 })()`)
      await sleep(400)
    }
    if (control) await p.css(NO_CUE)
    await sleep(200)
    const m = await p.evaluate(RAIL)
    const reachAll = await p.evaluate(REACH_ALL)
    const pressed = m.cueSeen ? await p.evaluate(PRESS_CUE) : null
    return { m, reachAll, pressed }
  } finally {
    await p.close()
  }
}

async function wizard(screen, locale, control) {
  const p = await page(windowOf(screen))
  try {
    await p.evaluate(`localStorage.clear(); localStorage.setItem('tempo-theme', 'dark'); localStorage.setItem('nexus.locale', '${locale}'); 1`)
    await p.call('Page.navigate', { url: `${origin}/#operate` })
    if (!(await p.until(`!!document.querySelector('.ui-dialog .wizard-dots')`, 100))) return { error: 'the wizard never opened' }
    await sleep(800)
    if (control) await p.css(FLOW)
    const steps = []
    // About you, Station, Radio, Log, Finish: measured at the top and at the foot of each step.
    for (let i = 0; i < 5; i++) {
      await sleep(250)
      steps.push([await p.evaluate(WIZ('top')), await p.evaluate(WIZ('foot'))])
      if (i === 4) break
      const moved = await p.evaluate(`(() => { const d = document.querySelector('.ui-dialog'); const b = d?.querySelector('.wizard-actions .wizard-go'); if (!b || b.disabled) return false; b.click(); return true })()`)
      if (!moved) break
    }
    return { steps }
  } finally {
    await p.close()
  }
}

let code = 0
let cueControl = false
let flowControl = false
const MODE = process.env.LAPTOPFIT_ONLY ?? ''
try {
  const done = new Set()
  for (const screen of SCREENS) {
    const tag = tagOf(screen)
    const win = windowOf(screen)
    // Two screens that give the same window lay out the same (1920×1080@150 % and 2560×1440@200 %, and
    // 1920×1080@100 % and 3840×2160@200 %): the first is measured, the second named beside it.
    const key = `${win.w}×${win.h}`
    if (done.has(key)) { console.log(`laptopfit-probe: ${tag} gives the ${key} window measured above`); continue }
    done.add(key)
    if (MODE !== 'wizard') {
      const r = await rail(screen, false)
      if (r.error) { console.error(`laptopfit-probe: ${tag}: ${r.error}`); code = 2; continue }
      const { m } = r
      console.log(`laptopfit-probe: RAIL ${tag} (window ${win.w}×${win.h}, zoom ${m.zoom}, ${m.viewport}): ${m.atRest} of ${m.total} in view at rest, scroller ${m.scroller[0]} px holding ${m.scroller[1]} px more, buttons ${m.btnH} px, cue ${m.cueSeen ? 'shown' : 'none'}${m.hidden.length ? `; hidden: ${m.hidden.join(', ')}` : ''}`)
      // Reported, not judged: a label a few px wider than its button predates this probe (origin/main).
      if (m.sideways > 0) console.log(`laptopfit-probe: NOTE ${tag}: the section list is ${m.sideways} px wider than the rail (${m.wide.join(', ')})`)
      const red = railFaults(m, r.reachAll, r.pressed)
      if (red.length) { console.error(`laptopfit-probe: RED rail at ${tag} — ${red.join('; ')}`); code = code || 1 }
      if (m.hidden.length && !cueControl) {
        const c = await rail(screen, true)
        if (!c.error && railFaults(c.m, c.reachAll, c.pressed).some((s) => s.includes('no cue'))) cueControl = true
      }
    }
    if (MODE !== 'rail') {
      for (const loc of LOCALE_SCREENS.has(tag) ? LOCALES : ['en']) {
        const r = await wizard(screen, loc, false)
        if (r.error) { console.error(`laptopfit-probe: ${tag} ${loc}: ${r.error}`); code = 2; continue }
        const red = []
        for (const [top, foot] of r.steps) red.push(...wizFaults(top, 'top'), ...wizFaults(foot, 'foot'))
        console.log(`laptopfit-probe: WIZARD ${tag} ${loc}: ${r.steps.map(([t]) => `${t.step} ${t.box[1]}/${t.box[2]} px${t.go?.[1] ? '' : ' go-hidden'} dots ${t.dotRows} row(s)`).join(' · ')}`)
        if (red.length) { console.error(`laptopfit-probe: RED wizard at ${tag} ${loc} — ${[...new Set(red)].join('; ')}`); code = code || 1 }
        if (!flowControl && loc === 'en') {
          const c = await wizard(screen, loc, true)
          if (!c.error && c.steps.some(([t, f]) => wizFaults(t, 'top').some((s) => s.includes('without scrolling')) || wizFaults(f, 'foot').some((s) => s.includes('without scrolling')))) flowControl = true
        }
      }
    }
  }
  if (MODE !== 'wizard' && !cueControl) { console.error('laptopfit-probe: CONTROL FAILED — with the cue taken away no size went red; the rail check cannot fail'); code = code || 1 }
  if (MODE !== 'rail' && !flowControl) { console.error('laptopfit-probe: CONTROL FAILED — with the step buttons in the flow no step hid Next; the wizard check cannot fail'); code = code || 1 }
  if (code === 0) console.log(`laptopfit-probe: PASS — every rail section reachable, the hidden ones announced by a cue that brings them into view, and the wizard's Next/Finish and step dots on screen on every step in en/de/ru, at ${SCREENS.map(tagOf).join(', ')}; each control caught`)
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
