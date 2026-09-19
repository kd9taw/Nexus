import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import {
  MODES,
  cmpSpec,
  contrast,
  expandWith,
  matchesRoot,
  parseRules,
  rgbHex as hex,
  rootTokensFrom,
  toRgb,
  topSplit,
  type Mode,
  type Rgb,
  type Rule,
} from './cssCascade'

// #323: "The active band is marked by (". The band menu's checked item drew a 3 px inset bar on
// its left edge and nothing else, which on a rounded item reads as a parenthesis rather than as
// the band you are on. The whole item is ringed now — what the reporter expected ("circled in
// its entirety") — and nothing under its text changes, so no word on the row loses contrast.
//
// Resolved from the sheet the way the other style guards do (cssCascade): for the item in each
// state and every theme mode, which rule WINS each property — not whether a declaration exists.

const RAW = readFileSync(fileURLToPath(new URL('./styles.css', import.meta.url)), 'utf8')
const CSS = RAW.replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES = parseRules(CSS)
const TOKENS = Object.fromEntries(MODES.map((m) => [m, rootTokensFrom(RULES, m)])) as Record<
  Mode,
  Map<string, string>
>

interface El {
  tag: string
  classes: string[]
  attrs: Record<string, string>
}

/** One compound selector against one element. `:not(…)` is honoured; any other pseudo-class
 *  (:hover, :focus-visible) is a state the element is not in, and a pseudo-element is not it. */
function compoundMatches(sel: string, el: El): boolean {
  if (sel.includes('::')) return false
  let rest = sel
  for (const m of sel.matchAll(/:not\(([^()]*)\)/g)) {
    if (compoundMatches(m[1], el)) return false
    rest = rest.replace(m[0], '')
  }
  const attrs = [...rest.matchAll(/\[([\w-]+)(?:=['"]?([^'"\]]*)['"]?)?\]/g)]
  const bare = rest.replace(/\[[^\]]*\]/g, '')
  if (/:[\w-]/.test(bare)) return false
  for (const [, name, value] of attrs) {
    if (!(name in el.attrs)) return false
    if (value !== undefined && el.attrs[name] !== value) return false
  }
  const classes = bare.match(/\.[\w-]+/g)?.map((c) => c.slice(1)) ?? []
  const tag = bare.replace(/\.[\w-]+/g, '').trim()
  if (tag && tag !== '*' && tag !== el.tag) return false
  if (!tag && classes.length === 0 && attrs.length === 0) return false
  return classes.every((c) => el.classes.includes(c))
}

/** The menu content the item sits in (BandMenu.tsx renders `.ui-menu.band-menu`). */
const MENU: El = { tag: 'div', classes: ['ui-menu', 'band-menu'], attrs: { role: 'menu' } }

/** Does a rule's selector reach this item under `mode`? Every ancestor compound must be the
 *  menu or the themed root; sibling combinators never reach it. */
function reaches(sel: string, el: El, mode: Mode): boolean {
  if (/[+~]/.test(sel.replace(/\[[^\]]*\]/g, ''))) return false
  const parts = sel.split(/\s*>\s*|\s+/).filter(Boolean)
  const subject = parts.pop()!
  return compoundMatches(subject, el) && parts.every((p) => compoundMatches(p, MENU) || matchesRoot(p, mode))
}

/** The rule that wins `prop` on the item in `mode`: specificity, then source order. */
function winner(el: El, prop: string, mode: Mode): { rule: Rule; value: string } | null {
  let win: { rule: Rule; value: string } | null = null
  for (const rule of RULES) {
    if (!reaches(rule.selector, el, mode)) continue
    const d = rule.decls.filter((x) => x.prop === prop).pop()
    if (!d) continue
    if (!win || cmpSpec(rule.spec, win.rule.spec) > 0 || (cmpSpec(rule.spec, win.rule.spec) === 0 && rule.order > win.rule.order)) {
      win = { rule, value: d.value }
    }
  }
  return win
}

/** A band menu item as Radix renders it: a radio item, checked or not, highlighted or not. */
const item = (checked: boolean, highlighted = false): El => ({
  tag: 'div',
  classes: ['ui-menu-item', 'band-menu-item'],
  attrs: {
    role: 'menuitemradio',
    'data-state': checked ? 'checked' : 'unchecked',
    'data-condition': 'open',
    ...(highlighted ? { 'data-highlighted': '' } : {}),
  },
})

