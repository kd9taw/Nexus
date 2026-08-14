// @vitest-environment jsdom
//
// A PLACEHOLDER MUST NOT READ AS AN ENTERED VALUE (issue #76: "Settings placeholders read as
// entered values in dark mode").
//
// THE MECHANISM. `::placeholder` is a pseudo-element with its own cascade, and it INHERITS
// `color` from its input when no `::placeholder` rule declares one. The sheet had exactly two
// such rules (`.composer-input`, `.np-search input`), so every other field — all 59 placeholder
// attributes in SettingsPanel.tsx, ~125 across the app — fell through to its input's own ink.
// `.settings-input` sets `color: var(--text)` (#e7edf6 on dark), and WebKit/Chromium derive the
// UA placeholder colour from currentColor, so a hint rendered at very nearly the brightness of
// a real value: the operator reads "127.0.0.1:5002" as a configured host, not as an example.
//
// WHY THIS FILE IS NOT A GREP. `grep '::placeholder' styles.css` passed before the fix (two
// rules) and would pass again against a rule that never wins — a dead selector, which is how
// dead CSS fixes shipped twice in this sheet. So this guard resolves the pseudo-element cascade
// the way a browser does: the winning `::placeholder` rule if one matches, otherwise the ink
// the input itself paints. The fall-through is a RESULT here, not an absence — which is exactly
// what the bug was.
//
// NO ITALIC — a deliberate call, recorded because the alternative was live. The fix could have
// made placeholders italic app-wide as a second, non-colour signal. It does not, for two
// reasons: the two placeholder rules already shipped set colour only, so italic would restyle
// two working inputs for something nobody reported; and most of these hints are literal
// examples of machine text — "COM16", "127.0.0.1:5002", "W9XYZ-9, KD9ABC" — where italic at
// 11-14px costs legibility precisely where character-by-character reading matters. The report
// is about contrast; the fix is contrast.
//
// WHAT IT DOES NOT PROVE. jsdom does not lay out or compute style — no pixel and no rendered
// colour is measured here. The app-wide sweep mounts each input by its class list alone, so a
// `color` an ANCESTOR sets is not modelled (the Settings suite mounts the real nesting); and
// nothing here can see a UA sheet, so "the browser's default placeholder colour" is inferred
// from the spec, not observed.
import { describe, it, expect } from 'vitest'
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join, resolve } from 'node:path'
import {
  MODES,
  cmpSpec,
  contrast,
  expandWith,
  parseHex,
  parseRules,
  rgbHex,
  rootTokensFrom,
  toRgb,
  type Mode,
  type Rgb,
  type Rule,
} from './cssCascade'

// jsdom leaves `import.meta.url` a non-file URL, so paths come off the vitest root (ui/).
const SRC_DIR = resolve(process.cwd(), 'src') + '/'
const read = (rel: string): string => readFileSync(join(SRC_DIR, rel), 'utf8')
/** Blank comment bodies in place — prose must never read as a declaration. */
const decomment = (s: string): string => s.replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))

// Both sheets main.tsx ships, parsed with ONE order counter so cross-sheet source order is the
// import order the bundle produces (styles.css, then cockpit-panes.css).
const ORDER = { n: 0 }
const RULES: Rule[] = [
  ...parseRules(decomment(read('styles.css')), ORDER),
  ...parseRules(decomment(read('cockpit-panes.css')), ORDER),
]
const TOKENS = Object.fromEntries(MODES.map((m) => [m, rootTokensFrom(RULES, m)])) as Record<
  Mode,
  Map<string, string>
>

// ── the pseudo-element cascade ──────────────────────────────────────────────────────────

/** The placeholder pseudo, in every spelling a sheet might use. */
const PLACEHOLDER = /::(?:-\w+-)?(?:input-)?placeholder$/
const hasPseudoEl = (sel: string): boolean => sel.includes('::')

interface Won {
  selector: string
  value: string
  important: boolean
  spec: readonly [number, number, number]
  order: number
}

/** The winner among the rules `pick` admits — importance, then specificity, then source order.
 *  `pick` returns the ORIGINATING-element half of a selector to match on, or null to skip it. */
