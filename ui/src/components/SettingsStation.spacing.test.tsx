// @vitest-environment jsdom
//
// Settings ▸ Station: the Units row keeps the same row gap as the station fields above it.
//
// THE BUG (1080p at 150% Windows scaling, 2026-11-01): the State hint ran straight into the UNITS
// label below it. Units used to be one more field of the station grid, separated from the row
// above by that grid's `gap`. It moved into a grid of its own, between the two disabling groups,
// so a Remote browser could still change it (see SettingsStation.tsx) — and the gap did not move
// with it: grid `gap` only separates items INSIDE one grid, and nothing separated the two grids.
// The spacing above Units was 0, and whichever hint was tallest in that column touched the label;
// a narrow window (more lines per hint) is where it showed.
//
// HOW IT COMPUTES. jsdom does not lay out, so this resolves the cascade over the real sheet with
// jsdom's real selector engine (sibling combinators included) on the real rendered component:
// importance, then specificity, then source order. Between two block siblings with no padding or
// border of their own, the space is the larger of the first one's margin-bottom and the second
// one's margin-top (margins collapse), and that is the number asserted against the grid's row gap.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { cleanup, render, screen } from '@testing-library/react'
import { SettingsStation } from './SettingsStation'
import { cmpSpec, parseRules, type Rule } from '../cssCascade'
import type { Settings } from '../types'

afterEach(cleanup)

const RULES: Rule[] = parseRules(
  readFileSync(resolve(process.cwd(), 'src', 'styles.css'), 'utf8').replace(/\/\*[\s\S]*?\*\//g, ''),
)

/** The value of `prop` that applies to `el`, or null when no rule sets it (initial value). */
function applied(el: Element, prop: string): string | null {
  let win: { value: string; important: boolean; rule: Rule } | null = null
  for (const rule of RULES) {
    const d = rule.decls.filter((x) => x.prop === prop).pop()
    if (!d) continue
    let hit = false
    try {
      hit = el.matches(rule.selector)
    } catch {
      continue
    }
    if (!hit) continue
    const important = /!\s*important\s*$/.test(d.value)
    const value = d.value.replace(/!\s*important\s*$/, '').trim()
    if (
      !win ||
      (important !== win.important
        ? important
        : cmpSpec(rule.spec, win.rule.spec) > 0 ||
          (cmpSpec(rule.spec, win.rule.spec) === 0 && rule.order > win.rule.order))
    ) {
      win = { value, important, rule }
    }
  }
  return win?.value ?? null
}

const ZERO = new Set([null, '0', '0px'])
/** The space between two adjacent block siblings: their collapsed margins. Only the two shapes
 *  this file meets are resolved — a zero, or one non-zero token on one side. */
function spaceBetween(above: Element, below: Element): string | null {
  const mb = applied(above, 'margin-bottom') ?? applied(above, 'margin')
  const mt = applied(below, 'margin-top')
  if (ZERO.has(mb)) return ZERO.has(mt) ? null : mt
  if (ZERO.has(mt)) return mb
  throw new Error(`both sides carry a margin (${mb} / ${mt}): resolve the collapse by hand`)
}

const form = {
  mycall: 'KD9TAW', gridsquare: 'EN61', opName: 'Op', licenseClass: 'general', opState: '',
  units: 'auto', dialMhz: 14.074, band: '20m', sideband: 'USB',
} as unknown as Settings

function mount() {
  render(
    <SettingsStation form={form} error={null} bandPlan={[]} onUpdate={vi.fn()} onSetFreq={vi.fn()} />,
  )
  const unitsGrid = screen.getByLabelText('Units').closest('.settings-grid')!
  const above = unitsGrid.previousElementSibling!
  const stationGrid = above.querySelector('.settings-grid')!
  return { unitsGrid, above, stationGrid }
}

describe('Settings ▸ Station row spacing', () => {
  it('the station fields and Units really are two grids (the shape this guards)', () => {
    const { unitsGrid, above, stationGrid } = mount()
    expect(above.matches('fieldset.settings-fieldgroup')).toBe(true)
    expect(stationGrid).toBeTruthy()
    expect(stationGrid.contains(screen.getByLabelText('Units'))).toBe(false)
    expect(unitsGrid.parentElement?.id).toBe('settings-operator-radio')
  })

  it('the space above UNITS is the station grid row gap, not zero', () => {
    const { unitsGrid, above, stationGrid } = mount()
    const rowGap = applied(stationGrid, 'row-gap') ?? applied(stationGrid, 'gap')
    expect(rowGap, 'positive control: the station grid separates its own rows').toBe('var(--space-4)')
    expect(spaceBetween(above, unitsGrid)).toBe(rowGap)
  })

  it('a disabling group adds no box of its own (its contract is unchanged)', () => {
    const { above } = mount()
    for (const p of ['margin', 'margin-top', 'margin-bottom', 'padding', 'border']) {
      expect(ZERO.has(applied(above, p)) || applied(above, p) === '0', p).toBe(true)
    }
  })

  it('a lone grid that OPENS its section gets no extra space under the legend', () => {
    // Map & globe, Performance and Logbook display open with a `.settings-grid--lone`.
    document.body.innerHTML =
      '<fieldset class="settings-section"><legend>X</legend>' +
      '<div class="settings-grid settings-grid--lone"><div class="settings-field"></div></div></fieldset>'
    const grid = document.querySelector('.settings-grid--lone')!
    expect(ZERO.has(applied(grid, 'margin-top'))).toBe(true)
  })
})
