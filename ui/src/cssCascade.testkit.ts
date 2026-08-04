// COMPUTING THE CASCADE WINNER OVER THE REAL SHEETS, for density guards.
//
// Lifted VERBATIM out of components/LogEntry.density.test.tsx (2026-08-04), which is now its
// first consumer, because CwCockpit.density.test.tsx is its second. A regex-presence CSS test
// is forbidden in this app (a dead selector passes one; two shipped that way), so a density
// guard has to resolve the winner — and a second hand-rolled copy of a cascade resolver is
// how the two copies start disagreeing about which rule wins.
//
// WHY IT EXISTS AT ALL: jsdom lays nothing out and its getComputedStyle neither substitutes
// `var()` nor expands a shorthand that carries one, so `padding: var(--space-1) var(--space-2)`
// — this sheet's own idiom — reads as "not declared". Selector MATCHING is jsdom's own
// (`Element.matches`), so `>` and `[data-viewport]` behave; only importance → specificity →
// source order is resolved here.
//
// WHAT IT DOES NOT DO: no layout. Nothing it returns is verified against a layout engine, so
// a guard built on it sums a stack in the block order the DOM actually renders and carries
// anything that WRAPS as a calibrated constant. Re-measure such a constant; never nudge it.
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

const FLAT: { rule: CSSStyleRule; order: number }[] = []
/** `:root` custom properties, resolved to px at --space-scale: 1 (the default viewport). */
export const TOKENS = new Map<string, string>()

// ── THE CANDIDATE INDEX, and why it cannot change an answer ────────────────────────────
// Resolving one property used to call `el.matches()` against every rule in a ~19 000-line
// sheet, and one pane-column measurement makes hundreds of lookups — ~1 s per measurement,
// which is a 5 s test timeout waiting to happen under a parallel run (it did, 2026-08-04).
//
// So each rule is filed once under the classes its RIGHTMOST COMPOUND requires. That is a
// NECESSARY condition and nothing more: `.pane-body > .cw-decode` can only ever match an
// element carrying `cw-decode`, whatever else is true. A lookup therefore tests only the rules
// filed under a class the element actually has, plus the ones that require no class at all
// (`body`, `button`, `:root`) — and the VERDICT is still `el.matches()`, jsdom's own selector
// engine, on every surviving candidate. The index can only ever remove rules that were going
// to return false; it can never add one or change a winner, so no number this file computes
// moves. There is no cache and nothing to invalidate: the DOM is read fresh every time.
//
// `:where()` / `:is()` / `:not()` contents are STRIPPED before the classes are read, because
// `:where(.a, .b)` requires neither `.a` nor `.b` on its own. Stripping only ever shrinks the
// required set, which keeps the condition necessary.
type Indexed = { rule: CSSStyleRule; order: number }
/** rules whose rightmost compound requires this class */
const BY_CLASS = new Map<string, Indexed[]>()
/** rules whose rightmost compound requires no class at all */
const UNFILED: Indexed[] = []

/** The classes REQUIRED by the rightmost compound of every selector in a selector list.
 *  Empty when any one selector in the list requires none — a rule is only filable if every
 *  branch of it is. */
function requiredClasses(selectorText: string): string[] {
  const required = new Set<string>()
  for (const sel of selectorText.split(',')) {
    const stripped = sel.replace(/:(?:where|is|not|has)\([^)]*\)/g, '')
    const last = stripped.split(/[\s>+~]+/).filter(Boolean).pop() ?? ''
    const classes = (last.match(/\.[\w-]+/g) ?? []).map((c) => c.slice(1))
    if (classes.length === 0) return [] // this branch can match a classless element
    // One class per branch is enough to file under; every branch must contribute one.
    required.add(classes[0])
  }
  return [...required]
}

function fileRule(entry: Indexed) {
  const classes = requiredClasses(entry.rule.selectorText)
  if (classes.length === 0) {
    UNFILED.push(entry)
    return
  }
  for (const c of classes) {
    const bucket = BY_CLASS.get(c)
    if (bucket) bucket.push(entry)
    else BY_CLASS.set(c, [entry])
  }
}

/** Every rule that COULD match `el`, in source order. A superset of the matching set. */
function candidates(el: Element): Indexed[] {
  const seen = new Set<Indexed>()
  const out: Indexed[] = []
  for (const e of UNFILED) {
    seen.add(e)
    out.push(e)
  }
  for (const c of Array.from(el.classList)) {
    for (const e of BY_CLASS.get(c) ?? []) {
      if (seen.has(e)) continue
      seen.add(e)
      out.push(e)
    }
  }
  return out.sort((a, b) => a.order - b.order)
}

