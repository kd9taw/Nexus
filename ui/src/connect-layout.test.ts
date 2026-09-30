import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

// THE CONNECT GRID UNDER CLOSE + RESIZE (2026-09-13). Computes the cascade winner of every
// grid property the feature depends on, for the element as it actually renders — its
// [data-rails] state, the <html> [data-viewport] tier and the map-full shell — never a regex
// over the sheet (a dead selector passes those; that is how two dead fixes shipped).
//
// The shapes that must hold, and why each is a guard rather than a style choice:
//   · the column template has exactly one track per rendered column. A template with a rail
//     track and no rail in it is a dead 300px band beside the map — the "empty black box".
//   · every grid-area a rendered child names exists in the template. A missing area name does
//     not fail loudly: the child is auto-placed into an IMPLICIT track outside the grid.
//   · the strip rides an implicit `auto` row, so a closed strip leaves no row and no gap, and
//     its panes flow as auto columns, so two panes split the width instead of leaving a third.
//   · xs keeps the single stacked column and map-full keeps the bare map, whatever the rails say.
//
// Connect's grid still lives in styles.css (it predates the flat structural sheet and its
// tiers are descendant rules), so this parser understands exactly the selector shapes that
// sheet uses on these elements: classes, [attr='v'], :where([attr='v']) (zero specificity),
// descendant and child combinators. Anything else never matches — a selector it cannot read
// must not count as a winner by accident.

const css = readFileSync(fileURLToPath(new URL('./styles.css', import.meta.url)), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '')

interface Rule {
  selector: string
  body: string
  order: number
  media: string | null
}

function parseRules(sheet: string): Rule[] {
  const out: Rule[] = []
  let i = 0
  let order = 0
  const n = sheet.length
  const skipBalanced = () => {
    let depth = 1
    while (i < n && depth > 0) {
      if (sheet[i] === '{') depth++
      else if (sheet[i] === '}') depth--
      i++
    }
  }
  const parseBlock = (media: string | null): void => {
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
          let depth = 0
          let start = 0
          const parts: string[] = []
          for (let k = 0; k < sel.length; k++) {
            if (sel[k] === '(') depth++
            else if (sel[k] === ')') depth--
            else if (sel[k] === ',' && depth === 0) {
              parts.push(sel.slice(start, k))
              start = k + 1
            }
          }
          parts.push(sel.slice(start))
          for (const s of parts) {
            const one = s.trim().replace(/\s+/g, ' ')
            if (one) out.push({ selector: one, body, order, media })
          }
        }
        selStart = i
      } else if (ch === ';') {
        i++
        selStart = i
      } else i++
    }
  }
  parseBlock(null)
  return out
}

/** A modelled element: its classes and attributes. */
interface El {
  cls: string[]
  attrs?: Record<string, string>
  /** The classes of its children, for `:has(> .x)` (`.layout.single:has(> .connect-shell)`). */
  kids?: string[]
}

interface Compound {
  cls: string[]
  /** [name, value]; a null value is a presence test. */
  attrs: Array<[string, string | null]>
  /** `:has(> .x)`: a child with class x. */
  has: string[]
  spec: number
}

/** One compound, or null when it uses anything this reader does not model. */
function compound(s: string): Compound | null {
  const out: Compound = { cls: [], attrs: [], has: [], spec: 0 }
  let rest = s
  while (rest) {
    // `:has(> .x)`, rewritten by matchSpec so its `>` survives the split. Its specificity is its
    // argument's, one class.
    let m = /^:haschild\(([\w-]+)\)/.exec(rest)
    if (m) {
      out.has.push(m[1])
      out.spec++
      rest = rest.slice(m[0].length)
      continue
    }
    m = /^\.([\w-]+)/.exec(rest)
    if (m) {
      out.cls.push(m[1])
      out.spec++
      rest = rest.slice(m[0].length)
      continue
    }
    m = /^\[([\w-]+)=['"]?([^'"\]]+)['"]?\]/.exec(rest)
    if (m) {
      out.attrs.push([m[1], m[2]])
      out.spec++
      rest = rest.slice(m[0].length)
      continue
    }
    m = /^\[([\w-]+)\]/.exec(rest)
    if (m) {
      out.attrs.push([m[1], null]) // presence: `[data-sized]` (layout L7)
      out.spec++
      rest = rest.slice(m[0].length)
      continue
    }
    m = /^:where\(\[([\w-]+)=['"]?([^'"\]]+)['"]?\]\)/.exec(rest)
    if (m) {
      out.attrs.push([m[1], m[2]]) // zero specificity, by definition of :where()
      rest = rest.slice(m[0].length)
      continue
    }
    return null
  }
  return out
}

function compoundMatches(c: Compound, el: El): boolean {
  return (
    c.cls.every((k) => el.cls.includes(k)) &&
    c.attrs.every(([a, v]) => (v === null ? el.attrs?.[a] !== undefined : el.attrs?.[a] === v)) &&
    c.has.every((k) => el.kids?.includes(k) ?? false)
  )
}

