// @vitest-environment jsdom
//
// BANDS FOR YOU — the tiles as drawn (features/bandTiles.test.ts has the rules): one button per band in
// band-stack order, the list's word, the heard dot, ★, the radio's ring, a VHF opening's mode, the map
// focus as a pressed tile — and the colours, computed on the cascade winner for the RENDERED tiles in
// every mode (MODES) and all ten themes (SKIN_MODES): the letters are the theme's inks at 4.5:1 (7:1 for
// --text under High contrast) on the tile's tint over the page, the panel and the raised surface; the
// green and amber edges stand 3:1 off the surface; a closed tile has no tint and a hollow one no colour.
// The positive controls letter the tiles in the band colour and lose the edge; each check refuses.
import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, cleanup, fireEvent, screen } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { BandTiles } from './BandTiles'
import type { BandReport, OpeningView, PropagationSnapshot } from '../../types'
import {
  BASE_MODES,
  MODES,
  SKIN_MODES,
  baseOf,
  chainOf,
  contrast,
  expandWith,
  isHigh,
  parseRules,
  tokensAt,
  toRgb,
  topSplit,
  winnerAt,
  type El,
  type Mode,
  type Rgb,
  type Rule,
} from '../../cssCascade'

const blank = (s: string) => s.replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
// By path, not `new URL(…, import.meta.url)`: under jsdom Vite rewrites that form as an asset URL.
const read = (name: string) => readFileSync(resolve(__dirname, '../..', name), 'utf8')
const RULES = parseRules(blank(read('styles.css')) + '\n' + blank(read('cockpit-panes.css')))

const NOW_MS = 1_800_000_000_000
const report = (band: string, modeled: BandReport['modeled'], tier: BandReport['tier'], over: Partial<BandReport> = {}): BandReport =>
  ({ band, modeled, tier, score: 0.1, nHearMe: 0, nIHear: 0, bestRegion: null, confidence: 'Likely', reason: 'r', ...over }) as BandReport
function snap(bands: BandReport[], openings: OpeningView[] = [], over: Partial<PropagationSnapshot> = {}): PropagationSnapshot {
  return { advisory: { headline: '', banners: [], bands }, openings, source: 'live', asOf: NOW_MS / 1000 - 60, ...over } as unknown as PropagationSnapshot
}
/** One tile of every state and colour: open by the model, open because heard on a band the model
 *  calls closed, marginal, closed, an opening on 2 m, and no model at all. */
const BANDS = [
  report('80m', 'Open', 'Quiet'),
  report('40m', 'Closed', 'Active', { nHearMe: 4, nIHear: 12, bestRegion: { region: 'Europe', octant: 'NE', bearingDeg: 47.6, stations: 5, bidirectional: true } }),
  report('30m', 'Marginal', 'Quiet'),
  report('20m', 'Closed', 'Quiet'),
]
const OPENINGS = [{ band: '2m', mode: 'Tropo' } as OpeningView]

afterEach(cleanup)

const tiles = () => [...document.querySelectorAll<HTMLButtonElement>('.bt-tile')]

