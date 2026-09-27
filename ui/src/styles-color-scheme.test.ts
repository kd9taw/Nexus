// THE BROWSER'S OWN CONTROLS FOLLOW NEXUS'S THEME, NOT THE COMPUTER'S (2026-09-27).
//
// index.html declares `<meta name="color-scheme" content="dark light">`, which lets the browser
// draw its own controls in either scheme, and until this rule it picked the COMPUTER's. So
// everything the sheet does not paint itself — a checkbox's box, a slider's track, a number
// field's spinner, a native button's face or its ink — followed Windows' or macOS's light/dark
// setting instead of the theme. A census of every view and Settings tab in real Chrome, both
// themes on both computer settings, found a Nexus-coloured word on a browser face (1.02–3.09:1),
// a browser-coloured word on a Nexus face (1.03–1.19:1), and the amp strip's Operate at 1.04:1.
// Declaring the theme as the root's colour scheme ends the class: re-run with this rule, no
// control's face or ink depended on the computer. `data-theme` is always light or dark (the
// System theme resolves it before the first paint, useTheme.ts), so an operator whose computer
// matches the theme sees nothing change.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { BASE_MODES, baseTheme, cmpSpec, matchesRoot, parseRules, type Mode, type Rule } from './cssCascade'

const blank = (s: string) => s.replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const read = (name: string) => readFileSync(fileURLToPath(new URL(`./${name}`, import.meta.url)), 'utf8')
const RULES = parseRules(blank(read('styles.css')) + '\n' + blank(read('cockpit-panes.css')))

const declares = (r: Rule) => r.decls.some((d) => d.prop === 'color-scheme')

/** What <html> computes for `color-scheme` under `mode`: the root rule that wins it, or null. */
function rootScheme(rules: Rule[], mode: Mode): string | null {
  let win: { rule: Rule; value: string } | null = null
  for (const r of rules) {
    const d = r.decls.filter((x) => x.prop === 'color-scheme').pop()
    if (!d || !matchesRoot(r.selector, mode)) continue
    const beats = !win || cmpSpec(r.spec, win.rule.spec) > 0 || (cmpSpec(r.spec, win.rule.spec) === 0 && r.order > win.rule.order)
    if (beats) win = { rule: r, value: d.value.trim() }
  }
  return win?.value ?? null
}

describe("the browser's own controls follow Nexus's theme, not the computer's", () => {
  it('the root declares the theme as its colour scheme, in every mode', () => {
    for (const mode of BASE_MODES) expect(rootScheme(RULES, mode), mode).toBe(baseTheme(mode))
  })

  it('nothing below the root declares one of its own, which would hand its subtree back to the computer', () => {
    const below = RULES.filter((r) => declares(r) && !BASE_MODES.some((m) => matchesRoot(r.selector, m)))
    expect(below.map((r) => r.selector)).toEqual([])
  })

  it('FIRES: the sheet without the rule, and with the two schemes swapped, is caught', () => {
    const without = RULES.filter((r) => !declares(r))
    expect(BASE_MODES.map((m) => rootScheme(without, m))).toEqual(BASE_MODES.map(() => null))
    const swapped = [...without, ...parseRules(":root[data-theme='light'] { color-scheme: dark; } :root[data-theme='dark'] { color-scheme: light; }", { n: 1e6 })]
    expect(rootScheme(swapped, 'light')).toBe('dark')
    expect(rootScheme(swapped, 'dark-night-high')).toBe('light')
  })
})