/** Specificity of a matching selector, or -1 when it does not match `chain` (root → subject). */
function matchSpec(selector: string, chain: El[]): number {
  const tokens = selector
    .replace(/:has\(\s*>\s*\.([\w-]+)\s*\)/g, ':haschild($1)')
    .split(/\s*(>)\s*|\s+/)
    .filter((p): p is string => !!p)
  const parsed: Array<Compound | '>'> = []
  for (const t of tokens) {
    if (t === '>') parsed.push('>')
    else {
      const c = compound(t)
      if (!c) return -1
      parsed.push(c)
    }
  }
  const subj = parsed[parsed.length - 1]
  if (subj === '>' || !compoundMatches(subj, chain[chain.length - 1])) return -1
  let spec = subj.spec
  let idx = chain.length - 2
  let child = false
  for (let k = parsed.length - 2; k >= 0; k--) {
    const p = parsed[k]
    if (p === '>') {
      child = true
      continue
    }
    if (child) {
      if (idx < 0 || !compoundMatches(p, chain[idx])) return -1
      idx--
      child = false
    } else {
      while (idx >= 0 && !compoundMatches(p, chain[idx])) idx--
      if (idx < 0) return -1
      idx--
    }
    spec += p.spec
  }
  return spec
}

function lastDecl(body: string, prop: string): string | null {
  let v: string | null = null
  for (const decl of body.split(';')) {
    const m = new RegExp(`^\\s*${prop}\\s*:\\s*(\\S[^]*?)\\s*$`).exec(decl)
    if (m) v = m[1].replace(/\s+/g, ' ')
  }
  return v
}

/** The cascade winner of `prop` for the subject of `chain` over `rules`. */
function winner(rules: Rule[], chain: El[], prop: string): { value: string; selector: string } | null {
  let win: { value: string; selector: string; spec: number; order: number } | null = null
  for (const r of rules) {
    if (r.media !== null) continue
    const spec = matchSpec(r.selector, chain)
    if (spec < 0) continue
    const v = lastDecl(r.body, prop)
    if (v === null) continue
    if (!win || spec > win.spec || (spec === win.spec && r.order >= win.order)) {
      win = { value: v, selector: r.selector, spec, order: r.order }
    }
  }
  return win && { value: win.value, selector: win.selector }
}

/** Top-level (paren-aware) whitespace split — minmax(a, b) is ONE track. */
function tracks(v: string): string[] {
  const out: string[] = []
  let depth = 0
  let cur = ''
  for (const ch of v) {
    if (ch === '(') depth++
    if (ch === ')') depth--
    if (/\s/.test(ch) && depth === 0) {
      if (cur) out.push(cur)
      cur = ''
    } else cur += ch
  }
  if (cur) out.push(cur)
  return out
}

/** grid-template-areas → rows of names. */
const areaRows = (v: string) => [...v.matchAll(/['"]([^'"]*)['"]/g)].map((m) => m[1].trim().split(/\s+/))

const RULES = parseRules(css)

type Rails = 'both' | 'left' | 'right' | 'none'
const RAILS: Rails[] = ['both', 'left', 'right', 'none']
const TIERS = ['sm', 'md', 'lg', 'xs'] as const

function connectChain(vp: string, rails: Rails, mapFull: boolean): El[] {
  return [
    { cls: [], attrs: { 'data-viewport': vp } }, // <html>
    { cls: ['app'] },
    { cls: ['shell'] },
    { cls: ['layout', 'single'] },
    { cls: mapFull ? ['connect-shell', 'map-full'] : ['connect-shell'] },
    { cls: ['connect'], attrs: { 'data-rails': rails } },
  ]
}

const railsIn = (r: Rails): Array<'left' | 'right'> =>
  r === 'both' ? ['left', 'right'] : r === 'none' ? [] : [r]

describe('the reader itself', () => {
  it('honours specificity over source order, and :where() adds none (the guard can fire)', () => {
    // Positive control on a synthetic sheet: a later rails rule WITHOUT :where() outranks the
    // xs tier and would restack a small window as a rails grid. The reader must see that.
    const bad = parseRules(`
      [data-viewport='xs'] .connect { grid-template-areas: 'center'; }
      .connect[data-rails='left'] { grid-template-areas: 'left center'; }
    `)
    const good = parseRules(`
      [data-viewport='xs'] .connect { grid-template-areas: 'center'; }
      .connect:where([data-rails='left']) { grid-template-areas: 'left center'; }
    `)
    const chain = connectChain('xs', 'left', false)
    expect(winner(bad, chain, 'grid-template-areas')!.value).toBe("'left center'")
    expect(winner(good, chain, 'grid-template-areas')!.value).toBe("'center'")
  })
})

