// @vitest-environment jsdom
//
// SETTINGS USES THE WINDOW IT HAS — and nothing inside it stretches to fill one.
//
// THE REPORT (operator, 2026-08-12): "make the settings full screen. It's small and in the
// middle. You have the opportunity to put far more on the page vs scrolling."
//
// THE CAUSE. `.layout.single > .panel { max-width: 1100px; margin: 0 auto }` capped every
// single-panel view, and the only override widened Logbook and Program at xl. Settings was
// classed as "prose-shaped" and kept the 1100px measure — but Settings is 40 `.settings-grid`s
// of labelled controls, where width buys COLUMNS, not longer lines. A 1920 window classifies
// as **lg**, so it was rendering a 1100px panel in a 1900px space: 820px of gutter, paid for
// in scrolling. (An xl-only override would not have touched the reported window at all.)
//
// WHY THIS FILE EXISTS RATHER THAN A GREP. Widening a container is the easy half; the half
// that breaks things is everything inside it that had been bounded only by accident. Four
// shapes needed their own measure and one grid needed a different auto-placement mode, and
// every one of those is a rule that has to WIN a cascade against the base rule it overrides.
// A presence check ("the file contains max-width: 72ch") passes just as happily when the rule
// loses — which is how dead fixes shipped twice in this sheet. So this guard resolves the
// cascade itself and asks what value actually applies, per viewport tier.
//
// HOW IT COMPUTES. Declarations are parsed from the RAW sheet text (brace- and comment-aware)
// rather than read back through jsdom's CSSOM, because jsdom's CSSStyleDeclaration silently
// drops properties it does not implement — `grid-template-columns` among them, which is half
// the fix. Matching is still done by jsdom's REAL selector engine on REAL markup shapes, so
// `>`, attribute selectors and specificity behave as the browser would. Winner = importance,
// then specificity, then source order.
//
// WHAT IT DOES NOT PROVE. jsdom does not lay out. No pixel here is measured: "the panel fills
// a 1920 window" is not verified, only that nothing caps it at that tier, and "the grid gains
// three columns" is not verified, only that the track-sizing function that produces columns
// rather than stretch is the one that applies. Those need eyes on a real window.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { classifyViewport } from './useViewport'

const SRC = (rel: string): string => readFileSync(resolve(process.cwd(), 'src', rel), 'utf8')

// ── the sheet, parsed ───────────────────────────────────────────────────────────────────

interface Rule {
  selector: string
  decls: Map<string, { value: string; important: boolean }>
  order: number
}

/** Brace-aware rule walk over the raw text, descending into at-rule bodies. (The sheet's
 *  only at-rules are `prefers-reduced-motion` — size-based `@media` is banned by the layout
 *  contract and responsive-vocab.test.ts enforces that — so descending unconditionally
 *  cannot admit a rule that would not otherwise apply.) */
function parse(sheet: string): Rule[] {
  const out: Rule[] = []
  let i = 0
  const walk = (): void => {
    let selStart = i
    while (i < sheet.length) {
      const ch = sheet[i]
      if (ch === '}') {
        i++
        return
      }
      if (ch === '{') {
        const sel = sheet.slice(selStart, i).trim()
        i++
        if (sel.startsWith('@')) {
          walk()
        } else {
          const bodyStart = i
          while (i < sheet.length && sheet[i] !== '}' && sheet[i] !== '{') i++
          const body = sheet.slice(bodyStart, i)
          if (sheet[i] === '}') i++
          const decls = new Map<string, { value: string; important: boolean }>()
          for (const d of splitDecls(body)) {
            const c = d.indexOf(':')
            if (c <= 0) continue
            let value = d.slice(c + 1).trim()
            const important = /!important$/.test(value)
            if (important) value = value.replace(/!important$/, '').trim()
            decls.set(d.slice(0, c).trim().toLowerCase(), { value, important })
          }
          for (const one of splitTopLevel(sel, ',')) {
            const s = one.trim().replace(/\s+/g, ' ')
            if (s) out.push({ selector: s, decls, order: out.length })
          }
        }
        selStart = i
      } else {
        i++
      }
    }
  }
  walk()
  return out
}