const expand = (value: string, mode: Mode) => expandWith(TOKENS[mode], value)
const tokenRgb = (token: string, mode: Mode, over: Rgb = [0, 0, 0]) => toRgb(expand(`var(${token})`, mode), over)
/** The menu surface every item sits on. */
const surface = (mode: Mode) => tokenRgb('--bg-elev-2', mode, tokenRgb('--bg', mode)!)!
/** What the item paints behind its text in `mode` — the menu surface when it paints nothing. */
function itemBackground(el: El, mode: Mode): Rgb {
  const w = winner(el, 'background', mode) ?? winner(el, 'background-color', mode)
  if (!w) return surface(mode)
  const c = toRgb(expand(w.value, mode), surface(mode))
  expect(c, `could not compute ${w.rule.selector} background: ${expand(w.value, mode)}`).not.toBeNull()
  return c!
}

/** The ring's colour in `mode`, from the winning box-shadow (whatever follows the lengths). */
function ringColour(shadow: string, mode: Mode): Rgb | null {
  const colour = shadow.trim().split(/\s+/).slice(1).filter((t) => !/^-?[\d.]+(px|r?em)?$/.test(t)).join(' ')
  return toRgb(expand(colour, mode), surface(mode))
}

describe('the checked band is ringed as a whole item, not marked by a bar on one edge (#323)', () => {
  it.each(MODES)('its outline runs round all four edges in %s', (mode) => {
    const w = winner(item(true), 'box-shadow', mode)
    expect(w, 'the checked item draws no ring').not.toBeNull()
    const shadow = expand(w!.value, mode)
    expect(topSplit(shadow), `one shadow, not a stack: ${shadow}`).toHaveLength(1)
    const [kw, ...rest] = shadow.trim().split(/\s+/)
    expect(kw, `not an inset shadow: ${shadow}`).toBe('inset')
    // The leading lengths: x, y, then optional blur and spread (each 0 when absent).
    const lengths: number[] = []
    for (const t of rest) {
      if (!/^-?[\d.]+(px|r?em)?$/.test(t)) break
      lengths.push(parseFloat(t))
    }
    const [x = 0, y = 0, blur = 0, spread = 0] = lengths
    // A ring is zero offset and a positive spread; the old bar was `inset 3px 0 0` — an offset
    // with no spread, which is exactly one edge.
    expect({ x, y, blur }, `offset/blurred — one edge, not a ring: ${shadow}`).toEqual({ x: 0, y: 0, blur: 0 })
    expect(spread, `no spread, no ring: ${shadow}`).toBeGreaterThan(0)
  })

  it.each(MODES)('the ring stands out from the menu in %s (3:1, the bar for a UI indicator)', (mode) => {
    const w = winner(item(true), 'box-shadow', mode)
    const ring = ringColour(expand(w!.value, mode), mode)
    expect(ring, `could not compute the ring colour of ${w!.value}`).not.toBeNull()
    const menu = surface(mode)
    expect(contrast(ring!, menu), `${mode}: ring ${hex(ring!)} on the menu ${hex(menu)}`).toBeGreaterThanOrEqual(3)
  })

  it.each(MODES)('nothing under the text changes in %s, so every word keeps its contrast', (mode) => {
    // The checked row paints what an unchecked one does — the menu surface — so the band name
    // and the condition word (the faint "unknown" one included) read exactly as on every row.
    expect(hex(itemBackground(item(true), mode))).toBe(hex(itemBackground(item(false), mode)))
    expect(hex(itemBackground(item(false), mode))).toBe(hex(surface(mode)))
    for (const prop of ['color', 'opacity']) {
      expect(winner(item(true), prop, mode)?.rule.selector, prop).toBe(winner(item(false), prop, mode)?.rule.selector)
    }
  })

  it.each(MODES)('uses only tokens %s defines', (mode) => {
    for (const prop of ['background', 'box-shadow']) {
      const w = winner(item(true), prop, mode)
      if (!w) continue
      for (const [, name] of w.value.matchAll(/var\((--[\w-]+)/g)) {
        expect(TOKENS[mode].has(name), `${w.rule.selector} ${prop} reads ${name}, undefined in ${mode}`).toBe(true)
      }
    }
  })

  it.each(MODES)('hovering the checked item still paints the highlight in %s', (mode) => {
    // The highlight is the solid accent with accent-ink text. A checked rule that painted a
    // background at the same specificity, later in the sheet, would win there and leave
    // accent-ink on whatever it painted.
    const w = winner(item(true, true), 'background', mode)
    expect(w?.rule.selector).toBe('.ui-menu-item[data-highlighted]')
    expect(hex(itemBackground(item(true, true), mode))).toBe(hex(itemBackground(item(false, true), mode)))
  })
})