describe('the .connect template has exactly the columns that render', () => {
  for (const vp of TIERS.filter((t) => t !== 'xs'))
    for (const rails of RAILS) {
      it(`[data-viewport=${vp}] data-rails=${rails}`, () => {
        const chain = connectChain(vp, rails, false)
        const cols = winner(RULES, chain, 'grid-template-columns')
        const areas = winner(RULES, chain, 'grid-template-areas')
        expect(cols, 'no grid-template-columns reaches .connect').not.toBeNull()
        expect(areas, 'no grid-template-areas reaches .connect').not.toBeNull()
        const present = railsIn(rails)
        const t = tracks(cols!.value)
        expect(
          t.length,
          `\`${cols!.selector} { grid-template-columns: ${cols!.value} }\` has ${t.length} tracks for ` +
            `${present.length} rail(s) + the map — a track with nothing in it is a dead band beside the map`,
        ).toBe(present.length + 1)
        expect(t.filter((x) => x === 'minmax(0,1fr)' || x === 'minmax(0, 1fr)').length, 'the map track').toBe(1)
        for (const side of present) {
          const v = side === 'left' ? '--cn-rail-l' : '--cn-rail-r'
          expect(t.some((x) => x.includes(v)), `the ${side} rail track must read ${v}`).toBe(true)
        }
        const rows = areaRows(areas!.value)
        expect(rows.length, 'one explicit row: the strip rides an implicit one').toBe(1)
        const expected = [...(present.includes('left') ? ['left'] : []), 'center', ...(present.includes('right') ? ['right'] : [])]
        expect(rows[0]).toEqual(expected)

        const tr = winner(RULES, chain, 'grid-template-rows')
        // One explicit row, flexible: the map's. Its minimum is the rails' floor, computed below the
        // floor at the end of this file.
        expect(tracks(tr!.value), `\`${tr!.selector}\``).toEqual(['minmax(max(calc(8em + var(--space-3)), min(calc(10em + var(--space-3)), calc(100% - 4em - var(--space-3)))), 1fr)'])
        // An `auto`-max implicit row: a fixed max would be maximised to its full value before the
        // fr row grows (css-grid §11.6) — a floor stealing height from the map, not a cap. The one
        // exception is the height the OPERATOR set with the strip's divider (layout L7), which the
        // row then is; unset, the fallback is content-sized over a 4em floor (end of this file).
        expect(winner(RULES, chain, 'grid-auto-rows')!.value).toBe('var(--cn-strip-h, minmax(4em, auto))')
      })
    }

  for (const rails of RAILS) {
    it(`xs stacks one column whatever the rails say (data-rails=${rails})`, () => {
      const chain = connectChain('xs', rails, false)
      expect(tracks(winner(RULES, chain, 'grid-template-columns')!.value)).toEqual(['minmax(0, 1fr)'])
      expect(areaRows(winner(RULES, chain, 'grid-template-areas')!.value)).toEqual([['center']])
      expect(winner(RULES, chain, 'overflow-y')!.value, 'the stack needs its own scroller').toBe('auto')
    })
  }

  for (const vp of TIERS)
    for (const rails of RAILS) {
      it(`map-full is the bare map at ${vp} (data-rails=${rails})`, () => {
        const chain = connectChain(vp, rails, true)
        expect(tracks(winner(RULES, chain, 'grid-template-columns')!.value)).toEqual(['minmax(0, 1fr)'])
        expect(areaRows(winner(RULES, chain, 'grid-template-areas')!.value)).toEqual([['center']])
        expect(tracks(winner(RULES, chain, 'grid-template-rows')!.value)).toEqual(['minmax(0, 1fr)'])
      })
    }
})

describe('rails, strip and separators place themselves where the template expects', () => {
  for (const vp of TIERS)
    for (const side of ['left', 'right'] as const) {
      it(`the ${side} rail at ${vp}`, () => {
        const chain = [...connectChain(vp, 'both', false), { cls: ['connect-rail'], attrs: { 'data-side': side } }]
        const area = winner(RULES, chain, 'grid-area')
        const flex = winner(RULES, chain, '--connect-pane-flex')
        if (vp === 'xs') {
          // No 'left'/'right' area exists at xs: a named area would drop the rail into an
          // implicit track. Auto-placed, it stacks under the map in DOM order.
          expect(area!.value).toBe('auto')
          // Content-height frames in a content-height stack (a basis-0 grower collapses there).
          expect(flex!.value).toBe('0 0 auto')
        } else {
          expect(area!.value).toBe(side)
          expect(flex, 'fill frames split the rail by share above xs').toBeNull()
        }
        expect(winner(RULES, chain, 'display')!.value).toBe('flex')
        expect(winner(RULES, chain, 'flex-direction')!.value).toBe('column')
      })
    }

  for (const vp of TIERS) {
    it(`the strip spans the grid in an implicit row and shares its width by pane count at ${vp}`, () => {
      const chain = [...connectChain(vp, 'both', false), { cls: ['connect-strip'] }]
      expect(winner(RULES, chain, 'grid-column')!.value).toBe('1 / -1')
      expect(winner(RULES, chain, 'grid-area'), 'a named strip area would re-create the explicit row').toBeNull()
      expect(winner(RULES, chain, 'grid-template-columns'), 'a fixed column count leaves a dead column').toBeNull()
      expect(winner(RULES, chain, 'grid-auto-columns')!.value).toBe('minmax(0, 1fr)')
      expect(winner(RULES, chain, 'grid-auto-flow')!.value).toBe(vp === 'xs' ? 'row' : 'column')
    })

    it(`separators are ${vp === 'xs' ? 'gone' : 'positioned in the gap'} at ${vp}`, () => {
      for (const orient of ['vertical', 'horizontal']) {
        const chain = [
          ...connectChain(vp, 'both', false),
          { cls: ['connect-rail'], attrs: { 'data-side': 'left' } },
          { cls: ['connect-sep'], attrs: { 'aria-orientation': orient } },
        ]
        const display = winner(RULES, chain, 'display')
        if (vp === 'xs') expect(display!.value).toBe('none')
        else {
          expect(display?.value ?? 'block').not.toBe('none')
          // Out of flow: a separator that took a track or a flex slot would move every
          // rectangle the default layout promises to keep.
          expect(winner(RULES, chain, 'position')!.value).toBe('absolute')
        }
      }
    })
  }
})

