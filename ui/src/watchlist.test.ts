// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from 'vitest'
import {
  matchCallPattern,
  watchLabel,
  loadWatchlist,
  saveWatchlist,
  newWatchFilter,
  type WatchFilter,
} from './watchlist'

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

describe('watchLabel', () => {
  const call: WatchFilter = { id: '1', kind: 'call', value: 'VP8*', label: 'Falklands' }
  const dxcc: WatchFilter = { id: '2', kind: 'dxcc', value: 'Bouvet' }

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

// #390 — every entry can carry a note: why it is on the list, and when it can come off. The note
// rides in the list the app already keeps (`nexus.watchlist`, mirrored to ui-state.json), so a list
// saved before notes existed must read back exactly as it was written, and a note must survive.
describe('persistence: the note on each entry (#390)', () => {
  beforeEach(() => localStorage.clear())

  it('reads a list saved before notes existed back exactly as it was written', () => {
    const raw = JSON.stringify([
      { id: 'call-VP8*-a1b2c3', kind: 'call', value: 'VP8*', cqOnly: true, minSnr: -10, label: 'Falklands' },
      { id: 'dxcc-Bouvet-d4e5f6', kind: 'dxcc', value: 'Bouvet' },
      { id: 'grid-EM7*-g7h8i9', kind: 'grid', value: 'EM7*' },
    ])
    localStorage.setItem('nexus.watchlist', raw)
    expect(JSON.stringify(loadWatchlist())).toBe(raw)
  })

  it('keeps an entry’s note through a save and a load', () => {
    const list = [newWatchFilter('call', '5W1SA', { notes: 'Samoa DXp 9/27-10/3' }), newWatchFilter('dxcc', 'Bouvet')]
    saveWatchlist(list)
    expect(loadWatchlist()).toEqual(list)
    expect(loadWatchlist()[0].notes).toBe('Samoa DXp 9/27-10/3')
  })

  it('drops a note that is not text, and never the entry it rides on', () => {
    localStorage.setItem(
      'nexus.watchlist',
      JSON.stringify([
        { id: 'call-5W-1', kind: 'call', value: '5W1SA', notes: 42 },
        { id: 'call-3Y-2', kind: 'call', value: '3Y0J', cqOnly: true, notes: { why: 'Bouvet' } },
      ]),
    )
    expect(loadWatchlist()).toStrictEqual([
      { id: 'call-5W-1', kind: 'call', value: '5W1SA' },
      { id: 'call-3Y-2', kind: 'call', value: '3Y0J', cqOnly: true },
    ])
  })
})
