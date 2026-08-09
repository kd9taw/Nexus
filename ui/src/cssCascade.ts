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
