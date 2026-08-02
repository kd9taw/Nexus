import { describe, it, expect } from 'vitest'
import { panelHost, type PanelHostSpec } from './panelHost'
import type { PanelState } from './panelState'

type P = 'main' | 'sideA' | 'sideB'

const spec: PanelHostSpec<P> = {
  menu: ['main', 'sideA', 'sideB'],
  side: ['sideA', 'sideB'],
  main: 'main',
  labels: { main: 'Main', sideA: 'Side A', sideB: 'Side B' },
}

const api = (state: Partial<Record<P, PanelState>>) => ({
  stateOf: (id: P): PanelState => state[id] ?? 'docked',
})

describe('panelHost', () => {
  it('shown() is true unless removed — popped still occupies a slot', () => {
    const h = panelHost(api({ sideA: 'removed', sideB: 'popped' }), spec)
    expect(h.shown('main')).toBe(true)
    expect(h.shown('sideA')).toBe(false)
    expect(h.shown('sideB')).toBe(true)
  })

  it('dataCols is two when both regions hold content, one when either empties', () => {
    expect(panelHost(api({}), spec).dataCols).toBe('two')
    // Whole rail removed → collapse to one column so the main pane reclaims the space.
    expect(panelHost(api({ sideA: 'removed', sideB: 'removed' }), spec).dataCols).toBe('one')
    // Main cell removed → one column.
    expect(panelHost(api({ main: 'removed' }), spec).dataCols).toBe('one')
  })

  it('sideShown reflects any still-docked rail panel', () => {
    expect(panelHost(api({ sideA: 'removed' }), spec).sideShown).toBe(true)
    expect(panelHost(api({ sideA: 'removed', sideB: 'removed' }), spec).sideShown).toBe(false)
  })

  it('columns spec: dataCols is the POPULATED-column count (the 3-col Classic grid)', () => {
    const spec3: PanelHostSpec<P> = {
      ...spec,
      columns: [['main'], ['sideA'], ['sideB']],
    }
    expect(panelHost(api({}), spec3).dataCols).toBe('three')
    // One column emptied → the survivors flow into the 2-track template.
    expect(panelHost(api({ sideA: 'removed' }), spec3).dataCols).toBe('two')
    // Two emptied → single column.
    expect(panelHost(api({ main: 'removed', sideB: 'removed' }), spec3).dataCols).toBe('one')
    // A column with ANY shown occupant stays populated (popped still holds a slot).
    const shared: PanelHostSpec<P> = { ...spec, columns: [['main'], ['sideA', 'sideB']] }
    expect(panelHost(api({ sideA: 'removed', sideB: 'popped' }), shared).dataCols).toBe('two')
    // Everything removed still renders a one-column region, never a zero-track grid.
    expect(
      panelHost(api({ main: 'removed', sideA: 'removed', sideB: 'removed' }), spec3).dataCols,
    ).toBe('one')
  })

  it('menuItems mirror the spec menu order, labels, and live state', () => {
    const h = panelHost(api({ sideB: 'removed' }), spec)
    expect(h.menuItems).toEqual([
      { id: 'main', label: 'Main', state: 'docked' },
      { id: 'sideA', label: 'Side A', state: 'docked' },
      { id: 'sideB', label: 'Side B', state: 'removed' },
    ])
  })
})
