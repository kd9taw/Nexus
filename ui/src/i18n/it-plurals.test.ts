// Italian plurals: one / other, and the million that asks for `many`.
//
// `Intl.PluralRules('it')` selects `one` for exactly 1, `other` for 0 and every other count, as English
// does, and `many` for an exact million (and its multiples: «1 milione di QSO»). The Italian catalog
// carries `one` and `other` only, and `selectPluralForm` falls back to `other` for a category a
// catalog omits, so a million reads the `other` sentence rather than nothing. What can go wrong is the
// CATALOG: a plural with one form missing, or a plain string where English has plural forms.

import { afterEach, describe, expect, it } from 'vitest'
import { EN, installCatalog, selectPluralForm, setLocale, t, type MessageKey } from './index'
import { IT } from './it'
import type { PluralForms } from './types'

afterEach(() => setLocale('en'))

describe('Italian plural selection', () => {
  it.each([
    [1, 'one'],
    [0, 'other'],
    [2, 'other'],
    [21, 'other'],
    [101, 'other'],
    [1.5, 'other'],
  ])('%s selects %s', (n, want) => {
    expect(selectPluralForm('it', n, { one: 'one', other: 'other' })).toBe(want)
  })

  it('an exact million asks for many, and an Italian entry without it reads other', () => {
    expect(selectPluralForm('it', 1_000_000, { one: 'one', many: 'many', other: 'other' })).toBe('many')
    expect(selectPluralForm('it', 1_000_000, { one: 'one', other: 'other' })).toBe('other')
  })

  it('renders a real Italian catalog entry for 0, 1, 2 and a million', () => {
    installCatalog('it', IT)
    setLocale('it')
    const key = 'spots.empty.worked' as MessageKey
    const entry = IT[key] as PluralForms
    expect(entry.one).not.toBe(entry.other) // control: the two sentences differ, so this can fail
    for (const [n, f] of [[1, 'one'], [0, 'other'], [2, 'other'], [1_000_000, 'other']] as const)
      expect(t(key, { count: n })).toBe(entry[f]!.replace('{{count}}', String(n)))
  })
})

/** What is wrong with an Italian catalog's plurals, as messages. */
function pluralProblems(cat: Record<string, unknown>): string[] {
  const out: string[] = []
  for (const [key, en] of Object.entries(EN)) {
    if (typeof en === 'string') continue
    const v = cat[key]
    if (v === undefined) continue // completeness is placeholders.test.ts's job
    if (typeof v === 'string') {
      out.push(`${key}: a plain string where English has plural forms`)
      continue
    }
    for (const f of ['one', 'other'] as const) if (!(v as PluralForms)[f]?.trim()) out.push(`${key}: no ${f} form`)
  }
  return out
}

describe('the Italian catalog carries both plural forms', () => {
  it('every plural has one and other', () => {
    expect(pluralProblems(IT as Record<string, unknown>)).toEqual([])
  })

  it('the check fires (positive control)', () => {
    const key = Object.keys(EN).find((k) => typeof EN[k as MessageKey] !== 'string')!
    expect(pluralProblems({ [key]: { one: '{{count}} x' } })).toEqual([`${key}: no other form`])
    expect(pluralProblems({ [key]: 'testo' })).toEqual([`${key}: a plain string where English has plural forms`])
  })
})
