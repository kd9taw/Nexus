// THE LOGBOOK'S ORDER, MOVED OUT OF THE VIEW WITHOUT CHANGING IT (SPEC-2 v3 C17b).
//
// `logQuery.ts` is the Logbook's filter-and-sort, lifted out of the component so a page source can
// run it. "Lifted verbatim" is a claim, so it is tested: `ORACLE` below is the component's own code
// as it stood before the move (Logbook.tsx at d5be14ea — `fmtUtc` :214, `sortVal` :263, the search
// :933, the sort :958), pasted unchanged, and the extracted `logOrder` must agree with it on the
// fixture log and on thousands of random logs, for every column, both directions, every search.
// One rule in it has moved since, decided rather than ported: the country column sorts on the
// country the Logbook shows, the DXCC entity, and the stored text only where the call has none.
// Two more were decided on 2026-10-27, to match the columns added since: text is read trimmed, and a
// contact with nothing in the column sorts after the rest in either direction (`decided` below).

import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import type { LoggedQso } from '../types'
import { modeKey } from './callHistory'
import { defaultAsc, logOrder, matchesLogQuery, type LogQuery } from './logQuery'

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
      return (q.entity ?? q.country ?? '').toUpperCase()
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
    const av = decided(sortVal(a.q, sortKey))
    const bv = decided(sortVal(b.q, sortKey))
    if (av === null || bv === null) {
      if (av !== bv) return av === null ? 1 : -1
      return sortAsc ? a.q.whenUnix - b.q.whenUnix : b.q.whenUnix - a.q.whenUnix
    }
    const cmp = av < bv ? -1 : av > bv ? 1 : a.q.whenUnix - b.q.whenUnix
    return sortAsc ? cmp : -cmp
  })
  return out
}
// ---- end of ORACLE ----
/** The rules decided since the move, not ported: text trimmed, blank text nothing (null). */
function decided(v: string | number): string | number | null {
  return typeof v === 'string' ? v.trim() || null : v
}

const FIXTURE = JSON.parse(
  readFileSync(new URL('./__fixtures__/log-query/log.json', import.meta.url), 'utf8'),
) as LoggedQso[]
// The columns the oracle knows; the keys added since have no pre-move code to agree with.
const SORTS: SortKey[] = ['call', 'country', 'band', 'freq', 'mode', 'sent', 'rcvd', 'time', 'park', 'qsl']
const SEARCHES = ['', 'k1a', 'dl', 'jo31', '2024-05', 'z', 'ssb', 'usb', '09-19', '14:2', '20m', ' W1 ', 'åland', 'ß', 'ſ', 'I', 'ı']