/** Split on a top-level separator only — `minmax(220px, 1fr)` is ONE value, and
 *  `:where(a, b) .c` is ONE selector. Naively splitting on commas halves both. */
function splitTopLevel(s: string, sep: string): string[] {
  const out: string[] = []
  let depth = 0
  let start = 0
  for (let k = 0; k < s.length; k++) {
    const c = s[k]
    if (c === '(') depth++
    else if (c === ')') depth--
    else if (c === sep && depth === 0) {
      out.push(s.slice(start, k))
      start = k + 1
    }
  }
  out.push(s.slice(start))
  return out
}
const splitDecls = (body: string): string[] =>
  splitTopLevel(body, ';')
    .map((d) => d.trim())
    .filter(Boolean)

/** Specificity as one comparable number, for the selector shapes this sheet uses.
 *  `:where()` contributes zero by definition. */
function specificity(sel: string): number {
  const s = sel.replace(/:where\([^()]*\)/g, ' ')
  const ids = (s.match(/#[\w-]+/g) ?? []).length
  const cls =
    (s.match(/\.[\w-]+/g) ?? []).length +
    (s.match(/\[[^\]]*\]/g) ?? []).length +
    (s.match(/(?<!:):[\w-]+(?:\([^()]*\))?/g) ?? []).length
  const typ = (s.replace(/[.#[:][^\s>+~]*/g, ' ').match(/[a-zA-Z][\w-]*/g) ?? []).length
  return ids * 10000 + cls * 100 + typ
}

const RULES = parse(SRC('styles.css').replace(/\/\*[\s\S]*?\*\//g, ''))

/** The value of `prop` that ACTUALLY applies to `el`: the winner over every rule of the
 *  real sheet whose selector really matches, by importance → specificity → source order.
 *  `null` means no rule declares it (the property keeps its initial value). */
function applied(el: Element, prop: string): string | null {
  let best: { value: string; important: boolean; spec: number; order: number } | null = null
  for (const r of RULES) {
    const d = r.decls.get(prop)
    if (!d) continue
    let hit = false
    try {
      hit = el.matches(r.selector)
    } catch {
      continue // a selector this jsdom cannot parse cannot be applying either
    }
    if (!hit) continue
    const cand = { value: d.value, important: d.important, spec: specificity(r.selector), order: r.order }
    if (
      !best ||
      (cand.important !== best.important
        ? cand.important
        : cand.spec !== best.spec
          ? cand.spec > best.spec
          : cand.order >= best.order)
    ) {
      best = cand
    }
  }
  return best ? best.value : null
}

/** `max-width` as the operator's window would resolve it — `none` is the initial value, so
 *  "no rule declares it" and "a rule declares none" are the same answer and must read alike. */
const maxWidth = (el: Element): string => applied(el, 'max-width') ?? 'none'

// ── the markup, as SettingsPanel.tsx builds it ──────────────────────────────────────────
//
// Hand-built rather than rendered: SettingsPanel is a 8.4k-line form behind a full settings
// fetch, and the only thing this file needs from it is the nesting. `structure` below is what
// keeps that honest — it re-derives the same shapes from the real source, so a refactor that
// moves a note out of a section fails here instead of quietly making these boxes fictional.

const TIERS = ['xs', 'sm', 'md', 'lg', 'xl'] as const
type Tier = (typeof TIERS)[number]
/** The tiers where the panel is unbound — and, by construction, the only ones where any of
 *  the caps below are live. Everything else must be bit-identical to the shipped layout. */
const WIDE: Tier[] = ['md', 'lg', 'xl']
const NARROW: Tier[] = ['xs', 'sm']

interface Boxes {
  panel: HTMLElement
  grid: HTMLElement
  griddedField: HTMLElement
  griddedHint: HTMLElement
  scope: HTMLElement
  loneField: HTMLElement
  loneHint: HTMLElement
  note: HTMLElement
  connLog: HTMLElement
  roster: HTMLElement
}

function el(tag: string, cls: string, parent?: HTMLElement): HTMLElement {
  const n = document.createElement(tag)
  n.className = cls
  parent?.appendChild(n)
  return n
}

/** `<main class="layout single">` → the Settings panel → form → scroll → a section holding
 *  one of each shape the audit found. Attached to the document so ancestor and `>`
 *  combinators really match. */
function mount(): Boxes {
  document.body.innerHTML = ''
  const main = el('main', 'layout single')
  document.body.appendChild(main)
  const panel = el('section', 'panel settings-panel', main)
  const form = el('form', 'settings-form', panel)
  const scroll = el('div', 'settings-scroll', form)
  const section = el('fieldset', 'settings-section', scroll)

  const grid = el('div', 'settings-grid', section)
  const griddedField = el('div', 'settings-field', grid)
  const griddedHint = el('span', 'settings-hint', griddedField)
  const scope = el('div', 'settings-field settings-audio-scope', grid)

  // The ungridded shapes — direct children of the section, with no track to bound them.
  const loneField = el('div', 'settings-field', section)
  const loneHint = el('span', 'settings-hint', section)
  const note = el('p', 'settings-note', section)
  // Audited and deliberately left unbounded: row lists, not lines of text.
  const connLog = el('div', 'conn-log', section)
  const roster = el('div', 'radios-manager', section)

  return { panel, grid, griddedField, griddedHint, scope, loneField, loneHint, note, connLog, roster }
}

function at(tier: Tier): Boxes {
  document.documentElement.setAttribute('data-viewport', tier)
  return mount()
}

/** The other `.layout.single` panels, to prove the Settings override is scoped to Settings. */
function siblingPanel(classes: string): HTMLElement {
  document.body.innerHTML = ''
  const main = el('main', 'layout single')
  document.body.appendChild(main)
  return el('section', classes, main)
}

// ── what the operator asked for ─────────────────────────────────────────────────────────

describe('the Settings panel uses the window it has', () => {
  it('is unbound at every tier where the 1100px cap could bind (md/lg/xl)', () => {
    const bound = WIDE.filter((t) => maxWidth(at(t).panel) !== 'none')
      .map((t) => `${t} → max-width: ${maxWidth(at(t).panel)}`)
    expect(
      bound,
      'Settings is still capped at these tiers, so it renders small and centred with the ' +
        'rest of the window as gutter — 820px of it at 1920, which classifies as lg:\n  ' +
        `${bound.join('\n  ')}\n`,
    ).toEqual([])
  })

  // POSITIVE CONTROL #1 — the same resolver, on the same element, must still report the
  // SHIPPED value below the widening. If the override had been written unscoped (or if this
  // resolver simply answered "none" to everything), this is the assertion that fails.
  it('still resolves the shipped 1100px cap at xs/sm — the floor cannot have moved', () => {
    for (const t of NARROW) {
      expect(
        maxWidth(at(t).panel),
        `[data-viewport='${t}']: the widening leaked below 1100px effective. The 1024×768 ` +
          'supported floor must be bit-identical — the cap is inert there by arithmetic, ' +
          'and that is the whole argument for scoping the override to md+.',
      ).toBe('1100px')
    }
  })

  it('1024×768 at every UI zoom the app offers still classifies below the widening', () => {
    // The scoping argument is arithmetic, so compute the arithmetic rather than trust it:
    // at the supported floor no zoom setting can reach md. (Zooming IN shrinks the effective
    // width; zooming OUT is what could cross 1100 — 65% takes 1024 to 1575 effective, which
    // IS md, and that is correct: at 65% there really is 1575px of room to use.)
    const IN = [1, 1.1, 1.15, 1.2, 1.25, 1.5, 1.75]
    for (const z of IN) {
      expect(
        WIDE.includes(classifyViewport(1024 / z) as Tier),
        `1024px at ${Math.round(z * 100)}% zoom → ${classifyViewport(1024 / z)}: the floor ` +
          'crossed into a widened tier.',
      ).toBe(false)
    }
    expect(classifyViewport(1024 / 0.65)).toBe('md') // zoomed OUT, there is room — by design
  })

  // POSITIVE CONTROL #2 — scope. Widening Settings must not have widened the panels the
  // 1100px measure is still right for, nor disturbed the xl data-surface override.
  it('leaves the other .layout.single panels exactly as they were', () => {
    const wrong: string[] = []
    for (const t of TIERS) {
      document.documentElement.setAttribute('data-viewport', t)
      const expected: Record<string, string> = {
        'panel pota-view pota-hunter': '1100px', // prose-shaped, and still is
        'panel log-view logbook': t === 'xl' ? '1600px' : '1100px',
        'radioprog panel': t === 'xl' ? '1600px' : '1100px',
      }
      for (const [classes, want] of Object.entries(expected)) {
        const got = maxWidth(siblingPanel(classes))
        if (got !== want) wrong.push(`[data-viewport='${t}'] .${classes} → ${got} (expected ${want})`)
      }
    }
    expect(wrong, `the Settings widening leaked into other panels:\n  ${wrong.join('\n  ')}\n`).toEqual([])
  })
})

// ── and nothing inside it stretches to fill it ──────────────────────────────────────────

describe('what the width reaches inside Settings', () => {
  it('the grid packs into COLUMNS instead of stretching its fields', () => {
    // The regression the widening would otherwise have caused, and the reason this is not a
    // one-line change: `auto-fit` collapses the tracks that ended up empty and gives the room
    // to the survivors, so a section holding one field gets one window-wide text input.
    // `auto-fill` keeps the tracks, so a field is one column wide whatever its section's
    // field count. Below the widening the shipped `auto-fit` must be untouched.
    for (const t of WIDE) {
      expect(
        applied(at(t).grid, 'grid-template-columns'),
        `[data-viewport='${t}']: .settings-grid still auto-FITs inside an unbounded panel. ` +
          'The 4 single-field and 8 two-field sections will stretch one control across the ' +
          'whole window instead of adding columns.',
      ).toBe('repeat(auto-fill, minmax(220px, 1fr))')
    }
    for (const t of NARROW) {
      expect(
        applied(at(t).grid, 'grid-template-columns'),
        `[data-viewport='${t}']: the track-sizing change reached a tier whose panel is still ` +
          'capped — that moves the 1024×768 layout for no reason.',
      ).toBe('repeat(auto-fit, minmax(220px, 1fr))')
    }
  })

  it('prose gets a measure; lone controls and the scope get a ceiling', () => {
    const WANT: [keyof Boxes, string, string][] = [
      ['note', '72ch', 'a ~130-word note as one window-wide line'],
      ['loneHint', '72ch', 'a hint with nothing between it and the panel edge'],
      ['loneField', '520px', 'a <select>/.settings-input with no track to sit in'],
      ['scope', '720px', "MiniSpectrum's clientWidth × DPR canvas, spanning 1 / -1"],
    ]
    const wrong: string[] = []
    for (const t of WIDE) {
      const b = at(t)
      for (const [key, want, why] of WANT) {
        const got = maxWidth(b[key])
        if (got !== want) wrong.push(`[data-viewport='${t}'] ${key} → ${got} (expected ${want}) — ${why}`)
      }
    }
    expect(wrong, `unbounded inside an unbounded panel:\n  ${wrong.join('\n  ')}\n`).toEqual([])
  })

  it('none of the caps are live below the widening', () => {
    // The whole safety argument in one assertion: at xs/sm every one of these boxes must
    // resolve to the initial value, i.e. the layout that shipped.
    const live: string[] = []
    for (const t of NARROW) {
      const b = at(t)
      for (const key of ['note', 'loneHint', 'loneField', 'scope', 'griddedHint'] as (keyof Boxes)[]) {
        const got = maxWidth(b[key])
        if (got !== 'none') live.push(`[data-viewport='${t}'] ${key} → ${got}`)
      }
    }
    expect(
      live,
      `these caps reach the 1024×768 floor, where the panel was never widened:\n  ${live.join('\n  ')}\n`,
    ).toEqual([])
  })

  // POSITIVE CONTROL #3 — the resolver must be able to say "unbounded". Two shapes were
  // audited and deliberately left alone (they are lists of rows: width is room for the row's
  // own fields, not a longer line). If these came back capped, the caps above are landing on
  // more than they claim to and every "expected none" result above is worthless.
  it('the shapes the audit deliberately left alone are still unbounded, at every tier', () => {
    const capped: string[] = []
    for (const t of TIERS) {
      const b = at(t)
      for (const key of ['connLog', 'roster', 'grid'] as (keyof Boxes)[]) {
        const got = maxWidth(b[key])
        if (got !== 'none') capped.push(`[data-viewport='${t}'] ${key} → max-width: ${got}`)
      }
    }
    expect(
      capped,
      'a cap is landing on boxes the audit ruled out — the selectors are broader than they ' +
        `read:\n  ${capped.join('\n  ')}\n`,
    ).toEqual([])
  })
})

// ── the link between those boxes and the real panel ─────────────────────────────────────

describe('the shapes this file reasons about are the ones SettingsPanel.tsx builds', () => {
  const tsx = SRC('components/SettingsPanel.tsx').split('\n')
  const indent = (l: string): number => l.length - l.trimStart().length

  /** Direct children of every `.settings-section` in the real source, by JSX indentation. */
  function sectionChildren(): string[] {
    const out: string[] = []
    for (let i = 0; i < tsx.length; i++) {
      if (!/className="settings-section[ "]/.test(tsx[i])) continue
      const base = indent(tsx[i])
      for (let j = i + 1; j < tsx.length; j++) {
        const s = tsx[j]
        if (!s.trim()) continue
        if (indent(s) <= base && s.trimStart().startsWith('</')) break
        if (indent(s) === base + 2) {
          const m = /className="([^"]*)"/.exec(s)
          if (m) out.push(m[1])
        }
      }
    }
    return out
  }

  it('sections really do hold prose and controls OUTSIDE any grid', () => {
    // The premise of the 72ch / 520px caps. A hint or field inside a `.settings-grid` is
    // already bounded by its track and needs nothing; these are the ones that were bounded
    // only by the panel. If a refactor moves them all into grids, the caps become dead
    // weight and should go — this is what would tell us.
    const kids = sectionChildren()
    const count = (c: string) => kids.filter((k) => k === c).length
    expect(count('settings-note'), 'no ungridded .settings-note — the 72ch note cap is dead').toBeGreaterThan(0)
    expect(count('settings-hint'), 'no ungridded .settings-hint — the 72ch hint cap is dead').toBeGreaterThan(0)
    expect(count('settings-field'), 'no ungridded .settings-field — the 520px cap is dead').toBeGreaterThan(0)
  })

  it('the audio scope is still a full-span grid item', () => {
    expect(
      SRC('components/SettingsPanel.tsx'),
      '.settings-audio-scope no longer exists — its 720px ceiling is dead',
    ).toContain('className="settings-field settings-audio-scope"')
  })

  it('Settings still roots on the class list the width rules target', () => {
    // `.layout.single > .panel.settings-panel` is a `>` combinator against an exact class
    // pair; rename either half and the widening silently stops applying.
    expect(SRC('components/SettingsPanel.tsx')).toContain('className="panel settings-panel"')
    expect(SRC('App.tsx'), 'Settings is no longer mounted inside `.layout.single`').toMatch(
      /className="layout single"/,
    )
  })
})