describe('the band tiles, drawn', () => {
  it('one button per band in band-stack order, each with its word, dot and marks', () => {
    render(<BandTiles prop={snap(BANDS, OPENINGS)} rigBand="30m" focusBand="40m" nowMs={NOW_MS} />)
    const drawn = tiles().map((b) => [
      b.querySelector('.bt-band')!.textContent,
      b.querySelector('.bt-word')!.textContent,
      [...b.classList].filter((c) => c.startsWith('is-')).join(' '),
      b.querySelector('.bt-dot')!.textContent,
      b.querySelector('.bt-star') != null,
      b.querySelector('.bt-mode')?.textContent ?? null,
      b.getAttribute('aria-pressed'),
    ])
    expect(drawn).toEqual([
      ['80m', 'Open', 'is-open', '○', true, null, 'false'],
      ['40m', 'Open', 'is-open', '●', false, null, 'true'],
      ['30m', 'Marginal', 'is-marginal is-rig', '○', false, null, 'false'],
      ['20m', 'Closed', 'is-closed', '○', false, null, 'false'],
      ['2m', 'Open', 'is-open', '●', false, 'Tropo', 'false'],
    ])
  })

  it('a tile’s tooltip says why: the reason, who hears whom, the best region, and the map click', () => {
    render(<BandTiles prop={snap(BANDS)} nowMs={NOW_MS} />)
    const forty = tiles().find((b) => b.querySelector('.bt-band')!.textContent === '40m')!
    const title = forty.title.split('\n')
    expect(title).toEqual(['40m: Open', 'r', '4 hear you · you hear 12', 'Best toward Europe (NE, 48°)', 'Click to show this band on the map, and again to clear it'])
  })

  it('a click shows the band on the map', () => {
    const onBandClick = vi.fn()
    render(<BandTiles prop={snap(BANDS)} nowMs={NOW_MS} onBandClick={onBandClick} />)
    fireEvent.click(screen.getByRole('button', { name: /^20m/ }))
    expect(onBandClick).toHaveBeenCalledWith('20m')
  })

  it('with stale data every tile is hollow and says so, and none is green', () => {
    render(<BandTiles prop={snap(BANDS, [], { asOf: NOW_MS / 1000 - 3600 })} nowMs={NOW_MS} />)
    expect(tiles().length).toBe(11)
    for (const b of tiles()) {
      expect(b.className).toContain('is-unknown')
      expect(b.style.getPropertyValue('--bt-color'), 'a hollow tile carries a colour').toBe('')
      expect(b.querySelector('.bt-word')!.textContent).toBe('No data')
    }
  })
})

// ── the colours, in every mode and theme ─────────────────────────────────────────────────────

const MODES_ALL: readonly Mode[] = [...MODES, ...SKIN_MODES]
const SURFACES = ['--bg', '--panel', '--bg-elev'] as const

/** A property's winner once per BASE mode: a theme or colour-role preset declares tokens only
 *  (styles-skins.test.ts and styles-palette-roles.test.ts refuse anything else). */
const WINNERS = new WeakMap<Rule[], Map<string, ReturnType<typeof winnerAt>>>()
function winner(rules: Rule[], mode: Mode, chain: El[], ...props: string[]): ReturnType<typeof winnerAt> {
  let byKey = WINNERS.get(rules)
  if (!byKey) WINNERS.set(rules, (byKey = new Map()))
  const key = `${baseOf(mode)}|${JSON.stringify(chain)}|${props.join()}`
  if (!byKey.has(key)) byKey.set(key, winnerAt(rules, baseOf(mode), chain, ...props))
  return byKey.get(key)!
}
/** Tiles differ by their tooltip (`title`) and pressed state, which no colour rule reads but the
 *  pressed outline; the tokens are the same for every tile, so they are computed once per mode. */
const TOKENS = new WeakMap<Rule[], Map<string, Map<string, string>>>()
function tokensFor(rules: Rule[], mode: Mode, chain: El[]): Map<string, string> {
  let byKey = TOKENS.get(rules)
  if (!byKey) TOKENS.set(rules, (byKey = new Map()))
  const bare = chain.map((el) => ({ ...el, attrs: Object.fromEntries(Object.entries(el.attrs).filter(([k]) => k !== 'title')) }))
  const key = `${mode}|${JSON.stringify(bare)}`
  if (!byKey.has(key)) byKey.set(key, tokensAt(rules, mode, bare))
  return byKey.get(key)!
}
function rgb(tokens: Map<string, string>, value: string, backdrop: Rgb = [0, 0, 0]): Rgb {
  const v = expandWith(tokens, value).trim()
  const c = v === '' ? null : toRgb(v, backdrop)
  if (!c) throw new Error(`"${value}" does not resolve to a colour: "${v}"`)
  return c
}
/** The ink of a tile's text: the first element from `node` up that the sheet colours; a <button>
 *  that sets none hands its text the BROWSER's ink (#382's lesson). */
