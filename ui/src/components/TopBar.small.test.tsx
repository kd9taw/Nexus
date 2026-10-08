// @vitest-environment jsdom
//
// THE TOP BAR AT THE SMALL SIZE (operator, 2026-10-01) — COMPUTED, never presence-matched.
//
// At sm and xs every screen without FT drops the FT-only items from the bar (the eleven mode pills,
// the Tx cycle, the slot countdown, Sync and DT), which takes it from four lines to two. Operate and
// Tempo keep them: the pills fold into one mode button and the Tx cycle loses its small captions.
// Larger windows are unchanged. The drop is CSS keyed on `[data-viewport]`, which jsdom never
// applies, so this file applies it: it reads the two sheets the bar renders under (styles.css and
// the hosted page's application.css), renders the REAL TopBar with the props App passes on Field
// Day, Operate and Tempo, and computes at every size class the winning `display` and `visibility`
// of each item and of every ancestor up to <html> (`!important`, then specificity with
// :is/:not/:where/:has, then source order), matching through jsdom's own selector engine.
//
// ⚠️ THE TX CLUSTER IS THE POINT. App's `hideDigitalChrome` hides the FT items AND the bar's TX
// cluster, and on Field Day and every other screen without a cockpit that cluster is the only Stop
// TX. So the small-size drop has its own flag (`ftScreen`), and the first test is that NO rule in
// either sheet, conditional or not, hides TX On/Off, Tune, Stop TX or Hold Tx at any size.
//
// WHAT THIS DOES NOT PROVE: geometry. A rule that moved the cluster off screen, shrank it to 0×0 or
// covered it passes here. The real-browser sweep (the bar's lines on the twelve screens, Stop TX
// reachable and enabled on every screen at every pinned scale) and scripts/browser-probe are the
// instruments for that. The App-level half (the twelve screens really render the bar's Stop TX,
// enabled and sending halt_tx) is in stop-control-wiring.test.tsx.
import { describe, it, expect, afterEach, beforeAll, vi } from 'vitest'
import { render, cleanup, screen, fireEvent, within } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { TopBar } from './TopBar'
import { StationControlContext } from '../stationAccess'
import type { RadioStatus, Tier } from '../types'

// THE BUDGET (2026-10-08). The top bar renders for real at each size, and the FT-only case's time scales with the CPU it
// gets: 0.57–0.67 s alone on a quiet box, but past vitest's 5 s default twice in full-suite runs at a load of 20–30.
// 15 s is the budget the other real-render files carry; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const read = (p: string): string => readFileSync(fileURLToPath(new URL(p, import.meta.url)), 'utf8')

interface Decl {
  value: string
  important: boolean
}
interface Rule {
  selector: string
  display: Decl | null
  visibility: Decl | null
  order: number
  /** The at-rule the rule sits in, or null. Only `@supports selector(...)` counts as applying (the
   *  WebViews all have :has); `@media` here is motion-only (responsive-vocab.test.ts). */
  cond: string | null
}

/** Split on top-level commas only: `:is(a, b) .c` is ONE selector. */
function splitTop(list: string): string[] {
  const out: string[] = []
  let depth = 0
  let start = 0
  for (let i = 0; i < list.length; i++) {
    const c = list[i]
    if (c === '(' || c === '[') depth++
    else if (c === ')' || c === ']') depth--
    else if (c === ',' && depth === 0) {
      out.push(list.slice(start, i).trim())
      start = i + 1
    }
  }
  out.push(list.slice(start).trim())
  return out.filter(Boolean)
}

/** The winning declaration of `prop` inside one block: `!important` beats normal, then the later. */
function blockDecl(body: string, prop: string): Decl | null {
  let win: Decl | null = null
  for (const part of body.split(';')) {
    const m = /^\s*([a-z-]+)\s*:\s*([\s\S]*?)\s*$/i.exec(part)
    if (!m || m[1].toLowerCase() !== prop) continue
    const important = /!\s*important$/i.test(m[2])
    const value = m[2].replace(/!\s*important$/i, '').trim().toLowerCase()
    if (!win || important || !win.important) win = { value, important }
  }
  return win
}

