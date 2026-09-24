// THE LOGBOOK'S ORDER, MOVED OUT OF THE VIEW WITHOUT CHANGING IT (SPEC-2 v3 C17b).
//
// `logQuery.ts` is the Logbook's filter-and-sort, lifted out of the component so a page source can
// run it. "Lifted verbatim" is a claim, so it is tested: `ORACLE` below is the component's own code
// as it stood before the move (Logbook.tsx at d5be14ea — `fmtUtc` :214, `sortVal` :263, the search
// :933, the sort :958), pasted unchanged, and the extracted `logOrder` must agree with it on the
// fixture log and on thousands of random logs, for every column, both directions, every search.

import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import type { LoggedQso } from '../types'
import { modeKey } from './callHistory'
import { defaultAsc, logOrder, matchesLogQuery, type LogQuery, type LogSortKey } from './logQuery'

// ---- ORACLE: Logbook.tsx at d5be14ea, verbatim (hooks unwrapped to plain functions) ----
function fmtUtc(whenUnix: number): string {
  const d = new Date(whenUnix * 1000)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getUTCFullYear()}-${p(d.getUTCMonth() + 1)}-${p(d.getUTCDate())} ${p(
    d.getUTCHours(),
  )}:${p(d.getUTCMinutes())}Z`
}
type SortKey = 'call' | 'country' | 'band' | 'freq' | 'mode' | 'sent' | 'rcvd' | 'time' | 'park' | 'qsl'
function sortVal(q: LoggedQso, k: SortKey): string | number {
  switch (k) {
    case 'call':
      return q.call.toUpperCase()
    case 'country':
      return (q.country ?? '').toUpperCase()
    case 'band':
    case 'freq':
      return q.freqMhz
    case 'mode':
      return q.mode.toUpperCase()
    case 'sent':
      return (q.rstSent ?? '').toUpperCase()
    case 'rcvd':
      return (q.rstRcvd ?? '').toUpperCase()
    case 'time':
      return q.whenUnix
    case 'park':
      return (q.ota?.theirRef ?? q.ota?.myRef ?? '').toUpperCase()
    case 'qsl':
      return q.awardConfirmed ? 2 : q.confirmed ? 1 : 0
  }
}
function oracleDefaultAsc(k: SortKey): boolean {
  return k === 'call' || k === 'country' || k === 'mode' || k === 'sent' || k === 'rcvd' || k === 'park'
}
function ORACLE(log: LoggedQso[], deferredSearch: string, needsConfirmOnly: boolean, sortKey: SortKey, sortAsc: boolean) {
  const control = true
  const matchesSearch = (q: LoggedQso): boolean => {
    if (needsConfirmOnly && q.awardConfirmed) return false
    const t = deferredSearch.trim().toLowerCase()
    if (!t) return true
    return (
      q.call.toLowerCase().includes(t) ||
      (q.country?.toLowerCase().includes(t) ?? false) ||
      (q.grid?.toLowerCase().includes(t) ?? false) ||
      q.band.toLowerCase().includes(t) ||
      q.mode.toLowerCase().includes(t) ||
      modeKey(q.mode).toLowerCase().includes(t) ||
      fmtUtc(q.whenUnix).toLowerCase().includes(t)
    )
  }
  const out = log.map((q, i) => ({ q, i })).filter(({ q }) => !control || matchesSearch(q))
  out.sort((a, b) => {
    const av = sortVal(a.q, sortKey)
    const bv = sortVal(b.q, sortKey)
    const cmp = av < bv ? -1 : av > bv ? 1 : a.q.whenUnix - b.q.whenUnix
    return sortAsc ? cmp : -cmp
  })
  return out
}
// ---- end of ORACLE ----

const FIXTURE = JSON.parse(
  readFileSync(new URL('./__fixtures__/log-query/log.json', import.meta.url), 'utf8'),
) as LoggedQso[]
const SORTS: LogSortKey[] = ['call', 'country', 'band', 'freq', 'mode', 'sent', 'rcvd', 'time', 'park', 'qsl']
const SEARCHES = ['', 'k1a', 'dl', 'jo31', '2024-05', 'z', 'ssb', 'usb', '09-19', '14:2', '20m', ' W1 ', 'åland', 'ß', 'ſ', 'I', 'ı']

function agree(log: LoggedQso[], query: LogQuery): void {
  const want = ORACLE(log, query.search, query.needsConfirmOnly, query.sort, query.asc).map(({ i }) => i)
  expect(logOrder(log, query), JSON.stringify(query)).toEqual(want)
}

/** A seeded PRNG (mulberry32), so a failure reproduces from its seed. */
function rng(seed: number) {
  let a = seed >>> 0
  return () => {
    a = (a + 0x6d2b79f5) >>> 0
    let t = a
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

/** Random rows drawn from small pools, so ties — on every key, and on the time — are common. */
function randomLog(seed: number, n: number): LoggedQso[] {
  const r = rng(seed)
  const pick = <T,>(xs: readonly T[]): T => xs[Math.floor(r() * xs.length)]
  const text = ['', 'w1aw', 'W1AW', 'K1ABC', 'dſ1x', 'Åland', 'straße', 'STRASSE', 'ı', 'I', 'z', 'Zz', '-10', '+05', '599']
  return Array.from({ length: n }, (_, i) => ({
    id: pick([`r${i}`, null]),
    call: pick(text),
    grid: pick([null, '', 'FN31', 'fn31pr', 'JO31', ' en52 ']),
    country: pick([null, ...text]),
    band: pick(['20m', '20M', '40m', '', '2m']),
    freqMhz: pick([14.074, 7.074, 0, 144.174, 14.074]),
    mode: pick(['FT8', 'ft8', 'USB', 'LSB', 'SSB', 'CW', 'BPSK31']),
    rstSent: pick([null, ...text]),
    rstRcvd: pick([null, ...text]),
    whenUnix: pick([0, 1_700_000_000, 1_700_000_060, 1_695_115_140, 1_715_350_800]),
    confirmed: r() < 0.5,
    awardConfirmed: r() < 0.3,
    ota: pick([null, undefined, { theirRef: 'US-1234' }, { myRef: 'k-0001' }, { theirRef: null, myRef: 'VK-1' }, {}]),
  })) as unknown as LoggedQso[]
}

describe('logOrder is the Logbook’s own order, moved', () => {
  it('agrees with the component’s code on the fixture: every column, both directions, every search', () => {
    for (const sort of SORTS)
      for (const asc of [true, false])
        for (const search of SEARCHES)
          for (const needsConfirmOnly of [false, true]) agree(FIXTURE, { sort, asc, search, needsConfirmOnly })
  })

  it('agrees on 400 random logs full of ties (seeded)', () => {
    for (let seed = 1; seed <= 400; seed++) {
      const log = randomLog(seed, 1 + (seed % 60))
      const r = rng(seed * 7)
      const query: LogQuery = {
        sort: SORTS[Math.floor(r() * SORTS.length)],
        asc: r() < 0.5,
        search: SEARCHES[Math.floor(r() * SEARCHES.length)],
        needsConfirmOnly: r() < 0.3,
      }
      agree(log, query)
    }
  })

  it('keeps the header’s default directions', () => {
    for (const k of SORTS) expect(defaultAsc(k), k).toBe(oracleDefaultAsc(k))
  })

  it('the filter is the component’s filter (the search trims and lower-cases; the chip drops confirmed rows)', () => {
    const row = FIXTURE[0] // W1AW, award-confirmed, 2024-05-10 14:20Z
    const at = (over: Partial<LogQuery>) => matchesLogQuery(row, { sort: 'time', asc: false, search: '', needsConfirmOnly: false, ...over })
    expect(at({ search: '  W1a ' })).toBe(true)
    expect(at({ search: '14:20z' })).toBe(true)
    expect(at({ search: 'nothing' })).toBe(false)
    expect(at({ needsConfirmOnly: true })).toBe(false)
  })
})