function winner(el: Element, prop: string, pick: (sel: string) => string | null): Won | null {
  let best: Won | null = null
  for (const r of RULES) {
    const origin = pick(r.selector)
    if (origin === null) continue
    const d = r.decls.filter((x) => x.prop === prop).slice(-1)[0]
    if (!d) continue
    let hit = false
    try {
      hit = el.matches(origin === '' ? '*' : origin)
    } catch {
      continue // a selector this jsdom cannot parse cannot be applying either
    }
    if (!hit) continue
    const important = /!important\s*$/.test(d.value)
    const cand: Won = {
      selector: r.selector,
      value: d.value.replace(/!important\s*$/, '').trim(),
      important,
      spec: r.spec,
      order: r.order,
    }
    const beats =
      !best
        ? true
        : cand.important !== best.important
          ? cand.important
          : cmpSpec(cand.spec, best.spec) !== 0
            ? cmpSpec(cand.spec, best.spec) > 0
            : cand.order >= best.order
    if (beats) best = cand
  }
  return best
}

/** A property as it resolves for `el`'s ::placeholder pseudo-element. */
const placeholderProp = (el: Element, prop: string): Won | null =>
  winner(el, prop, (sel) => (PLACEHOLDER.test(sel) ? sel.replace(PLACEHOLDER, '') : null))

/** A property as it resolves for the ELEMENT itself (pseudo-element rules are not it). */
const elementProp = (el: Element, prop: string): Won | null =>
  winner(el, prop, (sel) => (hasPseudoEl(sel) ? null : sel))

interface Ink {
  /** 'placeholder' = a ::placeholder rule declares the colour; 'inherited' = it falls through
   *  to the input's own ink, which is the whole of issue #76. */
  via: 'placeholder' | 'inherited'
  selector: string
  value: string
}

/** The ink the placeholder text actually paints: the ::placeholder rule if one wins, else the
 *  colour the input paints (inherited by the pseudo-element), else the body ink. */
function placeholderInk(el: Element): Ink {
  const own = placeholderProp(el, 'color')
  if (own) return { via: 'placeholder', selector: own.selector, value: own.value }
  return { via: 'inherited', ...valueInk(el) }
}

/** The ink an ENTERED value paints in the same field. */
function valueInk(el: Element): { selector: string; value: string } {
  const w = elementProp(el, 'color')
  return w ? { selector: w.selector, value: w.value } : { selector: 'body', value: 'var(--text)' }
}

function rgbOf(value: string, mode: Mode): Rgb {
  const page = parseHex(expandWith(TOKENS[mode], 'var(--bg)')) ?? ([255, 255, 255] as const)
  const c = toRgb(expandWith(TOKENS[mode], value), page)
  expect(c, `"${value}" did not resolve to a colour in ${mode}`).not.toBeNull()
  return c!
}

/** The surface under the field's text — its own background, else the page. */
const fieldSurface = (el: Element, mode: Mode): Rgb =>
  rgbOf(elementProp(el, 'background')?.value ?? elementProp(el, 'background-color')?.value ?? 'var(--bg)', mode)

// ── the markup, derived from the real components ────────────────────────────────────────

function tsxFiles(): string[] {
  const out: string[] = []
  const walk = (dir: string): void => {
    for (const e of readdirSync(dir)) {
      const p = join(dir, e)
      if (statSync(p).isDirectory()) walk(p)
      else if (/\.tsx$/.test(p) && !/\.test\.tsx$/.test(p)) out.push(p)
    }
  }
  walk(SRC_DIR)
  return out
}

interface Field {
  tag: 'input' | 'textarea'
  classes: string
  where: string
}

/** Every `<input>`/`<textarea>` in a source file that carries a `placeholder=`, with the STATIC
 *  half of its className (interpolations dropped — `.invalid` and friends are state, not the
 *  shape). Tag text is walked brace- and quote-aware so a multi-line element with `{…}` props
 *  ends at its own `>`, not at one inside a handler. */
function placeholderFields(src: string, where: string): Field[] {
  const out: Field[] = []
  const re = /<(input|textarea)\b/g
  let m: RegExpExecArray | null
  while ((m = re.exec(src)) !== null) {
    let i = m.index + m[0].length
    let depth = 0
    let quote = ''
    for (; i < src.length; i++) {
      const c = src[i]
      if (quote) {
        if (c === quote) quote = ''
        continue
      }
      if (c === '"' || c === "'" || c === '`') quote = c
      else if (c === '{') depth++
      else if (c === '}') depth--
      else if (c === '>' && depth === 0) break
    }
    const tag = src.slice(m.index, i)
    if (!/\bplaceholder\s*=/.test(tag)) continue
    const cm = /className=(?:"([^"]*)"|\{`([^`]*)`\})/.exec(tag)
    const raw = (cm?.[1] ?? cm?.[2] ?? '').replace(/\$\{[^{}]*\}/g, ' ')
    const classes = raw
      .split(/\s+/)
      .filter((t) => /^[a-zA-Z][\w-]*$/.test(t))
      .join(' ')
    out.push({ tag: m[1] as Field['tag'], classes, where })
  }
  return out
}

