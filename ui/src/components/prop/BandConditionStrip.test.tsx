// @vitest-environment jsdom
//
// THE MAP'S BAND CONDITIONS LIST IS LETTERED IN THE THEME'S INKS (issue #382).
//
// #382 (1.15.0, Windows, dark theme, German): the band names on the left of the list were nearly
// invisible; only the row under the pointer read. Each row is a <button>, and it set no colour of its
// own, so its text was drawn in the BROWSER's button ink. In 1.15.0 that ink followed the computer's
// light/dark, not Nexus's theme: a computer in light mode drew black names on the dark rail. Since
// 5d5549fc the browser's scheme follows the theme, which made that ink white on dark, but it is still
// the browser's white, not the theme's --text: Night, High contrast and the ten themes retune --text
// and the names ignored all of them. The row now takes the theme's ink.
//
// THE STATE WORDS (Open / Marginal / Closed) were lettered in the band colour on a 16 % tint of it,
// which fails the lettering floor (rendered in Chrome on 78e4a975: light theme Open 3.45, Marginal
// 3.00, Closed 2.61; dark Closed 3.82; the floor is 4.5). The letters are now the theme's
// --text (Closed: --text-dim, so a closed band recedes), and the colour rides in the tint and a 1 px
// edge.
//
// Computed on the cascade winner (cssCascade.ts) for the chain of the RENDERED rail, in every mode
// the app has: MODES (the base modes, every colour-role set, the worst-case themes) and SKIN_MODES
// (all ten themes in each of their modes). The positive controls at the end put back what shipped
// and watch each check refuse it.
import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { MapInsightRail } from './MapInsightRail'
import type { BandReport, PropagationSnapshot } from '../../types'
import {
  BASE_MODES,
  MODES,
  baseOf,
  SKIN_MODES,
  chainOf,
  contrast,
  expandWith,
  isHigh,
  parseRules,
  tokensAt,
  toRgb,
  winnerAt,
  type El,
  type Mode,
  type Rgb,
  type Rule,
} from '../../cssCascade'

// THE BUDGET (2026-10-09). The slowest case without a budget of its own, "every band name is drawn in the theme’s
// --text, never…", takes 0.57 s and 0.48 s on one core (two runs); a loaded full suite on this box has run cases up
// to 20 times slower than one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still
// fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const blank = (s: string) => s.replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
// By path, not `new URL(…, import.meta.url)`: under jsdom Vite rewrites that form as an asset URL.
const read = (name: string) => readFileSync(resolve(__dirname, '../..', name), 'utf8')
const SHEET = blank(read('styles.css')) + '\n' + blank(read('cockpit-panes.css'))
const RULES = parseRules(SHEET)

const report = (band: string, tier: BandReport['tier'], modeled?: BandReport['modeled']): BandReport =>
  ({ band, tier, modeled, score: 0, nHearMe: 0, nIHear: 0, bestRegion: null, confidence: 'Likely', reason: '' }) as BandReport
/** Every word the list draws, in every colour it can wear: Open by the model, Open because it is
 *  heard on a band the model calls closed (the grey one), Marginal, Closed, and three bands with no
 *  model, whose colour then comes from the activity tier. */
const BANDS: BandReport[] = [
  report('80m', 'Quiet', 'Open'),
  report('40m', 'Active', 'Closed'),
  report('30m', 'Quiet', 'Marginal'),
  report('20m', 'Quiet', 'Closed'),
  report('17m', 'Moderate'),
  report('15m', 'Quiet'),
  report('10m', 'Closed'),
]

afterEach(cleanup)

function renderRail(): HTMLElement {
  localStorage.clear()
  const prop = { advisory: { bands: BANDS } } as unknown as PropagationSnapshot
  return render(<MapInsightRail prop={prop} />).container
}

const MODES_ALL: readonly Mode[] = [...MODES, ...SKIN_MODES]
/** What the list's pills can sit on: the rail's own surface (the page colour: the rail is 82 % of it
 *  over the map), and the panels and the raised surface the same cell is drawn on elsewhere. */
const SURFACES = ['--bg', '--panel', '--bg-elev'] as const

/** A property's winner for a chain, once per BASE mode. A theme or a colour-role preset declares
 *  tokens only (styles-skins.test.ts and styles-palette-roles.test.ts refuse anything else), so no
 *  winner of an ordinary property can differ between two modes that share a base; the tokens it
 *  names are resolved per mode below. Without this the sweep takes ~10 s. */
