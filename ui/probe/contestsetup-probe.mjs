// The contest setup and its "Build your own contest" steps in real Chrome, for what jsdom cannot see:
// `scripts/browser-probe --desktop contestsetup`.
//
// The probe serves src/probe/contestsetup.tsx (the real `ContestSetupWizard`, the station answered at the
// IPC bridge: every shipped contest on the pick list plus three of the operator's own, one with a long
// name) with the app's own Vite config, in the dark theme at the app's 100 %, at the floor (1366×768 at
// 175 %, a 780×438 window), 1366×768 and 2560×1440 (probe/sizes.mjs). On the pick step and on EVERY step
// of the builder (Basics; Exchange with the RTTY Roundup question, then with five fields; Rules; Check
// with a wide Cabrillo line) it asserts:
//   1. the dialog is inside the window, and nothing in it or the page scrolls sideways;
//   2. Next (Save contest on the last step), Back and Cancel are each on screen and the topmost thing
//      where they stand WITHOUT scrolling the dialog: the step buttons are pinned to its bottom edge,
//      so an operator never hunts for a scrollbar to find Next (the pick list is taller than the dialog
//      on a laptop);
//   3. on the pick step, Build your own… and Import… are reachable (scrolled to), and on Check the
//      sample line is on screen;
//   4. THE KEYBOARD: on the pick step, the setup's exchange and entry steps and the builder's Basics,
//      every enabled control, focused from the top and then scrolled to with block 'nearest', stops
//      clear of the pinned buttons (the dialog's scroll-padding-bottom).
// THE CONTROLS, run at every size: a 2400 px unbreakable line planted in the dialog's heading must be
// seen to overflow; at the floor, the dialog without its scroll padding must leave a focused control
// under the buttons; and at the floor the step buttons un-pinned (`position: static`) must leave Next
// OFF screen on the pick step. If a control is not caught, the check cannot fail and the probe says so.
//
// CONTESTSETUP_PICK_ONLY=1 measures the pick step alone, for a tree with no builder (the two-sided run
// against the base the builder was built on).
//
// exit: 0 every size clear · 1 a red, or a control that could not fail · 2 a page never drew the setup
import { createServer } from 'vite'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'
import { FLOOR, LAPTOP, QHD, metrics } from './sizes.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const SIZES = [FLOOR, LAPTOP, QHD]
const PICK_ONLY = process.env.CONTESTSETUP_PICK_ONLY === '1'
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

const HELPERS = `
  const holds = (o, i) => i.left >= o.left - 0.5 && i.right <= o.right + 0.5 && i.top >= o.top - 0.5 && i.bottom <= o.bottom + 0.5
  // On screen as it stands: inside the window, held by every ancestor that clips, the topmost thing at its centre.
  const seen = (el) => {
    if (!el) return false
    const r = el.getBoundingClientRect()
    if (r.width < 1 || r.height < 1 || !holds({ left: 0, top: 0, right: innerWidth, bottom: innerHeight }, r)) return false
    for (let a = el.parentElement; a; a = a.parentElement) {
      const s = getComputedStyle(a)
      if (/(hidden|auto|scroll|clip)/.test(s.overflowX + s.overflowY) && !holds(a.getBoundingClientRect(), r)) return false
    }
    const t = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2)
    return t != null && (t === el || el.contains(t))
  }
  // Reachable: scrolled to, then on screen.
  const reach = (el) => {
    if (!el) return false
    el.scrollIntoView({ block: 'nearest' })
    return seen(el)
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
  const dlg = () => document.querySelector('.ui-dialog.contest-setup')
  const buttonNamed = (root, re) => [...(root?.querySelectorAll('button') ?? [])].find((b) => re.test(b.textContent.trim()))
  const inputLabelled = (re) => [...(dlg()?.querySelectorAll('label') ?? [])].find((l) => re.test(l.textContent.trim()))?.querySelector('input')
  const type = (el, v) => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, v)
    el.dispatchEvent(new Event('input', { bubbles: true }))
  }
`

