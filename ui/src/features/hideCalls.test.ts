// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from 'vitest'
import {
  endOfUtcDay,
  hideCall,
  hideEntryMatches,
  isCallHidden,
  isCallHiddenScoped,
  loadHideCalls,
  loadScopedHides,
  parseHideCalls,
  parseScopedHides,
  unhideScoped,
} from './hideCalls'

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

// #346 — the row menu's narrower hides: on one band, for a while.

describe('#346 scoped call hides', () => {
  const NOW = Date.UTC(2026, 10, 3, 22, 15)
  beforeEach(() => localStorage.clear())

  it('a band hide covers that band only; an every-band hide covers any band, even an unknown one', () => {
    const scoped = [
      { call: 'W1AW', band: '20m', until: null },
      { call: 'K1ABC', band: null, until: null },
    ]
    expect(isCallHiddenScoped('w1aw', '20m', scoped, NOW)).toBe(true)
    expect(isCallHiddenScoped('W1AW', '40m', scoped, NOW)).toBe(false)
    expect(isCallHiddenScoped('W1AW', undefined, scoped, NOW), 'no band known: a band hide cannot apply').toBe(false)
    expect(isCallHiddenScoped('K1ABC', '40m', scoped, NOW)).toBe(true)
    expect(isCallHiddenScoped('K1ABC', null, scoped, NOW)).toBe(true)
    expect(isCallHiddenScoped('N0CALL', '20m', scoped, NOW), 'the control').toBe(false)
  })

  it('a timed hide stops applying at its end, and is dropped on load once over', () => {
    const scoped = [{ call: 'W1AW', band: null, until: NOW + 1000 }]
    expect(isCallHiddenScoped('W1AW', '20m', scoped, NOW)).toBe(true)
    expect(isCallHiddenScoped('W1AW', '20m', scoped, NOW + 1000)).toBe(false)
    expect(parseScopedHides(JSON.stringify(scoped), NOW)).toHaveLength(1)
    expect(parseScopedHides(JSON.stringify(scoped), NOW + 1000)).toEqual([])
  })

  it('drops malformed entries without losing the good ones', () => {
    const raw = JSON.stringify([
      { call: 'W1AW', band: '20m', until: null },
      { call: '', band: null, until: null },
      { call: 'K1ABC', band: 20, until: null },
      { call: 'N1XYZ', band: null, until: 'soon' },
      null,
    ])
    expect(parseScopedHides(raw, NOW)).toEqual([{ call: 'W1AW', band: '20m', until: null }])
    expect(parseScopedHides('not json', NOW)).toEqual([])
  })

  it('"rest of today" ends at the next 00:00 UTC', () => {
    expect(endOfUtcDay(NOW)).toBe(Date.UTC(2026, 10, 4))
    expect(endOfUtcDay(Date.UTC(2026, 11, 31, 23, 59))).toBe(Date.UTC(2027, 0, 1))
  })

  it('every band for good joins the plain list; anything narrower is scoped, and hiding again replaces it', () => {
    hideCall('w1aw', null, null)
    expect(loadHideCalls()).toEqual(['W1AW'])
    expect(loadScopedHides()).toEqual([])
    hideCall('K1ABC', '20m', Date.now() + 3600_000)
    hideCall('K1ABC', '20m', null)
    hideCall('K1ABC', null, Date.now() + 3600_000)
    expect(loadScopedHides().map((h) => [h.call, h.band, h.until === null])).toEqual([
      ['K1ABC', '20m', true],
      ['K1ABC', null, false],
    ])
    unhideScoped('K1ABC', '20m')
    expect(loadScopedHides().map((h) => h.band)).toEqual([null])
  })
})
