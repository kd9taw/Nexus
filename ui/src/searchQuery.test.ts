import { describe, it, expect } from 'vitest'
import { compileTerm, matchAnyTerm, searchTerms } from './searchQuery'

describe('searchTerms', () => {
  it('splits on spaces AND commas, and uppercases', () => {
    expect(searchTerms('pa* , on4*')).toEqual(['PA*', 'ON4*'])
    expect(searchTerms('  ')).toEqual([])
  })
})

describe('a plain term is a substring', () => {
  it('finds the letters anywhere in the text', () => {
    const t = compileTerm('4fd')
    expect(t('4X4FD')).toBe(true)
    expect(t('PA0XYZ')).toBe(false)
  })
})

describe('a wildcard term is a whole-word pattern', () => {
  it('* matches a run of characters, anchored', () => {
    const t = compileTerm('PA*')
    expect(t('PA0XYZ')).toBe(true)
    expect(t('PA3EGH')).toBe(true)
    // THE POINT of the wildcard: a prefix hunt must not drag in calls that merely
    // contain the letters. This is the case a substring search gets wrong.
    expect(t('W1PA')).toBe(false)
    expect(t('ON4AOI')).toBe(false)
  })

  it('? matches exactly one character', () => {
    const t = compileTerm('ON?AOI')
    expect(t('ON4AOI')).toBe(true)
    expect(t('ON44AOI')).toBe(false)
  })

  it('matches any WORD of a multi-field row, not just the whole string', () => {
    const t = compileTerm('PA*')
    expect(t('PD5MVH NETHERLANDS PA0XYZ FT8 20M')).toBe(true)
    expect(t('EA5ISM SPAIN EW4DX FT8 20M')).toBe(false)
  })

  it('does not let a regex metacharacter through', () => {
    const t = compileTerm('PA.*')
    expect(t('PA0XYZ')).toBe(false)
    expect(t('PA.0XYZ')).toBe(true)
  })
})

describe('matchAnyTerm — the single-field list', () => {
  it('is null on an empty box, so the list skips filtering', () => {
    expect(matchAnyTerm('')).toBeNull()
    expect(matchAnyTerm('   ,  ')).toBeNull()
  })

  it('ORs the terms — the operator asked for PA* or ON4*', () => {
    const m = matchAnyTerm('PA* ON4*')!
    expect(m('PA0XYZ')).toBe(true)
    expect(m('ON4AOI')).toBe(true)
    expect(m('EA5ISM')).toBe(false)
  })

  it('is case-insensitive in both directions', () => {
    const m = matchAnyTerm('pa*')!
    expect(m('pa0xyz')).toBe(true)
  })

  it('mixes a wildcard term with a plain one', () => {
    const m = matchAnyTerm('ON4* 4FD')!
    expect(m('ON4AOI')).toBe(true)
    expect(m('4X4FD')).toBe(true)
    expect(m('OH3OJ')).toBe(false)
  })
})
