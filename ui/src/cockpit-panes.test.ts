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

/** A selector this sheet is allowed to use: one class, optionally + one region-state
 *  attribute (`[data-cols=…]` or `[data-flow=…]`). Anything else — a descendant, a
 *  combinator, a second class, a tag, an id, a pseudo — is a specificity gradient, i.e.
 *  a future cascade war. */
const FLAT = /^\.[a-z][a-z0-9-]*(\[data-(cols='[123]'|flow='(stack|fill)')\])?$/

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

/** Final value of a custom property a block computes (last declaration wins in-block).
 *  `name` is a literal property name — no regex metacharacters. */
function blockVar(name: string) {
  return (body: string): string | null => {
    let v: string | null = null
    for (const decl of body.split(';')) {
      const m = new RegExp(`^\\s*${name}\\s*:\\s*(\\S[^]*?)\\s*$`).exec(decl)
      if (m) v = m[1].replace(/\s+/g, ' ')
    }
    return v
  }
}

/** THE REGION'S STAMPED STATE — two facts, deliberately two attributes (useRegionCols).
 *
 *  `cols` is a TRACK COUNT: min(measured tier, the number of column groups the cockpit
 *  can actually fill right now). It is a CONTENT budget, so it may be 1 on a 3440-wide
 *  window — the ⊞ Panels menu reaches it.
 *  `flow` is the WIDTH claim: 'stack' only when the region MEASURED narrow, 'fill'
 *  otherwise. Everything that is true because the region is narrow (content-height rows,
 *  the region owning the scrollbar, fill panes collapsing to content) hangs off `flow`.
 *
 *  Conflating the two is the bug this split exists to kill: `data-cols='1'` used to carry
 *  both, so unticking a cockpit's panes on an ultrawide put the region into the narrow
 *  layout and left the whole surplus blank. */
interface RegionState {
  cols: 1 | 2 | 3
  flow: 'stack' | 'fill'
}

/** Every state useRegionCols can actually stamp. 'stack' implies measured tier 1, and
 *  cols = min(1, maxCols) = 1 — so there is no (2|3, 'stack'). Guarded live in
 *  useRegionCols.test.tsx; enumerated here so a sheet rule for an unreachable state
 *  cannot masquerade as coverage. */
const REGION_STATES: RegionState[] = [
  { cols: 1, flow: 'stack' },
  { cols: 1, flow: 'fill' },
  { cols: 2, flow: 'fill' },
  { cols: 3, flow: 'fill' },
]

const stateName = (st: RegionState) => `data-cols='${st.cols}' data-flow='${st.flow}'`

/** Cascade winner of a per-block-computed property for a `.cockpit-panes` element in
 *  state `st`: the base class rule plus whichever attribute variants match, resolved by
 *  specificity then source order. */
function regionWinner<T>(
  st: RegionState,
  blockValue: (body: string) => T | null,
): { value: T; selector: string } | null {
  const matching = new Set([
    '.cockpit-panes',
    `.cockpit-panes[data-cols='${st.cols}']`,
    `.cockpit-panes[data-flow='${st.flow}']`,
  ])
  let win: { value: T; selector: string; spec: number; order: number } | null = null
  for (const r of RULES) {
    if (!matching.has(r.selector)) continue
    const v = blockValue(r.body)
    if (v === null) continue
    const spec = specificity(r.selector)
    if (!win || spec > win.spec || (spec === win.spec && r.order >= win.order)) {
      win = { value: v, selector: r.selector, spec, order: r.order }
    }
  }
  return win && { value: win.value, selector: win.selector }
}

/** Cascade winner of a property for a `.cockpit-col` element, computed across BOTH
 *  sheets. The fence says styles.css may not name the class at all — but a guard that
 *  only reads its own sheet proves nothing about the cascade, and "the override lived in
 *  the other file" is this codebase's signature failure. styles.css rules sort first
 *  (main.tsx imports it first), so an equal-specificity structural rule wins the tie. */
