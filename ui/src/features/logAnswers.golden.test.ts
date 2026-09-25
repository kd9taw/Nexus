// THE LOG-QUERY GOLDENS — today's answer to every question the UI asks of the log, over one fixture
// log, frozen as JSON (SPEC-2 v2 §10, v3 C17a/C17b).
//
// They are the oracle the engine's queries are held to: C17a's Rust reads `log.json` and
// `answers.json` from this directory (`include_str!`, as `qso-edit-fields.json` already is) and
// must give the same answers. They were generated from the functions the views ran over the whole
// log (`answerFrom`), before any view stopped running them — so they record what the operator saw.
//
// Regenerate ONLY when a behaviour change has been decided, and say so in the commit:
//     NEXUS_WRITE_LOG_GOLDENS=1 npx vitest run src/features/logAnswers.golden.test.ts
//
// The fixture (`log.json`, 24 rows) is built to break a careless port: non-ASCII text whose upper
// case is ASCII or longer (ſ→S, ı→I, ß→SS, Å), a full tie that must keep log order, equal times
// under different keys, a call spelled in two cases, sidebands for the `ssb` search, the dates and
// times the spec names (`09-19`, `14:2`), grids with case and whitespace, date-only imports, and
// every LoTW upload state. Rows carry ids, as every row the engine serves does.
//
// Answers are stored NORMALISED where they hold rows: a row is its id. What a row IS is the DTO's
// business and has its own goldens; what these pin is WHICH rows, in WHAT order, and the numbers.

import { describe, expect, it } from 'vitest'
import { readFileSync, writeFileSync } from 'node:fs'
import type { LoggedQso } from '../types'
import { answerFrom, type LogAnswers, type LogQuestion } from './logAnswers'
import { DEFAULT_LOG_QUERY, type LogQuery, type LogSortKey } from './logQuery'

const LOG_URL = new URL('./__fixtures__/log-query/log.json', import.meta.url)
const ANSWERS_URL = new URL('./__fixtures__/log-query/answers.json', import.meta.url)
const LOG = JSON.parse(readFileSync(LOG_URL, 'utf8')) as LoggedQso[]

const SORTS: LogSortKey[] = ['call', 'country', 'band', 'freq', 'mode', 'sent', 'rcvd', 'time', 'park', 'qsl']
const SEARCHES = ['k1a', 'dl', 'jo31', '2024-05', 'z', 'ssb', 'usb', '09-19', '14:2', '20m', ' W1 ', 'åland', 'ß', 'ſ', 'no-such-row']
const q = (over: Partial<LogQuery>): LogQuery => ({ ...DEFAULT_LOG_QUERY, ...over })
/** The whole order a query gives: one page as long as the log. */
const order = (query: LogQuery): LogQuestion => ({ kind: 'page', query, offset: 0, limit: LOG.length })

export const QUESTIONS: LogQuestion[] = [
  // The Logbook's order: every column, both directions (v2 §10: 10 keys × 2 directions).
  ...SORTS.flatMap((sort) => [true, false].map((asc) => order(q({ sort, asc })))),
  // Its search, under the default order, and with the needs-confirmation chip.
  ...SEARCHES.map((search) => order(q({ search }))),
  order(q({ needsConfirmOnly: true })),
  order(q({ needsConfirmOnly: true, search: 'k1a' })),
  order(q({ sort: 'call', asc: true, search: 'w1' })),
  order(q({ sort: 'mode', asc: false, search: 'ssb' })),
  // Pages that are not the whole order: a middle one, and one running off the end.
  { kind: 'page', query: q({}), offset: 5, limit: 5 },
  { kind: 'page', query: q({ sort: 'call', asc: true }), offset: 20, limit: 10 },
  // The scroll anchor: a row in view, a row the chip filters out, a row that is not there.
  { kind: 'locate', query: q({}), id: 'fx05' },
  { kind: 'locate', query: q({ needsConfirmOnly: true }), id: 'fx01' },
  { kind: 'locate', query: q({ sort: 'country', asc: true }), id: 'fx10' },
  { kind: 'locate', query: q({}), id: 'no-such-id' },
  // One call's history: the dupe scope with and without the mode, trimming, case, sidebands.
  { kind: 'callHistory', call: 'W1AW', band: '20m', mode: 'FT8', matchMode: false },
  { kind: 'callHistory', call: 'W1AW', band: '40m', mode: 'CW', matchMode: true },
  { kind: 'callHistory', call: ' k1abc ', band: '40M', mode: 'SSB', matchMode: true },
  { kind: 'callHistory', call: 'K1ABC', band: '40m', mode: 'CW', matchMode: true },
  { kind: 'callHistory', call: 'K1ABC', band: '40m', mode: 'CW', matchMode: false },
  { kind: 'callHistory', call: 'DL1ABC', band: '', mode: '', matchMode: false },
  { kind: 'callHistory', call: 'DS1X', band: '20m', mode: 'FT8', matchMode: false },
  { kind: 'callHistory', call: 'PY2AA', band: '20m', mode: 'FT8', matchMode: false },
  { kind: 'callHistory', call: 'NONE', band: '20m', mode: 'FT8', matchMode: false },
  { kind: 'callHistory', call: '', band: '20m', mode: 'FT8', matchMode: false },
  // An entity: resolved names, QRZ's spelling, case, a fold that lengthens (ß), blank, unknown.
  ...['United States', 'united states ', 'Fed. Rep. of Germany', 'Germany', 'ſtraße', 'STRASSE', 'Japan', 'Aland Islands', '', 'Nowhere'].map(
    (entity): LogQuestion => ({ kind: 'entity', entity }),
  ),
  { kind: 'callsSummary', calls: ['W1AW', 'K1ABC', 'PY2AA', 'NONE', 'w1aw', 'DS1X'] },
  { kind: 'workedCalls', calls: ['W1AW', 'K1ABC', 'NONE', 'DS1X', 'W1AW ', 'N0CALL'] },
  { kind: 'workedGrids' },
  { kind: 'gridPoints', band: 'all' },
  { kind: 'gridPoints', band: '20m' },
  { kind: 'gridPoints', band: '2m' },
  { kind: 'bandsInLog' },
  { kind: 'statistics' },
  { kind: 'lotwBacklog' },
  { kind: 'rowsAt', indices: [0, 9, 23, 24, -1] },
  { kind: 'row', id: 'fx07' },
  { kind: 'row', id: 'no-such-id' },
  { kind: 'logSize' },
]

