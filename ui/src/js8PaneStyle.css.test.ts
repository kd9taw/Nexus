// The JS8 panes' colours and fonts in the stylesheet: every rule that reads an operator's --js8-*
// property falls back to theme tokens that EVERY base mode defines (light and dark, high contrast and
// night, with each one's sentinel themes), every colour's starting token is one of them, and the
// properties the cockpit can set (features/js8PaneStyle) and the ones styles.css reads are the same
// set, so neither side can drift into a colour that paints nothing or a rule nothing sets.
import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { MODES, parseRules, rootTokensFrom } from './cssCascade'
import { JS8_COLOR_TOKEN, JS8_COLOR_VAR, JS8_FONT_VAR } from './features/js8PaneStyle'

// Comments blanked, as styles-theme-cascade.test.ts reads the sheet: the parser takes what precedes a
// rule's `{` as its selector.
const SHEET = readFileSync(fileURLToPath(new URL('./styles.css', import.meta.url)), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) =>
  m.replace(/[^\n]/g, ' '),
)
const RULES = parseRules(SHEET)
/** Every declaration that reads an operator's property, as `selector { prop: value }`. */
const READERS = RULES.flatMap((r) => r.decls.filter((d) => d.value.includes('var(--js8-')).map((d) => ({ selector: r.selector, ...d })))
const VAR = /var\(\s*(--[\w-]+)/g
const read = (value: string) => [...value.matchAll(VAR)].map((m) => m[1])
/** The pane properties' families; other JS8 rules keep custom properties of their own (`--js8-wf-h`). */
const OURS = /^--js8-(rx|tx|table|compose)-/

describe('the JS8 panes’ colours and fonts in styles.css', () => {
  it('finds the rules that read them (the parse is real)', () => {
    expect(READERS.length, 'positive control').toBeGreaterThanOrEqual(20)
    expect(READERS.some((d) => d.selector === '.js8-rx' && d.prop === 'background'), 'the RX pane’s background').toBe(true)
  })

  it('reads exactly the properties the cockpit can set', () => {
    const settable = new Set([
      ...Object.values(JS8_COLOR_VAR),
      ...Object.values(JS8_FONT_VAR).flatMap((v) => [v.family, v.size]),
    ])
    const readHere = new Set(READERS.flatMap((d) => read(d.value)).filter((v) => OURS.test(v)))
    expect([...readHere].sort()).toEqual([...settable].sort())
    expect([...settable].every((v) => OURS.test(v)), 'premise: every settable property is of the families').toBe(true)
  })

  it('falls back to tokens every theme defines, and starts each colour from one', () => {
    const fallbacks = new Set(READERS.flatMap((d) => read(d.value)).filter((v) => !v.startsWith('--js8-')))
    for (const t of Object.values(JS8_COLOR_TOKEN)) fallbacks.add(t)
    expect(fallbacks.size, 'positive control: tokens found').toBeGreaterThanOrEqual(8)
    const missing: string[] = []
    for (const mode of MODES) {
      const tokens = rootTokensFrom(RULES, mode)
      for (const t of fallbacks) if (!tokens.has(t)) missing.push(`${t} in ${mode}`)
    }
    expect(missing).toEqual([])
    expect(rootTokensFrom(RULES, 'dark').has('--js8-rx-bg'), 'control: an operator property is never a theme token').toBe(false)
  })

  // Nexus's order, kept (operator decision): a CQ or highlight row keeps its tint when selected, the accent bar marking the
  // selection; JS8Call lets the selection win. The rules weigh the same (a class, a type, a class: `:is()` counts as its one
  // class), so the later one paints: every selected-row fill must come before every tint.
  it('draws the CQ and highlight tints over the selected row', () => {
    const fills = (r: (typeof RULES)[number]) => r.decls.some((d) => d.prop === 'background' && !d.value.includes('!important'))
    const selected = RULES.filter((r) => /\btr\.selected$/.test(r.selector) && r.decls.some((d) => d.prop === 'background'))
    const tints = RULES.filter((r) => /^\.js8-table tr\.(cq|hl-secondary|hl-primary)$/.test(r.selector) && fills(r))
    expect(selected.length, 'positive control: the selected row’s fills are found').toBeGreaterThanOrEqual(2)
    expect(selected.every(fills), 'no selected fill outweighs a tint by !important').toBe(true)
    expect(selected.every((r) => !r.selector.includes('#')), 'nor by an id').toBe(true)
    expect(tints.map((r) => r.selector).sort()).toEqual(['.js8-table tr.cq', '.js8-table tr.hl-primary', '.js8-table tr.hl-secondary'])
    for (const tint of tints) for (const sel of selected) expect(tint.order, `${tint.selector} after ${sel.selector}`).toBeGreaterThan(sel.order)
  })
})
