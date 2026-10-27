// @vitest-environment jsdom
//
// THE CONTEST LOGGER WINDOW PUTS THE ENTRY FIRST, AND LARGE (the operator's layout, 2026-10-11):
// "The call and exchange boxes go to the top in large text, with the dupe warning and the last
// contacts right under them. An A-/A+ text size control is remembered on that computer. The Contest
// screen (operator, band board, score) scrolls below."
//
// Mounted as the real `?panel=contestlog` window (`DetachedPanel`) and answered at the IPC bridge, the
// idiom of ContestLogger.txfree.test.tsx: an Illinois QSO Party running on 20 m phone, six contacts in
// the log, and the shared entry holding K9ABC / 59 / COOK — a call already in the log on 20 m PH, so
// the strip's own dupe verdict shows.
//
// THE SIZES ARE COMPUTED, NOT LOOKED FOR. jsdom lays nothing out and substitutes no var(), so the font
// size an element gets is walked here through the real sheets, as
// components/connect/PaneFrame.textScale.test.tsx walks a Connect box's: cssCascade.ts's selector
// matching, CSS's own inheritance of custom properties (a token arrives computed; one an element
// declares, in the sheet or inline, resolves on that element), and every length evaluated. Where the
// boxes land on the screen is a real browser's to say, not this file's.
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen, within } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { DetachedPanel } from './DetachedPanel'
import { LAST_CONTACTS, loggerTextScale } from './components/ContestLogger'
import { cmpSpec, elOf, expandWith, parseRules, reachesChain, rootTokensFrom, type El, type Rule } from './cssCascade'
import { t } from './i18n'
import defaultSettings from './components/__fixtures__/defaultSettings.json'

// ── the station ──────────────────────────────────────────────────────────────────────────────

const T0 = 1_792_342_800
const row = (mins: number, call: string, qth: string) => ({
  call,
  class: '',
  section: '',
  band: '20m',
  mode: 'PH',
  submode: 'USB',
  whenUnix: T0 - mins * 60,
  rcvd: ['59', qth],
})
/** Oldest first, as the engine keeps it. K9ABC is the second contact. */
const LOG = [
  row(41, 'W9XYZ', 'LAKE'),
  row(37, 'K9ABC', 'COOK'),
  row(29, 'K9AAA', 'KANE'),
  row(18, 'N9AAA', 'DUPG'),
  row(9, 'W0AAA', 'MO'),
  row(2, 'K9DEF', 'WILL'),
]
const snapshot = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  mode: 'FieldDay',
  radio: { dialMhz: 14.25, band: '20m', sideband: 'USB', operatingMode: 'phone', txEnabled: true, txAllowed: true },
  link: { tier: 'Ft8' },
  stations: [],
  conversations: [],
  activePeer: null,
  hunt: null,
  fieldDay: {
    running: true,
    state: 'Running',
    qsoCount: LOG.length,
    sections: 0,
    points: LOG.length * 2,
    multCount: 5,
    event: 'ilqp',
    role: 'in_state',
    log: LOG,
    receives: [
      { key: 'RST', kind: 'rst', required: true },
      { key: 'QTH', kind: 'oneOf', required: true, domains: ['il_counties', 'il_mults'] },
    ],
    composing: [
      { key: 'RST', raw: '59' },
      { key: 'QTH', raw: 'KANE', domain: 'il_counties' },
    ],
    composingText: '59 KANE',
    assistanceOn: [],
    upload: { enabled: false, destinations: [], available: [], hint: '' },
    eventStartUnix: T0 - 3600,
    eventEndUnix: T0 + 3600,
  },
  // The contact in progress, as the main window's strip put it to the engine.
  contestEntry: { rev: 2, call: 'K9ABC', fields: { RST: '59', QTH: 'COOK' }, marks: null },
}

let rev = 2
function respond(cmd: string): unknown {
  if (cmd === 'get_snapshot') return snapshot
  if (cmd === 'get_settings') return { ...defaultSettings, fdActive: true, fdEvent: 'ilqp' }
  if (cmd === 'contest_entry_put') return ++rev
  if (cmd === 'contest_removed' || cmd === 'contest_unmerged') return []
  if (/^(get_band_plan|get_need_alerts|get_scp_calls)$/.test(cmd)) return []
  if (/^(get_propagation|get_fd_ruleset|resolve_entity|contest_place|get_call_history)$/.test(cmd)) return null
  return {}
}