function agree(log: LoggedQso[], query: LogQuery & { sort: SortKey }): void {
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
      const query: LogQuery & { sort: SortKey } = {
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

describe('the search looks in the SUBMODE as well as the MODE', () => {
  it('finds a phone contact held as SSB by its sideband, and ssb finds all phone', () => {
    const row = (call: string, mode: string, submode?: string) =>
      ({ ...FIXTURE[0], call, mode, extra: submode ? [['SUBMODE', submode]] : [] }) as LoggedQso
    const log = [
      row('K1AAA', 'SSB', 'USB'),
      row('K2BBB', 'SSB', 'LSB'),
      row('K3CCC', 'SSB'),
      row('K4DDD', 'FT8'),
      row('K5EEE', 'DYNAMIC', 'VARA HF'), // as VarAC writes it
    ]
    const found = (search: string) => logOrder(log, { sort: 'call', asc: true, search, needsConfirmOnly: false }).map((i) => log[i].call)
    expect(found('usb')).toEqual(['K1AAA'])
    expect(found('lsb')).toEqual(['K2BBB'])
    expect(found('ssb')).toEqual(['K1AAA', 'K2BBB', 'K3CCC'])
    expect(found('vara')).toEqual(['K5EEE'])
  })
})

describe('the country column sorts on the country the Logbook shows', () => {
  it('sorts a QRZ Germany with Fed. Rep. of Germany, and by the stored text only without an entity', () => {
    const row = (call: string, entity: string | null, country: string, i: number) =>
      ({ ...FIXTURE[0], call, entity, country, whenUnix: FIXTURE[0].whenUnix + i * 60 }) as LoggedQso
    const log = [
      row('DL1ABC', 'Fed. Rep. of Germany', 'Germany', 0),
      row('F5ABC', 'France', 'France', 1),
      row('DL2XYZ', 'Fed. Rep. of Germany', 'Fed. Rep. of Germany', 2),
      row('G4ABC', 'England', 'England', 3),
      row('Q0ZZZ', null, 'Freedonia', 4), // no entity: the stored text decides
    ]
    const sorted = (asc: boolean) => logOrder(log, { sort: 'country', asc, search: '', needsConfirmOnly: false }).map((i) => log[i].call)
    expect(sorted(true)).toEqual(['G4ABC', 'DL1ABC', 'DL2XYZ', 'F5ABC', 'Q0ZZZ'])
    expect(sorted(false)).toEqual(['Q0ZZZ', 'F5ABC', 'DL2XYZ', 'DL1ABC', 'G4ABC'])
  })
})

describe('the search finds a contact by the country the Logbook shows', () => {
  it('finds a QRZ Germany by its entity, Fed. Rep. of Germany, and still by the stored text', () => {
    const row = (call: string, entity: string | null, country: string, i: number) =>
      ({ ...FIXTURE[0], call, entity, country, whenUnix: FIXTURE[0].whenUnix + i * 60 }) as LoggedQso
    const log = [
      row('DL1ABC', 'Fed. Rep. of Germany', 'Germany', 0),
      row('TA1XX', 'Turkey', 'Türkiye', 1),
      row('G4ABC', 'England', 'England', 2),
      row('Q0ZZZ', null, 'Freedonia', 3),
    ]
    for (const sort of ['call', 'country', 'time'] as const) {
      const found = (search: string) => logOrder(log, { sort, asc: true, search, needsConfirmOnly: false }).map((i) => log[i].call)
      expect(found('fed. rep'), sort).toEqual(['DL1ABC'])
      expect(found('TURKEY'), sort).toEqual(['TA1XX'])
      // The stored text still finds its contact.
      expect(found('germany'), sort).toEqual(['DL1ABC'])
      expect(found('türkiye'), sort).toEqual(['TA1XX'])
      expect(found('freedonia'), sort).toEqual(['Q0ZZZ'])
      expect(found('nowhere'), sort).toEqual([])
    }
  })
})

describe('every column the Logbook shows sorts, a contact with nothing in it last both ways', () => {
  // One table both sides are held to: the engine (`src-tauri/src/log_queries/tests.rs`
  // `every_added_column_sorts_as_the_shared_table_says`) reads the same file.
  const SHARED = JSON.parse(
    readFileSync(new URL('./__fixtures__/log-query/sort-columns.json', import.meta.url), 'utf8'),
  ) as { cases: { name: string; rows: LoggedQso[]; orders: (Partial<LogQuery> & { expect: string[] })[] }[] }
  const base = (call: string, i: number) =>
    ({ ...FIXTURE[0], call, whenUnix: 1_700_000_000 + i * 60, name: null, qth: null, comment: null, notes: null,
      myGrid: null, myRig: null, operator: null, txPower: null, timeOffUnix: null, qslSent: { sent: false },
      upload: undefined, contest: undefined, extra: [] }) as unknown as LoggedQso
  const logWith = (set: (q: LoggedQso, i: number) => object) =>
    ['AAA', 'BBB', 'CCC', 'EEE'].map((c, i) => (i < 3 ? (set(base(c, i), i) as LoggedQso) : base(c, i)))
  const order = (log: LoggedQso[], query: Partial<LogQuery>) =>
    logOrder(log, { sort: 'time', asc: true, search: '', needsConfirmOnly: false, ...query }).map((i) => log[i].call)

  it('each as the shared table says: by value, both ways, the empty contact last both ways', () => {
    // Every key but the four that always hold a value (band, frequency, time, QSL mark) and the
    // call, which names the rows here.
    const keys = new Set(SHARED.cases.flatMap((c) => c.orders.map((o) => o.sort)))
    expect(keys.size).toBe(32)
    for (const c of SHARED.cases)
      for (const { expect: want, ...query } of c.orders)
        expect(order(c.rows, query), `${c.name}: ${query.sort} ${query.asc ? 'ascending' : 'descending'}`).toEqual(want)
  })

  it('an upload outcome this build does not know is marked failed, and sorts so', () => {
    const log = logWith((q, i) => ({ ...q, upload: { lotw: { outcome: ['retry-later', 'accepted', 'pending'][i], whenUnix: 1 } } }))
    expect(order(log, { sort: 'upLotw', asc: true })).toEqual(['AAA', 'CCC', 'BBB', 'EEE'])
  })

  it('a blank value is empty, and the empties keep the time order in the first key’s direction', () => {
    const log = logWith((q, i) => ({ ...q, name: ['   ', 'bob', ''][i] }))
    expect(order(log, { sort: 'name', asc: true })).toEqual(['BBB', 'AAA', 'CCC', 'EEE'])
    expect(order(log, { sort: 'name', asc: false })).toEqual(['BBB', 'EEE', 'CCC', 'AAA'])
    const zones = logWith((q, i) => ({ ...q, extra: [['CQZ', ['x', '5', ' '][i]]] }))
    // A zone that is not a number shows as it is, so it sorts after the numbers, before the blank.
    expect(order(zones, { sort: 'cqz', asc: false })).toEqual(['BBB', 'AAA', 'EEE', 'CCC'])
  })

  it('as a shift-click key: a tie in the first ordered by the second, its empty last', () => {
    const log = logWith((q, i) => ({ ...q, extra: [['CONT', ['EU', 'EU', 'NA'][i]]], txPower: [5, 100, 50][i] }))
    expect(order(log, { sort: 'cont', asc: true, then: [{ sort: 'power', asc: false }] })).toEqual(['BBB', 'AAA', 'CCC', 'EEE'])
    expect(order(log, { sort: 'cont', asc: true, then: [{ sort: 'power', asc: true }] })).toEqual(['AAA', 'BBB', 'CCC', 'EEE'])
  })

  it('times and dates open newest first; the other added columns open ascending', () => {
    for (const k of ['end', 'qslSentDate', 'qslRcvdDate', 'lotwRcvdDate', 'eqslRcvdDate'] as const) expect(defaultAsc(k), k).toBe(false)
    for (const k of ['name', 'power', 'cqz', 'upLotw', 'notes', 'exchSent'] as const) expect(defaultAsc(k), k).toBe(true)
  })
})