/** Brace-aware walk of a whole sheet. Rules inside an at-rule carry it as `cond`; @keyframes and
 *  @font-face bodies are skipped wholesale. */
function parseRules(sheet: string): Rule[] {
  const css = sheet.replace(/\/\*[\s\S]*?\*\//g, '')
  const rules: Rule[] = []
  let order = 0
  const walk = (text: string, cond: string | null): void => {
    let i = 0
    while (i < text.length) {
      const open = text.indexOf('{', i)
      if (open < 0) return
      const prelude = text.slice(i, open).replace(/^[\s;]+/, '').trim()
      let depth = 1
      let j = open + 1
      while (j < text.length && depth > 0) {
        if (text[j] === '{') depth++
        else if (text[j] === '}') depth--
        j++
      }
      const body = text.slice(open + 1, j - 1)
      if (prelude.startsWith('@')) {
        if (/^@(media|supports|container)\b/i.test(prelude)) walk(body, prelude)
      } else {
        const display = blockDecl(body, 'display')
        const visibility = blockDecl(body, 'visibility')
        for (const selector of splitTop(prelude)) rules.push({ selector, display, visibility, order: order++, cond })
      }
      i = j
    }
  }
  walk(css, null)
  return rules
}

type Spec = [number, number, number]
const add = (a: Spec, b: Spec): Spec => [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
const cmp = (a: Spec, b: Spec): number => a[0] - b[0] || a[1] - b[1] || a[2] - b[2]

/** Selectors-4 specificity: :where() is zero, :is()/:not()/:has() take their most specific
 *  argument, any other pseudo-class and every class or attribute counts as a class. */
function specificity(sel: string): Spec {
  let s: Spec = [0, 0, 0]
  let i = 0
  const close = (from: number, o: string, c: string): number => {
    let d = 0
    for (let k = from; k < sel.length; k++) {
      if (sel[k] === o) d++
      else if (sel[k] === c && --d === 0) return k
    }
    return sel.length - 1
  }
  while (i < sel.length) {
    const ch = sel[i]
    if (ch === '#') {
      s = add(s, [1, 0, 0])
      i++
      while (i < sel.length && /[\w-]/.test(sel[i])) i++
    } else if (ch === '.') {
      s = add(s, [0, 1, 0])
      i++
      while (i < sel.length && /[\w-]/.test(sel[i])) i++
    } else if (ch === '[') {
      s = add(s, [0, 1, 0])
      i = close(i, '[', ']') + 1
    } else if (ch === ':') {
      if (sel[i + 1] === ':') {
        s = add(s, [0, 0, 1])
        i += 2
        while (i < sel.length && /[\w-]/.test(sel[i])) i++
        continue
      }
      const m = /^:([\w-]+)/.exec(sel.slice(i))!
      const name = m[1].toLowerCase()
      i += m[0].length
      let args: string | null = null
      if (sel[i] === '(') {
        const end = close(i, '(', ')')
        args = sel.slice(i + 1, end)
        i = end + 1
      }
      if (name === 'where') continue
      if (args !== null && (name === 'is' || name === 'not' || name === 'has' || name === 'matches')) {
        s = add(s, splitTop(args).map(specificity).reduce((a, b) => (cmp(a, b) >= 0 ? a : b), [0, 0, 0] as Spec))
      } else s = add(s, [0, 1, 0])
    } else if (/[a-z]/i.test(ch)) {
      s = add(s, [0, 0, 1])
      while (i < sel.length && /[\w-]/.test(sel[i])) i++
    } else i++
  }
  return s
}

const RULES = parseRules(read('../styles.css') + '\n' + read('../remote-web/application.css')).filter(
  (r) => r.display || r.visibility,
)
const BAR_WORDS = /topbar|tier-|tx-period|op-controls|op-btn|slot-clock|timesync|clock-repair|dt-readout|txrx|theme-chip/

/** Selectors a bar class appears in that jsdom could not evaluate. Collected, then asserted empty:
 *  skipping one would let a rule hide the cluster unseen. */
const unreadable = new Set<string>()
function matches(el: Element, r: Rule): boolean {
  try {
    return el.matches(r.selector)
  } catch {
    if (BAR_WORDS.test(r.selector)) unreadable.add(r.selector)
    return false
  }
}

const applies = (r: Rule): boolean => r.cond === null || /^@supports\s+selector\(/i.test(r.cond)

function winner(el: Element, prop: 'display' | 'visibility'): string | null {
  let win: { d: Decl; spec: Spec; order: number } | null = null
  for (const r of RULES) {
    const d = r[prop]
    if (!d || !applies(r) || !matches(el, r)) continue
    const spec = specificity(r.selector)
    const beats =
      !win ||
      (d.important && !win.d.important) ||
      (d.important === win.d.important && (cmp(spec, win.spec) > 0 || (cmp(spec, win.spec) === 0 && r.order > win.order)))
    if (beats) win = { d, spec, order: r.order }
  }
  return win ? win.d.value : null
}

/** Not drawn: `display: none` on it or any ancestor, or an inherited `visibility: hidden`. */
function hidden(el: Element): boolean {
  for (let e: Element | null = el; e; e = e.parentElement) if (winner(e, 'display') === 'none') return true
  for (let e: Element | null = el; e; e = e.parentElement) {
    const v = winner(e, 'visibility')
    if (v) return v === 'hidden' || v === 'collapse'
  }
  return false
}

/** Every rule in either sheet, at-rules included, that matches `el` and would hide it. */
function anyHider(el: Element): string[] {
  return RULES.filter((r) => matches(el, r) && (r.display?.value === 'none' || r.visibility?.value === 'hidden')).map(
    (r) => (r.cond ? `${r.cond} ` : '') + r.selector,
  )
}

const VIEWPORTS = ['xs', 'sm', 'md', 'lg', 'xl'] as const
const SMALL = new Set(['xs', 'sm'])

const radio = {
  dialMhz: 14.074,
  band: '20m',
  sideband: 'USB',
  rigMode: 'USB',
  rigConfirmed: true,
  nextSlotMs: 0,
  txEven: true,
  txCycleAuto: true,
  txEnabled: false,
  txAllowed: true,
  transmitting: false,
  tuning: false,
  qsoRecording: false,
  catOk: true,
  dtSec: 0,
  clockOffsetMs: 0,
  // Repair clock is drawn, so it is held to the chip's sizes: it goes and stays where the chip does.
  clockRepairAvailable: true,
  operatingMode: 'digital',
} as unknown as RadioStatus

const noop = () => {}

/** The bar as App mounts it on `screenName` (App.tsx's TopBar props for that view). */
function bar(screenName: 'fieldDay' | 'operate' | 'chat', over: Record<string, unknown> = {}) {
  const ft = screenName !== 'fieldDay'
  return render(
    <div className="app">
      <TopBar
        mycall="KD9TAW"
        mygrid="EN52xa"
        radio={radio}
        link={{ tier: 'FT8', dtSec: 0 } as never}
        bandPlan={[]}
        onSetFrequency={noop}
        onSetTxEnabled={noop}
        onSetTune={noop}
        onHaltTx={noop}
        onSetTxEven={noop}
        onSetTxCycleAuto={noop}
        onSetHoldTxFreq={noop}
        tier={screenName === 'chat' ? 'TempoFast' : 'FT8'}
        onTierChange={noop}
        onOpenGuide={noop}
        field={false}
        onFieldChange={noop}
        fdActive={screenName === 'fieldDay'}
        hideTxControls={ft}
        hideFrequencyControl={ft}
        hideDigitalChrome={false}
        ftScreen={ft}
        {...over}
      />
    </div>,
  )
}

const q = (c: HTMLElement, sel: string): Element[] => [...c.querySelectorAll(sel)]
const one = (c: HTMLElement, sel: string): Element => {
  const els = q(c, sel)
  expect(els, `exactly one ${sel}`).toHaveLength(1)
  return els[0]
}

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(() => {
  cleanup()
  delete document.documentElement.dataset.viewport
})

describe('the top bar at the small size', () => {
  it('reads both sheets, and every selector that names a bar class was evaluated', () => {
    // Control: the parse found the rules this file is about, so a green below is not an empty sheet.
    expect(RULES.some((r) => r.selector.includes('.tier-menu-btn') && r.display?.value === 'none')).toBe(true)
    expect(RULES.some((r) => r.selector.includes(".topbar:not(.topbar--ft)"))).toBe(true)
    const { container } = bar('fieldDay')
    for (const el of q(container, 'header.topbar *')) for (const r of RULES) matches(el, r)
    expect([...unreadable]).toEqual([])
  })

  it('no rule in either sheet hides the TX cluster, at any size: on Field Day it is the only Stop TX', () => {
    const { container } = bar('fieldDay')
    const cluster = one(container, 'header.topbar .op-controls')
    const buttons = q(container, 'header.topbar .op-controls button')
    expect(buttons.map((b) => b.textContent)).toEqual(['TX Off', 'Tune', 'Stop TX', 'Hold Tx'])
    for (const vp of VIEWPORTS) {
      document.documentElement.dataset.viewport = vp
      for (let e: Element | null = cluster; e; e = e.parentElement) expect(anyHider(e), `${vp}: ${e.className}`).toEqual([])
      for (const b of buttons) {
        expect(anyHider(b), `${vp}: ${b.textContent}`).toEqual([])
        expect(hidden(b), `${vp}: ${b.textContent}`).toBe(false)
      }
    }
  })

  it('the screens without FT drop exactly the FT-only items, and only at sm and xs', () => {
    const { container } = bar('fieldDay')
    const ftOnly = {
      pills: one(container, 'header.topbar > .tier-toggle:not(.tx-period)'),
      cycle: one(container, 'header.topbar > .tx-period'),
      slot: one(container, 'header.topbar .slot-clock'),
      sync: one(container, 'header.topbar .timesync'),
      repair: one(container, 'header.topbar .clock-repair'),
      dt: one(container, 'header.topbar .dt-readout'),
    }
    const kept = {
      brand: one(container, 'header.topbar > .brand'),
      readout: one(container, 'header.topbar > .radio-readout'),
      plate: one(container, 'header.topbar .txrx-indicator'),
      meter: one(container, 'header.topbar .rx-level'),
      utc: one(container, 'header.topbar .utc-clock'),
      chips: one(container, 'header.topbar > .topbar-chips'),
      ...Object.fromEntries(q(container, 'header.topbar > .topbar-chips > button').map((b) => [b.textContent!, b])),
    }
    expect(Object.keys(kept)).toEqual(expect.arrayContaining(['Help', 'Field', 'Set operator']))
    expect(q(container, '.tier-menu-btn'), 'no mode button off the FT screens').toEqual([])
    for (const vp of VIEWPORTS) {
      document.documentElement.dataset.viewport = vp
      for (const [name, el] of Object.entries(ftOnly)) expect(hidden(el), `${vp}: ${name}`).toBe(SMALL.has(vp))
      for (const [name, el] of Object.entries(kept)) expect(hidden(el), `${vp}: ${name}`).toBe(false)
    }
  })

  it.each(['operate', 'chat'] as const)(
    '%s keeps them: one mode button for the eleven pills, the Tx cycle without its captions, at sm and xs only',
    (screenName) => {
      const { container } = bar(screenName)
      expect(container.querySelector('header.topbar')!.classList.contains('topbar--ft')).toBe(true)
      const pills = q(container, 'header.topbar > .tier-toggle:not(.tx-period) > .tier-btn:not(.tier-menu-btn)')
      expect(pills).toHaveLength(11)
      const menu = one(container, 'header.topbar > .tier-toggle:not(.tx-period) > .tier-menu-btn')
      const cycle = q(container, 'header.topbar > .tx-period > .tier-btn')
      expect(cycle).toHaveLength(3)
      const captions = q(container, 'header.topbar > .tx-period small')
      expect(captions).toHaveLength(3)
      const keptAlways = [
        one(container, 'header.topbar .slot-clock'),
        one(container, 'header.topbar .timesync'),
        one(container, 'header.topbar .clock-repair'),
        one(container, 'header.topbar .dt-readout'),
        one(container, 'header.topbar > .topbar-chips'),
      ]
      for (const vp of VIEWPORTS) {
        document.documentElement.dataset.viewport = vp
        const small = SMALL.has(vp)
        for (const p of pills) expect(hidden(p), `${vp}: pill ${p.textContent}`).toBe(small)
        expect(hidden(menu), `${vp}: the mode button`).toBe(!small)
        for (const b of cycle) expect(hidden(b), `${vp}: ${b.textContent}`).toBe(false)
        for (const s of captions) expect(hidden(s), `${vp}: caption ${s.textContent}`).toBe(small)
        for (const el of keptAlways) expect(hidden(el), `${vp}: ${el.className}`).toBe(false)
      }
    },
  )
})

describe('the mode button', () => {
  it('shows the mode in use and opens all eleven, tags and all, in the pills’ order', () => {
    const picked: Tier[] = []
    const { container } = bar('chat', { onTierChange: (t: Tier) => picked.push(t) })
    const menu = one(container, '.tier-menu-btn') as HTMLButtonElement
    expect(menu.textContent).toBe('TempoFast')
    expect(menu.disabled).toBe(false)
    fireEvent.pointerDown(menu, { button: 0, ctrlKey: false })
    const items = screen.getAllByRole('menuitem')
    expect(items.map((i) => i.textContent)).toEqual([
      'TempoFast', 'TempoDeep', 'FT4', 'FT8', 'FT2', 'BCNWSPR', 'Q65', 'MSK144', 'RXJT65', 'FST4', 'BCNFST4W',
    ])
    fireEvent.click(within(items[6]).getByText('Q65'))
    expect(picked).toEqual(['Q65'])
  })

  it('names an rx-only mode as its pill does', () => {
    const { container } = bar('operate', { tier: 'JT65' })
    const menu = one(container, '.tier-menu-btn')
    expect(menu.textContent).toBe('RXJT65')
    expect(menu.classList.contains('rx-only')).toBe(true)
  })

  it('is disabled exactly when the pills are: a browser without tier control cannot open it', () => {
    const { container } = render(
      <StationControlContext.Provider value={false}>
        <TopBar
          mycall="KD9TAW" mygrid="EN52xa" radio={radio} link={{ tier: 'FT8', dtSec: 0 } as never} bandPlan={[]}
          onSetFrequency={noop} onSetTxEnabled={noop} onSetTune={noop} onHaltTx={noop} onSetTxEven={noop}
          onSetTxCycleAuto={noop} onSetHoldTxFreq={noop} tier="FT8" onTierChange={noop} onOpenGuide={noop}
          hideTxControls hideFrequencyControl ftScreen
        />
      </StationControlContext.Provider>,
    )
    const pills = q(container, '.tier-toggle:not(.tx-period) > .tier-btn') as HTMLButtonElement[]
    expect(pills).toHaveLength(12)
    expect(pills.every((p) => p.disabled), 'control: the pills are refused too').toBe(true)
    const menu = one(container, '.tier-menu-btn') as HTMLButtonElement
    fireEvent.pointerDown(menu, { button: 0, ctrlKey: false })
    expect(screen.queryAllByRole('menuitem')).toEqual([])
  })
})