const el = (tag: string, cls: string, parent?: HTMLElement): HTMLElement => {
  const n = document.createElement(tag)
  if (cls) n.className = cls
  parent?.appendChild(n)
  return n
}

/** A field mounted where SettingsPanel really puts it: `.layout.single` → the settings panel →
 *  form → scroll → section → `.settings-field`, so ancestor combinators match as they would. */
function inSettings(f: Field): HTMLElement {
  document.body.innerHTML = ''
  const main = el('main', 'layout single')
  document.body.appendChild(main)
  const panel = el('section', 'panel settings-panel', main)
  const form = el('form', 'settings-form', panel)
  const scroll = el('div', 'settings-scroll', form)
  const section = el('fieldset', 'settings-section', scroll)
  const grid = el('div', 'settings-grid', section)
  const field = el('label', 'settings-field', grid)
  const input = el(f.tag, f.classes, field)
  input.setAttribute('placeholder', 'e.g. COM16')
  return input
}

/** A field mounted by its class list alone — all the app-wide sweep claims to know. */
function bare(f: Field): HTMLElement {
  document.body.innerHTML = ''
  const input = el(f.tag, f.classes)
  document.body.appendChild(input)
  input.setAttribute('placeholder', 'e.g. COM16')
  return input
}

const SETTINGS = placeholderFields(read('components/SettingsPanel.tsx'), 'SettingsPanel.tsx')
const ALL = tsxFiles().flatMap((p) => placeholderFields(read(p.slice(SRC_DIR.length)), p.slice(SRC_DIR.length)))

// ── what the operator reported ──────────────────────────────────────────────────────────

describe('a Settings placeholder is styled as a placeholder, not left to the input ink', () => {
  it('the scan finds the fields it reasons about (it cannot silently empty out)', () => {
    // POSITIVE CONTROL for the derivation: every assertion below is `for (const f of …)`, so an
    // extractor that returned nothing would make this whole file green while proving nothing.
    expect(
      SETTINGS.length,
      'no placeholder-bearing input found in SettingsPanel.tsx — the extractor is broken, or ' +
        'the panel was refactored and these fixtures are now fiction',
    ).toBeGreaterThanOrEqual(50)
    expect(ALL.length, 'no placeholder-bearing input found anywhere in ui/src').toBeGreaterThanOrEqual(100)
  })

  it('a ::placeholder rule WINS for every one of them', () => {
    const fell = SETTINGS.filter((f) => placeholderInk(inSettings(f)).via === 'inherited').map(
      (f) => `<${f.tag} class="${f.classes}"> → ${placeholderInk(inSettings(f)).selector}`,
    )
    expect(
      [...new Set(fell)],
      'these fields declare no placeholder colour, so the pseudo-element inherits the input ink ' +
        'and the hint renders at very nearly the brightness of a real entered value — issue #76:' +
        `\n  ${[...new Set(fell)].join('\n  ')}\n`,
    ).toEqual([])
  })

  it('the placeholder ink is never the ink of an entered value, in any theme', () => {
    const same: string[] = []
    for (const mode of MODES) {
      for (const f of [...new Map(SETTINGS.map((f) => [f.classes + f.tag, f])).values()]) {
        const input = inSettings(f)
        const ph = rgbHex(rgbOf(placeholderInk(input).value, mode))
        const val = rgbHex(rgbOf(valueInk(input).value, mode))
        if (ph === val) same.push(`${mode} <${f.tag} class="${f.classes}"> → both ${ph}`)
      }
    }
    expect(
      same,
      `a hint and a configured value paint the same colour — indistinguishable:\n  ${same.join('\n  ')}\n`,
    ).toEqual([])
  })

  it('it is subordinate to the entered value, and still readable on the field', () => {
    // The relationship, not a hex: dimmer than a real value against the same well, and still
    // clear of the 4.5:1 text floor there. Re-tuning --text-faint is free; erasing the gap or
    // dimming past legibility is not.
    const bad: string[] = []
    for (const mode of MODES) {
      const input = inSettings({ tag: 'input', classes: 'settings-input', where: 'SettingsPanel.tsx' })
      const surface = fieldSurface(input, mode)
      const ph = contrast(rgbOf(placeholderInk(input).value, mode), surface)
      const val = contrast(rgbOf(valueInk(input).value, mode), surface)
      if (ph >= val) bad.push(`${mode}: placeholder ${ph.toFixed(2)}:1 ≥ value ${val.toFixed(2)}:1`)
      if (ph < 4.5) bad.push(`${mode}: placeholder ${ph.toFixed(2)}:1 on the field — below 4.5:1`)
    }
    expect(bad, `the hint/value distinction is wrong:\n  ${bad.join('\n  ')}\n`).toEqual([])
  })

  it('pins opacity: 1, or Firefox fades what the colour just fixed', () => {
    // Firefox applies its own opacity to ::placeholder; a colour alone lands washed out there.
    const input = inSettings({ tag: 'input', classes: 'settings-input', where: 'SettingsPanel.tsx' })
    expect(
      placeholderProp(input, 'opacity')?.value,
      'no ::placeholder rule sets opacity — the fix is Chromium-only',
    ).toBe('1')
  })
})

