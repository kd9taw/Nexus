import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

// THE PANE-GRID STRUCTURAL SHEET (2026-07-30 layout assessment, design3 §3/§5).
//
// cockpit-panes.css is deliberately a SEPARATE file with one rule of its own: every
// selector is flat — a single class, optionally qualified by `[data-cols]`. Uniform
// specificity means a cascade war between two structural rules is unrepresentable, which
// is the entire reason this file exists. The bug class it retires shipped twice in one
// day: `.phone-cockpit { overflow-y:auto }` lost to `.layout.single.phone-cockpit
// { overflow:hidden }` ((0,1,0) vs (0,3,0)) and CW's twin lost to a shorthand later in
// its own block. Both "fixes" were dead the moment they landed and nothing noticed for
// five weeks.
//
// So the guards below are: flat selectors only (no rule can outrank another by accident),
// a FENCE — styles.css declares none of these classes, so there is no second file to lose
// to — and the two structural contracts the region itself must satisfy.
//
// Honest note on grading: the fence tests are green-by-construction today (the classes
// are new; nothing in styles.css could name them yet). They are regression guards, not
// failing-first repros — their job starts the first time someone "just adds one override"
// to the 19k-line sheet. The specificity and overflow tests were run red against a
// deliberately-broken sheet before landing (a descendant selector and a flipped overflow),
// which is the evidence that they bite.

const read = (name: string) => readFileSync(fileURLToPath(new URL(`./${name}`, import.meta.url)), 'utf8')

