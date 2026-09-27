// @vitest-environment jsdom
//
// #215 TEXT SIZE — Normal / Large / Larger, and the Touch density that rides beside it.
//
// The operator's pick (2026-09-26): "Normal / Large / Larger" = +0 % / +12 % / +25 %, applied to
// ALL text, the decode and log lists included, separate from UI scale. That last clause is the
// whole difficulty. The three `--fs-*` tokens are read by only about a third of the sheet's
// font sizes; the rest were written as bare pixels (the decode rows are `12px`, the log rows
// `13px`) or in `rem`, which follows the ROOT font size. A setting that only moved the tokens
// would grow some text on a screen and leave the lists the operator reads most exactly where
// they were — so the property this file holds is sheet-wide:
//
//   EVERY font size in styles.css, evaluated at each text size, is its Normal size times the
//   pick. A bare `font-size: 12px` added anywhere fails here, naming its selector.
//
// It is a COMPUTED check, not a presence check: each value is resolved through the root tokens
// the cascade gives under each `data-text-size` (the resolver below walks specificity and source
// order exactly as cssCascade.ts does for the theme axis) and then evaluated to pixels. Relative
// values (`em`, `%`, `inherit`) are relative to a parent that is itself held to the rule, so
// they scale by construction; `rem` is evaluated against the root size the sheet sets for each
// pick, so the root rule is held too.
//
// TOUCH (Settings ▸ Workspace ▸ Density ▸ Touch) is Comfortable plus finger-sized targets:
// `--touch` 44 → 48 px and bigger chips. It is an explicit attribute (`data-touch`, written by
// useDensity) and never `@media (pointer: coarse)`, which the responsive vocabulary forbids. The
// chip half is checked on RENDERED chips through the testkit's winner computation, because the
// chips carry their own paddings at several specificities and only the winner says whether
// Touch reached them.
import { beforeAll, afterEach, describe, expect, it } from 'vitest'
import { readFileSync, readdirSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { cmpSpec, expandWith, parseRules, type Rule } from './cssCascade'
import { loadSheets, pxOf } from './cssCascade.testkit'

// Comments blanked (line breaks kept), as the theme guards do: parseRules would otherwise carry
// a preceding comment into the next selector, and `:root` would never match.
const SHEET = readFileSync(resolve(process.cwd(), 'src/styles.css'), 'utf8').replace(
  /\/\*[\s\S]*?\*\//g,
  (m) => m.replace(/[^\n]/g, ' '),
)
const RULES: Rule[] = parseRules(SHEET)

/** The operator's picks. The CSS is held to these numbers, not the other way round. */
const PICKS = { normal: 1, large: 1.12, larger: 1.25 } as const
type Size = keyof typeof PICKS
const SIZES = Object.keys(PICKS) as Size[]

/** The root attributes a combination sets. Theme and contrast are not in play: no text-size
 *  or touch block may be scoped to one, and none is. */
interface RootState {
  textSize: Size
  density?: 'guided' | 'standard' | 'dense'
  touch?: boolean
}

/** Does `sel` target <html> in this state? Every compound must be satisfied by the root's own
 *  attributes; any other selector (a descendant, a theme block) does not set root tokens. */
function matchesRoot(sel: string, st: RootState): boolean {
  let rest = sel.trim()
  if (/[\s>+~]/.test(rest)) return false
  rest = rest.replace(/^html/, '').replace(/:root/g, '')
  const attrs = rest.match(/\[[^\]]*\]/g) ?? []
  if (rest.replace(/\[[^\]]*\]/g, '') !== '') return false
  for (const a of attrs) {
    const m = /^\[([\w-]+)(?:='([^']*)')?\]$/.exec(a)
    if (!m) return false
    const [, name, value] = m
    if (name === 'data-text-size' && value === st.textSize) continue
    if (name === 'data-density' && value === (st.density ?? 'standard')) continue
    if (name === 'data-touch' && st.touch && value === undefined) continue
    return false
  }
  return true
}

