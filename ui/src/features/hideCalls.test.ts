import { describe, it, expect } from 'vitest'
import { parseHideCalls, hideEntryMatches, isCallHidden } from './hideCalls'

describe('wildcard call-hide (F4MQS)', () => {
  it('parses space/comma/newline lists, normalizing and deduping', () => {
    expect(parseHideCalls('vp8*  k1abc, k1abc\nR0*')).toEqual(['VP8*', 'K1ABC', 'R0*'])
    expect(parseHideCalls('')).toEqual([])
    expect(parseHideCalls(null)).toEqual([])
  })

  it('matches trailing-* prefixes and exact calls, case-insensitively', () => {
    expect(hideEntryMatches('VP8*', 'VP8PJ')).toBe(true)
    expect(hideEntryMatches('VP8*', 'VP9AB')).toBe(false)
    expect(hideEntryMatches('K1ABC', 'K1ABC')).toBe(true)
    expect(hideEntryMatches('K1ABC', 'K1ABCD')).toBe(false) // exact, not prefix
  })

  it('a bare * hides nothing (a stray wildcard must not empty the pane)', () => {
    expect(hideEntryMatches('*', 'ANYCALL')).toBe(false)
    expect(isCallHidden('W1AW', ['*'])).toBe(false)
  })

  it('isCallHidden is any-match, and safe on empty/absent', () => {
    expect(isCallHidden('VP8PJ', ['R0*', 'VP8*'])).toBe(true)
    expect(isCallHidden('W1AW', ['VP8*'])).toBe(false)
    expect(isCallHidden(null, ['VP8*'])).toBe(false)
    expect(isCallHidden('W1AW', [])).toBe(false)
  })
})
