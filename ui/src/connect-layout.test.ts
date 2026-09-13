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
}

interface Compound {
  cls: string[]
  attrs: Array<[string, string]>
  spec: number
}

/** One compound, or null when it uses anything this reader does not model. */
function compound(s: string): Compound | null {
  const out: Compound = { cls: [], attrs: [], spec: 0 }
  let rest = s
  while (rest) {
    let m = /^\.([\w-]+)/.exec(rest)
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
  return c.cls.every((k) => el.cls.includes(k)) && c.attrs.every(([a, v]) => el.attrs?.[a] === v)
}

/** Specificity of a matching selector, or -1 when it does not match `chain` (root → subject). */
function matchSpec(selector: string, chain: El[]): number {
  const tokens = selector.split(/\s*(>)\s*|\s+/).filter((p): p is string => !!p)
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
        expect(tracks(tr!.value), `\`${tr!.selector}\``).toEqual(['minmax(0, 1fr)'])
        // An `auto` implicit row: a fixed max would be maximised to its full value before the
        // fr row grows (css-grid §11.6) — a floor stealing height from the map, not a cap.
        expect(winner(RULES, chain, 'grid-auto-rows')!.value).toBe('auto')
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
