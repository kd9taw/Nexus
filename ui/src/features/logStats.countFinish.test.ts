// THE STATISTICS, COUNTED BY THE ENGINE AND ORDERED BY THE WINDOW (SPEC-2 v3 C17a).
//
// The Statistics dashboard is a roll-up of the log. Its counting moves to the engine; its ORDERING
// cannot: ties are broken by `localeCompare`, which orders labels as the webview's locale does, and
// the top twelve entities are cut after that order. So `computeLogStats` is now `countLogStats`
// then `finishLogStats`, the engine answers the `statistics` question with the counts, and
// `askLog` finishes them in the window.
//
// Pinned here, against an ORACLE — the whole of today's `computeLogStats`, copied verbatim from
// before the split:
//   1. count-then-finish is today's roll-up, on logs built to break it: tied counts everywhere,
//      labels that differ only in case or accents, full Unicode case maps, date-only contacts,
//      instants no `Date` holds, more than twelve entities;
//   2. the same, with the counts sent through JSON first (the IPC hop);
//   3. the fixtures are not vacuous: ordering their ties by code units instead of the locale
//      would change the answer;
//   4. the counts the engine is held to (`__fixtures__/log-query/statistics.json`, read by the
//      Rust port) are today's, and finish to the golden answer the goldens froze. Regenerate with
//      the other log-query goldens:  NEXUS_WRITE_LOG_GOLDENS=1 npx vitest run src/features/logStats.countFinish.test.ts

import { describe, expect, it } from 'vitest'
import { readFileSync, writeFileSync } from 'node:fs'
import type { LoggedQso } from '../types'
import { computeLogStats, countLogStats, finishLogStats, type LogStatCounts, type LogStats, type Tally } from './logStats'

// ---- ORACLE: today's computeLogStats, verbatim, from before the count/finish split ----

function tallyBy(log: LoggedQso[], key: (q: LoggedQso) => string | null | undefined): Map<string, number> {
  const m = new Map<string, number>()
  for (const q of log) {
    const k = key(q)?.trim()
    if (k) m.set(k, (m.get(k) ?? 0) + 1)
  }
  return m
}
function phoneModeLabel(mode: string | null | undefined): string {
  const m = (mode ?? '').trim()
  const u = m.toUpperCase()
  return u === 'USB' || u === 'LSB' ? 'SSB' : m
}
function tallyByCI(log: LoggedQso[], key: (q: LoggedQso) => string | null | undefined): Map<string, number> {
  const counts = new Map<string, number>()
  const labels = new Map<string, string>()
  for (const q of log) {
    const raw = key(q)?.trim()
    if (!raw) continue
    const u = raw.toUpperCase()
    counts.set(u, (counts.get(u) ?? 0) + 1)
    if (!labels.has(u)) labels.set(u, raw)
  }
  const out = new Map<string, number>()
  for (const [u, c] of counts) out.set(labels.get(u) ?? u, c)
  return out
}
const US_ENTITIES = new Set(['UNITED STATES', 'ALASKA', 'HAWAII'])
const WAS_STATES = new Set([
  'AK', 'AL', 'AR', 'AZ', 'CA', 'CO', 'CT', 'DE', 'FL', 'GA', 'HI', 'IA', 'ID', 'IL', 'IN', 'KS',
  'KY', 'LA', 'MA', 'MD', 'ME', 'MI', 'MN', 'MO', 'MS', 'MT', 'NC', 'ND', 'NE', 'NH', 'NJ', 'NM',
  'NV', 'NY', 'OH', 'OK', 'OR', 'PA', 'RI', 'SC', 'SD', 'TN', 'TX', 'UT', 'VA', 'VT', 'WA', 'WI',
  'WV', 'WY',
])
function wasState(q: LoggedQso): string | null {
  if (!US_ENTITIES.has(q.country?.trim().toUpperCase() ?? '')) return null
  const code = q.state?.trim().toUpperCase()
  return code && WAS_STATES.has(code) ? code : null
}
const compareTallies = (a: Tally, b: Tally): number => b.count - a.count || a.label.localeCompare(b.label)
function byCountDesc(m: Map<string, number>): Tally[] {
  return [...m.entries()]
    .map(([label, count]) => ({ label, count }))
    .sort(compareTallies)
}
function oracleLogStats(log: LoggedQso[]): LogStats {
  const hourUtc = new Array(24).fill(0) as number[]
  let hourUnknown = 0
  const calls = new Set<string>()
  const countries = new Set<string>()
  let confirmed = 0
  let awardConfirmed = 0
  const qsl = { card: 0, lotw: 0, eqsl: 0 }

  for (const q of log) {
    calls.add(q.call.trim().toUpperCase())
    const c = (q.entity ?? q.country)?.trim()
    if (c) countries.add(c.toUpperCase())
    if (q.confirmed) confirmed++
    if (q.awardConfirmed) awardConfirmed++
    if (q.qslRcvd?.card) qsl.card++
    if (q.qslRcvd?.lotw) qsl.lotw++
    if (q.qslRcvd?.eqsl) qsl.eqsl++
    if (Number.isFinite(q.whenUnix)) {
      if (q.whenUnix % 86400 === 0) {
        hourUnknown++
      } else {
        const h = new Date(q.whenUnix * 1000).getUTCHours()
        if (h >= 0 && h < 24) hourUtc[h]++
      }
    }
  }

  const byYear = [...tallyBy(log, (q) => {
    if (!Number.isFinite(q.whenUnix)) return null
    const y = new Date(q.whenUnix * 1000).getUTCFullYear() // NaN for an out-of-range timestamp
    return Number.isFinite(y) ? String(y) : null
  }).entries()]
    .map(([label, count]) => ({ label, count }))
    .sort((a, b) => a.label.localeCompare(b.label)) // chronological

  const entities = byCountDesc(tallyByCI(log, (q) => q.entity ?? q.country))

  return {
    total: log.length,
    uniqueCalls: calls.size,
    confirmed,
    awardConfirmed,
    dxccEntities: countries.size,
    byBand: byCountDesc(tallyBy(log, (q) => q.band)),
    byMode: byCountDesc(tallyBy(log, (q) => phoneModeLabel(q.mode))),
    byYear,
    byState: byCountDesc(tallyBy(log, wasState)),
    topEntities: entities.slice(0, 12),
    hourUtc,
    hourUnknown,
    qsl,
  }
}

