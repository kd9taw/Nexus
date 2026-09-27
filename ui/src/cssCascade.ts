// The CSS cascade resolver the style guards share.
//
// Extracted VERBATIM from styles-theme-cascade.test.ts (2026-08-09) so the field-mode contrast
// guard could reuse it rather than grow a second, weaker parser — a presence-grade parser is
// exactly how the --need-* light palette stayed broken for a month behind a passing test. One
// resolver, every guard computes winners the same way.
//
// Everything here is pure and parameterized: rules come in, tokens come in, nothing reads
// module state. The test files own loading styles.css and deciding which MODES exist.
//
// MODES: the sheet is themed on two axes — `data-theme` (light|dark, useTheme.ts) and, since
// field mode, `data-contrast` ('high' present or absent, useFieldMode.ts). A "mode" names one
// combination. High-contrast blocks use two-attribute selectors
// (`[data-theme='dark'][data-contrast='high']`, specificity (0,2,0)) so they outrank every
// (0,1,0) theme palette regardless of source order — specificity is the fix that source order
// is not (the --need-* lesson).

export interface Decl {
  prop: string
  value: string
}
export interface Rule {
  selector: string
  decls: Decl[]
  order: number
  spec: readonly [number, number, number]
}

export type Mode = 'dark' | 'light' | 'dark-high' | 'light-high'
export const MODES = ['dark', 'light', 'dark-high', 'light-high'] as const
export const baseTheme = (m: Mode): 'dark' | 'light' => (m.startsWith('dark') ? 'dark' : 'light')
export const isHigh = (m: Mode): boolean => m.endsWith('-high')

/** (ids, classes+attrs+pseudo-classes, types+pseudo-elements) — enough for the root
 *  selectors these guards arbitrate, and it is the tie that mattered: `:root` and
 *  `[data-theme='light']` both score (0,1,0). */