/** Every custom property on the root in this state, cascade computed (equal specificity: the
 *  LATER declaration wins). */
function rootTokens(st: RootState): Map<string, string> {
  const win = new Map<string, { spec: readonly [number, number, number]; value: string }>()
  for (const r of RULES) {
    if (!matchesRoot(r.selector, st)) continue
    for (const d of r.decls) {
      if (!d.prop.startsWith('--')) continue
      const prev = win.get(d.prop)
      if (!prev || cmpSpec(r.spec, prev.spec) >= 0) win.set(d.prop, { spec: r.spec, value: d.value })
    }
  }
  return new Map([...win].map(([k, v]) => [k, v.value]))
}

/** The root element's own declaration of `prop` in this state (the winner). */
function rootDecl(st: RootState, prop: string): string | null {
  let best: { spec: readonly [number, number, number]; value: string } | null = null
  for (const r of RULES) {
    if (!matchesRoot(r.selector, st)) continue
    for (const d of r.decls) {
      if (d.prop !== prop) continue
      if (!best || cmpSpec(r.spec, best.spec) >= 0) best = { spec: r.spec, value: d.value }
    }
  }
  return best?.value ?? null
}

/** A value relative to something outside this evaluation (a parent's font size). */
class Relative extends Error {}

/** Substitute every var() (fallbacks included) — the theme guards' own expansion. */
function expandAll(tokens: Map<string, string>, v: string): string {
  const out = expandWith(tokens, v)
  if (out.includes('var(')) throw new Error(`unresolved var() in ${v}`)
  return out
}

/** Evaluate a resolved length expression to px: calc/min/max/clamp over px, unitless, rem
 *  (against `remPx`) and vw (against `vwPx`). `em` and `%` are relative to the parent →
 *  Relative, unless `pctPx` gives `%` a base (the root's own font size does: 16 px). */
function evalPx(src: string, ctx: { remPx: number; vwPx: number; pctPx?: number }): number {
  let i = 0
  const s = src.trim()
  const ws = () => {
    while (i < s.length && /\s/.test(s[i])) i++
  }
  const expr = (): number => {
    let v = term()
    for (ws(); i < s.length && (s[i] === '+' || s[i] === '-'); ws()) {
      const op = s[i++]
      const r = term()
      v = op === '+' ? v + r : v - r
    }
    return v
  }
  const term = (): number => {
    let v = factor()
    for (ws(); i < s.length && (s[i] === '*' || s[i] === '/'); ws()) {
      const op = s[i++]
      const r = factor()
      v = op === '*' ? v * r : v / r
    }
    return v
  }
  const args = (): number[] => {
    const out: number[] = []
    ws()
    if (s[i] !== '(') throw new Error(`expected ( in ${s}`)
    i++
    for (;;) {
      out.push(expr())
      ws()
      if (s[i] === ',') {
        i++
        continue
      }
      if (s[i] === ')') {
        i++
        return out
      }
      throw new Error(`bad argument list in ${s}`)
    }
  }
  const factor = (): number => {
    ws()
    const fn = /^(calc|min|max|clamp)\b/.exec(s.slice(i))
    if (fn) {
      i += fn[1].length
      const a = args()
      if (fn[1] === 'calc') return a[0]
      if (fn[1] === 'min') return Math.min(...a)
      if (fn[1] === 'max') return Math.max(...a)
      return Math.min(Math.max(a[0], a[1]), a[2])
    }
    if (s[i] === '(') {
      i++
      const v = expr()
      ws()
      i++ // ')'
      return v
    }
    const num = /^(-?\d*\.?\d+)(px|rem|em|%|vw)?/.exec(s.slice(i))
    if (!num) throw new Error(`cannot read "${s.slice(i)}" in ${s}`)
    i += num[0].length
    const n = Number(num[1])
    switch (num[2]) {
      case undefined:
        return n
      case 'px':
        return n
      case 'rem':
        return n * ctx.remPx
      case 'vw':
        return (n * ctx.vwPx) / 100
      case '%':
        if (ctx.pctPx !== undefined) return (n * ctx.pctPx) / 100
        throw new Relative()
      default:
        throw new Relative()
    }
  }
  const v = expr()
  ws()
  if (i !== s.length) throw new Error(`trailing "${s.slice(i)}" in ${s}`)
  return v
}

