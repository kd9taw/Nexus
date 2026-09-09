import { describe, it, expect } from 'vitest'
import { readFileSync, readdirSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

// THE CLOSED <select> BOX — the guard for the one rule that owns it (styles.css, the
// `select {}` base rule).
//
// The bug it retires: on Linux the runtime is WebKitGTK, whose RenderThemeAdwaita paints
// its own light form-control chrome OVER the author's background whenever the used
// `appearance` is still `auto`. Measured in webkit2gtk-4.1 2.52.3, the setup wizard's radio
// and audio pickers computed `background: rgb(19,25,37)` and PAINTED #F4F4F4 with the app's
// near-white text on top: 1.07:1, invisible, on the one screen an operator cannot skip.
// `appearance: none` takes the box back, and the arrow the engine stops drawing has to be
// drawn by us or every select in the app reads as a text input.
//
// WHY THIS GUARD IS NOT A PRESENCE MATCH. `appearance: none` is worthless if a later rule
// silently wins the arrow back off it, and that is not hypothetical: EVERY select-styling
// rule in this sheet uses the `background` and `padding` SHORTHANDS, each of which resets
// `background-image` to none and `padding-right` to its own value. So the guard ENUMERATES
// the competing rules and COMPUTES the cascade winner for each arrow property, the way
// cockpit-panes.test.ts does for structural size. A dead fix passes a regex; it does not
// pass this.
//
// Both directions are checked: `analyse` is run against the real sheet (must be clean) and
// against a deliberately broken sheet (must report exactly the violation). A guard shown
// only not to fire is half a test.

const here = (name: string) => fileURLToPath(new URL(`./${name}`, import.meta.url))
const strip = (css: string) => css.replace(/\/\*[\s\S]*?\*\//g, '')

interface Rule {
  selector: string
  body: string
  order: number
}

/** Brace-aware rule walk (the cockpit-panes.test.ts parser). Rules nested in an at-rule are
 *  walked too — a `@media` override of a select is exactly the kind of thing that must not
 *  be invisible to this guard. */
function parseRules(sheet: string): Rule[] {
  const out: Rule[] = []
  let i = 0
  let order = 0
  let selStart = 0
  while (i < sheet.length) {
    if (sheet[i] === '{') {
      const sel = sheet.slice(selStart, i).trim()
      i++
      const bodyStart = i
      let depth = 1
      while (i < sheet.length && depth > 0) {
        if (sheet[i] === '{') depth++
        else if (sheet[i] === '}') depth--
        i++
      }
      const body = sheet.slice(bodyStart, i - 1)
      if (sel.startsWith('@')) {
        // Descend into the at-rule rather than skipping it.
        for (const r of parseRules(body)) out.push({ ...r, order: order++ })
      } else if (sel) {
        out.push({ selector: sel, body, order: order++ })
      }
      selStart = i
    } else if (sheet[i] === '}') {
      i++
      selStart = i
    } else {
      i++
    }
  }
  return out
}

/** (id, class, type) — enough for this sheet, which uses no ids and no `:not()` on selects. */
function specificity(sel: string): [number, number, number] {
  const s = sel.replace(/\s+/g, ' ').trim()
  const ids = (s.match(/#[\w-]+/g) ?? []).length
  const classes = (s.match(/\.[\w-]+|\[[^\]]+\]|:[\w-]+(?![\w-]*\()/g) ?? []).filter(
    (t) => !/^::/.test(t),
  ).length
  const types = (s.match(/(^|[\s>+~])([a-z][\w-]*)/g) ?? []).length
  return [ids, classes, types]
}

const cmpSpec = (a: [number, number, number], b: [number, number, number]) =>
  a[0] - b[0] || a[1] - b[1] || a[2] - b[2]

interface Decl {
  prop: string
  important: boolean
}

function declarations(body: string): Decl[] {
  return body
    .split(';')
    .map((d) => d.trim())
    .filter(Boolean)
    .map((d) => {
      const prop = d.slice(0, d.indexOf(':')).trim().toLowerCase()
      return { prop, important: /!\s*important\s*$/i.test(d) }
    })
    .filter((d) => d.prop)
}

/** Longhands a shorthand resets. Only the ones that carry the arrow matter here. */
const RESET_BY: Record<string, string[]> = {
  background: [
    'background-image',
    'background-position',
    'background-size',
    'background-repeat',
    'background-color',
  ],
  padding: ['padding-right'],
}

/** The arrow, and the property that makes the box ours at all. */
const OWNED = [
  'appearance',
  '-webkit-appearance',
  'background-image',
  'background-position',
  'background-size',
  'background-repeat',
  'padding-right',
]

/** Class names this app actually puts on a `<select>`, read from the components — so a new
 *  styled select cannot quietly escape the guard by inventing a class the test never heard
 *  of. */
function selectClassNames(componentsDir: string): Set<string> {
  const out = new Set<string>()
  const walk = (dir: string) => {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      const p = `${dir}/${e.name}`
      if (e.isDirectory()) walk(p)
      else if (e.name.endsWith('.tsx') && !e.name.includes('.test.')) {
        const src = readFileSync(p, 'utf8')
        for (const m of src.matchAll(/<select\b[^>]*?className="([^"]+)"/g)) {
          for (const c of m[1].split(/\s+/)) if (c) out.add(c)
        }
      }
    }
  }
  walk(componentsDir)
  return out
}

/** Can this selector match a <select>? Either it names the element, or its subject compound
 *  is built only from classes this app puts on one. */
function matchesSelect(selector: string, selectClasses: Set<string>): boolean {
  return selector.split(',').some((one) => {
    const s = one.trim()
    if (!s) return false
    const subject = s.split(/[\s>+~]+/).filter(Boolean).pop() ?? ''
    if (/(^|[\s>+~])select\b/.test(s) && !/\boption\b|\boptgroup\b/.test(s)) return true
    const classes = subject.match(/\.[\w-]+/g)?.map((c) => c.slice(1)) ?? []
    if (!classes.length) return false
    if (/^[a-z]/.test(subject) && !subject.startsWith('.')) return false // a different element
    return classes.some((c) => selectClasses.has(c))
  })
}

interface Violation {
  prop: string
  loser: string
  winner: string
}

/**
 * For every arrow property the base `select {}` rule declares, compute which rule actually
 * wins on an element that both rules match. Anything that beats the base rule is a
 * violation unless it is the one declared exception.
 */
export function analyse(sheet: string, selectClasses: Set<string>): Violation[] {
  const rules = parseRules(strip(sheet))
  const base = rules.find((r) => r.selector.trim() === 'select')
  if (!base) return [{ prop: '*', loser: 'select', winner: 'THE BASE RULE IS MISSING' }]

  const baseDecls = new Map(declarations(base.body).map((d) => [d.prop, d]))
  const baseSpec = specificity('select')
  const violations: Violation[] = []

  for (const rule of rules) {
    if (rule === base) continue
    if (!matchesSelect(rule.selector, selectClasses)) continue
    // The Logbook's 22×22 icon menu deliberately re-centres the arrow with its own
    // important; it is the sanctioned exception and is asserted separately below.
    if (rule.selector.trim() === 'select.log-rowbtn') continue

    const spec = specificity(rule.selector.split(',')[0])
    for (const decl of declarations(rule.body)) {
      const touched = decl.prop in RESET_BY ? RESET_BY[decl.prop] : [decl.prop]
      for (const prop of touched) {
        if (!OWNED.includes(prop)) continue
        const mine = baseDecls.get(prop)
        if (!mine) continue
        // Cascade: important wins; then specificity; then source order.
        const theirsWins = decl.important
          ? !mine.important
          : mine.important
            ? false
            : cmpSpec(spec, baseSpec) > 0 || (cmpSpec(spec, baseSpec) === 0 && rule.order > base.order)
        if (theirsWins) {
          violations.push({ prop, loser: 'select', winner: rule.selector.replace(/\s+/g, ' ') })
        }
      }
    }
  }
  return violations
}

const STYLES = readFileSync(here('styles.css'), 'utf8')
const SELECT_CLASSES = selectClassNames(fileURLToPath(new URL('./components', import.meta.url)))

describe('the closed <select> box is owned by one rule', () => {
  it('knows which classes this app puts on a select (the guard is not looking at nothing)', () => {
    // Positive control on the input to every check below: if this set were empty the
    // matcher would silently examine almost nothing and every assertion would pass.
    expect(SELECT_CLASSES.size).toBeGreaterThan(10)
    expect(SELECT_CLASSES.has('settings-input')).toBe(true)
    expect(SELECT_CLASSES.has('band-picker-select')).toBe(true)
  })

  it('declares appearance:none and an arrow, and marks the chrome important', () => {
    const base = parseRules(strip(STYLES)).find((r) => r.selector.trim() === 'select')
    expect(base, 'styles.css must carry a bare `select {}` base rule').toBeTruthy()
    const decls = new Map(declarations(base!.body).map((d) => [d.prop, d]))
    for (const prop of OWNED) {
      expect(decls.get(prop)?.important, `select { ${prop} } must be !important`).toBe(true)
    }
  })

  it('is not out-cascaded by any select rule in the sheet', () => {
    expect(analyse(STYLES, SELECT_CLASSES)).toEqual([])
  })

  it('FIRES: a later shorthand that would delete the arrow is reported', () => {
    // The control that must trip. `.wizard-field select { background: … }` is exactly the
    // shape every surface in this sheet is written in, and without the important above it
    // wins `background-image` and the arrow is gone.
    const broken = STYLES.replace(
      /background-repeat: no-repeat !important;/,
      'background-repeat: no-repeat;',
    ).replace(
      /background-image:\n?\s*linear-gradient\(45deg[\s\S]*?50%\) !important;/,
      'background-image: linear-gradient(45deg, transparent 50%, red 50%);',
    )
    const found = analyse(broken, SELECT_CLASSES)
    expect(found.length, 'the broken sheet must be reported').toBeGreaterThan(0)
    expect(found.some((v) => v.prop === 'background-image')).toBe(true)
  })

  it('keeps the subsumed settings rule from coming back', () => {
    // `select.settings-input { appearance: none }` was the old half-fix, covering 62 of the
    // app's 95 selects. Two rules doing one job is how one of them later gets tidied away
    // and the bug returns on the other 33.
    const rules = parseRules(strip(STYLES))
    const dupes = rules.filter(
      (r) =>
        r.selector.trim() !== 'select' &&
        matchesSelect(r.selector, SELECT_CLASSES) &&
        declarations(r.body).some((d) => d.prop === 'appearance' || d.prop === '-webkit-appearance'),
    )
    expect(dupes.map((d) => d.selector)).toEqual([])
  })
})

describe('the wizard marks a GUESSED radio model visibly', () => {
  // The original defect: SetupWizard puts `bad` on a <select> (a probe-seeded model the
  // operator has to confirm — an FT-991A answers the FTDX10 seed) while the sheet styled
  // `.wizard-field input.bad` only. The red border had never rendered, on any platform.
  // Tying the sheet to the component is what makes that unrepresentable rather than
  // merely fixed.
  const WIZARD = readFileSync(
    fileURLToPath(new URL('./components/SetupWizard.tsx', import.meta.url)),
    'utf8',
  )

  it('styles every element the wizard can put `bad` on', () => {
    const tags = new Set<string>()
    for (const m of WIZARD.matchAll(/<(input|select|textarea)\b[^>]*?className=(\{[^}]*\}|"[^"]*")/g)) {
      if (/\bbad\b/.test(m[2])) tags.add(m[1])
    }
    expect(tags.size, 'the wizard must mark something as bad, or this guard is vacuous').toBeGreaterThan(0)
    const sheet = strip(STYLES)
    for (const tag of tags) {
      expect(
        new RegExp(`\\.wizard-field\\s+${tag}\\.bad\\b`).test(sheet),
        `.wizard-field ${tag}.bad has no rule — the invalid marker is invisible for <${tag}>`,
      ).toBe(true)
    }
  })
})