/** One step as it stands, the dialog scrolled to its top: the step buttons judged where they are. */
const STEP = (control) => `(() => { ${HELPERS}
  const d = dlg()
  if (!d) return null
  d.scrollTop = 0
  const title = d.querySelector('.wizard-title')
  if (${control}) plant(title)
  const r = d.getBoundingClientRect()
  const go = buttonNamed(d, /^(Next|Save contest)$/)
  const back = buttonNamed(d, /^Back$/)
  const out = {
    title: title?.textContent ?? null,
    inside: r.left >= 0 && r.top >= 0 && r.right <= innerWidth && r.bottom <= innerHeight,
    box: [Math.round(r.left), Math.round(r.top), Math.round(r.width), Math.round(r.height)],
    tall: d.scrollHeight > d.clientHeight,
    page: wider(document.scrollingElement),
    dialog: wider(d),
    go: go?.textContent ?? null,
    goSeen: seen(go),
    cancelSeen: seen(buttonNamed(d, /^Cancel$/)),
    backSeen: back ? seen(back) : null,
    rows: d.querySelectorAll('[data-cc-row]').length,
    roundup: d.querySelector('[role=radiogroup][aria-label^="Use RTTY Roundup"]') != null,
  }
  const sample = d.querySelector('.cc-sample')
  if (sample) out.sampleOn = reach(sample)
  if (d.querySelector('[data-custom-contests]')) {
    out.buildOn = reach(buttonNamed(d, /^Build your own/))
    out.importOn = reach(buttonNamed(d, /^Import/))
    out.mine = d.querySelectorAll('[data-custom-contests] .contest-setup-pick').length
  }
  d.scrollTop = 0
  return out
})()`

/** ⭐ No control hides under the pinned buttons when it gets the keyboard: each enabled control outside
 *  the buttons, focused from the top of the dialog (the browser's own focus scroll), then brought into
 *  view with scrollIntoView({block:'nearest'}), must not overlap the buttons. The dialog's
 *  scroll-padding-bottom is what keeps them apart. Returns the covered ones. */
const FOCUS = `(() => { ${HELPERS}
  const d = dlg()
  if (!d) return null
  const foot = d.querySelector('.wizard-actions')
  const overlap = (el) => {
    const a = el.getBoundingClientRect(), f = foot.getBoundingClientRect()
    return Math.max(0, Math.min(a.bottom, f.bottom) - Math.max(a.top, f.top))
  }
  const controls = [...d.querySelectorAll('button, input, select, textarea')].filter(
    (c) => !foot.contains(c) && !c.disabled && c.getClientRects().length > 0,
  )
  const covered = []
  for (const c of controls) {
    d.scrollTop = 0
    c.focus()
    const byFocus = overlap(c)
    c.scrollIntoView({ block: 'nearest' })
    const byNearest = overlap(c)
    if (byFocus > 0.5 || byNearest > 0.5) covered.push(\`\${(c.textContent || c.getAttribute('aria-label') || c.closest('label')?.textContent || c.tagName).trim().slice(0, 30)} (focus \${Math.round(byFocus)} px, nearest \${Math.round(byNearest)} px)\`)
  }
  document.activeElement?.blur?.()
  d.scrollTop = 0
  return { controls: controls.length, covered }
})()`

const click = (re) => `(() => { ${HELPERS} const b = buttonNamed(dlg(), ${re}); if (!b || b.disabled) return false; b.click(); return true })()`
const FILL_BASICS = `(() => { ${HELPERS}
  const name = inputLabelled(/^Contest name/), token = inputLabelled(/^Cabrillo contest name/)
  if (!name || !token) return false
  type(name, 'County Hunters Weekend Marathon on Every Band and Mode')
  type(token, 'CH-MARATHON-2')
  const ft8 = inputLabelled(/^FT8$/)
  if (ft8 && !ft8.checked) ft8.click()
  return true
})()`