// ---- the logs ----

const FIXTURES = new URL('./__fixtures__/log-query/', import.meta.url)
const GOLDEN_LOG = JSON.parse(readFileSync(new URL('log.json', FIXTURES), 'utf8')) as LoggedQso[]
const COUNTS_URL = new URL('statistics.json', FIXTURES)

// Labels that tie, and differ only in case or accents — which the locale and code units order
// differently ('20m' before '20M' in the locale, after it by code units; 'Åland…' beside 'Aland…'
// in the locale, after 'Zambia' by code units).
const BANDS = ['20m', '20M', '40m', ' 40m ', '15m', '15M', '', '  ', '6m', '2m']
const MODES = ['SSB', 'ssb', 'USB', 'lsb', 'uſb', 'Usb', 'FT8', 'ft8', 'CW', 'cw', 'FM', ' FT4 ', 'ſſb', '']
const ENTITIES: (string | null)[] = [
  'Åland Islands', 'Aland Islands', 'aland islands', 'Réunion', 'Reunion', 'Curaçao', 'CURAÇAO', 'São Tomé', 'Sao Tome',
  'Zambia', 'zambia', 'Émile Land', 'Ängland', 'United States', 'UNITED STATES', 'Japan', '', null,
]
const COUNTRIES: (string | null)[] = ['United States', 'united states', ' ALASKA ', 'Hawaii', 'Australia', 'Japan', null, '']
const STATES: (string | null)[] = ['wi', 'WI', ' ct ', 'Ca', 'XX', 'wa', 'ſc', '', null]
const CALLS = [
  'W1AW', 'w1aw', ' W1AW ', 'ß1AA', 'ss1aa', 'K1ABC', 'k1abc', 'JA1XYZ', 'VE3ABC', 'DL1ABC', '',
  ...Array.from({ length: 30 }, (_, k) => `K${k}XY`),
]
const WHENS = [
  1_700_000_000, 1_600_000_123, 1_500_000_456, 1_704_067_200 /* midnight */, 1_262_304_000 /* midnight */,
  8_640_000_086_400 /* midnight, past the range a Date holds */, 9_000_000_000_001 /* past it */, 1_234_567_890,
]

/** A log built to break the port, deterministic (the Rust port reads it as written): every label
 *  above, each list walked in step with the row so that labels tie by construction — `n` a multiple
 *  of every list's length — with date-only contacts and instants no `Date` can hold. One entity per
 *  call, as the engine resolves them. */
function tiedLog(n: number): LoggedQso[] {
  return Array.from({ length: n }, (_, i) => {
    const c = i % CALLS.length
    const card = i % 5 === 0
    const lotw = i % 7 === 0
    const eqsl = i % 3 === 0
    return {
      ...GOLDEN_LOG[i % GOLDEN_LOG.length],
      id: `st${String(i).padStart(3, '0')}`,
      call: CALLS[c],
      entity: ENTITIES[c % ENTITIES.length],
      country: COUNTRIES[(i * 3) % COUNTRIES.length],
      state: STATES[(i * 5) % STATES.length],
      band: BANDS[i % BANDS.length],
      mode: MODES[i % MODES.length],
      whenUnix: WHENS[(i * 3) % WHENS.length] + (Math.floor(i / 8) % 2) * 3_600,
      qslRcvd: { card, lotw, eqsl },
      confirmed: card || lotw || eqsl,
      awardConfirmed: card || lotw,
    } as LoggedQso
  })
}