const RAW = read('cockpit-panes.css')
const CSS = RAW.replace(/\/\*[\s\S]*?\*\//g, '') // prose must never read as a declaration
const STYLES = read('styles.css').replace(/\/\*[\s\S]*?\*\//g, '')
const MAIN = read('main.tsx')

interface Rule {
  selector: string
  body: string
  order: number
}

/** Brace-aware rule walk (the cockpit-shells.test.ts parser, minus @media handling —
 *  this sheet has no conditional rules, and an at-rule appearing here should FAIL the
 *  flat-selector guard rather than be silently skipped). */
function parseRules(sheet: string): Rule[] {
  const out: Rule[] = []
  let i = 0
  let order = 0
  let selStart = 0
  while (i < sheet.length) {
    const ch = sheet[i]
    if (ch === '{') {
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
      order++
      for (const s of sel.split(',')) {
        const one = s.trim().replace(/\s+/g, ' ')
        if (one) out.push({ selector: one, body, order })
      }
      selStart = i
    } else {
      i++
    }
  }
  return out
}

const RULES = parseRules(CSS)

/** parseRules for the 19k-line sheet: DESCENDS into @media/@supports bodies (a fenced
 *  class or a pane-frame floor hiding inside a media block is still a violation), skips
 *  other at-rule bodies (@keyframes frame selectors are not rules on elements). The flat
 *  guard above deliberately keeps the NON-descending walk for cockpit-panes.css, where an
 *  at-rule must FAIL the flat-selector test rather than be recursed into. */
function parseStylesRules(sheet: string, out: Rule[] = [], counter = { n: 0 }): Rule[] {
  for (const r of parseRules(sheet)) {
    if (r.selector.startsWith('@')) {
      if (/^@(media|supports)\b/.test(r.selector)) parseStylesRules(r.body, out, counter)
    } else {
      counter.n++
      out.push({ ...r, order: counter.n })
    }
  }
  return out
}

const STYLES_RULES = parseStylesRules(STYLES)

/** A selector this sheet is allowed to use: one class, optionally + one `[data-cols=…]`.
 *  Anything else — a descendant, a combinator, a second class, a tag, an id, a pseudo —
 *  is a specificity gradient, i.e. a future cascade war. */
const FLAT = /^\.[a-z][a-z0-9-]*(\[data-cols='[123]'\])?$/

/** (classes+attributes, ids) — flat rules are all (0,1,0) or (0,2,0). */
function specificity(sel: string): number {
  return (sel.match(/\.[a-z][a-z0-9-]*|\[[^\]]+\]/g) ?? []).length
}

/** Final overflow-y a block computes — in-block declaration order, with the `overflow`
 *  shorthand resetting the longhand (its y value is the last of up to two). */
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

/** Cascade winner of overflow-y for `.cockpit-panes[data-cols='N']` within this sheet:
 *  the base class rule plus that tier's variant, resolved by specificity then order. */
function regionOverflowY(tier: 1 | 2 | 3): { value: string; selector: string } | null {
  const variant = `.cockpit-panes[data-cols='${tier}']`
  let win: { value: string; selector: string; spec: number; order: number } | null = null
  for (const r of RULES) {
    if (r.selector !== '.cockpit-panes' && r.selector !== variant) continue
    const v = blockOverflowY(r.body)
    if (v === null) continue
    const spec = specificity(r.selector)
    if (!win || spec > win.spec || (spec === win.spec && r.order >= win.order)) {
      win = { value: v, selector: r.selector, spec, order: r.order }
    }
  }
  return win && { value: win.value, selector: win.selector }
}

describe('every selector is flat (uniform specificity ⇒ no cascade war is possible)', () => {
  it('cockpit-panes.css uses only single classes and [data-cols] variants', () => {
    const offenders = RULES.map((r) => r.selector).filter((s) => !FLAT.test(s))
    expect(
      offenders,
      `these selectors can outrank (or be outranked by) a sibling structural rule:\n${offenders.join('\n')}\n` +
        'Structural rules here must all be (0,1,0)/(0,2,0). Style the CONTENT in styles.css instead.',
    ).toEqual([])
  })

  it('no selector exceeds (0,2,0)', () => {
    const over = RULES.map((r) => r.selector).filter((s) => specificity(s) > 2)
    expect(over, `over-specific:\n${over.join('\n')}`).toEqual([])
  })
})

describe('the fence: styles.css never names a structural class', () => {
  // The 19k-line sheet is where every previous override crept in. If it cannot name these
  // classes it cannot fight them — the isolation is the guarantee, not a convention.
  for (const cls of ['cockpit-panes', 'cockpit-col', 'cockpit-txdock', 'cockpit-pane-acts']) {
    it(`styles.css declares no .${cls} rule`, () => {
      const hits = STYLES_RULES
        .map((r) => r.selector)
        .filter((s) => new RegExp(`\\.${cls}(?![a-z0-9-])`).test(s))
      expect(
        hits,
        `styles.css styles .${cls}:\n${hits.join('\n')}\nStructural rules live in ` +
          'cockpit-panes.css ONLY — a second home for them is how the last four fixes died.',
      ).toEqual([])
    })
  }

  it('main.tsx imports cockpit-panes.css AFTER styles.css', () => {
    const styles = MAIN.indexOf("'./styles.css'")
    const panes = MAIN.indexOf("'./cockpit-panes.css'")
    expect(styles, 'main.tsx no longer imports styles.css').toBeGreaterThan(-1)
    expect(panes, 'main.tsx does not import cockpit-panes.css — the sheet is dead').toBeGreaterThan(-1)
    expect(
      panes,
      'cockpit-panes.css must be imported after styles.css: equal-specificity ties go to ' +
        'the later sheet, and the structural rules must win those.',
    ).toBeGreaterThan(styles)
  })
})

describe('the region owns exactly one scroll behaviour per tier', () => {
  it("data-cols='1': the REGION scrolls (the operator's scrollbar comes back)", () => {
    const win = regionOverflowY(1)
    expect(win, '.cockpit-panes[data-cols=\'1\']: no rule declares overflow').not.toBeNull()
    expect(
      win!.value,
      `winner is \`${win!.selector} { overflow-y: ${win!.value} }\` — at one column the stacked ` +
        'panes are content-height, so if the region does not scroll the tail is unreachable. ' +
        'This is the 2026-07-30 bug in its original form.',
    ).toBe('auto')
  })

  for (const tier of [2, 3] as const) {
    it(`data-cols='${tier}': the region is bounded and the PANES scroll`, () => {
      const win = regionOverflowY(tier)
      expect(win, `.cockpit-panes[data-cols='${tier}']: no rule declares overflow`).not.toBeNull()
      expect(
        win!.value,
        `winner is \`${win!.selector} { overflow-y: ${win!.value} }\` — a multi-column region ` +
          'that scrolls has two scroll owners (itself and every .pane-body), which is exactly ' +
          'the "which scrollbar am I in" state this rebuild removes.',
      ).toBe('hidden')
    })
  }

  it('the multi-column tiers size rows minmax(0,1fr) (a cell can always shrink)', () => {
    for (const tier of [2, 3] as const) {
      const r = RULES.find((x) => x.selector === `.cockpit-panes[data-cols='${tier}']`)
      expect(r, `no .cockpit-panes[data-cols='${tier}'] rule`).toBeDefined()
      expect(
        r!.body.replace(/\s+/g, ' '),
        `tier ${tier} rows must be minmax(0, 1fr) — an auto/content row cannot shrink, which ` +
          'is a floor by another name.',
      ).toMatch(/grid-auto-rows: minmax\(0, ?1fr\)/)
    }
  })
})

describe('panes are sized by the grid, never by themselves', () => {
  it('cockpit-panes.css does not restyle the shared .pane-frame family', () => {
    // The frame CSS (styles.css ~1497) is Connect's, shipped and correct. Forking it here
    // would give two owners of one box — and the pane-grid contract is that a pane declares
    // no size at all.
    // Anywhere in the selector, not just as the subject: `.cockpit-panes .pane-frame {…}`
    // is a fork too (and a specificity gradient the flat-selector guard also catches).
    const forks = RULES.map((r) => r.selector).filter((s) =>
      /\.pane-(frame|head|body|title|basic|pick)(?![a-z0-9-])/.test(s),
    )
    expect(forks, `forked shared pane CSS:\n${forks.join('\n')}`).toEqual([])
  })

  it('declares no min-height floor except the region shell-valve floor', () => {
    // The 18em floors this rebuild deleted (styles.css 5915/6988) sat under a CLIPPING
    // ancestor — that is what put the log form below the clip edge. The ONE legal floor
    // is on `.cockpit-panes` itself, and only because its ancestor is the shell whose
    // `overflow-y: auto` valve cockpit-shells.test.ts guards: past the floor, deficit
    // becomes the shell scrollbar, never a clip. Everything else keeps min-height: 0 —
    // a floor on a column or a tier variant sits under the region's own
    // `overflow: hidden` and is the clip mechanism reborn.
    const offenders: string[] = []
    for (const r of RULES) {
      for (const m of r.body.matchAll(/min-height\s*:\s*([^;]+)/g)) {
        const v = m[1].trim()
        if (v === '0') continue
        if (r.selector === '.cockpit-panes' && /^\d+(\.\d+)?em$/.test(v)) continue
        offenders.push(`${r.selector} { min-height: ${v} }`)
      }
    }
    expect(
      offenders,
      `manufactured floor:\n${offenders.join('\n')}\nA floor under a bounded ancestor is how ` +
        'deficit becomes clipping. Growth is expressed as fr shares (and row spans) only; ' +
        'the region base floor is the single shell-valve exception.',
    ).toEqual([])
  })

  it('the region base rule carries the shell-valve floor (the anti-crush guarantee)', () => {
    // Without it the region is the shell column's only unfloored shrinkable child, so a
    // big scope drag absorbs the whole deficit at overflow:hidden and Batch 1's shell
    // valve can never fire — panes crush to their title bars with no scrollbar anywhere.
    const r = RULES.find((x) => x.selector === '.cockpit-panes')
    expect(r, 'no .cockpit-panes rule').toBeDefined()
    expect(
      r!.body.replace(/\s+/g, ' '),
      'the region must floor itself (em units — px are zoom-hostile) so vertical deficit ' +
        'overflows the shell and its valve scrolls.',
    ).toMatch(/min-height: \d+(\.\d+)?em/)
  })

  it('the log column cap cannot out-pay the feed track (§11.6 maximize-tracks)', () => {
    // css-grid §11.6 pays a fixed-max track to its FULL growth limit before any fr track
    // sees free space (Chrome-verified on the Connect strip — "a cap that always paid
    // out"). A bare 44em max therefore made the log a constant 616px and left the feed
    // NARROWER than the form at region 1080–1244. The cap's max must stay
    // proportion-bounded: min(<em cap>, <percentage>).
    for (const tier of [2, 3] as const) {
      const r = RULES.find((x) => x.selector === `.cockpit-panes[data-cols='${tier}']`)
      expect(r, `no .cockpit-panes[data-cols='${tier}'] rule`).toBeDefined()
      const cols = /grid-template-columns\s*:\s*([^;]+)/.exec(r!.body)?.[1].trim() ?? ''
      expect(
        cols,
        `tier ${tier} log track is not proportion-bounded — its fixed max always pays out ` +
          'in full before the feed track gets anything.',
      ).toMatch(/minmax\(24em, min\(44em, \d+%\)\)\s*$/)
    }
  })

  it('the TX dock is pinned and unshrinkable (flex: 0 0 auto)', () => {
    const r = RULES.find((x) => x.selector === '.cockpit-txdock')
    expect(r, 'no .cockpit-txdock rule').toBeDefined()
    expect(
      r!.body.replace(/\s+/g, ' '),
      'the dock carries PTT/send: it must never be a flex grower or a shrink victim.',
    ).toMatch(/flex: 0 0 auto/)
  })
})

describe('styles.css cannot size a pane frame either (the fence has two sides)', () => {
  // The fence above stops styles.css naming the STRUCTURAL classes — but a pane can also
  // be re-sized through its own shared chrome: `.phone-cockpit .pane-frame { flex: 1 1 0 }`
  // names no fenced class, passes the flat guard (wrong sheet), and is exactly the
  // per-cockpit grower/floor mechanism the rebuild deletes. This closes that gap: in
  // styles.css, a rule ON .pane-frame may style paint/type, never growth or floors.
  //
  // The ONE allowed exception is RTTY's shell-owned frame (`.rtty-cockpit > .pane-frame`):
  // RTTY has no region — a single content pane — so the shell sizes the frame explicitly,
  // with the shell's own deficit valve (cockpit-shells.test.ts) behind the 10em floor.
  // The allowlist is exact-selector, so even a typo'd variant of it fails here.
  const ALLOWED = new Set(['.rtty-cockpit > .pane-frame'])

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

  /** Subject (rightmost compound) of a selector. */
  const subject = (sel: string) => {
    const parts = sel.split(/\s*[>+~]\s*|\s+/)
    return parts[parts.length - 1]
  }

  it('no styles.css rule on .pane-frame declares flex-grow or a min-height floor', () => {
    const offenders: string[] = []
    for (const r of STYLES_RULES) {
      if (!/\.pane-frame(?![a-z0-9-])/.test(subject(r.selector)) || ALLOWED.has(r.selector)) continue
      const grow = blockGrow(r.body)
      if (grow != null && grow > 0) offenders.push(`${r.selector} { flex-grow: ${grow} }`)
      for (const m of r.body.matchAll(/min-height\s*:\s*([^;]+)/g)) {
        if (m[1].trim() !== '0') offenders.push(`${r.selector} { min-height: ${m[1].trim()} }`)
      }
    }
    expect(
      offenders,
      `a pane frame sized from styles.css:\n${offenders.join('\n')}\nThe grid cell sizes the ` +
        'frame (design3 §5 rule 2). A grower/floor here is the per-cockpit sizing mechanism ' +
        'that recurred five times — express prominence as a row span via CockpitPaneFrame ' +
        '`rows`, or for a region-less cockpit add the exact selector to ALLOWED with its ' +
        'valve documented.',
    ).toEqual([])
  })

  it('no styles.css rule on .pane-body declares a min-height floor', () => {
    // `.pane-body { flex: 1 }` is the frame's internal contract (the body IS the frame's
    // grower) — but a floor on the body sits under `.pane-frame { overflow: hidden }`,
    // which is the documented clip mechanism. Content floors belong INSIDE the body
    // (e.g. `.pane-body > .cw-decode { min-height: 6em }`), where overflow:auto scrolls.
    const offenders: string[] = []
    for (const r of STYLES_RULES) {
      if (!/\.pane-body(?![a-z0-9-])/.test(subject(r.selector))) continue
      for (const m of r.body.matchAll(/min-height\s*:\s*([^;]+)/g)) {
        if (m[1].trim() !== '0') offenders.push(`${r.selector} { min-height: ${m[1].trim()} }`)
      }
    }
    expect(offenders, `floored pane body:\n${offenders.join('\n')}`).toEqual([])
  })
})
