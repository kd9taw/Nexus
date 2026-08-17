// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from 'vitest'
import type { DecodeRow } from './types'
import {
  matchCallPattern,
  matchWatchlist,
  watchLabel,
  loadWatchlist,
  saveWatchlist,
  newWatchFilter,
  type WatchFilter,
} from './watchlist'

const decode = (over: Partial<DecodeRow>): DecodeRow => ({
  from: 'W1ABC',
  snr: 0,
  dtSec: 0,
  freqHz: 1000,
  message: 'CQ W1ABC FN42',
  isCq: true,
  directedToMe: false,
  worked: false,
  country: undefined,
  tier: 'FT8',
  rv: 0,
  ...over,
})

describe('matchCallPattern', () => {
  it('matches exact and wildcard calls (case-insensitive)', () => {
    expect(matchCallPattern('K1ABC', 'k1abc')).toBe(true)
    expect(matchCallPattern('VP8DXA', 'VP8*')).toBe(true) // prefix
    expect(matchCallPattern('W1ABC', '*ABC')).toBe(true) // suffix
    expect(matchCallPattern('3Y0J', '3Y0*')).toBe(true) // Bouvet
    expect(matchCallPattern('K1ABC', 'K2*')).toBe(false)
    expect(matchCallPattern('W1ABC', 'W1ABD')).toBe(false)
  })
  it('treats regex metachars in the pattern literally (only * is a wildcard)', () => {
    expect(matchCallPattern('A.B', 'A.B')).toBe(true)
    expect(matchCallPattern('AXB', 'A.B')).toBe(false)
  })
})

describe('matchWatchlist', () => {
  const call: WatchFilter = { id: '1', kind: 'call', value: 'VP8*', label: 'Falklands' }
  const dxcc: WatchFilter = { id: '2', kind: 'dxcc', value: 'Bouvet' }

  it('matches a wildcard call and a DXCC entity, first-match-wins', () => {
    expect(matchWatchlist(decode({ from: 'VP8DXA' }), [call, dxcc])).toBe(call)
    expect(matchWatchlist(decode({ from: '3Y0J', country: 'Bouvet' }), [call, dxcc])).toBe(dxcc)
    expect(matchWatchlist(decode({ from: 'W1ABC', country: 'United States' }), [call, dxcc])).toBeNull()
  })

  it('respects cqOnly and minSnr gates', () => {
    const cqOnly: WatchFilter = { id: '3', kind: 'call', value: 'W1ABC', cqOnly: true }
    expect(matchWatchlist(decode({ isCq: false }), [cqOnly])).toBeNull()
    expect(matchWatchlist(decode({ isCq: true }), [cqOnly])).toBe(cqOnly)

    const strong: WatchFilter = { id: '4', kind: 'call', value: 'W1ABC', minSnr: -5 }
    expect(matchWatchlist(decode({ snr: -20 }), [strong])).toBeNull()
    expect(matchWatchlist(decode({ snr: 0 }), [strong])).toBe(strong)
  })

  it('is null for an empty call or an empty list', () => {
    expect(matchWatchlist(decode({ from: undefined }), [call])).toBeNull()
    expect(matchWatchlist(decode({}), [])).toBeNull()
  })

  it('matches a watched grid square, exact and by field prefix', () => {
    const exact: WatchFilter = { id: 'g1', kind: 'grid', value: 'FN42' }
    const field: WatchFilter = { id: 'g2', kind: 'grid', value: 'EM7*' }
    expect(matchWatchlist(decode({ grid: 'FN42' }), [exact])).toBe(exact)
    expect(matchWatchlist(decode({ grid: 'fn42' }), [exact])).toBe(exact)
    expect(matchWatchlist(decode({ grid: 'FN43' }), [exact])).toBeNull()
    expect(matchWatchlist(decode({ grid: 'EM79' }), [field])).toBe(field)
    expect(matchWatchlist(decode({ grid: 'EM89' }), [field])).toBeNull()
  })

  it('a grid-less decode never matches a grid watch — unknown is not a hit', () => {
    // Most FT8 frames (reports, RRR, 73) carry no grid by protocol; only CQ and the
    // first reply announce the square. Firing on "unknown" would alert on every frame.
    const exact: WatchFilter = { id: 'g1', kind: 'grid', value: 'FN42' }
    expect(matchWatchlist(decode({ grid: undefined }), [exact])).toBeNull()
    expect(matchWatchlist(decode({ grid: '' }), [exact])).toBeNull()
  })

  it('watchLabel prefers the friendly label, falls back to the value', () => {
    expect(watchLabel(call)).toBe('Falklands')
    expect(watchLabel(dxcc)).toBe('Bouvet')
    expect(watchLabel({ id: '5', kind: 'call', value: 'k1abc' })).toBe('K1ABC')
    expect(watchLabel({ id: 'g1', kind: 'grid', value: 'fn42' })).toBe('grid FN42')
  })
})

describe('persistence', () => {
  beforeEach(() => localStorage.clear())

  it('round-trips through localStorage and drops malformed entries', () => {
    // The grid entry pins loadWatchlist's kind validator: it hard-codes the accepted
    // kinds, so a new kind that misses it would save fine and then be SILENTLY dropped
    // on the next launch — the one way this feature fails quietly.
    const list = [
      newWatchFilter('call', 'VP8*', { label: 'Falklands' }),
      newWatchFilter('dxcc', 'Bouvet'),
      newWatchFilter('grid', 'EM7*'),
    ]
    saveWatchlist(list)
    expect(loadWatchlist()).toEqual(list)
  })

  it('returns [] on missing or corrupt storage', () => {
    expect(loadWatchlist()).toEqual([])
    localStorage.setItem('nexus.watchlist', 'not json')
    expect(loadWatchlist()).toEqual([])
    localStorage.setItem('nexus.watchlist', JSON.stringify([{ bogus: true }]))
    expect(loadWatchlist()).toEqual([])
  })
})
