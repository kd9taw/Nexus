// @vitest-environment jsdom
//
// THE THREE-PANE WORKSPACE'S NARROW COLLAPSE: EVERY CELL KEEPS A PLACE OF ITS OWN (2026-10-01).
//
// THE DEFECT. Tempo's workspace (App.tsx `threePane`: the header, the stations rail, the conversation and the
// waterfall rail) is a grid of NAMED areas: each cell declares `grid-area: stations` (and so on) and the layout's
// template lays the names out. Below md (data-viewport sm or xs — the 1024×768 floor at a 100 % scale, 1366×768 at
// 125 %, 1920×1080 at 175 %) the collapse stacks the cells by setting `grid-template-areas: none`, and it left every
// cell's area name in place. A name with no area resolves to the grid's implicit lines (CSS Grid §8.3.1), the SAME
// lines for every cell, so all four landed in one cell on top of each other: the rail painted over the station list
// and the conversation, and the station list's search box sat over the rail's first decode row. In Chrome at
// 1024×768 at 100 %, 73 of 110 controls, rows and chips were covered by another cell, and all four cells measured
// the same box. The rows were `minmax(0, auto)` on cells declared `min-height: 0`, so even placed apart a cell
// could shrink below what it holds, and the layout's own `overflow-y: auto` — the stack's scroller — never fired.
//
// THE RULE, computed here with the real sheet and the real selector engine: at every tier, each cell's grid-area is
// `auto` or names an area the layout's template at that tier really has; and in the collapse the stacked rows floor
// at their content, so the layout scrolls instead of letting one cell spill over the next. jsdom does not lay out:
// that the cells now stack apart, nothing is covered and every control can be reached is a Chrome measurement.
import { describe, it, expect, beforeAll, afterEach, vi } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

// THE BUDGET (2026-10-09). The slowest case here, "[data-viewport='xs'] with Tempo's header: every cell…", takes
// 0.45 s and 0.39 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const src = (rel: string) => readFileSync(resolve(process.cwd(), 'src', rel), 'utf8')

/** Every style rule of both sheets, flattened out of their at-rule blocks, in import order (styles.css, then
 *  cockpit-panes.css). Size-based @media is banned by the layout contract, so descending into every block cannot
 *  admit a rule that would not apply. */
const FLAT: { rule: CSSStyleRule; order: number }[] = []

beforeAll(() => {
  for (const sheet of ['styles.css', 'cockpit-panes.css']) {
    const style = document.createElement('style')
    style.textContent = src(sheet)
    document.head.appendChild(style)
    const walk = (rules: CSSRuleList) => {
      for (let i = 0; i < rules.length; i++) {
        const r = rules[i]
        if ((r as CSSStyleRule).selectorText) FLAT.push({ rule: r as CSSStyleRule, order: FLAT.length })
        else if ((r as CSSGroupingRule).cssRules) walk((r as CSSGroupingRule).cssRules)
      }
    }
    walk(style.sheet!.cssRules)
  }
})

afterEach(() => {
  document.body.innerHTML = ''
  document.documentElement.removeAttribute('data-viewport')
})

/** Selector specificity as one comparable number (`:where()` scores nothing; `:is()/:not()/:has()` their most
 *  specific argument) — the layout-single-deficit.test.tsx scorer. */
function specificity(sel: string): number {
  let a = 0
  let b = 0
  let c = 0
  let rest = sel.replace(/:(where|is|not|has|matches|any)\(([^()]*)\)/g, (_m, name: string, args: string) => {
    if (name !== 'where') {
      const inner = Math.max(0, ...args.split(',').map((s) => specificity(s.trim())))
      a += Math.floor(inner / 10000)
      b += Math.floor((inner % 10000) / 100)
      c += inner % 100
    }
    return ' '
  })
  rest = rest.replace(/::[a-z-]+/g, () => { c++; return ' ' })
  rest = rest.replace(/#[\w-]+/g, () => { a++; return ' ' })
  rest = rest.replace(/\.[\w-]+|\[[^\]]*\]|:[a-z-]+(\([^()]*\))?/g, () => { b++; return ' ' })
  rest.replace(/[a-zA-Z][\w-]*/g, () => { c++; return ' ' })
  return a * 10000 + b * 100 + c
}