function inkOf(rules: Rule[], mode: Mode, node: Element): { value: string; chain: El[] } | 'browser' {
  const chain = chainOf(node)
  for (let i = chain.length; i > 0; i--) {
    const at = chain.slice(0, i)
    const w = winner(rules, mode, at, 'color')
    if (w && w.value !== 'inherit') return { value: w.value, chain: at }
    if (at[at.length - 1].tag === 'button') return 'browser'
  }
  return { value: 'var(--text)', chain: [] }
}

function tileProblems(rules: Rule[], modes: readonly Mode[] = MODES_ALL): string[] {
  const out: string[] = []
  const all = tiles()
  expect(all.map((b) => b.className.split(' ').find((c) => /^is-(open|marginal|closed|unknown)$/.test(c)))).toEqual(
    expect.arrayContaining(['is-open', 'is-marginal', 'is-closed', 'is-unknown']),
  )
  for (const mode of modes)
    for (const tile of all) {
      const state = tile.className.split(' ').find((c) => c.startsWith('is-'))!
      const where = `${mode} ${tile.querySelector('.bt-band')!.textContent} ${state}`
      const tokens = new Map(tokensFor(rules, mode, chainOf(tile)))
      const own = tile.style.getPropertyValue('--bt-color').trim()
      if (own) tokens.set('--bt-color', expandWith(tokens, own))
      const chain = chainOf(tile)
      const fill = winner(rules, mode, chain, 'background', 'background-color')
      const edge = winner(rules, mode, chain, 'border-color')
      const tinted = state === 'is-open' || state === 'is-marginal'
      for (const part of ['.bt-band', '.bt-word']) {
        const ink = inkOf(rules, mode, tile.querySelector(part)!)
        if (ink === 'browser') {
          out.push(`${where} ${part}: the browser's button ink`)
          continue
        }
        const floor = ink.value === 'var(--text)' && isHigh(mode) ? 7 : 4.5
        for (const s of SURFACES) {
          const surface = rgb(tokens, `var(${s})`)
          const under = fill && fill.value !== 'none' ? rgb(tokens, fill.value, surface) : surface
          const ratio = contrast(rgb(tokens, ink.value, under), under)
          if (ratio < floor) out.push(`${where} ${part} on ${s}: letters ${ratio.toFixed(2)}:1 < ${floor}`)
        }
      }
      if (!tinted) {
        // No tint: what shows through the tile is its surface, exactly. (A colour mixed with
        // `transparent` is a tint too, so the resolved colour is compared, not the text.)
        for (const s of SURFACES) {
          const surface = rgb(tokens, `var(${s})`)
          const shown = fill && fill.value !== 'none' ? rgb(tokens, fill.value, surface) : surface
          if (shown.join() !== surface.join()) out.push(`${where} on ${s}: a closed or hollow tile is tinted (${fill!.value})`)
        }
        continue
      }
      if (!edge || !/--bt-color/.test(edge.value)) {
        out.push(`${where}: no edge carries the band colour`)
        continue
      }
      // Marginal's edge is DASHED and Open's solid: a second channel besides hue, since the two
      // tints are close to a protan eye (the word is the first channel).
      const style = winner(rules, mode, chain, 'border-style')?.value ?? (/\b(solid|dashed|dotted)\b/.exec(winner(rules, mode, chain, 'border')?.value ?? '')?.[1] ?? 'none')
      const wantStyle = state === 'is-marginal' ? 'dashed' : 'solid'
      if (style !== wantStyle) out.push(`${where}: a ${style} edge, not ${wantStyle}`)
      for (const s of SURFACES) {
        const surface = rgb(tokens, `var(${s})`)
        const off = contrast(rgb(tokens, edge.value, surface), surface)
        if (off < 3) out.push(`${where} on ${s}: edge ${off.toFixed(2)}:1 < 3`)
      }
    }
  return out
}