function stepFaults(tag, m) {
  const f = []
  const at = `"${m.title}"`
  if (!m.inside) f.push(`${at}: the setup is not inside the window (${m.box})`)
  if (m.page > 0) f.push(`${at}: the page scrolls sideways by ${m.page} px`)
  if (m.dialog > 0) f.push(`${at}: the setup scrolls sideways by ${m.dialog} px`)
  if (!m.goSeen) f.push(`${at}: ${m.go ?? 'Next'} is not on screen without scrolling`)
  if (!m.cancelSeen) f.push(`${at}: Cancel is not on screen without scrolling`)
  if (m.backSeen === false) f.push(`${at}: Back is not on screen without scrolling`)
  if (m.buildOn === false) f.push(`${at}: Build your own… is not reachable`)
  if (m.importOn === false) f.push(`${at}: Import… is not reachable`)
  if (m.sampleOn === false) f.push(`${at}: the sample Cabrillo line is not reachable`)
  return f.map((s) => `${tag} ${s}`)
}

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-contestsetup-probe-'))
const server = await createServer({
  root: UI,
  configFile: join(UI, 'vite.config.ts'),
  cacheDir,
  logLevel: 'warn',
  server: { host: '127.0.0.1', port: 0, strictPort: false, hmr: false },
})
await server.listen()
const origin = `http://127.0.0.1:${server.httpServer.address().port}`
const browser = await chrome()

/** The setup at `size`: every step measured. `control`: 'wide' plants an overflow on the pick step;
 *  'unpinned' un-pins the step buttons and measures the pick step. */
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
      await sleep(150)
      const v = await evaluate(expression).catch(() => null)
      if (v) return v
    }
    return null
  }
  /** A step once it has stopped moving (the preview answered): the same reading twice. */
  const settled = async (ctl = false) => {
    let last = null
    for (let i = 0; i < 40; i++) {
      await sleep(150)
      const now = await evaluate(STEP(ctl)).catch(() => null)
      if (now && last && JSON.stringify(now) === JSON.stringify(last)) return now
      last = now
    }
    return last
  }
  try {
    await call('Emulation.setDeviceMetricsOverride', metrics(size))
    await call('Page.navigate', { url: `${origin}/probe/contestsetup.html` })
    if (!(await until(`document.querySelectorAll('.ui-dialog.contest-setup .contest-setup-pick').length >= 19`))) return { error: 'the setup never drew' }
    if (!PICK_ONLY && !(await until(`document.querySelectorAll('[data-custom-contests] .contest-setup-pick').length === 3`))) {
      return { error: 'the operator\'s own contests never drew' }
    }
    if (control === 'unpadded') {
      await evaluate(`(() => { const s = document.createElement('style'); s.textContent = '.ui-dialog.contest-setup { scroll-padding-bottom: 0 !important }'; document.head.appendChild(s); return 1 })()`)
      await settled()
      return { focus: [{ step: 'pick', ...(await evaluate(FOCUS)) }] }
    }
    if (control === 'unpinned') {
      await evaluate(`(() => { const s = document.createElement('style'); s.textContent = '.contest-setup .wizard-actions { position: static !important }'; document.head.appendChild(s); return 1 })()`)
    }
    const steps = [{ step: 'pick', ...(await settled(control === 'wide')) }]
    if (control || PICK_ONLY) return { steps }
    // The keyboard on the pick step, and on the shipped setup's exchange and entry steps.
    const focus = [{ step: 'pick', ...(await evaluate(FOCUS)) }]
    for (const name of ['exchange', 'entry']) {
      if (!(await evaluate(click('/^Next$/')))) return { steps, focus, error: `Next refused on the way to the ${name} step` }
      await settled()
      focus.push({ step: name, ...(await evaluate(FOCUS)) })
    }
    await evaluate(click('/^Back$/'))
    await evaluate(click('/^Back$/'))
    await settled()
    if (!(await evaluate(click('/^Build your own/')))) return { steps, error: 'no Build your own… button' }
    if (!(await until(`document.querySelector('.ui-dialog.contest-setup')?.textContent.includes('Name it, and say when it runs')`))) return { steps, error: 'the builder never drew' }
    steps.push({ step: 'basics, empty', ...(await settled()) })
    if (!(await evaluate(FILL_BASICS))) return { steps, error: 'the Basics boxes were not found' }
    steps.push({ step: 'basics', ...(await settled()) })
    focus.push({ step: 'basics', ...(await evaluate(FOCUS)) })
    if (!(await evaluate(click('/^Next$/')))) return { steps, error: 'Next stayed refused on a filled Basics step' }
    steps.push({ step: 'exchange, Roundup asked', ...(await settled()) })
    for (let i = 0; i < 3; i++) await evaluate(click('/^Add a field$/'))
    steps.push({ step: 'exchange, five fields', ...(await settled()) })
    await evaluate(click('/^Next$/'))
    steps.push({ step: 'rules', ...(await settled()) })
    await evaluate(click('/^Next$/'))
    steps.push({ step: 'check', ...(await settled()) })
    return { steps, focus }
  } finally {
    await browser.call('Target.closeTarget', { targetId })
  }
}

