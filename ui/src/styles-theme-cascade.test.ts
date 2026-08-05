import { describe, it, expect } from 'vitest'
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { join } from 'node:path'

// THEME CASCADE GUARD — the guard that a presence test cannot be.
//
// Two defects shipped together and neither could be seen by looking at a declaration:
//
//   1. `.pounce-banner` painted `var(--bg-panel, #12161d)`. No sheet defines --bg-panel, so
//      BOTH themes rendered the hardcoded fallback — a dark-theme near-black — behind text
//      that inherits --text. Light mode was #14202e on #12161d: 1.18:1, invisible. The chip
//      and the button paint their own colours, so only the DATA went missing, which is
//      exactly how the operator reported it. `.update-banner` carried the same typo.
//
//   2. The eleven light --need-* inks were declared under [data-theme='light'] near the top
//      of the sheet and then re-declared by the `:root` palette 1500 lines LATER. Equal
//      specificity (0,1,0) — data-theme lives on documentElement, the same element :root
//      matches — so source order decided and every light override lost. Dead from the day they
//      landed (2026-07-06): light mode rendered the dark pastels on white until 2026-08-05.
//
// A test asserting `--need-entity: #c2187f` appears under [data-theme='light'] would have
// PASSED against the broken sheet: the declaration was there the whole time, it just lost.
// So this file resolves the cascade — matching selectors, specificity, then source order —
// and then measures contrast on what actually paints. Nothing here pins a hex; every
// assertion is a computed relationship, so re-tuning a colour is free and breaking one is not.

const CSS_PATH = fileURLToPath(new URL('./styles.css', import.meta.url))
const RAW = readFileSync(CSS_PATH, 'utf8')
// Prose must never read as a declaration, but line/offset arithmetic has to survive: blank
// the comment bodies in place rather than deleting them.
const CSS = RAW.replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))

interface Decl {
  prop: string
  value: string
}
interface Rule {
  selector: string
  decls: Decl[]
  order: number
  spec: readonly [number, number, number]
}

/** (ids, classes+attrs+pseudo-classes, types+pseudo-elements) — enough for the root
 *  selectors this file arbitrates, and it is the tie that mattered: `:root` and
 *  `[data-theme='light']` both score (0,1,0). */