describe('the band tiles’ colours read in every mode and theme', () => {
  const renderAll = () => {
    // Every state at once: the four bands above, the 2 m opening, a band with no model (its colour
    // is the tier's neutral), and beside them a box on stale data, whose tiles are all hollow.
    render(
      <>
        <BandTiles prop={snap([...BANDS, report('6m', undefined, 'Quiet')], OPENINGS)} nowMs={NOW_MS} />
        <BandTiles prop={snap(BANDS, [], { asOf: NOW_MS / 1000 - 3600 })} nowMs={NOW_MS} />
      </>,
    )
  }

  it('every letter at its floor on its tile, every coloured edge 3:1 off its surface', () => {
    expect(RULES.filter((r) => r.selector.includes('[title')).map((r) => r.selector)).toEqual([])
    renderAll()
    expect(tileProblems(RULES)).toEqual([])
  }, 60_000)

  it('FIRES: letters in the band colour, and an edge lost into the page, are refused', () => {
    renderAll()
    const bandLetters = [...RULES, ...parseRules('.bt-tile { color: var(--bt-color, var(--text)); }', { n: 1e6 })]
    const letters = tileProblems(bandLetters, BASE_MODES)
    expect(letters.some((p) => p.startsWith('light ') && p.includes('is-open') && p.includes('letters'))).toBe(true)
    // Still the band colour by name, but mixed away into the page: the numeric edge check must see it.
    const lostEdge = [...RULES, ...parseRules('.bt-tile.is-open { border-color: color-mix(in srgb, var(--bt-color) 0%, var(--bg)); }', { n: 1e6 })]
    expect(tileProblems(lostEdge, BASE_MODES).some((p) => p.includes('is-open on --bg: edge 1.00:1'))).toBe(true)
    // A marginal edge drawn solid loses the channel that is not hue.
    const solidMarginal = [...RULES, ...parseRules('.bt-tile.is-marginal { border-style: solid; }', { n: 1e6 })]
    expect(tileProblems(solidMarginal, BASE_MODES).some((p) => p.includes('is-marginal: a solid edge, not dashed'))).toBe(true)
    // And an edge in another colour altogether is no band edge.
    const otherEdge = [...RULES, ...parseRules('.bt-tile.is-open { border-color: var(--border); }', { n: 1e6 })]
    expect(tileProblems(otherEdge, BASE_MODES).some((p) => p.includes('is-open: no edge carries the band colour'))).toBe(true)
  }, 60_000)
})

// ── the words fit, in every language ─────────────────────────────────────────────────────────

/** The widest word a tile must hold on one line: "Marginal", 11 px bold at 0.02em, in DejaVu Sans —
 *  the widest UI font the app meets (`system-ui` on Linux and the Pi). MEASURED in Chrome, 2026-09-29:
 *  56 px at Normal text size. At the grid's old 64 px floor a tile had 45 px for its word, and
 *  "Marginal" was cut on every default layout from 1024 to 3440 wide. The no-data phrases are longer
 *  (up to 100 px, "Pas de données") but break at their spaces, and none of their words is wider.
 *  Re-measure this constant if the word's font changes; never nudge it to pass. */
const WIDEST_WORD_PX = 56
const TEXT_SCALES = [1, 1.12, 1.25] // Settings ▸ Text size: Normal / Large / Larger

