import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

// Guards the DEFICIT VALVE (2026-07-30 layout assessment, mechanism C1/C2): every
// non-Operate cockpit shell must resolve to `overflow-y: auto` so a genuine vertical
// deficit SCROLLS instead of clipping the log form / PTT tail unreachably.
//
// Why a cascade COMPUTER and not a regex: two scroll "fixes" shipped dead —
//   1. `.phone-cockpit { overflow-y:auto }` lost to `.layout.single.phone-cockpit
//      { overflow:hidden }` on specificity ((0,1,0) vs (0,3,0)), and
//   2. `.layout.single.cw-cockpit` declared `overflow-y:auto` and then reset it with
//      `overflow:hidden` LATER IN THE SAME BLOCK (the shorthand wins by declaration order).
// A regex sees the `auto` in both cases and passes. This test parses the sheet
// (brace/comment-aware, @media-aware) and computes the winning overflow-y the way the
// cascade does: same-block declaration order incl. the `overflow` shorthand resetting
// `overflow-y`, then specificity, then source order.

const css = readFileSync(fileURLToPath(new URL('./styles.css', import.meta.url)), 'utf8')
  // Strip comments first so prose can't be read as declarations (same trap documented
  // in cockpit-floors.test.ts).
  .replace(/\/\*[\s\S]*?\*\//g, '')

interface Rule {
  selector: string // one selector (lists are split on top-level commas)
  body: string
  order: number
  media: string | null // non-null ⇒ conditional; excluded from the unconditional cascade
}

/** Brace-aware rule walk. Handles @media/@supports nesting (rules inside carry the
 *  condition) and skips other at-rule bodies (@keyframes etc.) wholesale. */
function parseRules(sheet: string): Rule[] {
  const out: Rule[] = []
  let i = 0
  let order = 0
  const n = sheet.length

  function skipBalanced(): void {
    // positioned just past an opening '{'
    let depth = 1
    while (i < n && depth > 0) {
      if (sheet[i] === '{') depth++
      else if (sheet[i] === '}') depth--
      i++
    }
  }

  function parseBlock(media: string | null): void {
    let selStart = i
    while (i < n) {
      const ch = sheet[i]
      if (ch === '}') {
        i++
        return
      }
      if (ch === '{') {
        const sel = sheet.slice(selStart, i).trim()
        i++
        if (sel.startsWith('@')) {
          if (/^@(media|supports)\b/.test(sel)) parseBlock(sel)
          else skipBalanced()
        } else {
          const bodyStart = i
          while (i < n && sheet[i] !== '}' && sheet[i] !== '{') i++
          const body = sheet.slice(bodyStart, i)
          if (sheet[i] === '}') i++
          order++
          for (const s of sel.split(',')) {
            const one = s.trim().replace(/\s+/g, ' ')
            if (one) out.push({ selector: one, body, order, media })
          }
        }
        selStart = i
      } else if (ch === ';') {
        // stray at-statement (@import etc.)
        i++
        selStart = i
      } else {
        i++
      }
    }
  }

  parseBlock(null)
  return out
}

const RULES = parseRules(css)

/** One compound of class selectors ('.a.b') as its class list; null when the compound
 *  contains anything else (pseudos, attributes, tags, ids, or is empty). Rejecting is
 *  the fail-safe direction here: a selector this computer cannot evaluate must never
 *  count as a cascade winner by accident. */
function compoundClasses(compound: string): string[] | null {
  if (/[\s>+~:[#]/.test(compound)) return null
  const parts = compound.match(/\.[a-zA-Z0-9_-]+/g)
  if (!parts || parts.join('') !== compound) return null
  return parts.map((p) => p.slice(1))
}

/** Right-to-left match of a class-only selector (descendant/child combinators OK)
 *  against an explicit ancestor CHAIN (outermost → the element; each entry is that
 *  ancestor's class set). The first computer here matched bare compounds only, which
 *  left the strongest known override INVISIBLE to the guard: `.app.detached >
 *  .layout.single { overflow: hidden }` is (0,4,0) — it outranks every (0,3,0) shell
 *  rule — and the census (overflow-cascade #8) had already named it a silent trap.
 *  Combinator rules now participate; that one correctly does not match the main-window
 *  chain (the shells' parent is `.shell`, and `.app` never carries `detached` there).
 *  Descendant matching walks up the chain, so a modeled chain may omit unclassed
 *  wrapper divs; a child combinator checks the entry just above, which slightly
 *  over-matches on such gaps — the fail-safe direction for a guard. Selectors with
 *  sibling combinators or pseudos never match (none targets these elements today).
 *  ⚠️ If a cockpit ever becomes detachable, add its `.app.detached`-rooted chain to
 *  SHELLS — the detached rule above wins there and the valve dies in that window. */
function matchesChain(selector: string, chain: Array<Set<string>>): boolean {
  const parts = selector.split(/\s*([>+~])\s*|\s+/).filter((p): p is string => !!p)
  const subj = compoundClasses(parts[parts.length - 1] ?? '')
  if (!subj || !subj.every((c) => chain[chain.length - 1].has(c))) return false
  let idx = chain.length - 2
  let childOnly = false
  for (let i = parts.length - 2; i >= 0; i--) {
    const p = parts[i]
    if (p === '>') {
      childOnly = true
      continue
    }
    if (p === '+' || p === '~') return false
    const comp = compoundClasses(p)
    if (!comp) return false
    if (childOnly) {
      if (idx < 0 || !comp.every((c) => chain[idx].has(c))) return false
      idx--
      childOnly = false
    } else {
      while (idx >= 0 && !comp.every((c) => chain[idx].has(c))) idx--
      if (idx < 0) return false
      idx--
    }
  }
  return true
}

/** Class-count specificity — every candidate here is a class-only compound. */
function specificity(selector: string): number {
  return (selector.match(/\./g) ?? []).length
}

/** Final overflow-y a block computes, honouring in-block declaration order and the
 *  `overflow` shorthand (its y value is the last of up to two values). */
function blockOverflowY(body: string): string | null {
  let v: string | null = null
  for (const decl of body.split(';')) {
    const m = /^\s*(overflow(?:-y)?)\s*:\s*(\S[^]*?)\s*$/.exec(decl)
    if (!m) continue
    if (m[1] === 'overflow') {
      const parts = m[2].split(/\s+/)
      v = parts.length === 2 ? parts[1] : parts[0]
    } else {
      v = m[2]
    }
  }
  return v
}

/** Cascade winner of overflow-y for the element at the end of `chain`. `activeMedia`
 *  names the one media condition considered matched (null = none); a conditional rule
 *  adds no specificity, it simply participates when its condition is active. */
function winningOverflowY(
  chain: Array<Set<string>>,
  activeMedia: string | null,
): { value: string; selector: string } | null {
  let win: { value: string; selector: string; spec: number; order: number } | null = null
  for (const r of RULES) {
    if (r.media !== null && r.media !== activeMedia) continue
    if (!matchesChain(r.selector, chain)) continue
    const v = blockOverflowY(r.body)
    if (v === null) continue
    const spec = specificity(r.selector)
    if (!win || spec > win.spec || (spec === win.spec && r.order >= win.order)) {
      win = { value: v, selector: r.selector, spec, order: r.order }
    }
  }
  return win && { value: win.value, selector: win.selector }
}

/** Every distinct media condition that carries an overflow-declaring rule matching
 *  the element — each is a cascade context the valve must survive. */
function mediaContexts(chain: Array<Set<string>>): Array<string | null> {
  const out = new Set<string | null>([null])
  for (const r of RULES) {
    if (r.media !== null && matchesChain(r.selector, chain) && blockOverflowY(r.body) !== null) {
      out.add(r.media)
    }
  }
  return [...out]
}

/** Subject (rightmost compound) of a selector. */
function subject(selector: string): string {
  const parts = selector.split(/\s*[>+~]\s*|\s+/)
  return parts[parts.length - 1]
}

/** Rules whose subject carries the given class (i.e. rules that style that element). */
function rulesOn(cls: string): Rule[] {
  return RULES.filter((r) => {
    const parts: string[] = subject(r.selector).match(/\.[a-zA-Z0-9_-]+/g) ?? []
    return parts.includes(`.${cls}`)
  })
}

/** Final flex-grow a block computes (longhand + shorthand, in-block order). */
function blockGrow(body: string): number | null {
  let grow: number | null = null
  for (const decl of body.split(';')) {
    let m = /^\s*flex-grow\s*:\s*([\d.]+)/.exec(decl)
    if (m) {
      grow = parseFloat(m[1])
      continue
    }
    m = /^\s*flex\s*:\s*(\S[^]*?)\s*$/.exec(decl)
    if (!m) continue
    const v = m[1]
    if (v === 'none' || v === 'initial') grow = 0
    else if (v === 'auto') grow = 1
    else {
      const first = /^([\d.]+)/.exec(v)
      grow = first ? parseFloat(first[1]) : 0
    }
  }
  return grow
}

/** Main-window ancestor chain for a shell `<main class="layout single X">`:
 *  `.app` → `.shell` → the shell element (App.tsx ~2336; cockpits mount in `.shell`). */
function shellChain(cls: string): Array<Set<string>> {
  return [new Set(['app']), new Set(['shell']), new Set(['layout', 'single', cls])]
}

/** Final flex-direction a block computes (longhand + the `flex-flow` shorthand, whose
 *  direction keyword may sit in either position). */
function blockFlexDirection(body: string): string | null {
  let v: string | null = null
  for (const decl of body.split(';')) {
    let m = /^\s*flex-direction\s*:\s*(\S+)\s*$/.exec(decl)
    if (m) {
      v = m[1]
      continue
    }
    m = /^\s*flex-flow\s*:\s*(\S[^]*?)\s*$/.exec(decl)
    if (!m) continue
    const dir = m[1].split(/\s+/).find((t) => /^(row|column)(-reverse)?$/.test(t))
    if (dir) v = dir
  }
  return v
}

/** Cascade winner of a per-block-computed property for the element at the end of
 *  `chain` (unconditional rules only) — the same winner walk as winningOverflowY. */
function winningValue<T>(
  chain: Array<Set<string>>,
  blockValue: (body: string) => T | null,
): { value: T; selector: string } | null {
  let win: { value: T; selector: string; spec: number; order: number } | null = null
  for (const r of RULES) {
    if (r.media !== null || !matchesChain(r.selector, chain)) continue
    const v = blockValue(r.body)
    if (v === null) continue
    const spec = specificity(r.selector)
    if (!win || spec > win.spec || (spec === win.spec && r.order >= win.order)) {
      win = { value: v, selector: r.selector, spec, order: r.order }
    }
  }
  return win && { value: win.value, selector: win.selector }
}

const SHELLS: Array<[string, Array<Set<string>>]> = [
  ['.layout.single.phone-cockpit', shellChain('phone-cockpit')],
  ['.layout.single.cw-cockpit', shellChain('cw-cockpit')],
  ['.layout.single.rtty-cockpit', shellChain('rtty-cockpit')],
  ['.layout.single.sstv-view', shellChain('sstv-view')],
]

describe('cockpit shells are the deficit valve (winning overflow-y is auto)', () => {
  for (const [name, chain] of SHELLS) {
    it(`${name} resolves overflow-y:auto after the full cascade, in every media context`, () => {
      // Contexts: the plain cascade plus each @media condition that carries a matching
      // overflow rule (e.g. `@media (max-width:900px) { .layout { overflow-y:auto } }`,
      // styles.css ~10136 — same valve direction, but a future conditional `hidden`
      // must not sneak past this guard either).
      for (const ctx of mediaContexts(chain)) {
        const win = winningOverflowY(chain, ctx)
        expect(win, `${name}: no rule declares overflow at all`).not.toBeNull()
        expect(
          win!.value,
          `${name}${ctx ? ` (within \`${ctx}\`)` : ''}: the cascade winner is ` +
            `\`${win!.selector} { overflow-y: ${win!.value} }\` — a vertical deficit CLIPS the tail ` +
            '(log form / PTT / recall) unreachably instead of scrolling. This is the exact dead-fix ' +
            'mechanism of 2026-07-30; see the header comment.',
        ).toBe('auto')
      }
    })
  }
})

describe('lower regions carry no manufactured floor', () => {
  // 18em (= 252px at the 14px body font) guaranteed a band of empty black above an
  // unreachable log form once the cockpit-level scroll those floors assumed was dead.
  // Threshold equivalences: 10em/10rem = 140px at the 14px body font; 140px against the
  // 900px fit-scale target height ≈ 15vh.
  const LIMITS: Record<string, number> = { em: 10, rem: 10, px: 140, vh: 15 }
  for (const cls of ['ph-lower', 'cw-lower']) {
    it(`.${cls} has no min-height above 10em in any unit`, () => {
      const offenders: string[] = []
      for (const r of rulesOn(cls)) {
        const re = /min-height\s*:\s*([\d.]+)(px|em|rem|vh)/g
        let m: RegExpExecArray | null
        while ((m = re.exec(r.body)) !== null) {
          if (parseFloat(m[1]) > LIMITS[m[2]]) offenders.push(`${r.selector} { min-height: ${m[1]}${m[2]} }`)
        }
      }
      expect(offenders, `floor reintroduced:\n${offenders.join('\n')}`).toEqual([])
    })
  }
})

describe('band pane is content-height, not a grower', () => {
  it('.ph-band-pane winning flex-grow is 0 (a grower around a fixed-height strip = empty black box)', () => {
    let win: { grow: number; selector: string; spec: number; order: number } | null = null
    for (const r of rulesOn('ph-band-pane')) {
      if (r.media !== null) continue
      const g = blockGrow(r.body)
      if (g === null) continue
      const spec = specificity(r.selector)
      if (!win || spec > win.spec || (spec === win.spec && r.order >= win.order)) {
        win = { grow: g, selector: r.selector, spec, order: r.order }
      }
    }
    // No declared grow at all is also fine (initial flex-grow is 0).
    if (win) {
      expect(
        win.grow,
        `\`${win.selector}\` gives .ph-band-pane flex-grow ${win.grow} — its BandStrip content ` +
          'cannot stretch, so surplus renders as the "empty black box". Surplus is the ' +
          "operator's to give to the scope via its Splitter.",
      ).toBe(0)
    }
  })
})

/** Last max-height across the given exact selectors (later rule wins; same-spec). */
function finalMaxHeight(selectors: string[]): string | null {
  let v: string | null = null
  let at = -1
  for (const r of RULES) {
    if (r.media !== null || !selectors.includes(r.selector)) continue
    const m = [...r.body.matchAll(/max-height\s*:\s*([^;]+)/g)]
    if (m.length && r.order >= at) {
      v = m[m.length - 1][1].trim()
      at = r.order
    }
  }
  return v
}

describe('bounded, zoom-corrected caps', () => {
  for (const scope of ['.phone-cockpit .ph-scope-panel', '.cw-cockpit .ph-scope-panel']) {
    it(`${scope} max-height is a --vh-eff cap (drag-driven height, bounded)`, () => {
      const v = finalMaxHeight([scope])
      expect(v, `${scope}: no max-height declared`).not.toBeNull()
      expect(
        v!,
        `${scope} max-height is \`${v}\` — must reference var(--vh-eff): raw vh is zoom-blind ` +
          'and `none` lets the scope swallow the window.',
      ).toContain('var(--vh-eff')
    })
  }

  it('.mv-packs max-height references var(--vh-eff) (raw 86vh over-talls under UI zoom)', () => {
    const v = finalMaxHeight(['.mv-packs'])
    expect(v, '.mv-packs: no max-height declared').not.toBeNull()
    expect(v!, `.mv-packs max-height is \`${v}\``).toContain('var(--vh-eff')
  })
})

describe('the scope splitter drag is respected (winning flex-grow is 0)', () => {
  // The Splitter (PhoneCockpit.tsx ~728 / CwCockpit) drives --ph-scope-h / --cw-scope-h
  // as the scope's flex-BASIS. A basis only sets the rendered height while flex-grow
  // is 0: with grow 1 on the column's only grower the resolved height is
  // clamp(shellH − siblings, floor, cap) at EVERY basis — algebraically independent of
  // the drag — so the operator's live control (and the persisted %) silently did
  // nothing (review 2026-07-31; CW split the surplus 1:1 with `.cw-lower` and tracked
  // the pointer at half rate instead). Operate is the pattern: `.cockpit-waterfall
  // { flex: 0 1 var(--cockpit-wf-h, 22%) }` with the decode scroller as the grower.
  for (const shell of ['phone-cockpit', 'cw-cockpit']) {
    it(`.${shell} .ph-scope-panel resolves flex-grow 0 (grow ≥1 voids the dragged basis)`, () => {
      const chain = [...shellChain(shell), new Set(['ph-scope-panel'])]
      const win = winningValue(chain, blockGrow)
      expect(win, `.${shell} .ph-scope-panel: no rule declares flex at all`).not.toBeNull()
      expect(
        win!.value,
        `\`${win!.selector}\` gives the ${shell} scope flex-grow ${win!.value} — as the column's ` +
          'grower its height no longer follows the flex-basis the Splitter drives, so the drag ' +
          'and the persisted % are inert. Keep grow 0; pick the surplus sink deliberately.',
      ).toBe(0)
    })
  }
})

describe('Journey cards win their row direction against .panel', () => {
  // `<div className="jy-marathon panel">` / `<section className="jy-hero panel">`
  // (JourneyView.tsx ~60/~124): `.panel { flex-direction: column }` also targets the
  // element, so the card's `row` must actually WIN the cascade — a bare (0,1,0)
  // `.jy-marathon` earlier in the sheet loses to the later (0,1,0) `.panel` and ships
  // dead, the exact mechanism this file exists to catch (review 2026-07-31).
  for (const card of ['jy-marathon', 'jy-hero']) {
    it(`.${card}.panel resolves flex-direction row`, () => {
      // journey-view is a true ancestor of both cards (jy-marathon sits below an
      // unclassed wrapper too — descendant matching walks past it).
      const chain = [
        new Set(['app']),
        new Set(['shell']),
        new Set(['layout', 'single']),
        new Set(['journey-view']),
        new Set([card, 'panel']),
      ]
      const win = winningValue(chain, blockFlexDirection)
      expect(win, `.${card}: no rule declares flex-direction at all`).not.toBeNull()
      expect(
        win!.value,
        `the cascade winner is \`${win!.selector} { flex-direction: ${win!.value} }\` — the ` +
          `card stacks vertically and the .${card} row rule is dead. Outrank .panel ` +
          `(e.g. \`.panel.${card}\`) instead of relying on source order.`,
      ).toBe('row')
    })
  }
})

describe('Connect strip cap caps the PANES, not the grid track', () => {
  // css-grid §11.6 (maximize tracks) grows ANY fixed-max track to its growth limit
  // before the fr rows expand — the min sizing function is irrelevant — so both
  // `minmax(0, X)` and `minmax(auto, X)` FLOOR the strip at its full X on a tall
  // window: a "cap" that always pays out, stealing X from the globe's 1fr rows
  // (verified empirically in Chrome, review 2026-07-31; the census' prescribed
  // `minmax(auto, …)` spelling was wrong). The working shape: the track is `auto`
  // (content-sized) and the ceiling lives on the strip's pane children as max-height,
  // which DOES bound a box — a tall pane scrolls inside `.pane-body`.
  it('.connect grid-template-rows strip track is auto (no fixed max — it would always pay out)', () => {
    let rows: string | null = null
    let at = -1
    for (const r of RULES) {
      if (r.media !== null || r.selector !== '.connect') continue
      const m = [...r.body.matchAll(/grid-template-rows\s*:\s*([^;]+)/g)]
      if (m.length && r.order >= at) {
        rows = m[m.length - 1][1].trim()
        at = r.order
      }
    }
    expect(rows, '.connect: no grid-template-rows declared').not.toBeNull()
    // Paren-aware top-level track split (minmax(a, b) is one track).
    const tracks: string[] = []
    let depth = 0
    let cur = ''
    for (const ch of rows!) {
      if (ch === '(') depth++
      if (ch === ')') depth--
      if (/\s/.test(ch) && depth === 0) {
        if (cur) tracks.push(cur)
        cur = ''
      } else cur += ch
    }
    if (cur) tracks.push(cur)
    expect(tracks.length, `.connect rows are \`${rows}\``).toBe(3)
    expect(
      tracks[2],
      `.connect strip track is \`${tracks[2]}\` — a fixed max is maximized to its full value ` +
        'before the fr rows expand (§11.6), so it is a floor, not a cap.',
    ).toBe('auto')
  })

  it('.connect-strip > .pane-frame carries the zoom-corrected max-height cap', () => {
    const v = finalMaxHeight(['.connect-strip > .pane-frame'])
    expect(v, '.connect-strip > .pane-frame: no max-height — the strip is unbounded').not.toBeNull()
    expect(v!, `cap is \`${v}\``).toContain('var(--vh-eff')
  })
})
