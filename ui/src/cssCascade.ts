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
//
// COLOUR ROLES (2026-09-26) add one more attribute per role (`data-accent='violet'`,
// features/paletteRoles.ts), and a mode may carry them after its base, space-separated:
// `light-high accent=violet ok=teal` is <html data-theme='light' data-contrast='high'
// data-accent='violet' data-ok='teal'>. MODES sweeps each base mode bare and under every
// PALETTE_SETS entry, so every guard that walks MODES also walks every preset.
//
// NIGHT (2026-09-27) is a third axis, `data-night='1'` (useNight.ts): it dims and warms WHATEVER
// theme is on, so each of the four base modes has a night twin — `dark-night`, `light-night`,
// `dark-night-high`, `light-night-high` — and the colour-role sets ride on those too. `dayOf`
// names a night mode's twin with Night off, which is what "night changed this" is measured from.
//
// THEMES (2026-09-27, features/skins.ts) are one more attribute, `data-skin='<id>'`, carried the
// way a role preset is: `dark-night skin=lagoon accent=violet`. A skin applies only on its own
// base theme, so its modes are that base's four. SKIN_MODES sweeps every skin in each of them,
// bare and under every PALETTE_SETS entry (styles-skins.test.ts walks it). MODES carries only the
// themes that are the WORST CASE for their base (SENTINEL_SKINS), in their base modes, so every
// guard that walks MODES also walks the themes without multiplying its runtime by eleven.

import { PALETTE_ROLES } from './features/paletteRoles'
import { SKINS, type SkinId } from './features/skins'

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

export type BaseMode =
  | 'dark'
  | 'light'
  | 'dark-high'
  | 'light-high'
  | 'dark-night'
  | 'light-night'
  | 'dark-night-high'
  | 'light-night-high'
/** A base mode, optionally followed by colour-role presets and a theme: `dark accent=blue readout=amber`,
 *  `light-night skin=paper`. */
export type Mode = BaseMode | `${BaseMode} ${string}`
export const BASE_MODES = [
  'dark',
  'light',
  'dark-high',
  'light-high',
  'dark-night',
  'light-night',
  'dark-night-high',
  'light-night-high',
] as const

/**
 * The colour-role sets MODES adds to every base mode: set k puts EVERY role on its k-th
 * non-default preset (wrapping for a role with fewer), so each preset of each role is swept at
 * least once, alongside presets of the other roles. The roles drive disjoint tokens, so a
 * property of one token is covered by the set its preset is in; a property BETWEEN roles (the
 * accent's ink on the OK fill) is checked over every preset pair in styles-palette-roles.test.ts,
 * where this covering is not relied on.
 */
export const PALETTE_SETS: readonly string[] = (() => {
  const most = Math.max(...PALETTE_ROLES.map((r) => r.presets.length - 1))
  return Array.from({ length: most }, (_, k) =>
    PALETTE_ROLES.filter((r) => r.presets.length > 1)
      .map((r) => `${r.id}=${r.presets[1 + (k % (r.presets.length - 1))].id}`)
      .join(' '),
  )
})()

/** The base modes a skin can be on: its own base theme's four. */
export const skinBaseModes = (id: SkinId): BaseMode[] => {
  const base = SKINS.find((s) => s.id === id)!.base
  return BASE_MODES.filter((b) => b.startsWith(base))
}

/** Every skin in each of its base modes, bare and under every colour-role set. */
export const SKIN_MODES: readonly Mode[] = SKINS.flatMap((s) =>
  ['', ...PALETTE_SETS.map((set) => ` ${set}`)].flatMap((set) =>
    skinBaseModes(s.id).map((b): Mode => `${b} skin=${s.id}${set}`),
  ),
)

/**
 * The themes MODES carries: for each base, the theme with the lightest (dark base) or darkest
 * (light base) value of each surface the sheet letters on. Lettering is lighter than a dark
 * theme's surfaces and darker than a light theme's — a cut-out's page-coloured letters included —
 * so every contrast only falls as a surface moves toward its ink, and a component that reads on
 * these themes reads on every other. Picked from the table, so a new theme that is a worse case
 * joins the sweep the day it lands: today Slate (the page) and Lagoon (the panel and both raised
 * surfaces) on dark, and Silver on light.
 */