/** A resolved length: `12px`, or the sheet's `calc(<n>px * <n>)`. */
function lengthPx(v: string): number {
  const t = v.trim()
  const calc = /^calc\(\s*(-?[\d.]+)px\s*\*\s*(-?[\d.]+)\s*\)$/.exec(t)
  if (calc) return Number(calc[1]) * Number(calc[2])
  const px = /^(-?[\d.]+)px$/.exec(t)
  if (px) return Number(px[1])
  throw new Error(`not a length this guard reads: "${v}"`)
}
/** A shorthand's values, split at its top-level spaces (`calc(8px * 1)` stays whole). */
const valuesOf = (v: string) => topSplit(v.trim().replace(/\s+(?![^(]*\))/g, ','))

function fitProblems(rules: Rule[]): string[] {
  const out: string[] = []
  const grid = document.querySelector('.band-tiles')!
  const tile = tiles()[0]
  const word = tile.querySelector('.bt-word')!
  for (const mode of BASE_MODES)
    for (const scale of TEXT_SCALES) {
      const tokens = new Map(tokensFor(rules, mode, chainOf(tile)))
      tokens.set('--text-scale', String(scale))
      const cols = winner(rules, mode, chainOf(grid), 'grid-template-columns')?.value ?? ''
      const floor = /minmax\(\s*(.+?)\s*,\s*1fr\s*\)/.exec(expandWith(tokens, cols))?.[1]
      if (!floor) {
        out.push(`${mode}: the tile grid has no minmax floor (${cols})`)
        continue
      }
      const pad = winner(rules, mode, chainOf(tile), 'padding', 'padding-inline', 'padding-left')!
      const padParts = valuesOf(expandWith(tokens, pad.value)).map(lengthPx)
      const padX = pad.prop === 'padding' ? (padParts[1] ?? padParts[0]) : padParts[padParts.length - 1]
      const border = /(-?[\d.]+)px/.exec(winner(rules, mode, chainOf(tile), 'border', 'border-width')!.value)
      const room = lengthPx(floor) - 2 * padX - 2 * Number(border?.[1] ?? 0)
      const need = WIDEST_WORD_PX * scale
      if (room < need) out.push(`${mode} at text ×${scale}: the narrowest tile leaves ${room.toFixed(1)} px for a ${need.toFixed(1)} px word`)
    }
  // A phrase longer than the tile wraps at its spaces, and a word longer than it breaks: never cut.
  const ws = winner(rules, 'dark', chainOf(word), 'white-space')?.value ?? 'normal'
  if (/nowrap|pre/.test(ws)) out.push(`the word is kept on one line (white-space: ${ws}), so a longer phrase is cut`)
  const wrap = winner(rules, 'dark', chainOf(word), 'overflow-wrap', 'word-wrap')?.value ?? 'normal'
  if (!/break-word|anywhere/.test(wrap)) out.push(`a word wider than its tile is not broken (overflow-wrap: ${wrap})`)
  return out
}

describe('the band tiles’ words fit', () => {
  it('the narrowest tile holds the widest word at every text size, and a longer phrase wraps instead of being cut', () => {
    render(<BandTiles prop={snap(BANDS)} nowMs={NOW_MS} />)
    expect(fitProblems(RULES)).toEqual([])
  })

  it('FIRES: the old 64 px floor, and a word kept on one line, are refused', () => {
    render(<BandTiles prop={snap(BANDS)} nowMs={NOW_MS} />)
    const oldFloor = [...RULES, ...parseRules('.band-tiles { grid-template-columns: repeat(auto-fill, minmax(calc(64px * var(--text-scale)), 1fr)); }', { n: 1e6 })]
    expect(fitProblems(oldFloor)).toContain('dark at text ×1: the narrowest tile leaves 45.0 px for a 56.0 px word')
    const oneLine = [...RULES, ...parseRules('.bt-word { white-space: nowrap; overflow-wrap: normal; }', { n: 1e6 })]
    expect(fitProblems(oneLine)).toEqual([
      'the word is kept on one line (white-space: nowrap), so a longer phrase is cut',
      'a word wider than its tile is not broken (overflow-wrap: normal)',
    ])
  })
})
