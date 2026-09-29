import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { APRS_RAIL_MIN, APRS_RAIL_STOCK } from './features/aprsRail'

// THE OWN-GRID VIEWS' DIVIDERS (layout L7): APRS's station list width. Computes the cascade winner
// of every property a divider depends on, for the element as it renders — its data attributes and
// the <html> [data-viewport] tier — never a regex over the sheet (a dead selector passes those;
// that is how two dead fixes shipped).
//
// What must hold, and why each is a guard rather than a style choice:
//   · the stock template is today's layout: unset, the rail's track pays out the same 420 px, and
//     the module's constants are the sheet's own numbers (the divider's range IS the layout's);
//   · the map-side choice only mirrors the template and orders the map first, and the one-column
//     xs stack wins over it at every side (a :where() that stays (0,1,0), as Connect's rails do);
//   · the divider sits in the gap before the second track, out of flow (a grid item in flow would
//     take a track or a cell), is shown at sm where the two columns still stand, and gone at xs.
//
// These views' grids live in styles.css, so this reader understands exactly the selector shapes
// that sheet uses on them: classes, [attr='v'], [attr] (presence), :where([attr='v']) (zero
// specificity), descendant and child combinators. Anything else never matches — a selector it
// cannot read must not count as a winner by accident. (connect-layout.test.ts's reader, plus
// presence attributes.)

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
  /** [name, value]; a null value is a presence test. */
  attrs: Array<[string, string | null]>
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
    m = /^\[([\w-]+)\]/.exec(rest)
    if (m) {
      out.attrs.push([m[1], null])
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
    c.attrs.every(([a, v]) => (v === null ? el.attrs?.[a] !== undefined : el.attrs?.[a] === v))
  )
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

const RULES = parseRules(css)
const TIERS = ['xs', 'sm', 'md', 'lg', 'xl'] as const
const COLUMN_TIERS = TIERS.filter((t) => t !== 'xs')

const html = (vp: string): El => ({ cls: [], attrs: { 'data-viewport': vp } })
const value = (chain: El[], prop: string) => winner(RULES, chain, prop)?.value ?? null

describe('the reader itself', () => {
  it('honours specificity over source order, :where() adds none, and a presence attribute counts (the guards can fire)', () => {
    const bad = parseRules(`
      [data-viewport='xs'] .aprs-body { grid-template-columns: minmax(0, 1fr); }
      .aprs-body[data-map='left'] { grid-template-columns: minmax(0, 1fr) 420px; }
    `)
    const good = parseRules(`
      [data-viewport='xs'] .aprs-body { grid-template-columns: minmax(0, 1fr); }
      .aprs-body:where([data-map='left']) { grid-template-columns: minmax(0, 1fr) 420px; }
    `)
    const chain = [html('xs'), { cls: ['aprs-body'], attrs: { 'data-map': 'left' } }]
    expect(winner(bad, chain, 'grid-template-columns')!.value).toBe('minmax(0, 1fr) 420px')
    expect(winner(good, chain, 'grid-template-columns')!.value).toBe('minmax(0, 1fr)')
    const presence = parseRules(`.a { height: 1px; } .a[data-sized] { height: 2px; }`)
    expect(winner(presence, [{ cls: ['a'], attrs: { 'data-sized': '' } }], 'height')!.value).toBe('2px')
    expect(winner(presence, [{ cls: ['a'] }], 'height')!.value).toBe('1px')
  })
})

// ── APRS ────────────────────────────────────────────────────────────────────────────────────

const aprsBody = (vp: string, mapLeft = false): El[] => [
  html(vp),
  { cls: ['layout', 'single', 'needed-panel', 'aprs-cockpit'] },
  { cls: ['aprs-body'], attrs: mapLeft ? { 'data-map': 'left' } : {} },
]

describe('APRS: the station list column and the map', () => {
  for (const vp of COLUMN_TIERS) {
    it(`[data-viewport=${vp}] the rail's track reads the operator's width over the stock 420 px, floored at the divider's 260 px`, () => {
      const cols = tracks(value(aprsBody(vp), 'grid-template-columns')!)
      expect(cols).toEqual([`minmax(${APRS_RAIL_MIN}px, var(--aprs-rail-w, ${APRS_RAIL_STOCK}px))`, 'minmax(0, 1fr)'])
    })

    it(`[data-viewport=${vp}] the map on the left mirrors the template and goes first by order alone`, () => {
      const cols = tracks(value(aprsBody(vp, true), 'grid-template-columns')!)
      expect(cols).toEqual(['minmax(0, 1fr)', `minmax(${APRS_RAIL_MIN}px, var(--aprs-rail-w, ${APRS_RAIL_STOCK}px))`])
      expect(value([...aprsBody(vp, true), { cls: ['aprs-map'] }], 'order')).toBe('-1')
      expect(value([...aprsBody(vp, true), { cls: ['aprs-rail'] }], 'order'), 'the rail keeps its place').toBeNull()
      expect(value([...aprsBody(vp), { cls: ['aprs-map'] }], 'order'), 'the stock side orders nothing').toBeNull()
    })
  }

  for (const mapLeft of [false, true]) {
    it(`xs stacks one column, map first, whatever side the operator picked (map left: ${mapLeft})`, () => {
      expect(tracks(value(aprsBody('xs', mapLeft), 'grid-template-columns')!)).toEqual(['minmax(0, 1fr)'])
      expect(value([...aprsBody('xs', mapLeft), { cls: ['aprs-map'] }], 'order')).toBe('-1')
    })
  }

  const seam = (vp: string, mapLeft = false): El[] => [...aprsBody(vp, mapLeft), { cls: ['pane-splitter', 'aprs-railseam'] }]

  for (const vp of TIERS)
    for (const mapLeft of [false, true]) {
      it(`[data-viewport=${vp}] map left: ${mapLeft} — the divider is out of flow in the gap before the second track`, () => {
        expect(value(seam(vp, mapLeft), 'position')).toBe('absolute')
        expect(value(seam(vp, mapLeft), 'grid-column')).toBe('2 / 3')
        expect(value(seam(vp, mapLeft), 'grid-row')).toBe('1')
        // Centred on the body's gap, whatever width it paints.
        expect(value(seam(vp, mapLeft), 'left')).toBe('calc(var(--space-3) / -2 - 6px)')
        expect(value(seam(vp, mapLeft), 'width')).toBe('12px')
        expect(value(seam(vp, mapLeft), 'margin')).toBe('0')
        expect(value(aprsBody(vp, mapLeft), 'position'), 'the body is its containing block').toBe('relative')
        const display = value(seam(vp, mapLeft), 'display')
        if (vp === 'xs') expect(display, 'a stack cannot be divided sideways').toBe('none')
        else expect(display, 'two columns stand at this tier, so their divider does').not.toBe('none')
      })
    }
})