export function specificity(sel: string): readonly [number, number, number] {
  const attrs = (sel.match(/\[[^\]]*\]/g) || []).length
  const bare = sel.replace(/\[[^\]]*\]/g, ' ')
  const pseudoEls = (bare.match(/::[\w-]+/g) || []).length
  const s = bare.replace(/::[\w-]+/g, ' ')
  const ids = (s.match(/#[\w-]+/g) || []).length
  const cls = (s.match(/\.[\w-]+/g) || []).length + attrs + (s.match(/:[\w-]+/g) || []).length
  const typ = (s.replace(/[.#:][\w-]+/g, ' ').match(/[a-zA-Z][\w-]*/g) || []).length + pseudoEls
  return [ids, cls, typ] as const
}

export const cmpSpec = (a: readonly number[], b: readonly number[]) =>
  a[0] - b[0] || a[1] - b[1] || a[2] - b[2]

export function declsOf(body: string): Decl[] {
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
export function parseRules(sheet: string, order = { n: 0 }): Rule[] {
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

/** Does this selector match documentElement under `mode`? `data-theme` and `data-contrast`
 *  are both set on document.documentElement, so `:root`, `html`, `[data-theme='…']` and
 *  `[data-contrast='high']` all target the SAME element. A high mode strips the contrast
 *  attribute; a normal mode REJECTS any selector that requires it. */
export function matchesRoot(sel: string, mode: Mode): boolean {
  const theme = baseTheme(mode)
  if (!isHigh(mode) && sel.includes('[data-contrast=')) return false
  let rest = sel
    .replace(/:root/g, '')
    .replace(/^html/, '')
    .replace(new RegExp(`\\[data-theme='${theme}'\\]`, 'g'), '')
  if (isHigh(mode)) rest = rest.replace(/\[data-contrast='high'\]/g, '')
  if (rest.trim() !== '') return false
  return sel.includes(':root') || sel.startsWith('html') || sel.includes('[data-theme=')
}

/** Resolve every custom property visible on the root under `mode`, cascade computed —
 *  equal specificity → the LATER declaration wins, which is the whole bug class. */
export function rootTokensFrom(rules: Rule[], mode: Mode): Map<string, string> {
  const win = new Map<string, { spec: readonly [number, number, number]; value: string }>()
  for (const r of rules) {
    if (!matchesRoot(r.selector, mode)) continue
    for (const d of r.decls) {
      if (!d.prop.startsWith('--')) continue
      const prev = win.get(d.prop)
      if (!prev || cmpSpec(r.spec, prev.spec) >= 0) win.set(d.prop, { spec: r.spec, value: d.value })
    }
  }
  return new Map([...win].map(([k, v]) => [k, v.value]))
}

/** Split a comma list at depth 0 (var()/color-mix() arguments nest). */
export function topSplit(s: string): string[] {
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

/** Substitute every var() against a resolved token map, fallbacks included. */
export function expandWith(tokens: Map<string, string>, value: string, seen = new Set<string>()): string {
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
      const declared = tokens.get(name)
      const next = new Set(seen).add(name)
      repl =
        declared !== undefined
          ? expandWith(tokens, declared, next)
          : fb.length
            ? expandWith(tokens, fb.join(','), next)
            : ''
    }
    v = v.slice(0, at) + repl + v.slice(i)
  }
  return v
}

export type Rgb = readonly [number, number, number]

export function parseHex(h: string): Rgb | null {
  const m = /^#([0-9a-fA-F]{3,8})$/.exec(h.trim())
  if (!m) return null
  let d = m[1]
  if (d.length === 3 || d.length === 4) d = [...d].map((c) => c + c).join('')
  if (d.length !== 6 && d.length !== 8) return null
  return [0, 2, 4].map((i) => parseInt(d.slice(i, i + 2), 16)) as unknown as Rgb
}

/** A colour, already var()-expanded, over an opaque backdrop. Handles the subset the sheet
 *  uses for the surfaces under text: hex, rgb/rgba, transparent, and color-mix(in srgb, …). */
export function toRgb(value: string, backdrop: Rgb): Rgb | null {
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

export const rgbHex = (c: Rgb) => '#' + c.map((v) => v.toString(16).padStart(2, '0')).join('')

export function luminance(c: Rgb): number {
  const f = (x: number) => {
    const s = x / 255
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4
  }
  return 0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2])
}

export function contrast(fg: Rgb, bg: Rgb): number {
  const [hi, lo] = [luminance(fg), luminance(bg)].sort((a, b) => b - a)
  return (hi + 0.05) / (lo + 0.05)
}

/**
 * Contrast under a veiling reflection — the outdoor model (#POTA field report).
 *
 * Ambient light reflecting off the panel adds a CONSTANT luminance term R to ink and surface
 * alike, compressing every ratio toward 1:1 — low-contrast pairs die first, which is why the
 * de-emphasised (--text-dim/--text-faint) two-thirds of the sheet's text is what daylight
 * erases. R is a fraction of full white: 0.15 ≈ open shade, 0.30 ≈ bright shade.
 */
export function contrastUnderGlare(fg: Rgb, bg: Rgb, r: number): number {
  const [a, b] = [luminance(fg) + r, luminance(bg) + r].sort((x, y) => y - x)
  return (a + 0.05) / (b + 0.05)
}

// ── Element-level resolution (2026-09-26, the ON AIR pill and the display wells) ────────────
//
// Everything above resolves the ROOT. A guard about one element — a pill's fill, the tokens
// inside a `.well` — needs the same cascade computed for an element somewhere below it. The
// element is described as a CHAIN, outermost ancestor first and the element itself last, and a
// guard should read that chain off a component it actually rendered (`chainOf`) rather than
// write one out: a selector is only as good as its reach, and only the rendered DOM says which
// classes an element really carries.

/** One element as a selector sees it. */
export interface El {
  tag: string
  classes: string[]
  attrs: Record<string, string>
}

/** A rendered element as an `El`. `class` and `style` are not attributes a sheet selects on here. */
export function elOf(node: Element): El {
  const attrs: Record<string, string> = {}
  for (const a of [...node.attributes]) if (a.name !== 'class' && a.name !== 'style') attrs[a.name] = a.value
  return { tag: node.tagName.toLowerCase(), classes: [...node.classList], attrs }
}

/** `node` and its ancestors below <body>, outermost first — the chain the resolvers take. */
export function chainOf(node: Element): El[] {
  const out: El[] = []
  for (let n: Element | null = node; n && n.tagName !== 'BODY' && n.tagName !== 'HTML'; n = n.parentElement) {
    out.unshift(elOf(n))
  }
  return out
}

/** One compound selector against one element. `:not(…)` is honoured; any other pseudo-class
 *  (:hover, :focus-visible) is a state the element is not in, and a pseudo-element is not it. */
export function compoundMatches(sel: string, el: El): boolean {
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

/** A compound that can only be <html> under `mode` — the theme and contrast attributes, `:root`,
 *  `html`. Unlike `matchesRoot` it accepts a bare `[data-contrast='high']`, which is an ancestor
 *  the well scope is written against. Any other root attribute (density, viewport) is a state
 *  this resolver does not know, so it does not match. */
function rootCompoundMatches(t: string, mode: Mode): boolean {
  if (!isHigh(mode) && t.includes('[data-contrast=')) return false
  let rest = t
    .replace(/:root/g, '')
    .replace(/^html/, '')
    .replace(new RegExp(`\\[data-theme='${baseTheme(mode)}'\\]`, 'g'), '')
  if (isHigh(mode)) rest = rest.replace(/\[data-contrast='high'\]/g, '')
  return rest !== t && rest.trim() === ''
}

/** Does `sel` reach the LAST element of `chain` under `mode`? The subject compound must match
 *  it; each ancestor compound must match, right to left, an element further up the chain (the
 *  parent exactly, after `>`) or the themed root. A sibling combinator never reaches. */
export function reachesChain(sel: string, chain: El[], mode: Mode): boolean {
  if (/[+~]/.test(sel.replace(/\[[^\]]*\]/g, '').replace(/\([^)]*\)/g, ''))) return false
  const tokens = sel.replace(/\s*>\s*/g, ' > ').split(/\s+/).filter(Boolean)
  const subject = tokens.pop()
  if (!subject || !compoundMatches(subject, chain[chain.length - 1])) return false
  let at = chain.length - 1 // the element the last matched compound landed on
  let child = false
  while (tokens.length) {
    const t = tokens.pop()!
    if (t === '>') {
      child = true
      continue
    }
    let found = -1
    for (let i = at - 1; i >= 0; i--) {
      if (compoundMatches(t, chain[i])) {
        found = i
        break
      }
      if (child) break
    }
    if (found === -1) {
      // Past the top of the chain the only element left is the root, which carries the theme.
      // A root compound must be the LAST one standing: nothing can sit above <html>.
      return tokens.length === 0 && (!child || at === 0) && rootCompoundMatches(t, mode)
    }
    at = found
    child = false
  }
  return true
}

const important = (v: string) => /!\s*important\s*$/.test(v)

/** The declaration that wins `prop` on the chain's last element under `mode`: `!important`
 *  first, then specificity, then source order. Pass several props for a shorthand and its
 *  longhands (`background` + `background-color`): they compete as one cascade, and the returned
 *  `prop` says which one won. */
export function winnerAt(
  rules: Rule[],
  mode: Mode,
  chain: El[],
  ...props: string[]
): { rule: Rule; prop: string; value: string } | null {
  let win: { rule: Rule; prop: string; value: string } | null = null
  for (const rule of rules) {
    const d = rule.decls.filter((x) => props.includes(x.prop)).pop()
    if (!d || !reachesChain(rule.selector, chain, mode)) continue
    const beats =
      !win ||
      (important(d.value) !== important(win.value)
        ? important(d.value)
        : cmpSpec(rule.spec, win.rule.spec) > 0 || (cmpSpec(rule.spec, win.rule.spec) === 0 && rule.order > win.rule.order))
    if (beats) win = { rule, prop: d.prop, value: d.value.replace(/!\s*important\s*$/, '').trim() }
  }
  return win
}

/** The custom properties visible on the chain's last element under `mode`, with CSS's own
 *  semantics: a token the element INHERITS arrives already computed by the element that
 *  declared it, while a token declared ON an element (its cascade winner there) resolves its
 *  var()s against that element's own tokens. The difference is the whole reason a scope such as
 *  `.well` has to re-declare `--state-good` and not only `--snr-strong`: the alias was computed
 *  on <html>, and a descendant that re-declares only the target inherits the old answer. */
export function tokensAt(rules: Rule[], mode: Mode, chain: El[]): Map<string, string> {
  const root = rootTokensFrom(rules, mode)
  let tokens = new Map([...root].map(([k, v]) => [k, expandWith(root, v)]))
  for (let i = 0; i < chain.length; i++) {
    const at = chain.slice(0, i + 1)
    const declared = new Map<string, { rule: Rule; value: string }>()
    for (const rule of rules) {
      if (!rule.decls.some((d) => d.prop.startsWith('--')) || !reachesChain(rule.selector, at, mode)) continue
      for (const d of rule.decls) {
        if (!d.prop.startsWith('--')) continue
        const prev = declared.get(d.prop)
        if (!prev || cmpSpec(rule.spec, prev.rule.spec) > 0 || (cmpSpec(rule.spec, prev.rule.spec) === 0 && rule.order > prev.rule.order)) {
          declared.set(d.prop, { rule, value: d.value })
        }
      }
    }
    if (declared.size === 0) continue
    const own = new Map(tokens)
    for (const [k, { value }] of declared) own.set(k, value)
    const computed = new Map(tokens)
    for (const k of declared.keys()) computed.set(k, expandWith(own, own.get(k)!))
    tokens = computed
  }
  return tokens
}