/** A concrete effective viewport for the vw-based display size — any width does; the check
 *  is a ratio at a FIXED width. */
const VW_EFF = 1200

function context(size: Size) {
  const tokens = rootTokens({ textSize: size })
  tokens.set('--vw-eff', `${VW_EFF}px`)
  // The root's own font size: `%` there is of the UA's 16 px medium.
  const root = rootDecl({ textSize: size }, 'font-size')
  const remPx = root ? evalPx(expandAll(tokens, root), { remPx: 16, vwPx: VW_EFF, pctPx: 16 }) : 16
  return { tokens, remPx }
}

const near = (a: number, b: number) => Math.abs(a - b) < 1e-6

describe('#215 text size: the three picks resolve on the root', () => {
  it('--text-scale is exactly the pick under each data-text-size', () => {
    for (const size of SIZES) {
      const got = rootTokens({ textSize: size }).get('--text-scale')
      expect(got, `--text-scale is not declared for '${size}'`).toBeDefined()
      expect(Number(got), `'${size}' should scale text by ${PICKS[size]}`).toBe(PICKS[size])
    }
  })

  it('--fs-body / --fs-label / --fs-micro / --fs-title are the Normal size times the pick', () => {
    const NORMAL = { '--fs-title': 18, '--fs-body': 14, '--fs-label': 12, '--fs-micro': 11 }
    for (const size of SIZES) {
      const { tokens, remPx } = context(size)
      for (const [name, base] of Object.entries(NORMAL)) {
        const px = evalPx(expandAll(tokens, tokens.get(name)!), { remPx, vwPx: VW_EFF })
        expect(near(px, base * PICKS[size]), `${name} at '${size}' is ${px}px, want ${base * PICKS[size]}px`).toBe(true)
      }
    }
  })

  it('the root font size (what every rem is measured against) follows the pick', () => {
    for (const size of SIZES) {
      expect(context(size).remPx, `rem at '${size}'`).toBeCloseTo(16 * PICKS[size], 6)
    }
  })
})

/** Classes a component renders on SVG `<text>`/`<tspan>`. Text inside a drawing (the rotator's
 *  compass, the sky dome, the passband diagram) is sized in that drawing's own viewBox units,
 *  beside shapes and labels whose size is an SVG attribute; it follows UI scale with the drawing,
 *  like the waterfall's canvas text, and stays out of the text-size audit. The exemption is
 *  DERIVED from the components, so a rule cannot hide behind it by naming itself — only a class
 *  that really sits on SVG text is exempt. */