export const SENTINEL_SKINS: readonly SkinId[] = (() => {
  const worst = new Set<SkinId>()
  for (const base of ['dark', 'light'] as const) {
    const skins = SKINS.filter((s) => s.base === base)
    for (const t of ['--bg', '--panel', '--bg-elev', '--bg-elev-2']) {
      const y = (s: (typeof skins)[number]) => luminance(parseHex(s.day[t])!)
      worst.add(skins.reduce((a, b) => ((base === 'dark' ? y(b) > y(a) : y(b) < y(a)) ? b : a)).id)
    }
  }
  return SKINS.map((s) => s.id).filter((id) => worst.has(id))
})()

/** Each of those themes in its base's four modes. */
export const SENTINEL_MODES: readonly Mode[] = SENTINEL_SKINS.flatMap((id) => skinBaseModes(id).map((b): Mode => `${b} skin=${id}`))

export const MODES: readonly Mode[] = [
  ...BASE_MODES,
  ...PALETTE_SETS.flatMap((set) => BASE_MODES.map((b): Mode => `${b} ${set}`)),
  ...SENTINEL_MODES,
]

export const baseOf = (m: Mode): BaseMode => m.split(' ')[0] as BaseMode
export const baseTheme = (m: Mode): 'dark' | 'light' => (m.startsWith('dark') ? 'dark' : 'light')
export const isHigh = (m: Mode): boolean => baseOf(m).endsWith('-high')
export const isNight = (m: Mode): boolean => baseOf(m).includes('-night')
/** `m` with Night off: the same theme, contrast and colour-role presets. */
export const dayOf = (m: Mode): Mode => m.replace('-night', '') as Mode

/** The role presets a mode carries, by role id. */
export function rolesOf(m: Mode): Record<string, string> {
  return Object.fromEntries(
    m
      .split(' ')
      .slice(1)
      .map((kv) => kv.split('=') as [string, string]),
  )
}

/** `m` with these role presets set (a later value replaces an earlier one). */
export function withRoles(m: Mode, roles: Record<string, string>): Mode {
  const all = { ...rolesOf(m), ...roles }
  const extra = Object.entries(all).map(([k, v]) => `${k}=${v}`)
  return [baseOf(m), ...extra].join(' ') as Mode
}

/** Every attribute selector <html> satisfies under `m`, spelled as the sheet spells it. Memoized:
 *  the resolvers ask once per rule, and a sheet has thousands. */
const ROOT_ATTRS = new Map<Mode, readonly string[]>()
function rootAttrsOf(m: Mode): readonly string[] {
  let out = ROOT_ATTRS.get(m)
  if (!out) {
    const attrs = [`[data-theme='${baseTheme(m)}']`]
    if (isHigh(m)) attrs.push(`[data-contrast='high']`)
    if (isNight(m)) attrs.push(`[data-night='1']`)
    // A preset or a theme also satisfies its attribute's bare presence (`[data-skin] .x`).
    for (const [role, preset] of Object.entries(rolesOf(m))) attrs.push(`[data-${role}='${preset}']`, `[data-${role}]`)
    ROOT_ATTRS.set(m, (out = attrs))
  }
  return out
}

/** `sel` with every attribute selector the root satisfies under `m` removed. */
function stripRootAttrs(sel: string, m: Mode): string {
  if (!sel.includes('[')) return sel
  let rest = sel
  for (const a of rootAttrsOf(m)) if (rest.includes(a)) rest = rest.split(a).join('')
  return rest
}

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

/** Does this selector match documentElement under `mode`? `data-theme`, `data-contrast`,
 *  `data-night` and the colour-role attributes are all set on document.documentElement, so
 *  `:root`, `html`, `[data-theme='…']`, `[data-contrast='high']`, `[data-night='1']` and
 *  `[data-accent='…']` all target the SAME element. A mode strips the attributes it carries; any
 *  other attribute a selector requires (a contrast or a night the mode is not in, a role preset it
 *  does not carry) leaves the selector unmatched. */