const LOGS: [string, LoggedQso[]][] = [
  ['golden', GOLDEN_LOG],
  ['tied', tiedLog(140)],
  ['empty', []],
]

const viaJson = <T,>(x: T): T => JSON.parse(JSON.stringify(x)) as T

describe('count, then finish, is today’s statistics roll-up', () => {
  for (const [name, log] of LOGS) {
    it(`on the ${name} log`, () => {
      const today = oracleLogStats(log)
      expect(computeLogStats(log)).toEqual(today)
      // The engine's counts cross IPC as JSON before the window finishes them.
      expect(finishLogStats(viaJson(countLogStats(log)))).toEqual(today)
    })
  }

  it('finishing leaves the counts as they were', () => {
    const counts = countLogStats(LOGS[1][1])
    const before = viaJson(counts)
    finishLogStats(counts)
    expect(counts).toEqual(before)
  })

  it('the tied log is not vacuous: ordering its ties by code units would change the answer', () => {
    const log = LOGS[1][1]
    const today = oracleLogStats(log)
    const byCodeUnits = (t: readonly Tally[]) =>
      [...t].sort((a, b) => b.count - a.count || (a.label < b.label ? -1 : a.label > b.label ? 1 : 0))
    const counts = countLogStats(log)
    const differs = (['byBand', 'byMode'] as const).filter(
      (k) => JSON.stringify(byCodeUnits(counts[k])) !== JSON.stringify(today[k]),
    )
    expect(differs.length, 'a tie the locale orders differently from code units').toBeGreaterThan(0)
    // …and it reaches every rule the counting port must keep (the Rust port is held to its counts).
    const us = (q: LoggedQso) => US_ENTITIES.has(q.country?.trim().toUpperCase() ?? '')
    const reached: [string, boolean][] = [
      ['more than twelve entities', counts.entities.length > 12],
      ['a sideband spelled through the full case map (uſb)', log.some((q) => q.mode === 'uſb')],
      ['a mode whose upper case is not a sideband (ſſb)', log.some((q) => q.mode === 'ſſb')],
      ['a state spelled through the full case map (ſc) on a US contact', log.some((q) => q.state === 'ſc' && us(q))],
      ['a US state on a foreign contact', log.some((q) => q.state === 'wa' && !us(q))],
      ['an empty entity, which does not fall back to the country', log.some((q) => q.entity === '' && !!q.country?.trim())],
      ['no entity, which does', log.some((q) => q.entity === null && !!q.country?.trim())],
      ['a blank call', log.some((q) => q.call === '')],
      ['calls one only in upper case (ß1AA, ss1aa)', log.some((q) => q.call === 'ß1AA') && log.some((q) => q.call === 'ss1aa')],
      ['a blank and a whitespace band', log.some((q) => q.band === '') && log.some((q) => q.band === '  ')],
      ['date-only contacts', counts.hourUnknown > 0],
      ['a midnight past Date’s range', log.some((q) => q.whenUnix === 8_640_000_086_400)],
      ['an instant past Date’s range, in no year', log.some((q) => q.whenUnix > 9e12) && counts.byYear.every((t) => Number(t.label) < 3000)],
    ]
    expect(reached.filter(([, hit]) => !hit).map(([what]) => what)).toEqual([])
  })
})

describe('the counts the engine is held to', () => {
  const now = LOGS.slice(0, 2).map(([name, log]) => ({ name, log: name === 'golden' ? undefined : log, counts: countLogStats(log) }))

  it('are today’s, and frozen for the Rust port', () => {
    if (process.env.NEXUS_WRITE_LOG_GOLDENS === '1') {
      writeFileSync(COUNTS_URL, JSON.stringify(now, null, 1) + '\n')
      return
    }
    const frozen = JSON.parse(readFileSync(COUNTS_URL, 'utf8')) as typeof now
    expect(frozen).toEqual(viaJson(now))
  })

  it('finish to the statistics the log-query goldens froze', () => {
    const answers = JSON.parse(readFileSync(new URL('answers.json', FIXTURES), 'utf8')) as { q: { kind: string }; a: unknown }[]
    const golden = answers.find((x) => x.q.kind === 'statistics')?.a
    const counts = (JSON.parse(readFileSync(COUNTS_URL, 'utf8')) as { name: string; counts: LogStatCounts }[]).find(
      (x) => x.name === 'golden',
    )?.counts
    expect(golden, 'premise: the goldens hold the statistics').toBeDefined()
    expect(finishLogStats(counts!)).toEqual(golden)
  })
})
