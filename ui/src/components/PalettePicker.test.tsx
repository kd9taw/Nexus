// @vitest-environment jsdom
//
// The waterfall palette picker (the cockpit headers', and Settings ▸ Appearance ▸ Waterfall &
// scopes'). Cividis joins the curated list, named colour-blind safe (the look-and-feel redesign,
// piece 7). What must NOT move with it: Turbo stays the default when nothing is stored, and a
// palette the operator picked stays picked.
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { PalettePicker } from './PalettePicker'
import { MASTER_PALETTES, resolveColormap } from '../waterfall'
import { FT_PALETTE_SCOPE, WF_PALETTE_KEY, getWaterfallPalette } from '../waterfallPalette'
import { EN } from '../i18n'

beforeEach(() => localStorage.clear())
afterEach(() => {
  cleanup()
  localStorage.clear()
})

const select = () => screen.getByRole('combobox') as HTMLSelectElement
const optionFor = (v: string) => Array.from(select().options).find((o) => o.value === v)

describe('PalettePicker', () => {
  it('offers Cividis in both scopes, named colour-blind safe', () => {
    for (const scope of [undefined, FT_PALETTE_SCOPE]) {
      const { unmount } = render(<PalettePicker scope={scope} />)
      expect(optionFor('cividis')?.textContent, `scope ${scope ?? 'shared'}`).toBe(EN['waterfall.palette.cividis'])
      unmount()
    }
  })

  it('keeps it beside the other perceptual maps: Inferno, Viridis, Cividis, then Turbo', () => {
    const order = MASTER_PALETTES.map((p) => p.value)
    expect(order.slice(order.indexOf('inferno'), order.indexOf('turbo') + 1)).toEqual([
      'inferno',
      'viridis',
      'cividis',
      'turbo',
    ])
  })

  it('leaves Turbo the default, with nothing stored, in both scopes', () => {
    expect(getWaterfallPalette()).toBe('turbo')
    expect(getWaterfallPalette(FT_PALETTE_SCOPE)).toBe('turbo')
    render(<PalettePicker />)
    expect(select().value).toBe('turbo')
    expect(localStorage.getItem(WF_PALETTE_KEY), 'rendering the picker wrote a palette').toBeNull()
  })

  it('leaves a palette the operator picked exactly as picked', () => {
    localStorage.setItem(WF_PALETTE_KEY, 'inferno')
    render(<PalettePicker />)
    expect(select().value).toBe('inferno')
    expect(localStorage.getItem(WF_PALETTE_KEY)).toBe('inferno')
  })

  it('a Cividis pick is stored and resolves to Cividis in either theme and at night', () => {
    render(<PalettePicker />)
    fireEvent.change(select(), { target: { value: 'cividis' } })
    expect(localStorage.getItem(WF_PALETTE_KEY)).toBe('cividis')
    for (const theme of ['dark', 'light']) {
      expect(resolveColormap('cividis', theme)).toBe('cividis')
      expect(resolveColormap('cividis', theme, true)).toBe('cividis')
    }
  })

  it('takes a label from its host in place of the header wording', () => {
    render(<PalettePicker label={EN['settings.waterfallScopes.ft.label']} scope={FT_PALETTE_SCOPE} />)
    expect(screen.getByRole('combobox', { name: EN['settings.waterfallScopes.ft.label'] })).toBeTruthy()
  })
})