describe('the rest of the app', () => {
  // The bug was never Settings-only — Settings is where the operator hit it. Every input in the
  // app that shows a hint is swept, by the class list its component really gives it.
  const SHAPES = [...new Map(ALL.map((f) => [f.tag + '|' + f.classes, f])).values()]

  it('every placeholder in ui/src resolves through a ::placeholder rule', () => {
    const fell = SHAPES.filter((f) => placeholderInk(bare(f)).via === 'inherited').map(
      (f) => `<${f.tag} class="${f.classes}"> (${f.where}) → ${placeholderInk(bare(f)).selector}`,
    )
    expect(fell, `placeholders still inheriting an input's ink:\n  ${fell.join('\n  ')}\n`).toEqual([])
  })

  it('and stays readable on whatever well that field paints, in every theme', () => {
    // The blast radius of one bare selector, computed: a single ink now lands in ~33 different
    // fields, and some of them paint their own background. Anything that ends up under 4.5:1
    // there needs its own rule, not a shrug.
    const bad: string[] = []
    for (const mode of MODES) {
      for (const f of SHAPES) {
        const input = bare(f)
        const ratio = contrast(rgbOf(placeholderInk(input).value, mode), fieldSurface(input, mode))
        if (ratio < 4.5) bad.push(`${mode} <${f.tag} class="${f.classes}"> → ${ratio.toFixed(2)}:1`)
      }
    }
    expect(bad, `hints below the text floor on their own field:\n  ${bad.join('\n  ')}\n`).toEqual([])
  })
})

describe('positive controls — the resolver can say no, and the specific rules still win', () => {
  it('reports "inherited" for something that has no placeholder', () => {
    // If the bare `input, textarea` selector were written as `*::placeholder`, or if this
    // resolver simply answered "placeholder" to everything, this is what fails. A <div> can
    // never have a placeholder, so no rule may reach it.
    document.body.innerHTML = ''
    const note = el('p', 'settings-note')
    document.body.appendChild(note)
    expect(placeholderProp(note, 'color'), 'a placeholder rule is matching a <p>').toBeNull()
    expect(placeholderInk(note).via).toBe('inherited')
  })

  it('leaves the two rules that already shipped in charge of their own inputs', () => {
    // Specificity, computed rather than trusted: a bare `input::placeholder` is (0,0,2) and
    // must lose to both of these. If it won, the fix would have silently restyled two working
    // inputs — and the guard above would still be green.
    document.body.innerHTML = ''
    const composer = el('input', 'composer-input')
    document.body.appendChild(composer)
    expect(placeholderInk(composer).selector).toBe('.composer-input::placeholder')

    document.body.innerHTML = ''
    const wrap = el('span', 'np-search')
    document.body.appendChild(wrap)
    const search = el('input', '', wrap)
    expect(placeholderInk(search).selector).toBe('.np-search input::placeholder')
  })

  it('the ink token it uses exists in all four modes', () => {
    // The project's both-themes rule, computed: an undefined var() expands to empty and the
    // declaration is dropped, which is a fall-through wearing a rule.
    const input = inSettings({ tag: 'input', classes: 'settings-input', where: 'SettingsPanel.tsx' })
    for (const mode of MODES) {
      const v = expandWith(TOKENS[mode], placeholderInk(input).value)
      expect(parseHex(v), `the placeholder colour does not resolve in ${mode}: "${v}"`).not.toBeNull()
    }
  })
})