function specificity(sel: string): readonly [number, number, number] {
  const attrs = (sel.match(/\[[^\]]*\]/g) || []).length
  const bare = sel.replace(/\[[^\]]*\]/g, ' ')
  const pseudoEls = (bare.match(/::[\w-]+/g) || []).length
  const s = bare.replace(/::[\w-]+/g, ' ')
  const ids = (s.match(/#[\w-]+/g) || []).length
  const cls = (s.match(/\.[\w-]+/g) || []).length + attrs + (s.match(/:[\w-]+/g) || []).length
  const typ = (s.replace(/[.#:][\w-]+/g, ' ').match(/[a-zA-Z][\w-]*/g) || []).length + pseudoEls
  return [ids, cls, typ] as const
}

const cmpSpec = (a: readonly number[], b: readonly number[]) =>
  a[0] - b[0] || a[1] - b[1] || a[2] - b[2]

function declsOf(body: string): Decl[] {
  const out: Decl[] = []
  let depth = 0
  let buf = ''
  for (const ch of body) {
    if (ch === '(') depth++
    else if (ch === ')') depth--
    if (ch === ';' && depth === 0) {
      const i = buf.indexOf(':')
      if (i > 0) out.push({ prop: buf.slice(0, i).trim(), value: buf.slice(i + 1).trim() })
      buf = ''
    } else buf += ch
  }
  const i = buf.indexOf(':')
  if (i > 0) out.push({ prop: buf.slice(0, i).trim(), value: buf.slice(i + 1).trim() })
  return out
}

/** Brace-aware rule walk. Descends into @media/@supports (a token redefined inside one still
 *  competes); skips @keyframes, whose frame selectors are not rules on elements. */
function parseRules(sheet: string, order = { n: 0 }): Rule[] {
  const out: Rule[] = []
  let i = 0
  let selStart = 0
  while (i < sheet.length) {
    if (sheet[i] !== '{') {
      i++
      continue
    }
    const sel = sheet.slice(selStart, i).trim().replace(/\s+/g, ' ')
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
      if (/^@(media|supports|layer|container)/.test(sel)) out.push(...parseRules(body, order))
    } else {
      const decls = declsOf(body)
      for (const one of sel.split(',').map((s) => s.trim()).filter(Boolean)) {
        out.push({ selector: one, decls, order: order.n++, spec: specificity(one) })
      }
    }
    selStart = i
  }
  return out
}

const RULES = parseRules(CSS)
const THEMES = ['dark', 'light'] as const
type Theme = (typeof THEMES)[number]

/** Does this selector match documentElement under `theme`? `data-theme` is set on
 *  document.documentElement (useTheme.ts), so `:root`, `html` and `[data-theme='…']` all
 *  target the SAME element — which is why their equal specificity was decisive. */
function matchesRoot(sel: string, theme: Theme): boolean {
  const rest = sel
    .replace(/:root/g, '')
    .replace(/^html/, '')
    .replace(new RegExp(`\\[data-theme='${theme}'\\]`, 'g'), '')
    .trim()
  if (rest !== '') return false
  return sel.includes(':root') || sel.startsWith('html') || sel.includes('[data-theme=')
}

/** The cascade, for custom properties on the root element. */
function rootTokens(theme: Theme): Map<string, string> {
  const win = new Map<string, { spec: readonly [number, number, number]; value: string }>()
  for (const r of RULES) {
    if (!matchesRoot(r.selector, theme)) continue
    for (const d of r.decls) {
      if (!d.prop.startsWith('--')) continue
      const prev = win.get(d.prop)
      // >= 0: equal specificity means the LATER declaration wins, which is the whole bug.
      if (!prev || cmpSpec(r.spec, prev.spec) >= 0) win.set(d.prop, { spec: r.spec, value: d.value })
    }
  }
  return new Map([...win].map(([k, v]) => [k, v.value]))
}

const TOKENS: Record<Theme, Map<string, string>> = {
  dark: rootTokens('dark'),
  light: rootTokens('light'),
}

/** Custom properties injected from TypeScript at runtime — invisible to a CSS parser, so they
 *  are legitimately absent from the sheet and must not read as undefined. */
const RUNTIME_TOKENS = (() => {
  const found = new Set<string>()
  const root = fileURLToPath(new URL('.', import.meta.url))
  const walk = (dir: string) => {
    for (const e of readdirSync(dir)) {
      const p = join(dir, e)
      if (statSync(p).isDirectory()) walk(p)
      else if (/\.tsx?$/.test(p) && !/\.test\.tsx?$/.test(p)) {
        for (const m of readFileSync(p, 'utf8').matchAll(/setProperty\(\s*['"`](--[\w-]+)/g)) {
          found.add(m[1])
        }
      }
    }
  }
  walk(root)
  return found
})()

/** Split a comma list at depth 0 (var()/color-mix() arguments nest). */
function topSplit(s: string): string[] {
  const out: string[] = []
  let depth = 0
  let buf = ''
  for (const ch of s) {
    if (ch === '(') depth++
    else if (ch === ')') depth--
    if (ch === ',' && depth === 0) {
      out.push(buf.trim())
      buf = ''
    } else buf += ch
  }
  if (buf.trim()) out.push(buf.trim())
  return out
}

/** Substitute every var() against a theme's resolved tokens, fallbacks included. */
function expand(value: string, theme: Theme, seen = new Set<string>()): string {
  let v = value
  for (let guard = 0; guard < 24; guard++) {
    const at = v.indexOf('var(')
    if (at === -1) return v
    let i = at + 4
    let depth = 1
    while (i < v.length && depth > 0) {
      if (v[i] === '(') depth++
      else if (v[i] === ')') depth--
      i++
    }
    const inner = v.slice(at + 4, i - 1)
    const [name, ...fb] = topSplit(inner)
    let repl: string
    if (seen.has(name)) repl = ''
    else {
      const declared = TOKENS[theme].get(name)
      const next = new Set(seen).add(name)
      repl =
        declared !== undefined
          ? expand(declared, theme, next)
          : fb.length
            ? expand(fb.join(','), theme, next)
            : ''
    }
    v = v.slice(0, at) + repl + v.slice(i)
  }
  return v
}

type Rgb = readonly [number, number, number]

function parseHex(h: string): Rgb | null {
  const m = /^#([0-9a-fA-F]{3,8})$/.exec(h.trim())
  if (!m) return null
  let d = m[1]
  if (d.length === 3 || d.length === 4) d = [...d].map((c) => c + c).join('')
  if (d.length !== 6 && d.length !== 8) return null
  return [0, 2, 4].map((i) => parseInt(d.slice(i, i + 2), 16)) as unknown as Rgb
}

/** A colour, already var()-expanded, over an opaque backdrop. Handles the subset this sheet
 *  uses for the surfaces under text: hex, rgb/rgba, transparent, and color-mix(in srgb, …). */
function toRgb(value: string, backdrop: Rgb): Rgb | null {
  const v = value.trim()
  if (v === '' || v === 'transparent') return backdrop
  const hex = parseHex(v)
  if (hex) return hex
  const rgb = /^rgba?\(([^)]*)\)$/.exec(v)
  if (rgb) {
    const parts = rgb[1].split(/[,/\s]+/).filter(Boolean).map(Number)
    const [r, g, b] = parts
    const a = parts.length > 3 ? parts[3] : 1
    return [0, 1, 2].map((i) => Math.round([r, g, b][i] * a + backdrop[i] * (1 - a))) as unknown as Rgb
  }
  const mixArgs = /^color-mix\(([\s\S]*)\)$/.exec(v)
  if (mixArgs) {
    const args = topSplit(mixArgs[1])
    if (args.length !== 3 || !/^in\s+srgb$/.test(args[0])) return null
    const one = (a: string): { c: Rgb | null; p: number | null } => {
      const pm = /\s([\d.]+)%$/.exec(a)
      return { c: toRgb(pm ? a.slice(0, pm.index) : a, backdrop), p: pm ? Number(pm[1]) / 100 : null }
    }
    const A = one(args[1])
    const B = one(args[2])
    if (!A.c || !B.c) return null
    const pa = A.p ?? (B.p !== null ? 1 - B.p : 0.5)
    return [0, 1, 2].map((i) => Math.round(A.c![i] * pa + B.c![i] * (1 - pa))) as unknown as Rgb
  }
  return null
}

const hex = (c: Rgb) => '#' + c.map((v) => v.toString(16).padStart(2, '0')).join('')

function luminance(c: Rgb): number {
  const f = (x: number) => {
    const s = x / 255
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4
  }
  return 0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2])
}
function contrast(fg: Rgb, bg: Rgb): number {
  const [hi, lo] = [luminance(fg), luminance(bg)].sort((a, b) => b - a)
  return (hi + 0.05) / (lo + 0.05)
}

/** The colour a token resolves to in a theme, over the theme's panel. */
function tokenRgb(token: string, theme: Theme): Rgb | null {
  const page = parseHex(expand('var(--bg)', theme)) ?? ([255, 255, 255] as const)
  return toRgb(expand(`var(${token})`, theme), page)
}
const panelOf = (theme: Theme) => tokenRgb('--panel', theme)!
const inkOf = (theme: Theme) => tokenRgb('--text', theme)!

const lastRule = (selector: string) =>
  [...RULES].reverse().find((r) => r.selector === selector)
const declOf = (selector: string, prop: string) =>
  [...RULES]
    .reverse()
    .find((r) => r.selector === selector && r.decls.some((d) => d.prop === prop))
    ?.decls.filter((d) => d.prop === prop)
    .slice(-1)[0]?.value

const TEXT_MIN = 4.5

describe('the light theme wins the --need-* palette (the equal-specificity bug)', () => {
  // Every --need-* declared under [data-theme='light'] anywhere in the sheet, paired with the
  // value that actually WINS in light mode. If a light declaration is outranked or outlived
  // by a later :root one, these disagree — which is precisely what shipped.
  const lightDeclared = new Map<string, string>()
  for (const r of RULES) {
    if (!matchesRoot(r.selector, 'light') || !r.selector.includes("[data-theme='light']")) continue
    for (const d of r.decls) if (d.prop.startsWith('--need-')) lightDeclared.set(d.prop, d.value)
  }

  it('declares a light value for every --need-* the default palette defines', () => {
    const defaults = [...TOKENS.dark.keys()].filter((k) => k.startsWith('--need-'))
    expect(defaults.length).toBeGreaterThanOrEqual(11)
    for (const tok of defaults) {
      expect(lightDeclared.get(tok), `${tok} has no [data-theme='light'] value`).toBeDefined()
    }
  })

  it.each([...lightDeclared.keys()].sort())('%s: the light declaration is what resolves', (tok) => {
    // Not "the declaration exists" — "the declaration is the cascade winner".
    expect(TOKENS.light.get(tok), `${tok} declared for light but a later rule wins`).toBe(
      lightDeclared.get(tok),
    )
  })

  it.each([...lightDeclared.keys()].sort())('%s: light and dark actually differ', (tok) => {
    // A light override that computes to the dark value is a fall-through wearing a
    // declaration. --need-ink inverts; the hues darken; none may match.
    expect(hex(tokenRgb(tok, 'light')!), `${tok} resolves identically in both themes`).not.toBe(
      hex(tokenRgb(tok, 'dark')!),
    )
  })
})

describe('the pounce and update banners are readable in BOTH themes', () => {
  // The operator's report: "the data does not show up if you are using Light instead of Dark."
  // Neither banner sets a `color`, so the ink is the inherited --text; the background is
  // computed here from whatever the sheet actually says.
  it.each(['.pounce-banner', '.update-banner'])('%s sets no color of its own', (sel) => {
    const rule = lastRule(sel)
    expect(rule, `${sel} not found`).toBeDefined()
    // If this ever fails, the assertion below must use the rule's own colour instead of --text.
    expect(rule!.decls.some((d) => d.prop === 'color')).toBe(false)
  })

  const pairs = ['.pounce-banner', '.update-banner'].flatMap((sel) =>
    THEMES.map((theme) => [sel, theme] as const),
  )
  it.each(pairs)('%s: text clears 4.5:1 on its background in %s', (sel, theme) => {
    const bg = declOf(sel, 'background') ?? declOf(sel, 'background-color')
    expect(bg, `${sel} paints no background`).toBeDefined()
    const surface = toRgb(expand(bg!, theme), panelOf(theme))
    expect(surface, `could not compute ${sel} background: ${expand(bg!, theme)}`).not.toBeNull()
    const ink = inkOf(theme)
    expect(
      contrast(ink, surface!),
      `${sel} in ${theme}: --text ${hex(ink)} on ${hex(surface!)}`,
    ).toBeGreaterThanOrEqual(TEXT_MIN)
  })
})

describe('every chip that FILLS itself with a --need-* colour keeps a readable ink', () => {
  // The third face of the same bug: a solid --need-* fill with a hardcoded ink is legible in
  // exactly one theme, because the palette inverts between them. Discovered from the sheet,
  // so a new chip is covered the day it lands.
  interface Fill {
    selector: string
    bg: string
    ink: string
  }
  const fills: Fill[] = []
  for (const r of RULES) {
    const bg = r.decls.filter((d) => d.prop === 'background' || d.prop === 'background-color').slice(-1)[0]
    if (!bg || !/^var\(--need-[\w-]+[\s,)]/.test(bg.value.trim())) continue
    // Own colour, else the colour of the base rule this one qualifies (`.spot-type-badge` for
    // `.spot-type-badge.type-pota`). No ink either way = decorative tick/dot, not text.
    const base = r.selector.replace(/\.[\w-]+$/, '')
    const ink = declOf(r.selector, 'color') ?? (base ? declOf(base, 'color') : undefined)
    if (ink) fills.push({ selector: r.selector, bg: bg.value, ink })
  }

  it('finds the known filled chips (the discovery cannot silently empty out)', () => {
    const found = fills.map((f) => f.selector)
    expect(found).toEqual(
      expect.arrayContaining([
        '.pounce-tag',
        '.pounce-work',
        '.decode-tag.newdxcc',
        '.decode-tag.newgrid',
        '.decode-tag.newband',
        '.spot-type-badge.type-pota',
        '.spot-type-badge.type-sota',
        '.spot-type-badge.type-dxped',
        '.np-mode-col.np-mode-digital',
        '.np-mode-col.np-mode-rtty',
        '.np-mode-col.np-mode-ft4',
      ]),
    )
  })

  const cases = fills.flatMap((f) => THEMES.map((t) => [f.selector, t, f] as const))
  it.each(cases)('%s in %s', (_sel, theme, f) => {
    const bg = toRgb(expand(f.bg, theme), panelOf(theme))
    const ink = toRgb(expand(f.ink, theme), bg ?? panelOf(theme))
    expect(bg, `background did not compute: ${expand(f.bg, theme)}`).not.toBeNull()
    expect(ink, `ink did not compute: ${expand(f.ink, theme)}`).not.toBeNull()
    expect(
      contrast(ink!, bg!),
      `${f.selector} in ${theme}: ink ${hex(ink!)} on fill ${hex(bg!)}`,
    ).toBeGreaterThanOrEqual(TEXT_MIN)
  })
})

describe('--need-* inks read as text on the panel they land on', () => {
  // The hues are also used bare as text (.need-chip, .decode-*, badges). --need-ink is the ink
  // FOR a fill, never a fill itself, so it is measured by the suite above instead.
  const hues = [...TOKENS.dark.keys()].filter((k) => k.startsWith('--need-') && k !== '--need-ink')
  const cases = hues.flatMap((h) => THEMES.map((t) => [h, t] as const))
  it.each(cases)('%s in %s', (tok, theme) => {
    const c = tokenRgb(tok, theme)!
    const panel = panelOf(theme)
    expect(
      contrast(c, panel),
      `${tok} ${hex(c)} on --panel ${hex(panel)} in ${theme}`,
    ).toBeGreaterThanOrEqual(TEXT_MIN)
  })
})

describe('no background hides behind an undefined token (the --bg-panel mechanism)', () => {
  it('every --need-* the sheet references is declared somewhere in it', () => {
    const declared = new Set(RULES.flatMap((r) => r.decls.map((d) => d.prop)))
    const referenced = new Set([...CSS.matchAll(/var\(\s*(--need-[\w-]+)/g)].map((m) => m[1]))
    const missing = [...referenced].filter((t) => !declared.has(t)).sort()
    expect(missing, `undeclared --need-* tokens: ${missing.join(', ')}`).toHaveLength(0)
  })

  it('no background hardcodes a SURFACE behind an undefined token', () => {
    // The general form of the operator's bug, and the reason it survived review: the fallback
    // made the declaration look finished, and it painted a dark-theme surface in BOTH themes
    // under text that follows the theme.
    //
    // The discriminator is NEUTRALITY, not opacity. A surface is a theme decision — every one
    // this app paints is a near-grey (--bg #0b0f17 / #e5eaf0, --panel #111722 / #fbfcfe) — so a
    // hardcoded low-chroma fallback is a second theme smuggled in behind a var(). A chromatic
    // fallback (var(--danger, #e5484d) on a meter fill) is a brand hue, reads in both themes,
    // and is a different question; a translucent one composites over the real surface. The
    // sheet carries ~25 of those and this guard deliberately says nothing about them.
    const chroma = (c: readonly number[]) => Math.max(...c) - Math.min(...c)
    const surfaces: string[] = []
    for (const r of RULES) {
      for (const d of r.decls) {
        if (d.prop !== 'background' && d.prop !== 'background-color') continue
        for (const m of d.value.matchAll(/var\(\s*(--[\w-]+)\s*,/g)) {
          const tok = m[1]
          if (TOKENS.dark.has(tok) || TOKENS.light.has(tok) || RUNTIME_TOKENS.has(tok)) continue
          const fb = topSplit(d.value.slice(m.index! + 4, d.value.indexOf(')', m.index!)))
            .slice(1)
            .join(',')
            .trim()
          const c = parseHex(fb)
          if (c && (fb.length === 4 || fb.length === 7) && chroma(c) < 40) {
            surfaces.push(`${r.selector} { ${d.prop}: ${d.value} }`)
          }
        }
      }
    }
    expect(
      surfaces,
      `a theme surface hardcoded as the fallback of an undefined token:\n${surfaces.join('\n')}`,
    ).toHaveLength(0)
  })

  it('the pounce and update banners reference only tokens that resolve', () => {
    const bad: string[] = []
    for (const r of RULES) {
      if (!/^\.(pounce|update)-/.test(r.selector)) continue
      const local = new Set(r.decls.map((d) => d.prop))
      for (const d of r.decls) {
        for (const m of d.value.matchAll(/var\(\s*(--[\w-]+)/g)) {
          const tok = m[1]
          if (local.has(tok) || RUNTIME_TOKENS.has(tok)) continue
          for (const theme of THEMES) {
            if (!TOKENS[theme].has(tok)) bad.push(`${r.selector} { ${d.prop} } -> ${tok} (${theme})`)
          }
        }
      }
    }
    expect(bad, `unresolvable tokens in the banner rules:\n${bad.join('\n')}`).toHaveLength(0)
  })
})