function colWinner<T>(
  blockValue: (body: string) => T | null,
): { value: T; selector: string; sheet: string } | null {
  const candidates: Array<{ r: Rule; sheet: string; rank: number }> = [
    ...STYLES_RULES.map((r) => ({ r, sheet: 'styles.css', rank: 0 })),
    ...RULES.map((r) => ({ r, sheet: 'cockpit-panes.css', rank: 1 })),
  ]
  let win: { value: T; selector: string; sheet: string; spec: number; key: number } | null = null
  for (const { r, sheet, rank } of candidates) {
    // Subject-only: a rule whose SUBJECT is the column is a rule on the column.
    const parts = r.selector.split(/\s*[>+~]\s*|\s+/)
    if (!/\.cockpit-col(?![a-z0-9-])/.test(parts[parts.length - 1])) continue
    const v = blockValue(r.body)
    if (v === null) continue
    const spec = specificity(r.selector)
    const key = rank * 1e6 + r.order
    if (!win || spec > win.spec || (spec === win.spec && key >= win.key)) {
      win = { value: v, selector: r.selector, sheet, spec, key }
    }
  }
  return win && { value: win.value, selector: win.selector, sheet: win.sheet }
}

/** A box whose computed overflow-y makes it a scroll container. */
const SCROLLS = (v: string) => v === 'auto' || v === 'scroll'