const SVG_TEXT_CLASSES = (() => {
  const dir = resolve(process.cwd(), 'src')
  const found = new Set<string>()
  /** Text from `from` up to the brace that closes the one at `from` (JSX expressions nest). */
  const braced = (s: string, from: number) => {
    let depth = 0
    for (let i = from; i < s.length; i++) {
      if (s[i] === '{') depth++
      else if (s[i] === '}' && --depth === 0) return s.slice(from + 1, i)
    }
    return ''
  }
  for (const rel of readdirSync(dir, { recursive: true }) as string[]) {
    if (!/\.tsx$/.test(rel) || /\.test\.tsx$/.test(rel)) continue
    const src = readFileSync(join(dir, rel), 'utf8')
    for (const m of src.matchAll(/<(?:text|tspan)\b/g)) {
      // The tag's own attributes: up to the first `>` outside a `{…}` expression.
      let depth = 0
      let end = m.index! + m[0].length
      for (; end < src.length; end++) {
        if (src[end] === '{') depth++
        else if (src[end] === '}') depth--
        else if (src[end] === '>' && depth === 0) break
      }
      const tag = src.slice(m.index!, end)
      const at = tag.indexOf('className=')
      if (at < 0) continue
      const rest = tag.slice(at + 'className='.length)
      // A plain string, or every string literal inside the expression (template holes removed).
      const value = rest.startsWith('{')
        ? [...braced(rest, 0).matchAll(/(["'`])((?:(?!\1)[^\\]|\\.)*)\1/g)]
            .map((l) => l[2].replace(/\$\{[^}]*\}/g, ' '))
            .join(' ')
        : (/^(["'])(.*?)\1/.exec(rest)?.[2] ?? '')
      for (const c of value.split(/\s+/)) if (/^[a-z][\w-]*$/i.test(c)) found.add(c)
    }
  }
  return found
})()
/** Every class the rule's target carries is an SVG-text class — so a modifier that also sits on
 *  SVG text (`is-live`) cannot carry an HTML rule into the exemption on its own. */
const isSvgText = (selector: string) => {
  const classes = (selector.split(/[\s>+~]+/).pop() ?? '').match(/\.[\w-]+/g) ?? []
  return classes.length > 0 && classes.every((c) => SVG_TEXT_CLASSES.has(c.slice(1)))
}

describe('#215 text size: EVERY font size in the sheet scales with the pick', () => {
  type Finding = { selector: string; value: string; normal: number; got: Record<Size, number> }

  function audit(rules: Rule[]): { checked: number; relative: number; wrong: Finding[] } {
    const ctx = Object.fromEntries(SIZES.map((s) => [s, context(s)])) as Record<Size, ReturnType<typeof context>>
    const wrong: Finding[] = []
    let checked = 0
    let relative = 0
    for (const r of rules) {
      for (const d of r.decls) {
        if (d.prop !== 'font-size') continue
        const raw = d.value.replace(/\s*!important\s*$/, '')
        if (/^(inherit|initial|unset|revert|smaller|larger)$/.test(raw)) {
          relative++
          continue
        }
        // The root's own size is held by the case above (it is what `rem` means).
        if (matchesRoot(r.selector, { textSize: 'normal' })) continue
        if (isSvgText(r.selector)) continue
        const got = {} as Record<Size, number>
        try {
          for (const s of SIZES) got[s] = evalPx(expandAll(ctx[s].tokens, raw), { remPx: ctx[s].remPx, vwPx: VW_EFF })
        } catch (e) {
          if (e instanceof Relative) {
            relative++
            continue
          }
          throw new Error(`${r.selector} { font-size: ${d.value} }: ${(e as Error).message}`)
        }
        checked++
        if (!SIZES.every((s) => near(got[s], got.normal * PICKS[s])))
          wrong.push({ selector: r.selector, value: d.value, normal: got.normal, got })
      }
    }
    return { checked, relative, wrong }
  }

  it('no font size is left behind', () => {
    const { checked, relative, wrong } = audit(RULES)
    // The sheet has ~990 font-size declarations; a parser that silently skipped them would
    // pass the assertion below with nothing checked.
    expect(checked + relative, 'the audit read almost none of the sheet').toBeGreaterThan(900)
    expect(
      wrong.map((w) => `${w.selector} { font-size: ${w.value} } → ${w.got.normal}px / ${w.got.large}px / ${w.got.larger}px`),
      'these font sizes do not follow Settings ▸ Workspace ▸ Text size. Write the size as a ' +
        '--fs-* token or as calc(<n>px * var(--text-scale)) — a bare px or a token-less calc ' +
        'stays put while the rest of the screen grows',
    ).toEqual([])
  })

  it('FIRES: a bare pixel font size planted in the sheet is reported by selector', () => {
    // The positive control, through the SAME audit: without it a green run says nothing about
    // whether a bare size can be caught at all.
    const planted = parseRules(`.planted-row { font-size: 12px; }\n.planted-rem { font-size: 0.8rem; }`)
    const { wrong } = audit(planted)
    expect(wrong.map((w) => w.selector)).toEqual(['.planted-row'])
  })

  it('the SVG-text exemption is exactly the drawing labels, and nothing else rides on it', () => {
    // Derived from the components, so this pins what the audit skips: if a class joined it,
    // someone put it on an SVG <text>, and it should be read before it is accepted.
    expect([...SVG_TEXT_CLASSES].sort()).toEqual([
      'rose-label',
      'rotor-cardinal',
      'sat-dome-compass',
      'sat-dome-rimtag-line',
      'sat-dome-ringlabel',
      'sat-dome-tag-line',
      'sat-pb-axistitle',
      'sat-pb-label',
      'sat-pb-shift',
      'sat-pb-ticklabel',
    ])
    // …and an ordinary rule is not exempt by accident (the control for the skip above).
    expect(isSvgText('.decode-row')).toBe(false)
    expect(isSvgText('.decode-row.is-live'), 'a shared modifier must not exempt an HTML rule').toBe(false)
    expect(isSvgText('.sat-dome .sat-dome-ringlabel')).toBe(true)
    expect(isSvgText('.getout-rose .rose-label')).toBe(true)
  })
})

describe('#215 text size: components that size text inline follow the pick too', () => {
  // A handful of components size text in a `style` object (the Field Day club board, the
  // club-sync-off pop-out) instead of the sheet, where the audit above cannot see them. A bare
  // number there is pixels and stays put; `textPx(n)` (useTextSize.ts) and `em` follow the pick.
  const BARE = /fontSize:\s*(?:[\w.]+\s*\?\s*)?\d+(?:\.\d+)?(?:\s*:\s*\d+(?:\.\d+)?)?\s*(?=[,}\n])|fontSize:\s*['"`]\d+(?:\.\d+)?px/g
  const scan = (label: string, src: string) =>
    src.split('\n').flatMap((line, i) => (line.match(BARE) ?? []).map((m) => `${label}:${i + 1} ${m.trim()}`))

  it('no component sizes text in bare pixels', () => {
    const dir = resolve(process.cwd(), 'src')
    const hits: string[] = []
    let files = 0
    for (const rel of readdirSync(dir, { recursive: true }) as string[]) {
      if (!/\.tsx$/.test(rel) || /\.test\.tsx$/.test(rel)) continue
      files++
      hits.push(...scan(rel, readFileSync(join(dir, rel), 'utf8')))
    }
    expect(files, 'the scan found almost no components').toBeGreaterThan(150)
    expect(hits, 'size these with textPx(n) or em, so they grow with Settings ▸ Workspace ▸ Text size').toEqual([])
  })

  it('FIRES: planted bare sizes are reported, and the scaled forms are not', () => {
    const planted = [
      'const A = { fontSize: 11, color: x }',
      'const B = { fontSize: big ? 20 : 13 }',
      "const C = { fontSize: '12px' }",
      'const D = { fontSize: textPx(12) }',
      "const E = { fontSize: '0.9em' }",
    ].join('\n')
    expect(scan('planted', planted).map((h) => h.split(' ')[0])).toEqual(['planted:1', 'planted:2', 'planted:3'])
  })
})

describe('Touch density: finger-sized targets from an explicit attribute', () => {
  it('--touch is 44px, and 48px only under data-touch', () => {
    expect(rootTokens({ textSize: 'normal' }).get('--touch')).toBe('44px')
    expect(rootTokens({ textSize: 'normal', density: 'guided', touch: true }).get('--touch')).toBe('48px')
    // Comfortable alone does not change the target size: Touch is Comfortable PLUS it.
    expect(rootTokens({ textSize: 'normal', density: 'guided' }).get('--touch')).toBe('44px')
  })

  it('Touch keeps Comfortable’s row spacing (it rides on data-density=guided)', () => {
    expect(rootTokens({ textSize: 'normal', density: 'guided', touch: true }).get('--density-scale')).toBe('1.18')
  })

  it('no rule anywhere reaches for the pointer or hover media features', () => {
    // The responsive vocabulary forbids size media; pointer media is the same trap (a
    // touchscreen laptop reports `fine`, a mouse on a tablet reports `coarse`).
    expect(SHEET).not.toMatch(/@media[^{]*\((?:any-)?(?:pointer|hover)\s*:/)
  })
})

describe('Touch density: the chips the operator taps get bigger (computed winners)', () => {
  // Measured in a real browser (1920×1080, every cockpit + Logbook + Settings): these are the
  // chip BUTTONS the app renders, 19–26 px tall at Standard.
  // [class, tag, wrapping ancestors outermost first] — the last case is Classic's Stations
  // panel, which compacts its filter chips with a (0,4,0) rule of its own.
  const CHIPS: [string, string, string[]][] = [
    ['theme-chip', 'button', []],
    ['theme-chip field-chip', 'button', []],
    ['od-chip', 'button', []],
    ['log-filter-chip', 'button', []],
    ['filter-chip', 'button', []],
    ['nb-chip', 'button', []],
    ['nb-chip nb-feed', 'button', []],
    ['cockpit-depth-chip', 'button', []],
    ['filter-chip', 'button', ['cockpit-lower classic', 'cockpit-roster']],
  ]
  const TOUCH_MIN_H = 36

  beforeAll(() => loadSheets())
  afterEach(() => {
    document.body.replaceChildren()
    document.documentElement.removeAttribute('data-touch')
    document.documentElement.removeAttribute('data-density')
  })

  const mount = (cls: string, tag: string, ancestors: string[] = []) => {
    let host: HTMLElement = document.body
    for (const a of ancestors) {
      const wrap = document.createElement('div')
      wrap.className = a
      host.appendChild(wrap)
      host = wrap
    }
    const el = document.createElement(tag)
    el.className = cls
    host.appendChild(el)
    return el
  }
  const height = (el: Element) => pxOf(el, 'min-height')
  const padY = (el: Element) => pxOf(el, 'padding-top') + pxOf(el, 'padding-bottom')

  it(`every tapped chip is floored at ${TOUCH_MIN_H}px under Touch, with more padding`, () => {
    const bad: string[] = []
    for (const [cls, tag, ancestors] of CHIPS) {
      const plain = mount(cls, tag, ancestors)
      const where = [...ancestors, cls].map((c) => '.' + c.split(' ').join('.')).join(' ')
      const before = { h: height(plain), pad: padY(plain) }
      document.documentElement.setAttribute('data-density', 'guided')
      document.documentElement.setAttribute('data-touch', '1')
      const after = { h: height(plain), pad: padY(plain) }
      document.documentElement.removeAttribute('data-touch')
      document.documentElement.removeAttribute('data-density')
      if (after.h < TOUCH_MIN_H) bad.push(`${where}: min-height ${after.h}px under Touch`)
      if (!(after.pad > before.pad)) bad.push(`${where}: padding ${before.pad}px → ${after.pad}px`)
      document.body.replaceChildren()
    }
    expect(bad).toEqual([])
  })

  it('without Touch the chips keep the sizes they ship with (the control)', () => {
    // The mirror of the case above: a touch rule that leaked out of its attribute would pass
    // it and fail this. `.theme-chip` ships at min-height 26px (operator, 2026-08-09).
    expect(height(mount('theme-chip', 'button'))).toBe(26)
    document.documentElement.setAttribute('data-density', 'guided')
    expect(height(mount('theme-chip', 'button')), 'Comfortable alone must not grow the chips').toBe(26)
  })
})