export function matchesRoot(sel: string, mode: Mode): boolean {
  if (!isHigh(mode) && sel.includes('[data-contrast=')) return false
  if (!isNight(mode) && sel.includes('[data-night=')) return false
  const rest = stripRootAttrs(sel.replace(/:root/g, '').replace(/^html/, ''), mode)
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

// ── Perceptual distance (2026-09-26, the colour roles) ────────────────────────────────────────
//
// Ported from ui/design/verify.mjs, the design system's own gate, so a guard here measures a
// preset with the arithmetic the palette was designed with: OKLab (Björn Ottosson's matrices),
// ΔE as Euclidean distance in it (≈0.02 is a just-noticeable difference), and Machado 2009's
// colour-vision-deficiency simulation at severity 1.0, applied in linear RGB.

/** Linear-light RGB, 0–1 per channel. */
type Linear = readonly [number, number, number]
const toLinear = (c: Rgb): Linear =>
  c.map((v) => {
    const s = v / 255
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4
  }) as unknown as Linear
const fromLinear = (x: number) =>
  Math.round(Math.min(1, Math.max(0, x <= 0.0031308 ? 12.92 * x : 1.055 * x ** (1 / 2.4) - 0.055)) * 255)

export function oklab(c: Rgb): readonly [number, number, number] {
  const [r, g, b] = toLinear(c)
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b)
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b)
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b)
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ] as const
}

/** OKLCH: lightness 0–1, chroma, hue in degrees 0–360. */
export function oklch(c: Rgb): { L: number; C: number; H: number } {
  const [L, a, b] = oklab(c)
  const H = (Math.atan2(b, a) * 180) / Math.PI
  return { L, C: Math.hypot(a, b), H: H < 0 ? H + 360 : H }
}

/** ΔE_OK — the Euclidean distance between two colours in OKLab. */
export function deltaE(x: Rgb, y: Rgb): number {
  const [a, b] = [oklab(x), oklab(y)]
  return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2])
}

export type Cvd = 'deutan' | 'protan' | 'tritan'
export const CVDS: readonly Cvd[] = ['deutan', 'protan', 'tritan']
const MACHADO: Record<Cvd, readonly (readonly number[])[]> = {
  protan: [[0.152286, 1.052583, -0.204868], [0.114503, 0.786281, 0.099216], [-0.003882, -0.048116, 1.051998]],
  deutan: [[0.367322, 0.860646, -0.227968], [0.280085, 0.672501, 0.047413], [-0.01182, 0.04294, 0.968881]],
  tritan: [[1.255528, -0.076749, -0.178779], [-0.078411, 0.930809, 0.147602], [0.004733, 0.691367, 0.3039]],
}

/** How `c` looks to an operator with this colour-vision deficiency (Machado 2009, severity 1). */
export function simulateCvd(c: Rgb, type: Cvd): Rgb {
  const lin = toLinear(c)
  return MACHADO[type].map((row) => fromLinear(row[0] * lin[0] + row[1] * lin[1] + row[2] * lin[2])) as unknown as Rgb
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

/** A compound that can only be <html> under `mode` — the theme, contrast, night and colour-role
 *  attributes, `:root`, `html`. Unlike `matchesRoot` it accepts a bare `[data-contrast='high']`,
 *  `[data-night='1']` or `[data-accent='violet']`, which are ancestors the well scope is written
 *  against. Any other
 *  root attribute (density, viewport) is a state this resolver does not know, so it does not
 *  match. */
function rootCompoundMatches(t: string, mode: Mode): boolean {
  if (!isHigh(mode) && t.includes('[data-contrast=')) return false
  if (!isNight(mode) && t.includes('[data-night=')) return false
  const rest = stripRootAttrs(t.replace(/:root/g, '').replace(/^html/, ''), mode)
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