describe('the operator’s strip height (layout L7)', () => {
  for (const vp of TIERS.filter((t) => t !== 'xs'))
    for (const sized of [false, true]) {
      it(`[data-viewport=${vp}] a${sized ? ' sized' : 'n unsized'} strip’s panes ${sized ? 'fill the row the operator set' : 'keep the 30 % cap'}`, () => {
        const frame = [
          ...connectChain(vp, 'both', false),
          { cls: ['connect-strip'], attrs: sized ? { 'data-sized': '' } : {} },
          { cls: ['pane-frame'] },
        ]
        const cap = winner(RULES, frame, 'max-height')!.value
        if (sized) expect(cap, 'a cap below the row would stop the panes short of it').toBe('none')
        else expect(cap).toBe('calc(0.3 * var(--vh-eff, 100vh))')
      })
    }

  for (const rails of RAILS)
    it(`the xs stack keeps content-height rows and the pane cap, sized or not (data-rails=${rails})`, () => {
      // `max-content`, not `auto`: an auto row is floored at the rail's min-height (0) — see the
      // xs stack below, which computes that floor for every stacked child.
      expect(winner(RULES, connectChain('xs', rails, false), 'grid-auto-rows')!.value).toBe('max-content')
      const frame = [...connectChain('xs', rails, false), { cls: ['connect-strip'], attrs: { 'data-sized': '' } }, { cls: ['pane-frame'] }]
      expect(winner(RULES, frame, 'max-height')!.value).toBe('calc(0.3 * var(--vh-eff, 100vh))')
    })

  for (const vp of TIERS)
    it(`the map | strip divider sits in the gap above the strip, out of flow, ${vp === 'xs' ? 'and is gone in the stack' : 'and shows'} at ${vp}`, () => {
      const strip = [...connectChain(vp, 'both', false), { cls: ['connect-strip'] }]
      const seam = [...strip, { cls: ['pane-splitter', 'horizontal'] }]
      expect(winner(RULES, strip, 'position')!.value, 'the strip is its containing block').toBe('relative')
      expect(winner(RULES, seam, 'position')!.value).toBe('absolute')
      expect(winner(RULES, seam, 'bottom')!.value).toBe('100%')
      expect(winner(RULES, seam, 'height')!.value, 'exactly the grid’s row gap').toBe('var(--space-3)')
      expect(winner(RULES, [...connectChain(vp, 'both', false)], 'gap')!.value).toBe('var(--space-3)')
      expect(winner(RULES, seam, 'margin')!.value).toBe('0')
      const display = winner(RULES, seam, 'display')?.value ?? 'block'
      if (vp === 'xs') expect(display).toBe('none')
      else expect(display).not.toBe('none')
    })
})

// THE XS STACK PAINTED ITS PANES OVER EACH OTHER (found in the dashboard window at 760×660, the
// same on 78e4a975). An `auto` row is only content-height when the item in it lets it be:
// css-grid §11.5 floors an `auto` minimum at the item's MINIMUM CONTRIBUTION, which is its
// `min-height` whenever one is set. The rails carry `min-height: 0` for the bounded two-rail grid,
// and the stack's own box is bounded (it scrolls), so both rail rows sized to 0 px and their
// content-height frames spilled over the rows under them — in Chrome, rows `280px 0px 0px
// 331.75px` and ten overlapping pairs of frames. This computes the floor each stacked child's
// implicit row really gets, from the winners of both properties as they render.

/** The floor css-grid §11.5 gives an implicit row for one item in it: 'content', or what it falls
 *  to. An `auto` minimum is the item's minimum contribution — its min-height when one is set, its
 *  content only while min-height is `auto`; a max-/min-content minimum is the content whatever the
 *  item says. Anything else is a length, not the content. */