const WINNERS = new WeakMap<Rule[], Map<string, ReturnType<typeof winnerAt>>>()
function winner(rules: Rule[], mode: Mode, chain: El[], ...props: string[]): ReturnType<typeof winnerAt> {
  let byKey = WINNERS.get(rules)
  if (!byKey) WINNERS.set(rules, (byKey = new Map()))
  const key = `${baseOf(mode)}|${JSON.stringify(chain)}|${props.join()}`
  if (!byKey.has(key)) byKey.set(key, winnerAt(rules, baseOf(mode), chain, ...props))
  return byKey.get(key)!
}

type Ink = { value: string; chain: El[] } | 'browser'
/**
 * Where the text of `node` takes its colour: the first element, from `node` up, that the sheet gives
 * a `color` (other than `inherit`) — or the BROWSER, when the walk reaches a <button> first. A
 * button's text is drawn in the browser's own button ink unless the sheet colours the button or
 * something inside it; that is #382.
 */
function inkOf(rules: Rule[], mode: Mode, node: Element): Ink {
  const chain = chainOf(node)
  for (let i = chain.length; i > 0; i--) {
    const at = chain.slice(0, i)
    const w = winner(rules, mode, at, 'color')
    if (w && w.value !== 'inherit') return { value: w.value, chain: at }
    if (at[at.length - 1].tag === 'button') return 'browser'
  }
  return { value: 'var(--text)', chain: [] } // no button on the way: the body's ink, inherited
}

function rgb(tokens: Map<string, string>, value: string, backdrop: Rgb = [0, 0, 0]): Rgb {
  const v = expandWith(tokens, value).trim()
  const c = v === '' ? null : toRgb(v, backdrop)
  if (!c) throw new Error(`"${value}" does not resolve to a colour: "${v}"`)
  return c
}

/** The rows differ only by their `title` (the tooltip), which no selector in the sheet reads (the
 *  sweep checks that), so the tokens are computed once per mode for every row alike. */
const TOKENS = new WeakMap<Rule[], Map<string, Map<string, string>>>()
function tokensFor(rules: Rule[], mode: Mode, chain: El[]): Map<string, string> {
  let byKey = TOKENS.get(rules)
  if (!byKey) TOKENS.set(rules, (byKey = new Map()))
  const bare = chain.map((el) => ({ ...el, attrs: Object.fromEntries(Object.entries(el.attrs).filter(([k]) => k !== 'title')) }))
  const key = `${mode}|${JSON.stringify(bare)}`
  if (!byKey.has(key)) byKey.set(key, tokensAt(rules, mode, bare))
  return byKey.get(key)!
}

/** The pill's tokens: the sheet's at its place in the rail, plus the band colour the component sets
 *  inline (the resolver does not read inline styles). */
function pillTokens(rules: Rule[], mode: Mode, node: HTMLElement): Map<string, string> {
  const tokens = new Map(tokensFor(rules, mode, chainOf(node)))
  const own = node.style.getPropertyValue('--bc-color').trim()
  if (own) tokens.set('--bc-color', expandWith(tokens, own))
  return tokens
}

/** Every band name that does not take the theme's --text, and why. */
function nameProblems(rules: Rule[], rail: HTMLElement): string[] {
  const names = [...rail.querySelectorAll<HTMLElement>('.bc-band')]
  expect(names.map((n) => n.textContent)).toEqual(BANDS.map((b) => b.band))
  const out: string[] = []
  for (const mode of BASE_MODES)
    for (const n of names) {
      const ink = inkOf(rules, mode, n)
      if (ink === 'browser') {
        out.push(`${mode} ${n.textContent}: drawn in the browser's button ink`)
        continue
      }
      const tokens = tokensAt(rules, mode, ink.chain)
      const got = rgb(tokens, ink.value)
      const want = rgb(tokens, 'var(--text)')
      if (got.join() !== want.join()) out.push(`${mode} ${n.textContent}: ${ink.value}, not the theme's --text`)
    }
  return out
}

