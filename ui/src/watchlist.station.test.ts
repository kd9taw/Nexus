// The watch list as the LISTS read it — the Call Roster, the Classic station list and Spots all
// mark a station on the operator's watch list, and they all ask `watchedEntry` (through
// `watchMatcher`), so a watched station reads the same on every surface.
//
// Two properties are pinned here, and each has a way to fail quietly:
//
//   • IDENTITY ONLY. `cqOnly` and `minSnr` are the ALERT's gates ("only alert on a CQ call",
//     "only alert when SNR ≥"). A station stays on the watch list when it answers somebody or
//     fades a few dB, and a cluster spot carries neither a CQ flag nor an SNR — gated, a roster
//     row would blink its tile off every time the station stopped calling CQ, and Spots could
//     never agree with the roster about the same call. The alert keeps its gates: that half is
//     pinned too, so the shared identity check cannot have loosened `matchWatchlist`.
//
//   • THE MEMO IS KEYED ON THE WHOLE ROW. The same callsign with and without a resolved entity
//     (or a grid) is two different answers; a memo keyed on the call alone would hand the
//     second row the first row's verdict.
import { describe, it, expect } from 'vitest'
import type { DecodeRow } from './types'
import { matchWatchlist, watchedEntry, watchMatcher, type WatchFilter } from './watchlist'

const VP8: WatchFilter = { id: 'c1', kind: 'call', value: 'VP8*' }
const EXACT: WatchFilter = { id: 'c2', kind: 'call', value: 'K1ABC' }
const SUFFIX: WatchFilter = { id: 'c3', kind: 'call', value: '*XYZ' }
const BOUVET: WatchFilter = { id: 'd1', kind: 'dxcc', value: 'Bouvet' }
const FIELD: WatchFilter = { id: 'g1', kind: 'grid', value: 'FN3*' }
const SQUARE: WatchFilter = { id: 'g2', kind: 'grid', value: 'EM79' }
const ALL = [VP8, EXACT, SUFFIX, BOUVET, FIELD, SQUARE]