describe('every selector is flat (uniform specificity ⇒ no cascade war is possible)', () => {
  it('cockpit-panes.css uses only single classes and region-state variants', () => {
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

describe('the region owns exactly one scroll behaviour per state', () => {
  it("flow='stack': the REGION scrolls (the operator's scrollbar comes back)", () => {
    const st = REGION_STATES[0]
    const win = regionWinner(st, blockOverflowY)
    expect(win, `${stateName(st)}: no rule declares overflow`).not.toBeNull()
    expect(
      win!.value,
      `winner is \`${win!.selector} { overflow-y: ${win!.value} }\` — when the region measured ` +
        'narrow the stacked panes are content-height, so if the region does not scroll the tail ' +
        'is unreachable. This is the 2026-07-30 bug in its original form.',
    ).toBe('auto')
  })

  for (const st of REGION_STATES.filter((s) => s.flow === 'fill')) {
    it(`${stateName(st)}: the region is bounded and its columns scroll`, () => {
      const win = regionWinner(st, blockOverflowY)
      expect(win, `${stateName(st)}: no rule declares overflow`).not.toBeNull()
      expect(
        win!.value,
        `winner is \`${win!.selector} { overflow-y: ${win!.value} }\` — a bounded region that ` +
          'also scrolls has two scroll owners (itself and every column), which is exactly the ' +
          '"which scrollbar am I in" state this rebuild removes.',
      ).toBe('hidden')
    })
  }

  it('the bounded states size rows minmax(0,1fr) (a cell can always shrink)', () => {
    for (const st of REGION_STATES.filter((s) => s.flow === 'fill')) {
      const win = regionWinner(st, (b) => /grid-auto-rows\s*:\s*([^;]+)/.exec(b)?.[1].trim() ?? null)
      expect(win, `${stateName(st)}: no rule declares grid-auto-rows`).not.toBeNull()
      expect(
        win!.value.replace(/\s+/g, ' '),
        `${stateName(st)} rows come from \`${win!.selector}\` — they must be minmax(0, 1fr); an ` +
          'auto/content row cannot shrink, which is a floor by another name.',
      ).toMatch(/^minmax\(0, ?1fr\)$/)
    }
  })

  it('the content-height override is keyed on the measured FLOW, never on the track count', () => {
    // `--cockpit-pane-flex: 0 0 auto` makes every fill pane content-height. That is right
    // when the region MEASURED narrow (rows are auto, so a basis-0 grower would collapse to
    // min-content anyway) and wrong everywhere else. Keyed on data-cols it fired whenever
    // the ⊞ menu emptied a cockpit down to one column — so on a 3440-wide window CW's log
    // pane went content-height and the entire surplus was left blank. The knob must be
    // absent in every 'fill' state, at ANY track count.
    const pf = blockVar('--cockpit-pane-flex')
    const offenders = REGION_STATES.filter((s) => s.flow === 'fill')
      .map((st) => ({ st, win: regionWinner(st, pf) }))
      .filter((x) => x.win !== null && x.win.value !== '1 1 0')
      .map((x) => `${stateName(x.st)} ← ${x.win!.selector} { --cockpit-pane-flex: ${x.win!.value} }`)
    expect(
      offenders,
      `a bounded region forces its fill panes to content height:\n${offenders.join('\n')}\n` +
        'This is the blank-space-hoarding half of the bug class, reached through the ⊞ menu ' +
        'instead of the window. Hang the override off [data-flow=\'stack\'].',
    ).toEqual([])
  })
})

describe('a clipping region never contains a rigid stack without an interposed scroller', () => {
  // THE CONTRACT (CLAUDE.md): "an overflow:hidden ancestor may never contain hard-floored
  // descendants without an interposed scroller."
  //
  // .cockpit-col is that ancestor's only child kind, and a column IS a rigid stack:
  // CockpitPaneFrame stamps every fit="content" frame `flex: 0 0 auto` (shrink 0), so a
  // column of control strips — Phone's NR slider / AGC chips / DSP toggles / voice-keyer
  // F4–F6, CW's six aux strips — cannot shrink at all. With the region clipping and the
  // column declaring no overflow, the tail of that stack renders past the region edge and
  // is simply gone: no scrollbar, nothing to drag, controls unreachable.
  //
  // THE SHELL'S OWN VALVE IS NOT A SUBSTITUTE. It fires only once the region bottoms out on
  // its 18em floor, and even then it scrolls the header/scope/dock, never the region's
  // interior. The column is where the deficit is, so the column is where the valve goes.
  for (const st of REGION_STATES) {
    it(`${stateName(st)}: region overflow and column overflow are a legal pair`, () => {
      const region = regionWinner(st, blockOverflowY)
      expect(region, `${stateName(st)}: the region declares no overflow at all`).not.toBeNull()
      const col = colWinner(blockOverflowY)
      if (region!.value === 'hidden') {
        expect(
          col,
          `${stateName(st)}: \`${region!.selector} { overflow-y: hidden }\` clips, and NO rule in ` +
            'either sheet declares overflow on .cockpit-col — a column of fit="content" frames ' +
            '(flex: 0 0 auto) is a rigid stack clipping against the region edge with nothing ' +
            'between them.',
        ).not.toBeNull()
        expect(
          SCROLLS(col!.value),
          `${stateName(st)}: the region clips but the winning column rule is \`${col!.sheet}: ` +
            `${col!.selector} { overflow-y: ${col!.value} }\` — deficit inside a column has ` +
            'nowhere to go.',
        ).toBe(true)
      } else {
        // The region owns the scrollbar here; a column valve is inert (rows are auto, so a
        // column is exactly its content height) but must not clip on its own.
        expect(
          col === null || SCROLLS(col.value),
          `${stateName(st)}: the region scrolls, but \`${col?.sheet}: ${col?.selector} ` +
            `{ overflow-y: ${col?.value} }\` clips inside it — a second clip edge under a ` +
            'scrolling ancestor is the bug in miniature.',
        ).toBe(true)
      }
    })
  }
})

describe('the fill-pane floor exists exactly where a scroller stands behind it', () => {
  // CockpitPaneFrame's fill branch floors at `var(--cockpit-fill-min, 0)`. Unset, every
  // fill pane in a REGION cockpit floors at ZERO — so CW's six content-fit aux strips
  // (flex: 0 0 auto, unshrinkable) starve DECODE, the pane the cockpit exists for and the
  // one declared weight={3}, toward nothing.
  //
  // The floor is legal only where deficit past it has somewhere to go. That used to be
  // nowhere inside a region, which is why the knob was fenced to the two region-less
  // shells; with the column valve above it is the column's scrollbar.
  const fillMin = blockVar('--cockpit-fill-min')
  for (const st of REGION_STATES) {
    it(`${stateName(st)}: the knob matches the state`, () => {
      const knob = regionWinner(st, fillMin)
      const region = regionWinner(st, blockOverflowY)
      if (region!.value === 'hidden') {
        expect(
          knob,
          `${stateName(st)}: --cockpit-fill-min is unset, so every fill pane floors at 0 and the ` +
            "column's unshrinkable content strips starve it to zero height.",
        ).not.toBeNull()
        expect(
          knob!.value,
          `${stateName(st)}: the floor is \`${knob!.value}\` (from ${knob!.selector}). Under a ` +
            'bounded parent a floor must be written to YIELD — min(<em>, <share>) — never a bare ' +
            'length, and never px (zoom-hostile).',
        ).toMatch(/^min\(\d+(\.\d+)?em, ?\d+%\)$/)
      } else {
        expect(
          knob,
          `${stateName(st)}: --cockpit-fill-min is set to \`${knob?.value}\` by ${knob?.selector}, ` +
            'but this state makes every fill pane content-height in a content-height row. A ' +
            'percentage share there resolves against an indefinite height (engine-variable), and ' +
            'an em floor is dead weight the region must scroll past.',
        ).toBeNull()
      }
    })
  }
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
        'deficit becomes clipping. Growth is expressed as fr shares (and fill weights) only; ' +
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
  // No exceptions. The former one (`.rtty-cockpit > .pane-frame { min-height: 10em }`)
  // was discovered DEAD in the 2026-07-31 review round: CockpitPaneFrame stamps
  // `min-height: 0` inline on fill frames, and an inline declaration outranks every
  // sheet selector — the rule shipped as exactly the dead-fix class this file documents.
  // The sanctioned floor channel for a region-less cockpit (RTTY / SSTV) is now the
  // frame's own inline `min-height: var(--cockpit-fill-min, 0)`, with the knob set by
  // the shell rule and fenced below.
  const ALLOWED = new Set<string>([])

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
        'that recurred five times — express prominence as a fill weight via CockpitPaneFrame ' +
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

  it('only a scroller-backed owner may set --cockpit-fill-min (the inline floor knob)', () => {
    // The knob inherits, so ANY ancestor rule could floor every fill frame below it —
    // including frames with nothing between them and a clip edge, where a floor IS the
    // clip mechanism. So the allowlist is not "these files" but "these owners, each of
    // which has a scroller standing behind the floor":
    //   · the two REGION-LESS shells (RTTY / SSTV): their own `overflow-y: auto` deficit
    //     valve (cockpit-shells.test.ts), and their frames are bare shell children.
    //   · the region's BOUNDED flow: the column valve above (.cockpit-col scrolls).
    // Anything else — a shell that clips, a column, a tier variant, `:root` — is refused.
    const ALLOWED_KNOB = new Set(['.layout.single.rtty-cockpit', '.layout.single.sstv-view'])
    const ALLOWED_REGION = new Set([".cockpit-panes[data-flow='fill']"])
    const offenders = [
      ...STYLES_RULES.filter(
        (r) => /--cockpit-fill-min\s*:/.test(r.body) && !ALLOWED_KNOB.has(r.selector),
      ).map((r) => `styles.css: ${r.selector}`),
      ...RULES.filter(
        (r) => /--cockpit-fill-min\s*:/.test(r.body) && !ALLOWED_REGION.has(r.selector),
      ).map((r) => `cockpit-panes.css: ${r.selector}`),
    ]
    expect(
      offenders,
      `--cockpit-fill-min set by an owner with no scroller behind it:\n${offenders.join('\n')}\n` +
        'A frame floor with a clip edge and nothing else beneath it is the documented clip ' +
        'bug; the knob is legal only where a deficit valve scrolls behind it.',
    ).toEqual([])
  })
})