/** This window's own storage key: per surface, and the surface is `?panel=contestlog`. */
const OWN_KEY = 'nexus.contestLogger.textScale.contestlog'

beforeEach(() => {
  localStorage.clear()
  rev = 2
  // The window's own address, which is what scopes its storage to it.
  window.history.replaceState(null, '', '/?panel=contestlog')
  window.__TAURI_INTERNALS__ = { invoke: async <T,>(cmd: string): Promise<T> => respond(cmd) as T }
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(() => {
  cleanup()
  delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
  window.history.replaceState(null, '', '/')
})

async function mountLogger(): Promise<void> {
  render(<DetachedPanel panel="contestlog" />)
  // The first snapshot poll lands after 300 ms; the shared entry in the Call box is the sign it did.
  await act(async () => new Promise((r) => setTimeout(r, 400)))
  await act(async () => new Promise((r) => setTimeout(r, 0)))
}

const callBox = () => screen.getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' }) as HTMLInputElement
const sizeLine = () => document.querySelector('.contest-logger-size')!.textContent
const larger = () => screen.getByRole('button', { name: t('contestLogger.size.larger') }) as HTMLButtonElement
const smaller = () => screen.getByRole('button', { name: t('contestLogger.size.smaller') }) as HTMLButtonElement
const newestRow = () => document.querySelector('.contest-logger-recent .fd-log-row')!

// ── the font size the real sheets give an element ────────────────────────────────────────────

const read = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES: Rule[] = parseRules(read('styles.css') + '\n' + read('cockpit-panes.css')).filter((r) =>
  r.decls.some((d) => d.prop === 'font-size' || d.prop === 'font' || d.prop.startsWith('--')),
)
const ROOT = rootTokensFrom(RULES, 'dark')

const important = (v: string) => /!\s*important\s*$/.test(v)
const bare = (v: string) => v.replace(/!\s*important\s*$/, '').trim()
/** Does `a` beat `b` in the cascade? Importance, then specificity, then source order. */
const beats = (a: { rule: Rule; value: string }, b: { rule: Rule; value: string }) =>
  important(a.value) !== important(b.value)
    ? important(a.value)
    : cmpSpec(a.rule.spec, b.rule.spec) > 0 || (cmpSpec(a.rule.spec, b.rule.spec) === 0 && a.rule.order > b.rule.order)

/** Evaluate a resolved length to px: calc/min/max/clamp over px, unitless, rem, em and %. */
function evalPx(src: string, em: number): number {
  let i = 0
  const s = src.trim()
  const ws = () => {
    while (i < s.length && /\s/.test(s[i])) i++
  }
  const expr = (): number => {
    let v = term()
    for (ws(); i < s.length && (s[i] === '+' || s[i] === '-'); ws()) {
      const op = s[i++]
      const r = term()
      v = op === '+' ? v + r : v - r
    }
    return v
  }
  const term = (): number => {
    let v = factor()
    for (ws(); i < s.length && (s[i] === '*' || s[i] === '/'); ws()) {
      const op = s[i++]
      const r = factor()
      v = op === '*' ? v * r : v / r
    }
    return v
  }
  const args = (): number[] => {
    const out: number[] = []
    ws()
    if (s[i] !== '(') throw new Error(`expected ( in ${s}`)
    i++
    for (;;) {
      out.push(expr())
      ws()
      if (s[i] === ',') {
        i++
        continue
      }
      if (s[i] === ')') {
        i++
        return out
      }
      throw new Error(`bad argument list in ${s}`)
    }
  }
  const factor = (): number => {
    ws()
    const fn = /^(calc|min|max|clamp)\b/.exec(s.slice(i))
    if (fn) {
      i += fn[1].length
      const a = args()
      if (fn[1] === 'calc') return a[0]
      if (fn[1] === 'min') return Math.min(...a)
      if (fn[1] === 'max') return Math.max(...a)
      return Math.min(Math.max(a[0], a[1]), a[2])
    }
    if (s[i] === '(') {
      i++
      const v = expr()
      ws()
      i++
      return v
    }
    const num = /^(-?\d*\.?\d+)(px|rem|em|%)?/.exec(s.slice(i))
    if (!num) throw new Error(`cannot read "${s.slice(i)}" in ${s}`)
    i += num[0].length
    const n = Number(num[1])
    if (num[2] === 'rem') return n * 16
    if (num[2] === 'em') return n * em
    if (num[2] === '%') return (n * em) / 100
    return n
  }
  const v = expr()
  ws()
  if (i !== s.length) throw new Error(`trailing "${s.slice(i)}" in ${s}`)
  return v
}

/** The font size, in px, the real sheets give `node` at the app's Normal text size: CSS's own
 *  inheritance walked from <body> down to it. A custom property an element declares (its sheet
 *  winner, or its inline style) resolves against that element's own tokens and arrives computed in
 *  its children; font-size is the winning declaration evaluated against the parent's size, and
 *  inherits when nothing declares it or what it declares does not compute. */
function fontPx(node: Element): number {
  const path: Element[] = []
  for (let n: Element | null = node; n && n !== document.documentElement; n = n.parentElement) path.unshift(n)
  let tokens = new Map([...ROOT].map(([k, v]) => [k, expandWith(ROOT, v)]))
  let px = 16 // html { font-size: calc(100% * var(--text-scale)) } over the UA's 16 px, at Normal
  const chain: El[] = []
  for (const el of path) {
    chain.push(elOf(el))
    const cands = RULES.filter((r) => reachesChain(r.selector, chain, 'dark'))
    const declared = new Map<string, string>()
    const winners = new Map<string, { rule: Rule; value: string }>()
    for (const rule of cands) {
      for (const d of rule.decls) {
        if (!d.prop.startsWith('--')) continue
        const prev = winners.get(d.prop)
        if (!prev || beats({ rule, value: d.value }, prev)) winners.set(d.prop, { rule, value: d.value })
      }
    }
    for (const [k, w] of winners) declared.set(k, bare(w.value))
    const style = (el as HTMLElement).style
    for (let i = 0; i < style.length; i++) {
      if (style[i].startsWith('--')) declared.set(style[i], style.getPropertyValue(style[i]))
    }
    if (declared.size) {
      const own = new Map(tokens)
      for (const [k, v] of declared) own.set(k, v)
      const next = new Map(tokens)
      for (const k of declared.keys()) next.set(k, expandWith(own, own.get(k)!))
      tokens = next
    }
    let win: { rule: Rule; value: string } | null = null
    for (const rule of cands) {
      const d = rule.decls.filter((x) => x.prop === 'font-size' || x.prop === 'font').pop()
      if (d && (!win || beats({ rule, value: d.value }, win))) win = { rule, value: d.value }
    }
    const value = win ? bare(win.value) : null
    if (value && value !== 'inherit' && value !== 'unset') {
      try {
        const n = evalPx(expandWith(tokens, value), px)
        if (Number.isFinite(n) && n > 0) px = n
      } catch {
        // invalid at computed-value time: font-size behaves as `unset`, i.e. inherits
      }
    }
  }
  return px
}

// ── the window ───────────────────────────────────────────────────────────────────────────────

describe('the contest logger window puts the entry first', () => {
  it('the boxes, the dupe warning right under them, the newest contacts, then the contest screen', async () => {
    await mountLogger()
    const app = document.querySelector('.app.detached.contest-logger')!
    const parts = Array.from(app.children).filter(
      (c) => !c.classList.contains('ui-toast-viewport') && !c.querySelector('.ui-toast-viewport'),
    )
    const tagged = (el: Element) => `${el.tagName.toLowerCase()}.${[...el.classList].join('.')}`
    expect(parts.map(tagged)).toEqual([
      'section.contest-logger-entry',
      'section.contest-logger-recent',
      // The divider that sizes the last contacts against the screen under them.
      'div.pane-splitter.horizontal.contest-seam',
      'main.layout.single',
    ])
    const [entry, recent, , contestScreen] = parts as HTMLElement[]
    // The entry holds the contact in progress, the main window's, in its boxes.
    expect(within(entry).getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' })).toBe(callBox())
    expect([...entry.querySelectorAll<HTMLInputElement>('.le-fd-big input')].map((b) => b.value)).toEqual([
      'K9ABC',
      '59',
      'COOK',
    ])
    // …under the captions the log line gives them: the county box says it is the county box.
    // The Call caption's own words, beside the DUPE badge its line carries.
    expect(
      [...entry.querySelectorAll('.le-fd-big .le-fd-cap')].map(
        (c) => (c.querySelector('.le-fd-cap-text') ?? c).textContent,
      ),
    ).toEqual([t('logEntry.fd.call.label'), 'RST', 'County / State'])
    // …and the badge shows in this window too: K9ABC is a dupe here, worked at 16:23 on 20 m USB.
    expect(entry.querySelector('.le-fd-field-call .le-fd-dupe')!.textContent).toBe('DUPE 16:23 20m USB')
    // The dupe warning, as the log line computes it, right under the boxes: after them in the entry,
    // so before the last contacts.
    const dupe = within(entry).getByRole('alert')
    expect(dupe.textContent).toBe(t('logEntry.fd.dupe.own', { call: 'K9ABC', band: '20m', mode: 'PH' }))
    expect(entry.querySelector('.le-fd-big')!.compareDocumentPosition(dupe) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    // The last contacts: the newest ten (here all six), newest first, under the contest screen's own columns.
    expect(LAST_CONTACTS).toBe(10)
    expect([...recent.querySelectorAll('.fd-log-row .fd-col.call')].map((c) => c.textContent)).toEqual([
      'K9DEF',
      'W0AAA',
      'N9AAA',
      'K9AAA',
      'K9ABC',
      'W9XYZ',
    ])
    expect([...recent.querySelectorAll('.fd-log-row .fd-col.slot')].slice(0, 2).map((c) => c.textContent)).toEqual([
      '59',
      'WILL',
    ])
    expect([...recent.querySelectorAll('.fd-log-head .fd-col')].map((c) => c.textContent)).toEqual([
      t('fieldDay.log.column.time'),
      t('fieldDay.log.column.call'),
      'RST',
      'County / State',
      t('fieldDay.log.column.band'),
      t('fieldDay.log.column.mode'),
    ])
    expect(within(recent).getByRole('heading').textContent).toBe(t('contestLogger.recent.title'))
    // Control: the contest screen below still lists the whole log, oldest first, as it always did.
    expect([...contestScreen.querySelectorAll('.fd-log-row .fd-col.call')].map((c) => c.textContent)).toEqual(
      LOG.map((q) => q.call),
    )
  })
})

describe('A− / A+ sizes the entry and the last contacts', () => {
  it('large at 100 %, and every press moves the boxes and the rows by its factor, computed', async () => {
    await mountLogger()
    expect(sizeLine()).toBe(t('contestLogger.size.label', { pct: 100 }))
    // At 100 % the boxes are read from a chair: 40 px against the app's 14 px input…
    expect(fontPx(callBox())).toBe(40)
    expect(fontPx(newestRow())).toBe(20)
    // …and outside this window the same box keeps the entry's own 30 px: the main window's log line
    // is untouched by the window's scale. (It read 14 here while `.settings-input` out-cascaded the
    // entry's own size in the main window; styles-contest-entry-size.test.tsx holds that down.)
    const nest = (parent: Element, tag: string, className: string) =>
      parent.appendChild(Object.assign(document.createElement(tag), { className }))
    const strip = nest(document.body, 'div', 'log-entry log-entry-fd')
    const field = nest(nest(strip, 'div', 'le-fd-big'), 'label', 'le-fd-field le-fd-field-call')
    const mainBox = nest(field, 'input', 'settings-input mono le-fd-input le-fd-input-call')
    expect(fontPx(mainBox), "the main window keeps the entry's own size").toBe(30)
    strip.remove()

    fireEvent.click(larger())
    expect(sizeLine()).toBe(t('contestLogger.size.label', { pct: 110 }))
    expect(fontPx(callBox())).toBeCloseTo(44, 6)
    expect(fontPx(newestRow())).toBeCloseTo(22, 6)
    // The bar's own buttons stay put under the finger pressing them.
    expect(fontPx(larger())).toBe(14)

    // To the top of the range, where A+ stops…
    for (let i = 0; i < 5; i++) fireEvent.click(larger())
    expect(sizeLine()).toBe(t('contestLogger.size.label', { pct: 160 }))
    expect(larger().disabled).toBe(true)
    expect(fontPx(callBox())).toBeCloseTo(64, 6)
    // …and down to the bottom, where A− does.
    for (let i = 0; i < 8; i++) fireEvent.click(smaller())
    expect(sizeLine()).toBe(t('contestLogger.size.label', { pct: 80 }))
    expect(smaller().disabled).toBe(true)
    expect(fontPx(callBox())).toBeCloseTo(32, 6)
    expect(fontPx(newestRow())).toBeCloseTo(16, 6)
  })

  it('a press takes no focus, so the caret stays in the box it was in', async () => {
    await mountLogger()
    callBox().focus()
    expect(fireEvent.mouseDown(larger()), 'the press is held from taking focus').toBe(false)
    expect(document.activeElement).toBe(callBox())
  })

  it('is remembered on this computer for this window, and clamped as it is read back', async () => {
    await mountLogger()
    fireEvent.click(larger())
    fireEvent.click(larger())
    expect(localStorage.getItem(OWN_KEY)).toBe('1.2')
    expect(localStorage.getItem('nexus.contestLogger.textScale'), "the main window's key is not this window's").toBeNull()
    cleanup()
    await mountLogger()
    expect(sizeLine()).toBe(t('contestLogger.size.label', { pct: 120 }))
    expect(fontPx(callBox())).toBeCloseTo(48, 6)
    // A value no press could have written opens inside the range.
    cleanup()
    localStorage.setItem(OWN_KEY, '9')
    await mountLogger()
    expect(sizeLine()).toBe(t('contestLogger.size.label', { pct: 160 }))
    expect(loggerTextScale('0.5')).toBe(0.8)
    expect(loggerTextScale('1.234')).toBe(1.23)
    expect(loggerTextScale('nonsense')).toBe(1)
    expect(loggerTextScale(null)).toBe(1)
  })
})

// ⭐ CTRL+O IN THE LOGGER WINDOW (N1MM's "Ctrl+O = Set operator call"): the operator box is this
// window's own, on its entry's title line beside Sending, so the caret goes straight to it.
describe('Ctrl+O in the logger window', () => {
  it('puts the caret in the window’s own operator box, from the log line', async () => {
    await mountLogger()
    const operator = screen.getByLabelText(t('fieldDay.operator.aria')) as HTMLInputElement
    expect(operator.closest('.contest-logger-entry .le-fd-header'), 'the box is on the entry’s title line').not.toBeNull()
    callBox().focus()
    expect(document.activeElement).toBe(callBox())
    await act(async () => {
      expect(fireEvent.keyDown(callBox(), { key: 'o', code: 'KeyO', ctrlKey: true })).toBe(false)
    })
    expect(document.activeElement).toBe(operator)
    expect(callBox().value, 'the contact in progress is untouched').toBe('K9ABC')
  })
})

describe('a last contact’s edit in the logger window', () => {
  it('gives the caret back to Call once it is over, as the strip’s own Ctrl+Y edit does', async () => {
    const asked: string[] = []
    window.__TAURI_INTERNALS__ = {
      invoke: async <T,>(cmd: string): Promise<T> => {
        asked.push(cmd)
        // Esc is also this window's halt, which answers with the station's snapshot.
        return (cmd === 'halt_tx' ? snapshot : respond(cmd)) as T
      },
    }
    await mountLogger()
    const recent = document.querySelector<HTMLElement>('.contest-logger-recent')!
    fireEvent.click(within(recent).getAllByRole('button', { name: /^Edit / })[0])
    const edit = recent.querySelector<HTMLInputElement>('.contest-edit input')!
    expect(document.activeElement).toBe(edit)
    fireEvent.keyDown(edit, { key: 'Escape', code: 'Escape' })
    await act(async () => new Promise((r) => setTimeout(r, 50)))
    expect(recent.querySelector('.contest-edit')).toBeNull()
    expect(document.activeElement).toBe(callBox())
    expect(callBox().value, 'the contact in progress is untouched').toBe('K9ABC')
    expect(asked, 'Esc edits nothing').not.toContain('contest_edit')
  })
})