function stackRowFloor(autoRows: string, itemMinHeight: string): string {
  const t = tracks(autoRows)
  if (t.length !== 1) return `unmodelled grid-auto-rows: ${autoRows}`
  const min = /^minmax\(\s*([^,]+?)\s*,/.exec(t[0])?.[1] ?? t[0]
  if (min === 'max-content' || min === 'min-content') return 'content'
  if (min === 'auto') return itemMinHeight === 'auto' ? 'content' : itemMinHeight
  return min
}

describe('the xs stack: every stacked row holds what is in it', () => {
  it('the reader itself: an auto row takes the item’s min-height as its floor (the guard can fire)', () => {
    expect(stackRowFloor('auto', '0')).toBe('0')
    expect(stackRowFloor('minmax(auto, 300px)', '0')).toBe('0')
    expect(stackRowFloor('auto', 'auto')).toBe('content')
    expect(stackRowFloor('max-content', '0')).toBe('content')
    expect(stackRowFloor('var(--cn-strip-h, auto)', 'auto')).not.toBe('content')
  })

  for (const rails of RAILS)
    for (const sized of [false, true]) {
      it(`no stacked row falls below the rail or strip in it (data-rails=${rails}${sized ? ', a sized strip' : ''})`, () => {
        const grid = connectChain('xs', rails, false)
        const autoRows = winner(RULES, grid, 'grid-auto-rows')!.value
        const stacked: Array<[string, El]> = [
          ...railsIn(rails).map((side): [string, El] => [`the ${side} rail`, { cls: ['connect-rail'], attrs: { 'data-side': side } }]),
          ['the strip', { cls: ['connect-strip'], attrs: sized ? { 'data-sized': '' } : {} }],
        ]
        for (const [name, el] of stacked) {
          const minH = winner(RULES, [...grid, el], 'min-height')?.value ?? 'auto'
          expect(
            stackRowFloor(autoRows, minH),
            `${name}: \`grid-auto-rows: ${autoRows}\` with its \`min-height: ${minH}\` lets its row shrink below its ` +
              'frames, and they paint over the rows under it',
          ).toBe('content')
        }
      })
    }
})

// BELOW THE FLOOR, EVERY BOX KEEPS ITS TITLE BAR AND A LINE (2026-09-30; the operator: "the area is
// unusable in Japanese today"). Measured in Chrome at 1920×1080 with the app pinned at 175 % — an
// effective 1097×617, the sm tier — the top bar, the now bar and Connect's header take 273 px of the
// 617 and the grid gets 258. Its rows were the map's `minmax(0, 1fr)` and the strip's implicit `auto`.
// An `auto` minimum is the item's automatic minimum (css-grid §6.6), and `.connect-strip` is neither
// min-height 0 nor a scroll container, so its row could not go below its panes' content — capped at
// 30 % of the effective VIEWPORT, 185 px here. The map row's minimum was 0, so it took the whole
// deficit: 39 px, and both rails live in that row, two boxes and a gap each — 13.7 px a box, the title
// bar clipped and its picker out of reach (6 px in Japanese, where every rail title takes two lines).
// Shorter still (1366×768 at 175 %) the strip overflows the grid, and the grid's host clips it. The
// cap is a share of the viewport, so it bounds the strip fairly only while the grid has most of the
// viewport; nothing bounded the strip against the grid it shares.
//
// The fix floors both rows in the unit Connect's own strip divider already uses — 4em, "a pane's
// title bar and a line of it" (features/paneSeam CONNECT_STRIP_SPLIT_MIN): the map row holds two rail
// boxes and the rail's gap, the strip one box. Floors that do not yield need a scroller between them
// and the clip, so the Connect view is one (`.layout.single` around `.connect-shell`, overflow-y
// auto): the third legal fate of cockpit-panes.css, past the region's own floor the shell scrolls.
// A rail box asks for 5em while that leaves the strip its 4em: a 248 px rail wraps its titles onto
// two lines (every rail title in Japanese at 175 %), and 4em under a two-line title bar left no line
// of the pane showing. It yields before it makes the view scroll. At every supported size the rows
// already get more than the floors, so nothing there moves — computed here at the census's
// geometries, and measured in Chrome (the report's census).

/** A grid the Chrome census measured (CSS px): its content height (Chrome's rows plus the gap between
 *  them, map + gap + strip), its gap (var(--space-3): 12 px, 11.28 px in the sm tier's tighter
 *  spacing), its font (what em means) and --vh-eff. */
interface GridGeom {
  name: string
  content: number
  space: number
  fontPx: number
  vhEff: number
}

/** Resolve a length Connect's row rules write, to CSS px at `g`: px, em, % (of the grid's content
 *  height, what a row size's percentage means), var(--space-3), var(--vh-eff, …), `calc()` sums of
 *  those and products with a number, and min()/max() over them. Null for anything else, so a guard
 *  fails loudly instead of passing on a value it could not read. */
function lenPx(v: string, g: GridGeom): number | null {
  const s = v.trim()
  const fn = /^(min|max|calc)\(([^]*)\)$/.exec(s)
  if (fn) {
    if (fn[1] !== 'calc') {
      const parts = splitTop(fn[2]).map((p) => lenPx(p, g))
      if (parts.some((p) => p === null)) return null
      return (fn[1] === 'min' ? Math.min : Math.max)(...(parts as number[]))
    }
    return sumPx(fn[2], g)
  }
  let m = /^(-?[\d.]+)%$/.exec(s)
  if (m) return (parseFloat(m[1]) / 100) * g.content
  m = /^(-?[\d.]+)px$/.exec(s)
  if (m) return parseFloat(m[1])
  m = /^(-?[\d.]+)em$/.exec(s)
  if (m) return parseFloat(m[1]) * g.fontPx
  if (/^0$/.test(s)) return 0
  if (/^var\(\s*--space-3\s*\)$/.test(s)) return g.space
  if (/^var\(\s*--vh-eff\s*(?:,[^)]*)?\)$/.test(s)) return g.vhEff
  return null
}