/** Parse the app's real sheets into the flat, source-ordered rule list `css()` resolves over.
 *  Call once from `beforeAll`. Idempotent per module instance (vitest isolates by file). */
export function loadSheets(sheets: readonly string[] = ['styles.css', 'cockpit-panes.css']) {
  if (FLAT.length) return
  for (const rel of sheets) {
    const style = document.createElement('style')
    style.textContent = readFileSync(resolve(process.cwd(), 'src', rel), 'utf8')
    document.head.appendChild(style)
    const walk = (rules: CSSRuleList) => {
      for (let i = 0; i < rules.length; i++) {
        const r = rules[i]
        if ((r as CSSStyleRule).selectorText) {
          const entry = { rule: r as CSSStyleRule, order: FLAT.length }
          FLAT.push(entry)
          fileRule(entry)
        }
        else if ((r as CSSGroupingRule).cssRules) walk((r as CSSGroupingRule).cssRules)
      }
    }
    walk(style.sheet!.cssRules)
  }
  // The token table is the FIRST `:root` block only — the later ones are theme overrides
  // and the [data-viewport] blocks, none of which apply at the default window.
  for (const { rule } of FLAT) {
    if (rule.selectorText !== ':root') continue
    for (let i = 0; i < rule.style.length; i++) {
      const name = rule.style[i]
      if (name.startsWith('--') && !TOKENS.has(name)) TOKENS.set(name, rule.style.getPropertyValue(name).trim())
    }
  }
}

/** Specificity as one comparable number: (ids, classes+attrs+pseudo-classes, elements). */
function spec(sel: string): number {
  const s = sel.replace(/:where\([^)]*\)/g, '')
  const ids = (s.match(/#[\w-]+/g) ?? []).length
  const cls = (s.match(/\.[\w-]+|\[[^\]]+\]|:[a-z-]+(?:\([^)]*\))?/g) ?? []).filter(
    (t) => !t.startsWith('::'),
  ).length
  const el = (s.match(/(?:^|[\s>+~(])([a-z][\w-]*)/g) ?? []).length
  return ids * 10000 + cls * 100 + el
}

/** Substitute `var()` and evaluate the sheet's `calc(<n>px * <n>)` form. */
export function resolveVal(raw: string, depth = 0): string {
  if (depth > 8) return raw
  let v = raw.trim()
  v = v.replace(/var\(\s*(--[\w-]+)\s*(?:,\s*([^)]*))?\)/g, (_m, name, fallback) =>
    resolveVal(TOKENS.get(name) ?? fallback ?? '0', depth + 1),
  )
  v = v.replace(/calc\(\s*([\d.]+)px\s*\*\s*([\d.]+)\s*\)/g, (_m, a, b) => `${Number(a) * Number(b)}px`)
  return v.trim()
}

/** Split a resolved box shorthand into its four sides (CSS 1/2/3/4-value form). */
const SIDES = ['top', 'right', 'bottom', 'left'] as const
type Side = (typeof SIDES)[number]
function sideOf(shorthand: string, side: Side): string {
  const p = shorthand.split(/\s+/).filter(Boolean)
  if (p.length === 0) return '0'
  const four = p.length === 1 ? [p[0], p[0], p[0], p[0]]
    : p.length === 2 ? [p[0], p[1], p[0], p[1]]
      : p.length === 3 ? [p[0], p[1], p[2], p[1]]
        : p.slice(0, 4)
  return four[SIDES.indexOf(side)]
}

/** The value one RULE contributes for `prop`, falling back to the shorthand it lives in.
 *  jsdom expands a box shorthand into its longhands ONLY when the whole declaration parses,
 *  and `padding: var(--space-3) var(--space-4)` does not — so the sheet's own idiom would
 *  otherwise read as "not declared" and a whole model would silently measure zero. */
function fromRule(style: CSSStyleDeclaration, prop: string): string | null {
  const direct = style.getPropertyValue(prop)
  if (direct) return direct
  const box = /^(padding|margin)-(top|right|bottom|left)$/.exec(prop)
  if (box) {
    const sh = style.getPropertyValue(box[1])
    return sh ? sideOf(resolveVal(sh), box[2] as Side) : null
  }
  const bw = /^border-(top|right|bottom|left)-width$/.exec(prop)
  if (bw) {
    for (const sh of [style.getPropertyValue(`border-${bw[1]}`), style.getPropertyValue('border')]) {
      if (!sh) continue
      const v = resolveVal(sh)
      if (/\bnone\b/.test(v)) return '0px'
      const m = /(^|\s)(-?[\d.]+)(px)?(\s|$)/.exec(v)
      return m ? `${m[2]}px` : '0px'
    }
  }
  return null
}

