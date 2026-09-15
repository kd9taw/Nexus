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
  // `closeProps` writes, so the stub needs the one writer — and mutating the same record
  // `stateOf` reads is how the ✕-is-the-same-act case below sees what the button did.
  setPanelState: (id: P, s: PanelState) => {
    state[id] = s
  },
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

  // ── THE PANE'S OWN ✕ (2026-09-15). The operator went looking for a way to close a panel
  //    on the FT8 screen and could not find one; the fix is a SECOND DOOR onto the existing
  //    mechanism, never a second mechanism. These cases are what "same act, same record"
  //    means when it is computed rather than asserted in a comment.
  describe('closeProps — the ✕ is the same act as the ⊞ tick', () => {
    it('presses through to setPanelState(id, "removed"), which stateOf then reports', () => {
      const state: Partial<Record<P, PanelState>> = {}
      const h = panelHost(api(state), spec)
      expect(h.shown('sideA')).toBe(true)
      h.closeProps('sideA').onRemove?.()
      // The RECORD moved — not a local flag, not a second store. A ✕ that merely unmounted
      // its own subtree would pass a presence-only test and fail this one.
      expect(state.sideA).toBe('removed')
      expect(panelHost(api(state), spec).shown('sideA')).toBe(false)
      // …and the ⊞ entry now reads unticked, which is the operator-visible half of "the
      // same act": the menu is still where the pane comes back from.
      expect(panelHost(api(state), spec).menuItems.find((i) => i.id === 'sideA')?.state).toBe(
        'removed',
      )
    })

    it('is EMPTY for an id this layout does not list — no entry, no ✕', () => {
      // A pane with no vocabulary entry is not removable, and the ✕ must be unrepresentable
      // there rather than guarded: `{}` spread onto a frame leaves `onRemove` undefined and
      // the button simply is not built.
      const narrow: PanelHostSpec<P> = { ...spec, menu: ['main'] }
      expect(panelHost(api({}), narrow).closeProps('sideA')).toEqual({})
      expect(panelHost(api({}), narrow).closeProps('main').onRemove).toBeTypeOf('function')
    })

    it('carries the cockpit\'s endsOnHide copy — the same string the ⊞ entry prints', () => {
      const ENDS = 'hiding this stops a voice message that is playing'
      const warned: PanelHostSpec<P> = { ...spec, endsOnHide: { sideA: ENDS } }
      const h = panelHost(api({}), warned)
      // ONE wording, two doors. If these two ever diverge, the operator is being told two
      // different things about the same act.
      expect(h.closeProps('sideA').hideNote).toBe(ENDS)
      expect(h.menuItems.find((i) => i.id === 'sideA')?.note).toBe(ENDS)
      // A pane whose hide ends nothing carries NO note — one the operator cannot act on
      // teaches him to ignore the next one (THE PRACTICE, features/panelState.ts).
      expect(h.closeProps('sideB').hideNote).toBeUndefined()
    })

    it('an AVAILABILITY reason stays on the menu and never reaches the ✕', () => {
      // The two kinds used to share one field. "Your radio is not reporting DSP functions
      // over CAT" is an answer to "why is this pane empty" — it is not a warning about
      // pressing Hide, and a ✕ that carried it would be lying about the consequence.
      const h = panelHost(api({}), { ...spec, notes: { sideA: 'no native scope streaming' } })
      expect(h.menuItems.find((i) => i.id === 'sideA')?.note).toBe('no native scope streaming')
      expect(h.closeProps('sideA').hideNote).toBeUndefined()
    })

    it('a consequence outranks an availability reason on the shared menu note', () => {
      const h = panelHost(api({}), {
        ...spec,
        notes: { sideA: 'empty until the first over' },
        endsOnHide: { sideA: 'hiding this stops what is playing' },
      })
      expect(h.menuItems.find((i) => i.id === 'sideA')?.note).toBe(
        'hiding this stops what is playing',
      )
    })
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
