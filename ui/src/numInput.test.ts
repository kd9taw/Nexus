import { describe, it, expect } from 'vitest'
import { parseOperatorNumber } from './numInput'

// The Greek-Windows report, 2026-08. Every case below was a real wrong answer somewhere in
// the tree before this helper existed; the positive controls are here so a future
// "simplification" back to `parseFloat` fails loudly instead of passing.

describe('parseOperatorNumber', () => {
  it('reads a comma decimal the way the operator meant it', () => {
    // ⚠️ THE ONE THAT WENT ON THE AIR. `parseFloat('37,98')` is 37 — a latitude off by a
    // hundred kilometres, beaconed by AprsCockpit with no warning.
    expect(parseOperatorNumber('37,98')).toBe(37.98)
    expect(parseOperatorNumber('14,074')).toBe(14.074)
    expect(parseOperatorNumber('-23,5')).toBe(-23.5)
    expect(parseOperatorNumber('88,5')).toBe(88.5)
  })

  it('POSITIVE CONTROL: a dot decimal is untouched', () => {
    // The whole fix is worthless if it breaks the locale that already worked.
    expect(parseOperatorNumber('14.074')).toBe(14.074)
    expect(parseOperatorNumber('37.98')).toBe(37.98)
    expect(parseOperatorNumber('0.6')).toBe(0.6)
    expect(parseOperatorNumber('146')).toBe(146)
    expect(parseOperatorNumber('0')).toBe(0)
  })

  it('trims the surrounding whitespace an input hands back', () => {
    expect(parseOperatorNumber('  14,074  ')).toBe(14.074)
    expect(parseOperatorNumber('\t7.030\n')).toBe(7.03)
  })

  it('refuses empty and blank instead of returning 0', () => {
    // `Number('')` is 0. That is how a blank field — or a `<input type="number">` whose own
    // locale parsing refused the text — committed a memory channel on 0 MHz.
    expect(parseOperatorNumber('')).toBeNaN()
    expect(parseOperatorNumber('   ')).toBeNaN()
  })

  it('refuses a valid PREFIX rather than silently truncating it', () => {
    // The parseFloat failure mode in a different costume: it takes the prefix and reports
    // success. Every one of these must be a refusal, not a number.
    expect(parseOperatorNumber('14.0.74')).toBeNaN()
    expect(parseOperatorNumber('14,0,74')).toBeNaN()
    expect(parseOperatorNumber('14.074 MHz')).toBeNaN()
    expect(parseOperatorNumber('abc')).toBeNaN()
  })

  it('refuses genuinely ambiguous grouped input', () => {
    // `1.234,56` is one-point-two-three-four in one locale and a thousand-plus in another.
    // Guessing is how you transmit the wrong thing; refusing makes the operator say which.
    expect(parseOperatorNumber('1.234,56')).toBeNaN()
    expect(parseOperatorNumber('1,234.56')).toBeNaN()
  })

  it('refuses the non-finite spellings', () => {
    expect(parseOperatorNumber('Infinity')).toBeNaN()
    expect(parseOperatorNumber('-Infinity')).toBeNaN()
    expect(parseOperatorNumber('NaN')).toBeNaN()
  })
})