/** The inside of a calc(): a left-to-right sum of terms, a term being a product with a number or a
 *  length lenPx reads. */
function sumPx(expr: string, g: GridGeom): number | null {
  const inner = expr.trim()
  let depth = 0
  for (let i = inner.length - 1; i > 0; i--) {
    const ch = inner[i]
    if (ch === ')') depth++
    else if (ch === '(') depth--
    else if (depth === 0 && (ch === '+' || ch === '-') && inner[i - 1] === ' ') {
      const a = sumPx(inner.slice(0, i), g)
      const b = sumPx(inner.slice(i + 1), g)
      return a === null || b === null ? null : ch === '+' ? a + b : a - b
    }
  }
  const prod = /^(-?[\d.]+)\s*\*\s*([^]+)$/.exec(inner)
  if (prod) {
    const b = lenPx(prod[2], g)
    return b === null ? null : parseFloat(prod[1]) * b
  }
  return lenPx(inner, g)
}

function splitTop(s: string): string[] {
  const out: string[] = []
  let depth = 0
  let start = 0
  for (let i = 0; i < s.length; i++) {
    if (s[i] === '(') depth++
    else if (s[i] === ')') depth--
    else if (s[i] === ',' && depth === 0) {
      out.push(s.slice(start, i))
      start = i + 1
    }
  }
  out.push(s.slice(start))
  return out
}

/** The two rows css-grid §12 gives THIS grid, for strip panes whose content is taller than their
 *  cap (as every census measured): the explicit `minmax(<a>, 1fr)` map row and the strip's implicit
 *  row, unsized (`var(--cn-strip-h, <b>)`). Base sizes (§12.4): a fixed minimum is itself; an `auto`
 *  minimum is the strip item's content-based minimum — its panes' content, capped by their
 *  max-height (§6.6). Positive free space grows the strip first, to its growth limit (§12.6, maximize
 *  tracks), then the flexible map row takes the rest (§12.7). Negative free space shrinks nothing:
 *  the rows overflow the grid by that much. */
function connectRows(g: GridGeom, rowsDecl: string, autoRowsDecl: string, capDecl: string) {
  const t = tracks(rowsDecl)
  const mm = t.length === 1 ? /^minmax\(([^]*)\)$/.exec(t[0]) : null
  const args = mm ? splitTop(mm[1]).map((a) => a.trim()) : []
  const map = args.length === 2 && args[1] === '1fr' ? [t[0], args[0]] : null
  const unsized = /^var\(\s*--cn-strip-h\s*,\s*(.+)\)$/.exec(autoRowsDecl.trim())?.[1].trim()
  const strip = unsized === 'auto' ? ['auto', 'auto'] : unsized ? /^minmax\(\s*([^,]+?)\s*,\s*auto\s*\)$/.exec(unsized)?.slice(1) : undefined
  const cap = lenPx(capDecl, g)
  if (!map || !strip || cap === null) return null
  const mapBase = lenPx(map[1], g)
  const stripBase = strip[0] === 'auto' ? cap : lenPx(strip[0], g)
  if (mapBase === null || stripBase === null) return null
  const rowsRoom = g.content - g.space
  let free = rowsRoom - mapBase - stripBase
  let stripSize = stripBase
  if (free > 0) {
    const grow = Math.min(free, Math.max(0, cap - stripBase))
    stripSize += grow
    free -= grow
  }
  const mapSize = free > 0 ? mapBase + free : mapBase
  return { map: mapSize, strip: stripSize, overflow: Math.max(0, -free), railBox: (mapSize - g.space) / 2 }
}

