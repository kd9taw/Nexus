import { describe, it, expect } from 'vitest'
import { workedGridSet } from './coverage'
import type { LoggedQso } from './types'

const q = (grid: string | null | undefined): LoggedQso =>
  ({ call: 'W1AW', grid, band: '20m', mode: 'FT8' }) as unknown as LoggedQso

describe('workedGridSet', () => {
  it('reduces a log to its unique 4-character squares', () => {
    const s = workedGridSet([q('EN52'), q('FN31'), q('EN52')])
    expect([...s].sort()).toEqual(['EN52', 'FN31'])
  })

  it('cuts a 6-character locator to its square rather than counting it twice', () => {
    // EN52tk and EN52ab are the same square. A coverage map that drew both would
    // claim two worked squares where the operator has one.
    const s = workedGridSet([q('EN52tk'), q('EN52ab')])
    expect([...s]).toEqual(['EN52'])
  })

  it('upper-cases, so case in the log cannot split one square into two', () => {
    expect([...workedGridSet([q('en52'), q('EN52')])]).toEqual(['EN52'])
  })

  it('skips a QSO with no grid instead of guessing one', () => {
    // The important half: a missing grid must not become a mark on the map. Plenty of
    // logged contacts carry no grid at all, and inventing a square would show the
    // operator coverage they never worked.
    expect(workedGridSet([q(null), q(undefined), q(''), q('   ')]).size).toBe(0)
  })

  it('skips a grid too short to name a square', () => {
    // "EN" is a field, not a square — there is nothing to place.
    expect(workedGridSet([q('EN'), q('E')]).size).toBe(0)
  })

  it('tolerates surrounding whitespace from an imported log', () => {
    expect([...workedGridSet([q('  EN52  ')])]).toEqual(['EN52'])
  })

  it('is empty for an empty log', () => {
    expect(workedGridSet([]).size).toBe(0)
  })
})