let code = 0
try {
  for (const size of SIZES) {
    const tag = size.name
    const r = await run(size, null)
    console.log(`contestsetup-probe: ${tag}: ${JSON.stringify(r)}`)
    if (r.error) {
      console.error(`contestsetup-probe: at ${tag} ${r.error}`)
      code = 2
      continue
    }
    const red = r.steps.flatMap((m) => stepFaults(tag, m))
    if (!PICK_ONLY) {
      const want = ['pick', 'basics, empty', 'basics', 'exchange, Roundup asked', 'exchange, five fields', 'rules', 'check']
      if (r.steps.map((m) => m.step).join() !== want.join()) red.push(`${tag}: steps drawn ${r.steps.map((m) => m.step)}`)
      if (!r.steps.find((m) => m.step === 'exchange, Roundup asked')?.roundup) red.push(`${tag}: the RTTY Roundup question is not asked`)
      if (r.steps.find((m) => m.step === 'exchange, five fields')?.rows !== 5) red.push(`${tag}: five exchange fields were not drawn`)
      if (r.steps.at(-1)?.go !== 'Save contest') red.push(`${tag}: the last step has no Save contest`)
      for (const f of r.focus ?? []) {
        if (!f.controls) red.push(`${tag}: the ${f.step} step's controls were not measured`)
        if (f.covered?.length) red.push(`${tag}: on the ${f.step} step the pinned buttons cover ${f.covered.join(', ')}`)
      }
      if ((r.focus ?? []).length !== 4) red.push(`${tag}: the keyboard was checked on ${(r.focus ?? []).length} steps, not 4`)
    }
    if (red.length) {
      console.error(`contestsetup-probe: RED at ${tag} — ${red.join('; ')}`)
      code = code || 1
    }
    const wide = await run(size, 'wide')
    const caught = wide.steps?.length ? stepFaults(tag, wide.steps[0]).filter((s) => s.includes('sideways')) : []
    console.log(`contestsetup-probe: CONTROL wide ${tag}: ${JSON.stringify(caught)}`)
    if (!caught.length) {
      console.error(`contestsetup-probe: CONTROL FAILED at ${tag} — a planted overflow was not seen; the check cannot fail`)
      code = code || 1
    }
    if (size === FLOOR && !PICK_ONLY) {
      const bare = await run(size, 'unpadded')
      const f = bare.focus?.[0]
      console.log(`contestsetup-probe: CONTROL unpadded ${tag}: ${JSON.stringify(f)}`)
      if (!f?.covered?.length) {
        console.error(`contestsetup-probe: CONTROL FAILED at ${tag} — without the scroll padding no focused control was covered; the focus check cannot fail`)
        code = code || 1
      }
    }
    if (size === FLOOR) {
      const loose = await run(size, 'unpinned')
      const pick = loose.steps?.[0]
      console.log(`contestsetup-probe: CONTROL unpinned ${tag}: ${JSON.stringify(pick)}`)
      if (!pick || !pick.tall || pick.goSeen) {
        console.error(`contestsetup-probe: CONTROL FAILED at ${tag} — with the step buttons un-pinned, Next was ${pick?.goSeen ? 'still on screen' : 'not measured'}; the pin check cannot fail`)
        code = code || 1
      }
    }
  }
  if (code === 0) {
    console.log(
      `contestsetup-probe: PASS — ${PICK_ONLY ? 'the pick step' : 'the pick step and every builder step'} inside the window with Next${PICK_ONLY ? '' : '/Save contest'}, Back and Cancel on screen without scrolling and nothing sideways, at ${SIZES.map((s) => s.name).join(', ')}; each control caught`,
    )
  }
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