/** Chrome's own numbers (the census, main at 426e2e73 and the fix injected into the same build). */
const G175_EN: GridGeom = { name: '1920×1080 pinned 175 %, en', content: 235.173, space: 11.28, fontPx: 14, vhEff: 617.143 }
const G175_JA: GridGeom = { name: '1920×1080 pinned 175 %, ja', content: 219.744, space: 11.28, fontPx: 14, vhEff: 617.143 }
const BELOW: GridGeom[] = [
  G175_EN,
  G175_JA,
  { name: '1280×800 pinned 150 %', content: 149.14, space: 11.28, fontPx: 14, vhEff: 533.33 },
  { name: '1366×768 pinned 175 %', content: 56.8, space: 11.28, fontPx: 14, vhEff: 438.86 },
]
const SUPPORTED: Array<[GridGeom, string]> = [
  [{ name: '1024×768 (auto, 85 %)', content: 605.824, space: 12, fontPx: 14, vhEff: 903.529 }, 'md'],
  [{ name: '1366×768 (auto, 85 %)', content: 662.9, space: 12, fontPx: 14, vhEff: 903.529 }, 'lg'],
  [{ name: '1920×1080 (100 %)', content: 840, space: 12, fontPx: 14, vhEff: 1080 }, 'lg'],
]
/** The floor's own size with the zoom pinned at 100 % (the sm tier): two rail titles there take two
 *  lines, and their panes were 61 px, a title bar with no line of the pane under it. */
const G1024_100_EN: GridGeom = { name: '1024×768 pinned 100 %, en', content: 375.155, space: 11.28, fontPx: 14, vhEff: 768 }
const OLD_ROWS = 'minmax(0, 1fr)'
const OLD_AUTO_ROWS = 'var(--cn-strip-h, auto)'

/** The rules as they render for the unsized strip at `vp` (both rails, as the stock layout). */
function rowRules(vp: string) {
  const grid = connectChain(vp, 'both', false)
  const frame = [...grid, { cls: ['connect-strip'] }, { cls: ['pane-frame'] }]
  return {
    rows: winner(RULES, grid, 'grid-template-rows')!.value,
    autoRows: winner(RULES, grid, 'grid-auto-rows')!.value,
    cap: winner(RULES, frame, 'max-height')!.value,
    railGap: winner(RULES, [...grid, { cls: ['connect-rail'], attrs: { 'data-side': 'left' } }], 'gap')?.value ?? null,
  }
}

/** The winning overflow on one axis, the `overflow` shorthand included (the reader's `winner` sees
 *  longhands only, and the Connect host's rule has always been written as the shorthand). */
function overflowOn(rules: Rule[], chain: El[], axis: 'x' | 'y'): { value: string; selector: string } | null {
  let win: { value: string; selector: string; spec: number; order: number } | null = null
  for (const r of rules) {
    if (r.media !== null) continue
    const spec = matchSpec(r.selector, chain)
    if (spec < 0) continue
    let v: string | null = null
    for (const decl of r.body.split(';')) {
      const m = /^\s*overflow(-x|-y)?\s*:\s*(\S[^]*?)\s*$/.exec(decl)
      if (!m) continue
      if (m[1] === undefined) {
        const parts = m[2].split(/\s+/)
        v = axis === 'x' ? parts[0] : (parts[1] ?? parts[0])
      } else if (m[1] === `-${axis}`) v = m[2]
    }
    if (v === null) continue
    if (!win || spec > win.spec || (spec === win.spec && r.order >= win.order)) win = { value: v, selector: r.selector, spec, order: r.order }
  }
  return win && { value: win.value, selector: win.selector }
}

/** The Connect view's host as it renders: `.layout.single` with `.connect-shell` as its child. */
const connectHost = (vp: string): El[] => [
  { cls: [], attrs: { 'data-viewport': vp } },
  { cls: ['app'] },
  { cls: ['shell'] },
  { cls: ['layout', 'single'], kids: ['connect-shell'] },
]

