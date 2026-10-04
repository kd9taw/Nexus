// @vitest-environment jsdom
//
// THE KEY NAMES BOTH POTA COLOURS, SIDE BY SIDE (operator, 2026-10-03, "Dim POTA color for all").
// Band activity and its band map paint a park still to be worked in the full POTA colour and any
// other POTA activator in a dim one (BandMap.potaDim.test.tsx). The key under both shows the two
// swatches next to each other in plain words, "New park" and then "POTA activator", where the park
// ranks among the colours: after a new mode and before a confirmation (LoTW).
//
// "Both themes" is held on what reaches each swatch: the legend is rendered inside both hosts and the
// cascade is resolved on the swatches' own chains with the app's resolver (cssCascade.ts), in every
// base mode (day and night, standard and high contrast), bare and on every built-in theme of its
// base. The colour-role presets are left out, as in BandMap.potaDim.contrast.test.tsx, which also
// holds that none of them sets `--need-pota` or `--pota-dim`.
import { describe, it, expect, afterEach } from 'vitest'
import { cleanup, render } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { BandMap } from './BandMap'
import { BandStrip } from './BandStrip'
import { SKINS } from '../features/skins'
import {
  BASE_MODES,
  baseTheme,
  chainOf,
  expandWith,
  parseRules,
  rgbHex as hex,
  rootTokensFrom,
  toRgb,
  tokensAt,
  winnerAt,
  withRoles,
  type El,
  type Mode,
  type Rule,
} from '../cssCascade'

afterEach(cleanup)

const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES = parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css'))
/** The sheet without the dim colour's class, so the dim swatch falls back to whatever is under it. */
const NO_DIM_CLASS = RULES.filter((r) => r.selector !== '.pota-dim')

const skinsOf = (theme: 'light' | 'dark') => ['', ...SKINS.filter((s) => s.base === theme).map((s) => s.id)]
/** Every base mode, bare and on every built-in theme of its base. */
const MODES: Mode[] = BASE_MODES.flatMap((b) => skinsOf(baseTheme(b)).map((skin) => (skin ? withRoles(b, { skin }) : b)))

const BODY: El = { tag: 'body', classes: [], attrs: {} }

/** The key's items, in order, as each host renders it (shown by default). */
function legends(): { name: string; items: Element[] }[] {
  const out: { name: string; items: Element[] }[] = []
  const strip = render(
    <div className="app">
      <div className="pane-body">
        <BandStrip band="20m" dialMhz={14.2} txAllowed spots={[]} onWorkSpot={() => {}} />
      </div>
    </div>,
  ).container
  out.push({ name: 'Band activity', items: [...strip.querySelectorAll('.spot-legend .spot-legend-item')] })
  const map = render(
    <div className="app detached">
      <BandMap band="20m" dialMhz={14.2} txAllowed spots={[]} onWorkSpot={() => {}} />
    </div>,
  ).container
  out.push({ name: 'the band map', items: [...map.querySelectorAll('.spot-legend .spot-legend-item')] })
  return out
}

const word = (el: Element) => el.textContent ?? ''
const colourClasses = (el: Element) => [...el.classList].filter((c) => c === 'pota-dim' || c.startsWith('need-'))

/** The colour a swatch's dot paints in `mode`, or a note saying why it paints none. */
function swatch(rules: Rule[], mode: Mode, item: Element): string {
  const dot = item.querySelector('.spot-legend-dot')
  if (!dot) return 'no swatch'
  const chain = [BODY, ...chainOf(dot)]
  const w = winnerAt(rules, mode, chain, 'background', 'background-color')
  if (!w) return 'paints nothing'
  const c = toRgb(expandWith(tokensAt(rules, mode, chain), w.value), [0, 0, 0])
  return c ? hex(c) : `not a colour: ${w.value}`
}

/** Every swatch that does not paint its theme's own token: "New park" the full POTA colour,
 *  "POTA activator" the dim one. */
function offToken(rules: Rule[]): string[] {
  const out: string[] = []
  for (const { name, items } of legends()) {
    const park = items.find((i) => word(i) === 'New park')
    const dim = items.find((i) => word(i) === 'POTA activator')
    for (const mode of MODES) {
      const tokens = rootTokensFrom(RULES, mode)
      const want = { park: tokens.get('--need-pota')?.toLowerCase(), dim: tokens.get('--pota-dim')?.toLowerCase() }
      const got = { park: park ? swatch(rules, mode, park) : 'absent', dim: dim ? swatch(rules, mode, dim) : 'absent' }
      if (got.park !== want.park) out.push(`${name} ${mode}: New park ${got.park}, want ${want.park}`)
      if (got.dim !== want.dim) out.push(`${name} ${mode}: POTA activator ${got.dim}, want ${want.dim}`)
    }
  }
  // Only now: an unmounted swatch has lost the hosts' chain the cascade is resolved on.
  cleanup()
  return out
}

describe('the spot key under Band activity and the band map', () => {
  it('shows "New park" and then "POTA activator" side by side, between MODE and LoTW, on both', () => {
    const found = legends()
    expect(found.map((l) => l.name)).toEqual(['Band activity', 'the band map'])
    for (const { name, items } of found) {
      const words = items.map(word)
      const at = words.indexOf('MODE')
      expect(at, `${name}: the key is there`).toBeGreaterThan(0)
      expect(words.slice(at, at + 4), name).toEqual(['MODE', 'New park', 'POTA activator', 'LoTW'])
      const [park, dim] = [items[at + 1], items[at + 2]]
      expect(colourClasses(park), `${name}: New park`).toEqual(['need-pota'])
      expect(colourClasses(dim), `${name}: POTA activator`).toEqual(['pota-dim'])
      // A word for each, on hover, that is not the other's.
      expect(park.getAttribute('title'), name).toBeTruthy()
      expect(dim.getAttribute('title'), name).toBeTruthy()
      expect(dim.getAttribute('title'), name).not.toBe(park.getAttribute('title'))
    }
  })

  it('in every theme, "New park" paints the full POTA colour and "POTA activator" the dim one', () => {
    expect(offToken(RULES)).toEqual([])
    // Both themes really are measured, and their two colours really differ.
    const values = (theme: 'dark' | 'light') =>
      new Set(MODES.filter((m) => baseTheme(m) === theme).map((m) => rootTokensFrom(RULES, m).get('--pota-dim')))
    expect([...values('dark')]).toEqual(['#34845a'])
    expect([...values('light')]).toEqual(['#52836a'])
    for (const mode of MODES) {
      const t = rootTokensFrom(RULES, mode)
      expect(t.get('--pota-dim'), `${mode}: dim is not the full colour`).not.toBe(t.get('--need-pota'))
    }
  }, 60_000)

  it('FIRES: without the dim colour’s class, the "POTA activator" swatch is caught in every theme', () => {
    const found = offToken(NO_DIM_CLASS)
    expect(found).toHaveLength(MODES.length * 2)
    expect(found.every((f) => f.includes('POTA activator'))).toBe(true)
  }, 60_000)
})