describe('watchedEntry — which entry puts a station on the watch list', () => {
  it('names a station by exact call, prefix and suffix wildcards (any case)', () => {
    expect(watchedEntry({ call: 'VP8PJ' }, ALL)).toBe(VP8)
    expect(watchedEntry({ call: 'k1abc' }, ALL)).toBe(EXACT)
    expect(watchedEntry({ call: 'W9XYZ' }, ALL)).toBe(SUFFIX)
  })

  it('names a station by its DXCC entity, and never by an unresolved one', () => {
    expect(watchedEntry({ call: '3Y0J', entity: 'Bouvet' }, ALL)).toBe(BOUVET)
    expect(watchedEntry({ call: '3Y0J', entity: 'bouvet ' }, ALL)).toBe(BOUVET)
    expect(watchedEntry({ call: '3Y0J', entity: '' }, ALL)).toBeNull()
    expect(watchedEntry({ call: '3Y0J', entity: null }, ALL)).toBeNull()
  })

  it('names a station by its grid, exact or by field, and never a grid-less row', () => {
    expect(watchedEntry({ call: 'W1AW', grid: 'FN31' }, ALL)).toBe(FIELD)
    expect(watchedEntry({ call: 'N5XX', grid: 'em79' }, ALL)).toBe(SQUARE)
    expect(watchedEntry({ call: 'N5XX', grid: 'EM78' }, ALL)).toBeNull()
    expect(watchedEntry({ call: 'W1AW', grid: null }, ALL)).toBeNull()
    expect(watchedEntry({ call: 'W1AW', grid: '' }, ALL)).toBeNull()
  })

  it('is null for a station no entry names, for no call, and for an empty list', () => {
    expect(watchedEntry({ call: 'G4ABC', entity: 'England', grid: 'IO91' }, ALL)).toBeNull()
    expect(watchedEntry({ call: '', entity: 'Bouvet' }, ALL)).toBeNull()
    expect(watchedEntry({ call: null, entity: 'Bouvet' }, ALL)).toBeNull()
    expect(watchedEntry({ call: 'VP8PJ' }, [])).toBeNull()
  })

  it('returns the FIRST entry that names the station, in list order', () => {
    const both = { call: 'VP8PJ', entity: 'Falkland Islands' }
    const falklands: WatchFilter = { id: 'd2', kind: 'dxcc', value: 'Falkland Islands' }
    expect(watchedEntry(both, [VP8, falklands])).toBe(VP8)
    expect(watchedEntry(both, [falklands, VP8])).toBe(falklands)
  })

  it('ignores the alert-only gates: a CQ-only or minimum-SNR entry still names its station', () => {
    const cqOnly: WatchFilter = { id: 'q', kind: 'call', value: 'VP8*', cqOnly: true }
    const loud: WatchFilter = { id: 's', kind: 'call', value: 'VP8*', minSnr: 0 }
    expect(watchedEntry({ call: 'VP8PJ' }, [cqOnly])).toBe(cqOnly)
    expect(watchedEntry({ call: 'VP8PJ' }, [loud])).toBe(loud)
  })

  it('leaves the ALERT gated exactly as before (the shared identity check did not loosen it)', () => {
    const decode = (over: Partial<DecodeRow>): DecodeRow =>
      ({ from: 'VP8PJ', snr: -20, dtSec: 0, freqHz: 1000, message: '', isCq: false, ...over }) as DecodeRow
    const cqOnly: WatchFilter = { id: 'q', kind: 'call', value: 'VP8*', cqOnly: true }
    const loud: WatchFilter = { id: 's', kind: 'call', value: 'VP8*', minSnr: 0 }
    expect(matchWatchlist(decode({ isCq: false }), [cqOnly])).toBeNull()
    expect(matchWatchlist(decode({ isCq: true }), [cqOnly])).toBe(cqOnly)
    expect(matchWatchlist(decode({ snr: -20 }), [loud])).toBeNull()
    expect(matchWatchlist(decode({ snr: 3 }), [loud])).toBe(loud)
    // …and the identity half is the same one the lists use.
    expect(matchWatchlist(decode({ from: '3Y0J', country: 'Bouvet' }), ALL)).toBe(BOUVET)
    expect(matchWatchlist(decode({ from: 'W1AW', grid: 'FN31' }), ALL)).toBe(FIELD)
  })
})

describe('watchMatcher — the per-row memo the list surfaces share', () => {
  it('answers exactly what watchedEntry answers, row for row', () => {
    const rows = [
      { call: 'VP8PJ' },
      { call: 'K1ABC', entity: 'United States', grid: 'FN42' },
      { call: '3Y0J', entity: 'Bouvet' },
      { call: 'W1AW', grid: 'FN31' },
      { call: 'G4ABC', entity: 'England', grid: 'IO91' },
      { call: '' },
    ]
    const watchOf = watchMatcher(ALL)
    for (const r of rows) {
      expect(watchOf(r), JSON.stringify(r)).toBe(watchedEntry(r, ALL))
      // Asked again — the memoised answer must be the same one.
      expect(watchOf(r), `${JSON.stringify(r)} (again)`).toBe(watchedEntry(r, ALL))
    }
  })

  it('keys the memo on the whole row, not on the call alone', () => {
    const watchOf = watchMatcher(ALL)
    // Same callsign, entity resolved then not: the second answer must not be the first's.
    expect(watchOf({ call: '3Y0J', entity: 'Bouvet' })).toBe(BOUVET)
    expect(watchOf({ call: '3Y0J', entity: '' })).toBeNull()
    // Same callsign, grid heard then not.
    expect(watchOf({ call: 'W1AW', grid: 'FN31' })).toBe(FIELD)
    expect(watchOf({ call: 'W1AW', grid: null })).toBeNull()
  })

  it('reads the list it was built from: a new list is a new matcher', () => {
    expect(watchMatcher([VP8])({ call: 'VP8PJ' })).toBe(VP8)
    expect(watchMatcher([])({ call: 'VP8PJ' })).toBeNull()
  })
})