describe('below the floor, every Connect box keeps its title bar and a line', () => {
  it('the reader matches `:has(> .x)` on the child it names, and only there (the guard can fire)', () => {
    const sheet = parseRules(`
      .layout.single { overflow-y: auto; }
      .layout.single:has(> .connect-shell) { overflow: hidden; }
    `)
    const other: El[] = [{ cls: ['app'] }, { cls: ['layout', 'single'], kids: ['settings-panel'] }]
    expect(overflowOn(sheet, connectHost('lg'), 'y')!.value, 'the shorthand, read on its axis').toBe('hidden')
    expect(overflowOn(sheet, other, 'y')!.value).toBe('auto')
    expect(overflowOn(parseRules('.a { overflow: hidden auto; }'), [{ cls: ['a'] }], 'y')!.value).toBe('auto')
  })

  it('the model reproduces Chrome: the old rows at 175 % and at 1024×768, and the fixed rows at 175 %', () => {
    const cap = 'calc(0.3 * var(--vh-eff, 100vh))'
    const oldEn = connectRows(G175_EN, OLD_ROWS, OLD_AUTO_ROWS, cap)!
    expect(oldEn.map).toBeCloseTo(38.75, 1)
    expect(oldEn.strip).toBeCloseTo(185.14, 1)
    expect(oldEn.railBox, 'the bug: a rail box at 175 %').toBeCloseTo(13.7, 1)
    expect(connectRows(G175_JA, OLD_ROWS, OLD_AUTO_ROWS, cap)!.map).toBeCloseTo(23.32, 1)
    const stock = connectRows(SUPPORTED[0][0], OLD_ROWS, OLD_AUTO_ROWS, cap)!
    expect(stock.map).toBeCloseTo(322.78, 1)
    expect(stock.strip).toBeCloseTo(271.05, 1)
    // …and what Chrome drew with this file's rules injected into the same build:
    const r = rowRules('sm')
    const now = connectRows(G175_EN, r.rows, r.autoRows, r.cap)
    expect(now, `the model cannot read \`${r.rows}\` / \`${r.autoRows}\``).not.toBeNull()
    expect(now!.map).toBeCloseTo(151.28, 1)
    expect(now!.strip).toBeCloseTo(72.62, 1)
    const ja = connectRows(G175_JA, r.rows, r.autoRows, r.cap)!
    expect(ja.map).toBeCloseTo(151.28, 1)
    expect(ja.strip).toBeCloseTo(57.19, 1)
    const floor100 = connectRows(G1024_100_EN, r.rows, r.autoRows, r.cap)!
    expect(floor100.map).toBeCloseTo(151.27, 1)
    expect(floor100.strip).toBeCloseTo(212.61, 1)
  })

  for (const vp of ['sm', 'md', 'lg'])
    it(`[data-viewport=${vp}] the map row holds two rail boxes of 4em and the rail’s gap; the strip holds one`, () => {
      const r = rowRules(vp)
      expect(r.railGap, 'the rail’s own gap between its two boxes').toBe('var(--space-3)')
      for (const g of BELOW) {
        const rows = connectRows(g, r.rows, r.autoRows, r.cap)
        expect(rows, `${g.name}: the model cannot read \`${r.rows}\` / \`${r.autoRows}\``).not.toBeNull()
        expect(rows!.railBox, `${g.name}: a rail box is ${rows!.railBox.toFixed(1)} px`).toBeGreaterThanOrEqual(4 * g.fontPx - 0.01)
        expect(rows!.strip, `${g.name}: a strip box is ${rows!.strip.toFixed(1)} px`).toBeGreaterThanOrEqual(4 * g.fontPx - 0.01)
      }
    })

  it('at 175 % a rail box keeps a two-line title bar and a line (5em), the strip its 4em, and nothing scrolls', () => {
    const r = rowRules('sm')
    for (const g of [G175_EN, G175_JA, G1024_100_EN]) {
      const rows = connectRows(g, r.rows, r.autoRows, r.cap)!
      expect(rows.railBox, `${g.name}: a rail box is ${rows.railBox.toFixed(1)} px`).toBeGreaterThanOrEqual(5 * g.fontPx - 0.01)
      expect(rows.strip, `${g.name}: a strip box is ${rows.strip.toFixed(1)} px`).toBeGreaterThanOrEqual(4 * g.fontPx - 0.01)
      expect(rows.overflow, `${g.name}: the view would scroll`).toBe(0)
    }
  })

  it('where 5em rail boxes would take the strip below its 4em, the rail boxes give way first: nothing scrolls for it', () => {
    // A few px shorter than Japanese at 175 % — another platform's fonts, a taller banner — must
    // not put a scrollbar on the view for the sake of the rails' second em.
    const r = rowRules('sm')
    const tight: GridGeom = { ...G175_JA, name: 'Japanese at 175 %, 5 px shorter', content: G175_JA.content - 5 }
    const rows = connectRows(tight, r.rows, r.autoRows, r.cap)!
    expect(rows.overflow, 'the view would scroll').toBe(0)
    expect(rows.strip, 'the strip keeps its 4em').toBeCloseTo(4 * tight.fontPx, 6)
    expect(rows.railBox, 'the rails gave up the difference').toBeLessThan(5 * tight.fontPx)
    expect(rows.railBox, '…and never their own 4em').toBeGreaterThanOrEqual(4 * tight.fontPx)
  })

  for (const vp of ['sm', 'md', 'lg'])
    it(`[data-viewport=${vp}] where the floors do not fit, the Connect view scrolls to them`, () => {
      const r = rowRules(vp)
      const short = BELOW.filter((g) => connectRows(g, r.rows, r.autoRows, r.cap)!.overflow > 0)
      expect(short.map((g) => g.name), 'the geometries whose floors overflow the grid').toEqual(['1280×800 pinned 150 %', '1366×768 pinned 175 %'])
      expect(overflowOn(RULES, connectHost(vp), 'y')!.value, 'the deficit valve').toBe('auto')
      expect(overflowOn(RULES, connectHost(vp), 'x')!.value, 'and only vertically').toBe('hidden')
    })

  for (const [g, vp] of SUPPORTED)
    it(`${g.name}: the floors never bind — the rows are what they were`, () => {
      const r = rowRules(vp)
      const now = connectRows(g, r.rows, r.autoRows, r.cap)!
      const old = connectRows(g, OLD_ROWS, OLD_AUTO_ROWS, r.cap)!
      expect(now.map).toBeCloseTo(old.map, 6)
      expect(now.strip).toBeCloseTo(old.strip, 6)
      expect(now.overflow).toBe(0)
    })
})
