// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from 'vitest'
import { orderNav, moveNav, loadNavOrder, saveNavOrder, resetNavOrder } from './navOrder'

const DEFAULT = ['connect', 'needed', 'spots', 'logbook', 'awards', 'stats']

describe('orderNav', () => {
  it('applies a saved order', () => {
    expect(orderNav(DEFAULT, ['stats', 'logbook'])).toEqual([
      'stats',
      'logbook',
      'connect',
      'needed',
      'spots',
      'awards',
    ])
  })

  it('keeps a NEW section (not in the saved order) — it must not vanish', () => {
    // saved order predates 'awards' being added; awards still appears, after the known ones.
    const withNew = [...DEFAULT, 'brandnew']
    const out = orderNav(withNew, ['logbook', 'connect'])
    expect(out).toContain('brandnew')
    expect(out.slice(0, 2)).toEqual(['logbook', 'connect'])
  })

  it('drops a saved id that no longer exists', () => {
    expect(orderNav(DEFAULT, ['deleted', 'spots'])).toEqual([
      'spots',
      'connect',
      'needed',
      'logbook',
      'awards',
      'stats',
    ])
  })

  it('empty saved order → the default order, unchanged', () => {
    expect(orderNav(DEFAULT, [])).toEqual(DEFAULT)
  })
})

describe('moveNav', () => {
  it('moves an id to just before the target', () => {
    expect(moveNav(DEFAULT, 'stats', 'needed')).toEqual([
      'connect',
      'stats',
      'needed',
      'spots',
      'logbook',
      'awards',
    ])
  })

  it('null target drops it at the end', () => {
    expect(moveNav(DEFAULT, 'connect', null)).toEqual([
      'needed',
      'spots',
      'logbook',
      'awards',
      'stats',
      'connect',
    ])
  })

  it('a bad drag never loses an item', () => {
    expect(moveNav(DEFAULT, 'nope', 'spots')).toEqual(DEFAULT) // unknown id → unchanged
    expect(moveNav(DEFAULT, 'stats', 'nope')).toContain('stats') // unknown target → appended
  })
})

describe('persistence (shared, global key)', () => {
  beforeEach(() => localStorage.clear())

  it('round-trips through a plain global key', () => {
    saveNavOrder(['logbook', 'connect'])
    expect(loadNavOrder()).toEqual(['logbook', 'connect'])
    // Stored under a NON-surface-scoped key so every window shares one rail order.
    expect(localStorage.getItem('nexus.navOrder')).toBe('["logbook","connect"]')
  })

  it('reset clears it', () => {
    saveNavOrder(['stats'])
    resetNavOrder()
    expect(loadNavOrder()).toEqual([])
  })

  it('garbage in storage → empty, no throw', () => {
    localStorage.setItem('nexus.navOrder', '{not json')
    expect(loadNavOrder()).toEqual([])
  })
})