/** Every pill whose word or colour misses its floor, in every mode and theme. */
function pillProblems(rules: Rule[], rail: HTMLElement, modes: readonly Mode[] = MODES_ALL): string[] {
  const pills = [...rail.querySelectorAll<HTMLElement>('.bc-state')]
  expect(pills.length).toBe(BANDS.length)
  const out: string[] = []
  for (const mode of modes)
    for (const p of pills) {
      const where = `${mode} ${p.closest('.bc-cell')!.querySelector('.bc-band')!.textContent} "${p.textContent}"`
      const ink = inkOf(rules, mode, p)
      if (ink === 'browser') {
        out.push(`${where}: its letters are the browser's button ink`)
        continue
      }
      const tokens = pillTokens(rules, mode, p)
      const chain = chainOf(p)
      const fill = winner(rules, mode, chain, 'background', 'background-color')
      const edge = winner(rules, mode, chain, 'border-color', 'border')
      const closed = p.textContent === 'Closed'
      const floor = closed ? 4.5 : isHigh(mode) ? 7 : 4.5
      // The edge must stand off its surface where its colour SAYS something: the open green and the
      // marginal amber. The closed grey and the tier neutrals mean "nothing to show" and recede by
      // design; the word carries the state.
      const says = /var\(--band-(open|marginal)\)/.test(p.style.getPropertyValue('--bc-color'))
      if (!edge) out.push(`${where}: no edge carries the band colour`)
      for (const s of SURFACES) {
        const surface = rgb(tokens, `var(${s})`)
        const under = fill ? rgb(tokens, fill.value, surface) : surface
        const letters = rgb(tokens, ink.value, under)
        const ratio = contrast(letters, under)
        if (ratio < floor) out.push(`${where} on ${s}: letters ${ratio.toFixed(2)}:1 < ${floor}`)
        if (!edge || !says) continue
        const edgeColour = rgb(tokens, edge.value.replace(/^\s*[\d.]+px\s+(solid|dashed)\s+/, ''), surface)
        const off = contrast(edgeColour, surface)
        if (off < 3) out.push(`${where} on ${s}: edge ${off.toFixed(2)}:1 < 3`)
      }
    }
  return out
}

describe('the map’s Band conditions list is lettered in the theme’s inks (#382)', () => {
  it('every band name is drawn in the theme’s --text, never the browser’s button ink', () => {
    expect(nameProblems(RULES, renderRail())).toEqual([])
  })

  it('the state words set no letter colour of their own: the band colour is the tint and the edge', () => {
    const pills = [...renderRail().querySelectorAll<HTMLElement>('.bc-state')]
    for (const p of pills) {
      expect(p.style.color, `"${p.textContent}" is lettered inline`).toBe('')
      expect(p.style.getPropertyValue('--bc-color'), `"${p.textContent}" carries no band colour`).toMatch(/^var\(--/)
    }
    const ink = (p: HTMLElement) => inkOf(RULES, 'dark', p)
    const words = pills.map((p) => [p.textContent, ink(p) === 'browser' ? 'browser' : (ink(p) as { value: string }).value])
    expect(words).toEqual(
      BANDS.map((b) => {
        const word = b.tier === 'Active' || b.tier === 'Moderate' ? 'Open' : (b.modeled ?? 'Open')
        return [word, word === 'Closed' ? 'var(--text-dim)' : 'var(--text)']
      }),
    )
  })

  it('every word reads on its tint, and every edge stands off its surface, in every mode and theme', () => {
    // What makes computing the tokens once for every row exact (tokensFor).
    expect(RULES.filter((r) => r.selector.includes('[title')).map((r) => r.selector)).toEqual([])
    expect(pillProblems(RULES, renderRail())).toEqual([])
  }, 60_000)

  it('FIRES: without the row’s ink the names are the browser’s again, and band-coloured letters fail', () => {
    const rail = renderRail()
    const withoutRowInk = RULES.map((r) =>
      r.selector === '.bc-cell' ? { ...r, decls: r.decls.filter((d) => d.prop !== 'color') } : r,
    )
    expect(nameProblems(withoutRowInk, rail).some((p) => p.includes("browser's button ink"))).toBe(true)
    // What shipped: the word painted in the band colour. It must fail in light, and "Closed" in dark.
    const bandLetters = [...RULES, ...parseRules('.bc-state { color: var(--bc-color); } .bc-state.is-closed { color: var(--bc-color); }', { n: 1e6 })]
    // The controls need only show each check CAN refuse, so they run the base modes.
    const problems = pillProblems(bandLetters, rail, BASE_MODES)
    expect(problems.some((p) => p.startsWith('light ') && p.includes('"Open"'))).toBe(true)
    expect(problems.some((p) => p.startsWith('dark ') && p.includes('"Closed"'))).toBe(true)
    // An edge that vanishes into the page carries no colour.
    const lostEdge = [...RULES, ...parseRules('.bc-state { border: 1px solid var(--bg); }', { n: 1e6 })]
    expect(pillProblems(lostEdge, rail, BASE_MODES).some((p) => p.includes('"Open" on --bg: edge'))).toBe(true)
  }, 60_000)
})
