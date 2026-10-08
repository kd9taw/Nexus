import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { LEFT_SIDE_MAX_SHARE, LEFT_SIDE_MIN_EM, leftSideRange, logColValue } from './features/paneColumns'

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
  // The dashboard rail's track and its parts (components/DashRail) are structural too: the rail is a
  // column beside the cockpit, sized only here. So are Phone's LEFT SIDE and the two wrappers it
  // stands in (2026-10-03): a styles.css rule naming one could give the wrappers a box, or the side a
  // size, from the sheet that has lost every one of these fights before.
  // The RF scope pane's modifier on FT's waterfall strip (2026-10-04) too: the strip's direction is
  // structure, and the styles.css `.panel` it overrides is exactly the kind of rule that wins a tie.
  for (const cls of ['cockpit-panes', 'cockpit-col', 'cockpit-txdock', 'cockpit-txstrip', 'remote-observer-strip', 'cockpit-pane-acts', 'cockpit-recall', 'cockpit-recall-kept', 'remote-cockpit-lower', 'remote-observer-dock', 'cockpit-colseam', 'cockpit-colseam-2', 'cockpit-colseam-3', 'dash-rail', 'dash-rail-seam', 'dash-rail-head', 'dash-rail-acts', 'dash-rail-col', 'cockpit-flat', 'cockpit-leftrow', 'cockpit-stage', 'cockpit-left', 'cockpit-left-col', 'cockpit-left-seam', 'cockpit-rfbeside']) {
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

/** Cascade winner of a per-block-computed property for an element carrying exactly the class `cls`,
 *  across BOTH sheets (colWinner's rule: styles.css sorts first, so an equal-specificity structural
 *  rule wins a tie). Subject-only. */
function classWinner<T>(
  cls: string,
  blockValue: (body: string) => T | null,
): { value: T; selector: string; sheet: string } | null {
  const candidates: Array<{ r: Rule; sheet: string; rank: number }> = [
    ...STYLES_RULES.map((r) => ({ r, sheet: 'styles.css', rank: 0 })),
    ...RULES.map((r) => ({ r, sheet: 'cockpit-panes.css', rank: 1 })),
  ]
  let win: { value: T; selector: string; sheet: string; spec: number; key: number } | null = null
  for (const { r, sheet, rank } of candidates) {
    const parts = r.selector.split(/\s*[>+~]\s*|\s+/)
    if (!new RegExp(`\\.${cls}(?![a-z0-9-])`).test(parts[parts.length - 1])) continue
    const v = blockValue(r.body)
    if (v === null) continue
    const spec = specificity(r.selector)
    const key = rank * 1e6 + r.order
    if (!win || spec > win.spec || (spec === win.spec && key >= win.key)) win = { value: v, selector: r.selector, sheet, spec, key }
  }
  return win && { value: win.value, selector: win.selector, sheet: win.sheet }
}

/** Final value of one longhand (or a shorthand read whole) a block computes. */
const blockDecl = (prop: string) => (body: string): string | null => {
  let v: string | null = null
  for (const decl of body.split(';')) {
    const m = new RegExp(`^\\s*${prop}\\s*:\\s*(\\S[^]*?)\\s*$`).exec(decl)
    if (m) v = m[1].replace(/\s+/g, ' ')
  }
  return v
}

/** The Phone shell's own gap, from styles.css — the gap its scope divider's margins give back. */
const phoneShellGap = () => {
  let v: string | null = null
  for (const r of STYLES_RULES) if (r.selector === '.layout.single.phone-cockpit') v = blockDecl('gap')(r.body) ?? v
  return v
}

describe('THE LEFT SIDE (2026-10-03): a full-height column beside the scope, and the five-kind shell is unchanged without it', () => {
  // The operator's pick: ⊞ Panels ▸ Arrange's "Left side", a column from under the header to the
  // dock, beside the scope, the TX strip and the pane region, for Band Activity, Spots and Needed.
  // Phone renders the scope, the strip and the region inside TWO WRAPPERS that are always there — so
  // showing the side never moves the voice keyer or the log form to a new parent — and the wrappers
  // generate NO BOX until the side shows. These compute that the two states are what they claim.

  it('the wrappers generate no box until the side shows, so the shell lays out exactly as the five-kind one', () => {
    const win = classWinner('cockpit-flat', blockDecl('display'))
    expect(win, 'nothing declares display on .cockpit-flat — a wrapper with a box changes every Phone screen').not.toBeNull()
    expect(
      win!.value,
      `\`${win!.sheet}: ${win!.selector} { display: ${win!.value} }\` — the scope, the TX strip and the region ` +
        'must stay flex items of the SHELL while no side shows: the strip\'s sticky edges are measured against ' +
        'the shell, which is what keeps Stop TX on screen at a 175 % pin.',
    ).toBe('contents')
    // A flat wrapper is never a scroll container or a clip edge either.
    expect(classWinner('cockpit-flat', blockOverflowY)).toBeNull()
  })

  it('with the side shown, the row lays the side beside the stage, and the stage stacks with the shell’s own gap', () => {
    expect(classWinner('cockpit-leftrow', blockDecl('display'))?.value).toBe('flex')
    expect(classWinner('cockpit-leftrow', blockDecl('flex-direction'))?.value).toBe('row')
    expect(classWinner('cockpit-stage', blockDecl('display'))?.value).toBe('flex')
    expect(classWinner('cockpit-stage', blockDecl('flex-direction'))?.value).toBe('column')
    // The scope's divider gives back a gap of the shell's size (cockpit-shells.test.ts computes the net
    // in both states), so the stage must stack with that same gap or the stock layout moves when the
    // side shows.
    const stage = classWinner('cockpit-stage', blockDecl('gap'))
    expect(stage, '.cockpit-stage declares no gap').not.toBeNull()
    expect(stage!.value, 'the stage stacks with a different gap than the shell it stands in for').toBe(phoneShellGap())
    // The row is the shell's grower in the side state, as the region is the stage's: basis 0, so the
    // scope keeps the height it was dragged to.
    expect(classWinner('cockpit-leftrow', blockDecl('flex'))?.value).toBe('1 1 0')
    expect(classWinner('cockpit-stage', blockDecl('flex'))?.value).toBe('1 1 0')
  })

  it('neither the row nor the stage scrolls or clips: the TX strip keeps the SHELL as its scroll container', () => {
    // A sticky box sticks within its nearest scroll container. A stage that scrolled or clipped would
    // become the strip's, and Stop TX would park inside the stage instead of on the screen.
    for (const cls of ['cockpit-leftrow', 'cockpit-stage']) {
      const win = classWinner(cls, blockOverflowY)
      expect(win, `\`${win?.sheet}: ${win?.selector} { overflow-y: ${win?.value} }\``).toBeNull()
    }
  })

  it('the width is the operator’s, clamped by the layout itself between an em floor and a share of the row', () => {
    const r = RULES.filter((x) => x.selector === '.cockpit-left')
    expect(r.length, 'exactly one .cockpit-left rule').toBe(1)
    const flex = blockDecl('flex')(r[0].body)
    const m = /^0 0 clamp\((\d+(?:\.\d+)?)em, var\(--cockpit-left-w, (\d+(?:\.\d+)?)em\), (\d+(?:\.\d+)?)%\)$/.exec(flex ?? '')
    expect(
      m,
      `.cockpit-left { flex: ${flex} } — the side must neither grow nor shrink, and its basis must be the ` +
        "operator's width clamped by the sheet: clamp(<floor>em, var(--cockpit-left-w, <default>em), <share>%). " +
        'A width stored on a wide window is then clamped on load and on every resize without being rewritten.',
    ).not.toBeNull()
    const [floorEm, defaultEm, sharePct] = [Number(m![1]), Number(m![2]), Number(m![3])]
    expect(defaultEm, 'the default sits on (or under) the floor').toBeGreaterThan(floorEm)
    expect(sharePct, 'the side may take more than half of the row it shares with the scope and the panes').toBeLessThanOrEqual(50)
    // The divider moves through exactly the range the sheet honours (features/paneColumns).
    expect(floorEm).toBe(LEFT_SIDE_MIN_EM)
    expect(sharePct).toBe(LEFT_SIDE_MAX_SHARE * 100)
    for (const [rowW, font] of [[1200, 14], [1900, 14], [3300, 16], [400, 14]]) {
      expect(leftSideRange(rowW, font)).toEqual({ min: floorEm * font, max: Math.max(floorEm * font, (sharePct / 100) * rowW) })
    }
  })

  it('the width token rides inline on the side: no rule declares it, and only .cockpit-left reads it', () => {
    const reads = [
      ...RULES.filter((r) => r.body.includes('var(--cockpit-left-w')).map((r) => `cockpit-panes.css: ${r.selector}`),
      ...STYLES_RULES.filter((r) => r.body.includes('var(--cockpit-left-w')).map((r) => `styles.css: ${r.selector}`),
    ]
    expect(reads).toEqual(['cockpit-panes.css: .cockpit-left'])
    expect([...RULES, ...STYLES_RULES].filter((r) => /--cockpit-left-w\s*:/.test(r.body)).map((r) => r.selector)).toEqual([])
  })

  it('the side’s panes can never stretch the row: its column is out of flow, fills the side, and scrolls', () => {
    // Its content taller than the row would otherwise raise the row's automatic minimum and make the
    // whole SHELL scroll for a long Spots list. Out of flow it contributes nothing; the deficit is the
    // column's own scrollbar — the pane contract's second legal fate.
    expect(classWinner('cockpit-left', blockDecl('position'))?.value, 'the side is not the containing block of its column').toBe('relative')
    expect(classWinner('cockpit-left-col', blockDecl('position'))?.value).toBe('absolute')
    expect(classWinner('cockpit-left-col', blockDecl('inset'))?.value).toBe('0')
    const y = classWinner('cockpit-left-col', blockOverflowY)
    expect(y && SCROLLS(y.value), `the side's column does not scroll: ${JSON.stringify(y)}`).toBe(true)
    expect(classWinner('cockpit-left-col', blockDecl('flex-direction'))?.value).toBe('column')
    // Its frames stack with the region's columns' gap.
    expect(classWinner('cockpit-left-col', blockDecl('gap'))?.value).toBe(classWinner('cockpit-col', blockDecl('gap'))?.value)
    // A fill pane there floors as it does in a bounded column, and the floor yields.
    expect(classWinner('cockpit-left-col', blockVar('--cockpit-fill-min'))?.value).toMatch(/^min\(\d+(\.\d+)?em, ?100%\)$/)
  })

  it('the side’s divider straddles the gap beside it, the full height, over no pane', () => {
    const seam = RULES.filter((x) => x.selector === '.cockpit-left-seam')
    expect(seam.length).toBe(1)
    const d = (p: string) => blockDecl(p)(seam[0].body)
    expect([d('position'), d('top'), d('bottom'), d('margin')]).toEqual(['absolute', '0', '0', '0'])
    const w = /^(\d+)px$/.exec(d('width') ?? '')
    expect(w, `.cockpit-left-seam { width: ${d('width')} }`).not.toBeNull()
    // Centred on the gap between the side and the stage: half the row's gap, then half its own width.
    const gap = classWinner('cockpit-leftrow', blockDecl('gap'))!.value
    expect(d('right')).toBe(`calc(${gap} / -2 - ${Number(w![1]) / 2}px)`)
  })
})

describe("FT'S ARRANGED COLUMNS (2026-10-07): stacks that scroll, FT's gap, and the rail first only by its own class", () => {
  // FT keeps its own grid; once the operator arranges anything, each of its columns is an `.op-stack`
  // (OperateCockpit's arranged branch). Its frames are content-sized strips and floored feeds, so the
  // column itself must be the scroller behind them, or the Tx1–Tx6 machine's tail is past a clip edge.
  it('each column is a flex column that scrolls, and a feed in it floors at a yielding 10em', () => {
    const y = classWinner('op-stack', blockOverflowY)
    expect(y && SCROLLS(y.value), `an arranged column does not scroll: ${JSON.stringify(y)}`).toBe(true)
    expect(classWinner('op-stack', blockDecl('display'))?.value).toBe('flex')
    expect(classWinner('op-stack', blockDecl('flex-direction'))?.value).toBe('column')
    expect(classWinner('op-stack', blockDecl('min-height'))?.value, 'a column that cannot shrink to its track').toBe('0')
    expect(classWinner('op-stack', blockDecl('min-width'))?.value).toBe('0')
    expect(classWinner('op-stack', blockVar('--cockpit-fill-min'))?.value).toMatch(/^min\(10em, ?100%\)$/)
  })

  it("its frames stack with FT's own gap, the stock rail's", () => {
    expect(classWinner('op-stack', blockDecl('gap'))?.value).toBe(classWinner('cockpit-side', blockDecl('gap'))?.value)
  })

  it('the rail goes first by `order` only, through its own class, never every arranged column', () => {
    expect(classWinner('op-stack-lead', blockDecl('order'))?.value).toBe('-1')
    expect(classWinner('op-stack', blockDecl('order')), 'every arranged column would stand first').toBeNull()
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

describe('the rail-bound callsign card is a CEILING, and a small one', () => {
  // #168 gave the Operate rail's callsign card a bound so a selected station could not take
  // the rail apart. On 2026-09-15 the operator sent a screenshot of a station with ELEVEN
  // previous contacts at 100 %: the card had squeezed Band Activity out and shortened Rx
  // Frequency. The first bound was written against the TWO-pane Classic rail (card +
  // Stations); Roster's rail holds THREE panes, and 45 % of it is half the rail once the two
  // feeds have paid their gaps.
  //
  // What is computed here is the SHAPE and the BOUNDS, never the exact number — a guard that
  // pins a literal goes red on a healthy re-tune, which is how two of them blocked a release
  // in one day. Measurement is what says whether the number is RIGHT; headless Chrome does
  // that (ui/layout-harness, and the numbers are in the cockpit-panes.css comment).
  const recall = () => RULES.filter((r) => r.selector === '.cockpit-recall')

  it('exists, exactly once', () => {
    expect(recall(), 'the card lost its placement rule — it would take the whole rail').toHaveLength(1)
  })

  it('is a max-height and never a min-height', () => {
    const body = recall()[0].body
    expect(/max-height\s*:/.test(body)).toBe(true)
    // A FLOOR here would sit under the region's own clipping and be the crush mechanism
    // reborn — the whole reason the 2026-07 rebuild deleted the 18em floors it found.
    expect(
      /(^|[;{\s])min-height\s*:/.test(body),
      '.cockpit-recall declares a min-height — a floor under a clipping ancestor is the bug ' +
        'this sheet exists to retire',
    ).toBe(false)
  })

  it('yields two ways: an em ceiling AND a share of the rail', () => {
    const m = /max-height\s*:\s*min\(\s*([\d.]+)em\s*,\s*([\d.]+)%\s*\)/.exec(recall()[0].body)
    expect(
      m,
      'the cap is no longer min(<em>, <share>). One arm alone is wrong in a known direction: ' +
        'a bare em cannot yield on a short rail, and a bare % scales the card with the screen ' +
        'when it is a reference card, not a feed.',
    ).not.toBeNull()
    const [, em, pct] = m!.map(Number) as unknown as [string, number, number]
    // THE SHARE. Roster's rail holds three panes; the card is one of them and is the only one
    // that is not a live feed, so it may never claim more than an equal share. At 45 % it was
    // taking half the rail and both feeds paid for it.
    expect(pct, 'the card may not claim more than a third of a rail it shares with two feeds').toBeLessThanOrEqual(34)
    // THE CEILING. Identity + badges + a few prior contacts; past that the card scrolls
    // inside itself (below). A long history must not buy a bigger card.
    expect(em, 'the em ceiling is large enough for a long history to claim a big screen').toBeLessThanOrEqual(16)
  })

  it('scrolls inside itself past the ceiling, so the tail is reachable rather than clipped', () => {
    expect(blockOverflowY(recall()[0].body)).toBe('auto')
  })
})

describe("in FT's arranged columns the card keeps its size, and the column scrolls instead", () => {
  // The operator's pick (2026-10-07, "Card keeps its size"): an arranged column floors its feeds and boxes
  // at a yielding 10em (`.op-stack`), which left the card the only item that could shrink, so in a crowded
  // column it gave up its whole height, down to its padding, before the column scrolled. The host adds a
  // second flat class to the card in the arranged branch only: the card keeps its stock bound and never
  // shrinks, and the column, which is a scroller, takes the overflow. Computed for the element the arranged
  // branch draws, which carries both classes (`recall-card cockpit-recall cockpit-recall-kept`).
  const CARD = ['recall-card', 'cockpit-recall', 'cockpit-recall-kept']
  const kept = () => RULES.filter((r) => r.selector === '.cockpit-recall-kept')

  /** Cascade winner for an element carrying all of `classes`: a rule applies when its subject is made of
   *  those classes only (this sheet's rules are one class each), by specificity, then the later sheet and
   *  the later rule (classWinner's order). */
  function cardWinner<T>(blockValue: (body: string) => T | null): { value: T; selector: string } | null {
    const candidates = [
      ...STYLES_RULES.map((r) => ({ r, rank: 0 })),
      ...RULES.map((r) => ({ r, rank: 1 })),
    ]
    let win: { value: T; selector: string; spec: number; key: number } | null = null
    for (const { r, rank } of candidates) {
      const parts = r.selector.split(/\s*[>+~]\s*|\s+/)
      const subject = parts[parts.length - 1]
      if (parts.length !== 1 || !/^(\.[a-z][a-z0-9-]*)+$/.test(subject)) continue
      if (!subject.slice(1).split('.').every((c) => CARD.includes(c))) continue
      const v = blockValue(r.body)
      if (v === null) continue
      const spec = specificity(r.selector)
      const key = rank * 1e6 + r.order
      if (!win || spec > win.spec || (spec === win.spec && key >= win.key)) win = { value: v, selector: r.selector, spec, key }
    }
    return win && { value: win.value, selector: win.selector }
  }

  /** The flex-shrink a block computes: the longhand, or the shorthand's second number (`flex: 0 0 auto`). */
  const blockShrink = (body: string): string | null => {
    let v: string | null = null
    for (const decl of body.split(';')) {
      const long = /^\s*flex-shrink\s*:\s*(\S+)\s*$/.exec(decl)
      if (long) v = long[1]
      const short = /^\s*flex\s*:\s*(\S[^]*?)\s*$/.exec(decl)
      if (short) {
        const parts = short[1].trim().split(/\s+/)
        v = parts[0] === 'none' ? '0' : parts.length > 1 && /^[\d.]+$/.test(parts[1]) ? parts[1] : '1'
      }
    }
    return v
  }

  it('is one flat class, declared once', () => {
    expect(kept(), 'the arranged card lost its rule: it shrinks to its padding in a crowded column again').toHaveLength(1)
  })

  it('never shrinks: its flex-shrink wins over the stock bound’s', () => {
    const shrink = cardWinner(blockShrink)
    expect(shrink?.selector).toBe('.cockpit-recall-kept')
    expect(shrink?.value).toBe('0')
  })

  it('keeps the stock ceiling and its own scroll: the kept class bounds nothing differently', () => {
    expect(cardWinner(blockDecl('max-height'))?.value).toBe(blockDecl('max-height')(RULES.find((r) => r.selector === '.cockpit-recall')!.body))
    expect(cardWinner(blockOverflowY)?.value).toBe('auto')
  })

  it('is never a floor: no min-height and no height, from either sheet', () => {
    expect(/(^|[;{\s])(min-)?height\s*:/.test(kept()[0]?.body ?? ''), 'a floor in a column is the crush mechanism reborn').toBe(false)
    expect(cardWinner(blockDecl('min-height')), 'something gives the card a floor').toBeNull()
    expect(cardWinner(blockDecl('height')), 'something gives the card a fixed height').toBeNull()
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

  it('declares no min-height floor except regions with a shell scrolling fallback', () => {
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
        // The hosted FT region has the same shell-valve contract. Its smaller
        // viewport-capped floor is paired with computed overflow guards in
        // cockpit-shells.test.ts and real compiled-browser reachability tests.
        if (r.selector === '.remote-cockpit-lower' && v === 'min(18em, calc(0.35 * var(--vh-eff)))') continue
        offenders.push(`${r.selector} { min-height: ${v} }`)
      }
    }
    expect(
      offenders,
      `manufactured floor:\n${offenders.join('\n')}\nA floor under a bounded ancestor is how ` +
        'deficit becomes clipping. Growth is expressed as fr shares (and fill weights) only; ' +
        'only the declared region floors have a shell scrolling fallback.',
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
    //
    // Since layout L2 the max is the operator's (`--cockpit-col-log`, the log column divider),
    // so BOTH its values are held to it: the stock fallback the template carries, and the only
    // value the divider ever writes (features/paneColumns.logColValue), at any width it could
    // write — a narrow one, a wide one and one far past any window.
    for (const tier of [2, 3] as const) {
      const r = RULES.find((x) => x.selector === `.cockpit-panes[data-cols='${tier}']`)
      expect(r, `no .cockpit-panes[data-cols='${tier}'] rule`).toBeDefined()
      const cols = /grid-template-columns\s*:\s*([^;]+)/.exec(r!.body)?.[1].trim() ?? ''
      const log = /minmax\(24em, var\(--cockpit-col-log, (min\(44em, \d+%\))\)\)\s*$/.exec(cols)
      expect(
        log,
        `tier ${tier} log track is \`${cols}\`: not the 24em-floored track whose max is the log ` +
          'divider\'s width with a proportion-bounded stock fallback.',
      ).not.toBeNull()
    }
    for (const px of [336, 900, 100_000]) {
      expect(
        logColValue(px),
        'the log divider writes a width that is not capped by a share of the region — its fixed ' +
          'max would pay out in full before the feed track gets anything.',
      ).toMatch(/^min\(\d+px, (\d+)%\)$/)
      expect(Number(/, (\d+)%\)$/.exec(logColValue(px))![1])).toBeLessThanOrEqual(50)
    }
  })

  it('the column dividers’ tokens are read by the two- and three-track templates and nothing else', () => {
    // The tokens ride INLINE on the region (RegionColumnSeams), so whatever rule reads one is the
    // whole of what a divider can resize. Computed over every rule of both sheets.
    const reads = (name: string) => [
      ...RULES.filter((r) => r.body.includes(`var(${name}`)).map((r) => `cockpit-panes.css: ${r.selector}`),
      ...STYLES_RULES.filter((r) => r.body.includes(`var(${name}`)).map((r) => `styles.css: ${r.selector}`),
    ]
    expect(reads('--cockpit-col-log')).toEqual([
      "cockpit-panes.css: .cockpit-panes[data-cols='2']",
      "cockpit-panes.css: .cockpit-panes[data-cols='3']",
    ])
    for (const t of ['--cockpit-col-a', '--cockpit-col-b']) {
      expect(reads(t), `${t} is read outside the three-track template`).toEqual([
        "cockpit-panes.css: .cockpit-panes[data-cols='3']",
      ])
    }
    // And nothing DECLARES them in either sheet: the region's inline style is their only source,
    // so the stacking tier (which reads none) is untouched by a stored width.
    const declares = [...RULES, ...STYLES_RULES].filter((r) => /--cockpit-col-(a|b|log)\s*:/.test(r.body))
    expect(declares.map((r) => r.selector)).toEqual([])
  })

  it('three columns: the feed columns keep a floor a dragged split cannot pass, and it always fits', () => {
    // The divider between the two feed columns stops their shares at MIN_SHARE, 7.5 % of the
    // pair — ~85 px at 1920×1080. The template floors both at 16em; that must never bind at the
    // stock split and must always fit, with the log at its 50 % ceiling, in the narrowest region
    // the tier is used at (classifyRegionCols: 1700 CSS px). Computed at the 14 px body font.
    const r = RULES.find((x) => x.selector === ".cockpit-panes[data-cols='3']")!
    const tracks = /grid-template-columns\s*:\s*minmax\((\d+)em, var\(--cockpit-col-a, ([\d.]+)fr\)\) minmax\((\d+)em, var\(--cockpit-col-b, ([\d.]+)fr\)\)/.exec(r.body)
    expect(tracks, `the three-track template no longer floors both feed columns: ${r.body.trim()}`).not.toBeNull()
    const [, aEm, aFr, bEm, bFr] = tracks!.map(Number) as unknown as [string, number, number, number, number]
    const FONT = 14
    const REGION = 1700
    const GAP = 12
    const stockLog = Math.min(44 * FONT, 0.4 * REGION)
    const pair = REGION - stockLog - 2 * GAP
    expect(pair * Math.min(aFr, bFr) / (aFr + bFr), 'the stock split already sits on a floor').toBeGreaterThan(Math.max(aEm, bEm) * FONT)
    expect((aEm + bEm) * FONT + 0.5 * REGION + 2 * GAP, 'two floors and a log at its ceiling overrun the region').toBeLessThan(REGION)
  })

  it('the TX dock is pinned and unshrinkable (flex: 0 0 auto)', () => {
    const r = RULES.find((x) => x.selector === '.cockpit-txdock')
    expect(r, 'no .cockpit-txdock rule').toBeDefined()
    expect(
      r!.body.replace(/\s+/g, ' '),
      'the dock carries PTT/send: it must never be a flex grower or a shrink victim.',
    ).toMatch(/flex: 0 0 auto/)
  })

  // THE TX STRIP (2026-10-01) carries Stop TX, Tune and the latch on every screen but
  // FT's. The fence above is what makes this one rule the whole cascade for it: styles.css may
  // not name the class, so no later or heavier rule can unpin it.
  it('the TX strip is unshrinkable and sticky on BOTH edges, the bottom clearing the dock', () => {
    const r = RULES.filter((x) => x.selector === '.cockpit-txstrip')
    expect(r.length, 'exactly one .cockpit-txstrip rule').toBe(1)
    const decl = (prop: string) => {
      let v: string | null = null
      for (const d of r[0].body.split(';')) {
        const m = new RegExp(`^\\s*${prop}\\s*:\\s*([^]*?)\\s*$`).exec(d)
        if (m) v = m[1].replace(/\s+/g, ' ')
      }
      return v
    }
    expect(decl('flex'), 'a grower or a shrink victim').toBe('0 0 auto')
    expect(decl('position'), 'not sticky: at a large pin it scrolls out of the window').toBe('sticky')
    expect(decl('top'), 'scrolled down, it would leave through the top').toBe('0')
    expect(decl('bottom'), 'parked at the bottom, it must clear the sticky dock').toBe('var(--cockpit-txstrip-bottom, 0px)')
    // A sticky box over scrolled content must be opaque and stack above the panes it covers.
    expect(decl('background')).toBe('var(--panel)')
    expect(Number(decl('z-index'))).toBeGreaterThanOrEqual(1)
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

  /** Per-PROPERTY exemptions: a rule that may declare one named size property because a
   *  scroller stands behind it. `.connect-strip > .pane-frame` caps the Connect bottom
   *  strip at 30% of --vh-eff — the strip's real ceiling, since a grid track max always
   *  pays out (cockpit-shells.test.ts computes that the cap stays --vh-eff-bounded) — and
   *  a pane taller than the cap scrolls inside its own `.pane-body`, which the same file
   *  now COMPUTES for this exact chain instead of regex-matching. */
  const SIZED_OK = new Map<string, Set<string>>([
    ['.connect-strip > .pane-frame', new Set(['max-height'])],
    // Once the operator sizes the strip (layout L7) the ROW is its height (`.connect`'s
    // grid-auto-rows) and this lifts the cap so the panes fill it — each still scrolls inside its
    // own `.pane-body`; in the xs stack the same cap as above comes back.
    ['.connect-strip[data-sized] > .pane-frame', new Set(['max-height'])],
    ["[data-viewport='xs'] .connect-strip[data-sized] > .pane-frame", new Set(['max-height'])],
  ])

  /** A flex-basis that leaves the frame's height to the grid/column. Anything else is the
   *  frame sizing itself, which is what this file exists to forbid. */
  const OK_BASIS = new Set(['0', '0px', '0%', 'auto', 'content'])

  /** Final flex triple a block computes (longhands + shorthand, in-block declaration
   *  order). The shorthand's omitted components take their SHORTHAND defaults, not the
   *  property initials — `flex: 2` is `2 1 0`, and `flex: 0 0 12em` pins a frame at 12em
   *  with a grow of 0, i.e. sails straight past a grow-only census. */
  function blockFlex(body: string): {
    grow: number | null
    shrink: number | null
    basis: string | null
  } {
    let grow: number | null = null
    let shrink: number | null = null
    let basis: string | null = null
    for (const decl of body.split(';')) {
      // `!important` must NOT hide a declaration from this census. It is the one spelling that
      // actually beats CockpitPaneFrame's inline `flex`, so an anchored `\s*$` here made the
      // most dangerous rule the only invisible one. Caught by adversarial review, 2026-08-04.
      let m = /^\s*flex-grow\s*:\s*([\d.]+)\s*(?:!\s*important\s*)?$/.exec(decl)
      if (m) {
        grow = parseFloat(m[1])
        continue
      }
      m = /^\s*flex-shrink\s*:\s*([\d.]+)\s*(?:!\s*important\s*)?$/.exec(decl)
      if (m) {
        shrink = parseFloat(m[1])
        continue
      }
      m = /^\s*flex-basis\s*:\s*(\S[^]*?)\s*$/.exec(decl)
      if (m) {
        basis = m[1].replace(/\s+/g, ' ')
        continue
      }
      m = /^\s*flex\s*:\s*(\S[^]*?)\s*$/.exec(decl)
      if (!m) continue
      const v = m[1].replace(/\s+/g, ' ')
      if (v === 'none') [grow, shrink, basis] = [0, 0, 'auto']
      else if (v === 'initial') [grow, shrink, basis] = [0, 1, 'auto']
      else if (v === 'auto') [grow, shrink, basis] = [1, 1, 'auto']
      else {
        const parts = v.split(' ')
        grow = /^[\d.]+$/.test(parts[0]) ? parseFloat(parts[0]) : 0
        if (parts[1] !== undefined && /^[\d.]+$/.test(parts[1])) {
          shrink = parseFloat(parts[1])
          basis = parts[2] ?? '0'
        } else {
          shrink = 1
          basis = parts[1] ?? '0'
        }
      }
    }
    return { grow, shrink, basis }
  }

  /** Subject (rightmost compound) of a selector. */
  const subject = (sel: string) => {
    const parts = sel.split(/\s*[>+~]\s*|\s+/)
    return parts[parts.length - 1]
  }

  it('the census itself sees `!important` — the one spelling that beats the inline flex', () => {
    // A REGRESSION GUARD ON THE GUARD. blockFlex's grow/shrink patterns were once anchored
    // `\s*$`, so `flex-grow: 1 !important` parsed as nothing and the census reported a clean
    // tree. That is precisely backwards: CockpitPaneFrame stamps `flex` INLINE, so a plain
    // stylesheet `flex-grow` loses to it and is nearly harmless, while `!important` wins and
    // is the rule that can actually re-size a frame from styles.css. The most dangerous
    // spelling was the only invisible one, and the full suite stayed green.
    expect(blockFlex('flex-grow: 2 !important').grow).toBe(2)
    expect(blockFlex('flex-grow:1!important').grow).toBe(1)
    expect(blockFlex('flex-shrink: 0 !important').shrink).toBe(0)
    // and the ordinary spellings still parse
    expect(blockFlex('flex-grow: 1').grow).toBe(1)
    expect(blockFlex('flex-shrink: 0').shrink).toBe(0)
    // a value that is not a bare number must still not be mistaken for one
    expect(blockFlex('flex-grow: var(--x)').grow).toBeNull()
  })

  it('no styles.css rule on .pane-frame sizes it (grow, shrink pin, basis, floor, height or cap)', () => {
    // The census used to read flex-GROW and min-height only, so four properties that size a
    // frame just as hard walked straight through it: `height`/`max-height` (a definite box,
    // the mechanism `.connect-strip > .pane-frame` already legitimately uses — an exemption
    // that was invisible rather than declared), a flex-BASIS length, and `flex-shrink: 0`,
    // which under the region's `overflow: hidden` is the rigid-stack-with-no-scroller shape
    // the contract forbids. All of them are reachable through the flex SHORTHAND too, where
    // a grow of 0 made them doubly invisible.
    const offenders: string[] = []
    for (const r of STYLES_RULES) {
      if (!/\.pane-frame(?![a-z0-9-])/.test(subject(r.selector)) || ALLOWED.has(r.selector)) continue
      const { grow, shrink, basis } = blockFlex(r.body)
      if (grow != null && grow > 0) offenders.push(`${r.selector} { flex-grow: ${grow} }`)
      if (shrink === 0) offenders.push(`${r.selector} { flex-shrink: 0 }`)
      if (basis != null && !OK_BASIS.has(basis)) offenders.push(`${r.selector} { flex-basis: ${basis} }`)
      for (const m of r.body.matchAll(/min-height\s*:\s*([^;]+)/g)) {
        if (m[1].trim() !== '0') offenders.push(`${r.selector} { min-height: ${m[1].trim()} }`)
      }
      // Declaration-split so a `height` probe can never match `min-height`/`max-height`.
      for (const prop of ['height', 'max-height'] as const) {
        if (SIZED_OK.get(r.selector)?.has(prop)) continue
        for (const decl of r.body.split(';')) {
          const m = new RegExp(`^\\s*${prop}\\s*:\\s*(\\S[^]*?)\\s*$`).exec(decl)
          if (m) offenders.push(`${r.selector} { ${prop}: ${m[1].replace(/\s+/g, ' ')} }`)
        }
      }
    }
    expect(
      offenders,
      `a pane frame sized from styles.css:\n${offenders.join('\n')}\nThe grid cell sizes the ` +
        'frame (design3 §5 rule 2). A grower/floor/cap/pin here is the per-cockpit sizing ' +
        'mechanism that recurred five times — express prominence as a fill weight via ' +
        'CockpitPaneFrame `weight`, or add the exact selector to ALLOWED (whole rule) or ' +
        'SIZED_OK (one property) with the scroller that stands behind it documented.',
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
    //   · the three REGION-LESS shells (RTTY / PSK / SSTV): their own `overflow-y: auto`
    //     deficit valve (cockpit-shells.test.ts), and their frames are bare shell children.
    //   · the region's BOUNDED flow: the column valve above (.cockpit-col scrolls).
    // Anything else — a shell that clips, a column, a tier variant, `:root` — is refused.
    const ALLOWED_KNOB = new Set([
      '.layout.single.rtty-cockpit',
      '.layout.single.psk-cockpit',
      '.layout.single.sstv-view',
    ])
    // …and Phone's LEFT SIDE (2026-10-03), whose column IS the scroller its frames sit in
    // (`.cockpit-left-col`, overflow-y: auto — computed in the left-side block above), and FT's arranged
    // columns (2026-10-07), the same: `.op-stack` scrolls (computed in its block above).
    const ALLOWED_REGION = new Set([".cockpit-panes[data-flow='fill']", '.cockpit-left-col', '.op-stack'])
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