/** The declaration that wins `prop` on `el`: importance, then specificity, then source order. */
function declWinner(el: Element, prop: string): { value: string; selector: string } | null {
  let win: { value: string; selector: string; important: boolean; spec: number; order: number } | null = null
  for (const { rule, order } of FLAT) {
    let hit = false
    try {
      hit = el.matches(rule.selectorText)
    } catch {
      continue // a selector this jsdom cannot parse cannot be applying either
    }
    const value = hit ? rule.style.getPropertyValue(prop).trim() : ''
    if (!value) continue
    const cand = { value, selector: rule.selectorText, important: rule.style.getPropertyPriority(prop) === 'important', spec: specificity(rule.selectorText), order }
    if (!win || (cand.important !== win.important ? cand.important : cand.spec !== win.spec ? cand.spec > win.spec : cand.order >= win.order)) win = cand
  }
  return win && { value: win.value, selector: win.selector }
}

/** The names a `grid-template-areas` value lays out ('.' is an empty cell). */
const areaNames = (v: string | undefined) =>
  new Set([...(v ?? '').matchAll(/["']([^"']*)["']/g)].flatMap((m) => m[1].trim().split(/\s+/)).filter((n) => n && n !== '.'))

/** The workspace exactly as App.tsx's threePane renders it, attached so ancestor selectors really match. */
function mount(vp: string, header: boolean) {
  document.documentElement.setAttribute('data-viewport', vp)
  document.body.innerHTML =
    '<div class="app"><div class="shell">' +
    `<main class="layout${header ? ' has-tempo-header' : ''}" data-three-pane>` +
    (header ? '<div class="grid-header"></div>' : '') +
    '<div class="grid-stations"></div><div class="pane-splitter left"></div><div class="grid-center"></div>' +
    '<div class="pane-splitter right"></div><div class="grid-waterfall"></div>' +
    '</main></div></div>'
  const layout = document.querySelector('main')!
  const cells = [...layout.children].filter((c) => /^grid-/.test(c.className))
  return { layout, cells }
}

const TIERS = ['xs', 'sm', 'md', 'lg', 'xl'] as const
const NARROW = new Set(['xs', 'sm'])

describe("the three-pane workspace's collapse leaves every cell a place of its own", () => {
  it('still mounts the class list this file models (App.tsx threePane)', () => {
    const app = src('App.tsx')
    for (const s of ['<main className={`layout${header ? \' has-tempo-header\' : \'\'}`} data-three-pane>', '<div className="grid-header">', '<div className="grid-stations">', '<div className="grid-center">', '<div className="grid-waterfall">']) {
      expect(app, `App.tsx no longer renders \`${s}\` — this guard models a workspace that is not mounted any more`).toContain(s)
    }
  })

  for (const header of [true, false]) {
    for (const vp of TIERS) {
      it(`[data-viewport='${vp}']${header ? ' with Tempo\'s header' : ''}: every cell names an area the template has, or none`, () => {
        const { layout, cells } = mount(vp, header)
        const tmpl = declWinner(layout, 'grid-template-areas')
        const names = areaNames(tmpl?.value)
        // Positive control: on the wide tiers the cells DO name areas, so the pairing below is exercised both ways.
        if (!NARROW.has(vp)) expect(names.size, `[data-viewport='${vp}']: the workspace lost its named template`).toBeGreaterThan(0)
        for (const cell of cells) {
          const area = declWinner(cell, 'grid-area')
          const v = area?.value ?? 'auto'
          expect(
            v === 'auto' || names.has(v),
            `[data-viewport='${vp}'] .${cell.className}: \`${area?.selector} { grid-area: ${v} }\` names an area the ` +
              `template (\`${tmpl?.selector ?? 'none'} { grid-template-areas: ${tmpl?.value ?? 'none'} }\`) does not have, ` +
              'so the cell lands on the implicit lines every other such cell lands on — on top of them.',
          ).toBe(true)
        }
      })
    }
  }

  for (const vp of NARROW) {
    it(`[data-viewport='${vp}']: the stacked rows floor at their content, so the workspace scrolls instead of spilling`, () => {
      const { layout } = mount(vp, true)
      const rows = declWinner(layout, 'grid-auto-rows')
      const min = /^minmax\(\s*([^,]+?)\s*,/.exec(rows?.value ?? '')?.[1] ?? rows?.value
      expect(
        min,
        `\`${rows?.selector} { grid-auto-rows: ${rows?.value} }\`: a stacked cell may be shorter than what it holds, ` +
          'so its content paints over the next cell and the workspace never scrolls.',
      ).toBe('min-content')
    })
  }
})