const idOf = (r: LoggedQso | null) => (r === null ? null : (r.id ?? null))

/** An answer as the golden stores it: rows by id; a page as its numbers and its keys. */
export function normalise(question: LogQuestion, answer: unknown): unknown {
  switch (question.kind) {
    case 'callHistory': {
      const { qsos, ...rest } = answer as LogAnswers['callHistory']
      return { ...rest, qsos: qsos.map(idOf) }
    }
    case 'rowsAt':
      return (answer as LogAnswers['rowsAt']).map(idOf)
    case 'row':
      return idOf(answer as LogAnswers['row'])
    case 'page': {
      const { total, logSize, offset, keys } = answer as LogAnswers['page']
      return { total, logSize, offset, keys }
    }
    case 'locate':
      return { index: (answer as LogAnswers['locate']).index }
    default:
      return answer
  }
}

const computed = () => QUESTIONS.map((question) => ({ q: question, a: normalise(question, answerFrom(LOG, question, 1)) }))

describe('the log-query goldens (the oracle for the engine’s queries)', () => {
  it('today’s answers to every question over the fixture log are the frozen ones', () => {
    const now = computed()
    if (process.env.NEXUS_WRITE_LOG_GOLDENS === '1') {
      writeFileSync(ANSWERS_URL, JSON.stringify(now, null, 1) + '\n')
      return
    }
    const frozen = JSON.parse(readFileSync(ANSWERS_URL, 'utf8')) as typeof now
    expect(frozen.length, 'questions in the golden file').toBe(now.length)
    for (let i = 0; i < now.length; i++) {
      expect(now[i].q, `question ${i} of the golden file`).toEqual(frozen[i].q)
      expect(now[i].a, `answer to ${JSON.stringify(now[i].q)}`).toEqual(frozen[i].a)
    }
  })

  it('the fixture exercises what it claims to (the goldens are not vacuous)', () => {
    const answerTo = (question: LogQuestion) => normalise(question, answerFrom(LOG, question, 1))
    // "z" is in every formatted time, so it matches every row — a SQL filter finds a fraction.
    expect((answerTo(order(q({ search: 'z' }))) as { total: number }).total).toBe(LOG.length)
    // "ssb" reaches the sideband rows only through the mode fold.
    const ssb = (answerTo(order(q({ search: 'ssb' }))) as { keys: string[] }).keys
    expect(ssb).toEqual(expect.arrayContaining(['fx03', 'fx04']))
    // The full tie keeps log order in both directions (the twins are fx17, fx18).
    for (const asc of [true, false]) {
      const keys = (answerTo(order(q({ sort: 'call', asc }))) as { keys: string[] }).keys
      expect(keys.indexOf('fx17'), `twins out of log order, asc=${asc}`).toBeLessThan(keys.indexOf('fx18'))
    }
    // ß folds to SS: the entity asked as 'STRASSE' is the row whose country is 'ſtraße'.
    expect((answerTo({ kind: 'entity', entity: 'STRASSE' }) as { newEntity: boolean }).newEntity).toBe(false)
    // The band map's rule is UNTRIMMED: a trailing space is a different call.
    expect(answerTo({ kind: 'workedCalls', calls: ['W1AW', 'W1AW '] })).toEqual(['W1AW'])
  })
})
