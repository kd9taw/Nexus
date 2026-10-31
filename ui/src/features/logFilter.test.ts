// The Logbook's filter shapes, saved filters and multi-key sort (features/logFilter.ts), and the
// reference order's multi-key comparator (logQuery.ts `logOrder`), which the engine's port is held
// to (`a_second_and_third_key_order_ties_in_their_own_direction`).
import { describe, expect, it } from 'vitest'
import {
  activeFilterCount,
  cleanFilter,
  cleanSavedFilters,
  clickSort,
  dayStartUnix,
  filterKey,
  sortPlace,
  unixDay,
  withFilterRenamed,
  withoutFilter,
  withSavedFilter,
  type LogSort,
} from './logFilter'
import { defaultAsc, logOrder, logQueryKey, DEFAULT_LOG_QUERY } from './logQuery'
import type { LoggedQso } from '../types'

describe('cleanFilter', () => {
  it('keeps what the engine takes and drops the rest', () => {
    const f = cleanFilter({
      call: '  K1* ',
      band: '',
      cqZone: 41,
      ituZone: 28,
      continent: 'eu',
      need: 'dxcc',
      qsl: { lotw: 'confirmed', clublog: 'confirmed', card: 'bogus', eqsl: 'notSent' },
      surprise: 'x',
      fromUnix: -1,
      toUnix: 1_700_000_000,
    })
    expect(f).toEqual({
      call: 'K1*',
      ituZone: 28,
      continent: 'EU',
      need: 'dxcc',
      qsl: { lotw: 'confirmed', eqsl: 'notSent' },
      toUnix: 1_700_000_000,
    })
    expect(activeFilterCount(f)).toBe(7)
    expect(cleanFilter('nope')).toEqual({})
    expect(cleanFilter({ continent: 'XX' })).toEqual({})
  })

  it('keeps a path filter the engine knows, and drops one it does not', () => {
    expect(cleanFilter({ path: 'long' })).toEqual({ path: 'long' })
    expect(cleanFilter({ path: 'polar', band: '20m' })).toEqual({ path: 'polar', band: '20m' })
    expect(cleanFilter({ path: 'sideways' })).toEqual({})
    expect(cleanFilter({ path: 1 })).toEqual({})
    expect(activeFilterCount({ path: 'long' })).toBe(1)
  })

  it('names the same filter the same way whatever order its fields came in', () => {
    const a = filterKey({ band: '20m', call: 'K1', qsl: { lotw: 'sent', card: 'confirmed' } })
    const b = filterKey({ qsl: { card: 'confirmed', lotw: 'sent' }, call: 'K1', band: '20m', mode: '' })
    expect(a).toBe(b)
    expect(a).not.toBe(filterKey({ band: '20m', call: 'K1' }))
  })

  it('a query key is unchanged for a query with no filter and one key', () => {
    expect(logQueryKey(DEFAULT_LOG_QUERY)).toBe('time|desc|all|')
    expect(logQueryKey({ ...DEFAULT_LOG_QUERY, filter: {}, then: [] })).toBe('time|desc|all|')
    expect(logQueryKey({ ...DEFAULT_LOG_QUERY, filter: { band: '20m' } })).not.toBe('time|desc|all|')
  })

  it('dates are UTC days', () => {
    expect(dayStartUnix('2024-03-01')).toBe(1_709_251_200)
    expect(unixDay(1_709_251_200)).toBe('2024-03-01')
    expect(dayStartUnix('soon')).toBeUndefined()
  })
})

describe('saved filters', () => {
  it('save, recall, rename, delete — and survive a settings round trip', () => {
    let list = withSavedFilter([], 'DX on 20', { band: '20m', need: 'dxcc' })
    list = withSavedFilter(list, 'Parks', { reference: 'K-' })
    // Saving under a name that exists (any case) replaces it in place.
    list = withSavedFilter(list, 'dx on 20', { band: '20m' })
    expect(list.map((s) => s.name)).toEqual(['dx on 20', 'Parks'])
    expect(list[0].filter).toEqual({ band: '20m' })
    // A rename onto another filter's name is refused.
    expect(withFilterRenamed(list, 'Parks', 'DX ON 20')).toEqual(list)
    list = withFilterRenamed(list, 'Parks', 'POTA hunts')
    expect(list.map((s) => s.name)).toEqual(['dx on 20', 'POTA hunts'])
    const back = cleanSavedFilters(JSON.parse(JSON.stringify(list)))
    expect(back).toEqual(list)
    expect(withoutFilter(back, 'dx on 20').map((s) => s.name)).toEqual(['POTA hunts'])
  })

  it('reads a list another version wrote without trusting it', () => {
    expect(
      cleanSavedFilters([
        { name: 'A', filter: { band: '20m', future: 1 } },
        { name: 'a', filter: {} },
        { name: '  ', filter: {} },
        'junk',
        { name: 7 },
      ]),
    ).toEqual([{ name: 'A', filter: { band: '20m' } }])
    expect(cleanSavedFilters(null)).toEqual([])
  })
})

describe('multi-key sort', () => {
  const start: LogSort = { sort: 'time', asc: false, then: [] }

  it('a click orders by one column; Shift adds up to two more', () => {
    let s = clickSort(start, 'band', false, defaultAsc)
    expect(s).toEqual({ sort: 'band', asc: false, then: [] })
    s = clickSort(s, 'mode', true, defaultAsc)
    s = clickSort(s, 'call', true, defaultAsc)
    expect(s.then).toEqual([
      { sort: 'mode', asc: true },
      { sort: 'call', asc: true },
    ])
    // A fourth is refused; Shift on a key already there flips it.
    expect(clickSort(s, 'grid', true, defaultAsc)).toBe(s)
    expect(clickSort(s, 'mode', true, defaultAsc).then[0]).toEqual({ sort: 'mode', asc: false })
    expect(sortPlace(s, 'call')).toEqual({ place: 3, asc: true })
    // A plain click starts over.
    expect(clickSort(s, 'call', false, defaultAsc)).toEqual({ sort: 'call', asc: true, then: [] })
  })

  const q = (call: string, band: string, mode: string, whenUnix: number) =>
    ({ call, band, mode, whenUnix, freqMhz: band === '20m' ? 14.074 : 7.074 }) as unknown as LoggedQso

  it('the reference order: each key in its own direction, time last in the first key’s', () => {
    const log = [q('K1A', '20m', 'FT8', 30), q('K1B', '40m', 'CW', 20), q('K1C', '20m', 'CW', 10), q('K1D', '40m', 'CW', 40)]
    const order = logOrder(log, {
      sort: 'band',
      asc: false,
      search: '',
      needsConfirmOnly: false,
      then: [{ sort: 'mode', asc: true }],
    })
    // 20 m first (higher frequency, descending); within it CW before FT8; within 40 m CW, the
    // tie broken by time descending.
    expect(order.map((i) => log[i].call)).toEqual(['K1C', 'K1A', 'K1D', 'K1B'])
  })
})