/** The winning declaration of `prop` for a RENDERED element: every rule the real selector
 *  engine matches, ordered by importance, then specificity, then source order. */
export function css(el: Element, prop: string): string | null {
  let win: { value: string; important: boolean; spec: number; order: number } | null = null
  for (const { rule, order } of candidates(el)) {
    let matches = false
    try {
      matches = el.matches(rule.selectorText)
    } catch {
      continue
    }
    if (!matches) continue
    const value = fromRule(rule.style, prop)
    if (!value) continue
    const important = rule.style.getPropertyPriority(prop) === 'important'
    const s = spec(rule.selectorText)
    const better =
      win === null ||
      (important && !win.important) ||
      (important === win.important && (s > win.spec || (s === win.spec && order >= win.order)))
    if (better) win = { value, important, spec: s, order }
  }
  return win ? resolveVal(win.value) : null
}

/** `css` as a px number; 0 when the property is not declared anywhere that matches.
 *  A `min()` / `max()` of plain px lengths is evaluated — the sheet uses that form for
 *  floors that must yield (`min(10em, 100%)`); a percentage arm is skipped, because the
 *  box it resolves against is exactly what jsdom does not have. */
export function pxOf(el: Element, prop: string): number {
  const v = css(el, prop)
  if (!v) return 0
  const t = v.trim()
  const fn = /^(min|max)\(\s*(.+)\s*\)$/.exec(t)
  if (fn) {
    const arms = fn[2].split(',').map((a) => a.trim()).filter((a) => /^-?[\d.]+(px|em)?$/.test(a))
    const vals = arms.map((a) => lenPx(a, el))
    if (vals.length === 0) return 0
    return fn[1] === 'min' ? Math.min(...vals) : Math.max(...vals)
  }
  return lenPx(t, el)
}

/** One length in px. `em` resolves against the element's own inherited font size. */
function lenPx(raw: string, el: Element): number {
  const m = /^(-?[\d.]+)(px|em)?$/.exec(raw.trim())
  if (!m) return 0
  const n = Number(m[1])
  return m[2] === 'em' ? n * fontSizeOf(el) : n
}

/** The inherited font size of `el` in px: the nearest ancestor-or-self that declares one,
 *  falling back to the sheet's `body { font-size }`. */
export function fontSizeOf(el: Element): number {
  for (let e: Element | null = el; e; e = e.parentElement) {
    const v = css(e, 'font-size')
    if (!v) continue
    const m = /^(-?[\d.]+)px$/.exec(v.trim())
    if (m) return Number(m[1])
  }
  return 14 // styles.css `body { font-size: 14px }`
}

/** Top + bottom border of an element, resolved through the same cascade. */
export function borderY(el: Element): number {
  return pxOf(el, 'border-top-width') + pxOf(el, 'border-bottom-width')
}
export function borderTop(el: Element): number {
  return pxOf(el, 'border-top-width')
}
/** Top + bottom padding. */
export function padY(el: Element): number {
  return pxOf(el, 'padding-top') + pxOf(el, 'padding-bottom')
}
/** Top + bottom margin. */
export function marginY(el: Element): number {
  return pxOf(el, 'margin-top') + pxOf(el, 'margin-bottom')
}

/** The height of ONE line box on `el`: its resolved font-size through its resolved
 *  line-height, defaulting to the 1.2 a UA uses for `normal`. */
export function lineBox(el: Element): number {
  const fs = fontSizeOf(el)
  const lh = css(el, 'line-height')
  if (!lh) return Math.round(fs * 1.2)
  const t = lh.trim()
  if (/px$/.test(t)) return Math.round(Number(t.replace('px', '')))
  if (t === 'normal') return Math.round(fs * 1.2)
  return Math.round(Number(t) * fs)
}

/** Run `fn` with one token overridden — the shipped `--space-scale` walk. */
export function atToken<T>(name: string, value: string, fn: () => T): T {
  const prev = TOKENS.get(name)
  TOKENS.set(name, value)
  try {
    return fn()
  } finally {
    if (prev === undefined) TOKENS.delete(name)
    else TOKENS.set(name, prev)
  }
}
