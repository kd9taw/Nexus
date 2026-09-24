// THE QUESTIONS THE UI ASKS OF THE LOG, AND TODAY'S ANSWERS TO THEM (SPEC-2 v3 C17b).
//
// Every view that shows log data used to hold the whole log and compute what it showed from it
// (spec §2.3, the ten sources). Each such computation is a QUESTION here, with a plain-JSON answer
// an engine can send over IPC. A window then asks for what it shows and holds only the answers.
//
// `answerFrom` is the reference answer to every question: it runs the SAME functions the views
// ran over the whole log — `callHistory`, `entitySlots`, `computeLogStats`, `lotwBacklog`,
// `workedGridSet`, the Logbook's filter and sort — so the whole-log adapter built on it
// (features/wholeLogSource.ts) answers exactly what the views computed, by construction. The
// engine's queries (C17a) must answer the same; the goldens in `__fixtures__/log-query/` are
// generated from this module and are what its Rust port is held to.
//
// ⚠️ Answers are JSON-shaped on purpose (arrays and records, never Sets or Maps): they cross IPC
// once the engine answers them. A view builds its own Set from an answer, memoized on the answer.

import type { LoggedQso } from '../types'
import { workedGridSet } from '../coverage'
import {
  callHistory,
  callsSummary,
  entitySlots,
  isNewEntity,
  type CallHistory,
  type CallSummary,
  type EntitySlots,
} from './callHistory'
import { computeLogStats, type LogStats } from './logStats'
import { logOrder, type LogQuery } from './logQuery'
import { lotwBacklog } from './lotwBacklog'
import { qsoGridCounts, type QsoGridCount } from './qsoPoints'

/** Everything the UI asks of the log. */
export type LogQuestion =
  /** Prior contacts with one call — the B4/dupe badges and the recall card (LogEntry, Operate). */
  | { kind: 'callHistory'; call: string; band: string; mode: string; matchMode: boolean }
  /** Whether an entity is new, and its worked band/mode slots — the NEW ONE / new-slot badges. */
  | { kind: 'entity'; entity: string }
  /** Per-call detail for a roster (JS8): count, last time, and the latest contact's grid/name/comment. */
  | { kind: 'callsSummary'; calls: readonly string[] }
  /** Which of `calls` are in the log: `call.toUpperCase()` equality, UNTRIMMED (the band map). */
  | { kind: 'workedCalls'; calls: readonly string[] }
  /** Every worked 4-char square (the map's and the globe's coverage layers). */
  | { kind: 'workedGrids' }
  /** The Logbook globe's dots for one band ('all' pools them). */
  | { kind: 'gridPoints'; band: string }
  /** The bands in the log, first-seen order (the Logbook globe's band picker sorts them). */
  | { kind: 'bandsInLog' }
  /** The Statistics view's roll-up. */
  | { kind: 'statistics' }
  /** The Logbook's "Upload to LoTW" count, and the date-only contacts LoTW can never match. */
  | { kind: 'lotwBacklog' }
  /** How many contacts the log holds (the Logbook's count badge, its purge warning, its gates). */
  | { kind: 'logSize' }
  /** Rows by LOG POSITION — the Awards diagnosis addresses its QSOs that way (see §C17a in the
   *  report: the diagnosis gaining row ids retires this question). */
  | { kind: 'rowsAt'; indices: readonly number[] }
  /** One row by its key (see `LogPage.keys`). */
  | { kind: 'row'; id: string }
  /** A page of the Logbook list: `limit` rows from `offset` of the order `query` gives. */
  | { kind: 'page'; query: LogQuery; offset: number; limit: number }
  /** Where the row with key `id` sits in the order `query` gives — the Logbook's scroll anchor. */
  | { kind: 'locate'; query: LogQuery; id: string }

export interface EntityAnswer {
  /** The entity is known and no contact in the log has it (`isNewEntity`). */
  newEntity: boolean
  slots: EntitySlots
}

/** One page of the Logbook list, and the revisions it was cut at (SPEC-2 v2 §1, R4). */
export interface LogPage {
  /** The query this page answers — echoed, so an answer to an older query is never applied. */
  query: LogQuery
  /** The log revision the page was read at. */
  revision: number
  /** The revision the ORDER was built at. Pages of one view must share it; a newer one means the
   *  rows moved (an insert, a delete, an edit of a sorted field) and the view re-anchors. */
  orderRev: number
  /** The revision the rows' CONTENT was read at. A newer one with the same `orderRev` is an upload
   *  stamp or a QSL mark: the rows changed, their places did not. */
  contentRev: number
  /** Rows the query shows. */
  total: number
  /** Rows in the whole log (the Logbook's count badge and its purge warning). */
  logSize: number
  offset: number
  rows: LoggedQso[]
  /** Each row's identity: its id. Only a row without one (a test fixture, a pre-1.14 station's
   *  row) gets `#<log position>` from the whole-log adapter — the engine's rows always carry ids. */
  keys: string[]
}

export interface LogLocate {
  query: LogQuery
  orderRev: number
  /** The row's place in the order, or null when the query does not show it (or it is gone). */
  index: number | null
}

export interface LogAnswers {
  callHistory: CallHistory
  entity: EntityAnswer
  /** Worked calls only, keyed by the call exactly as asked. */
  callsSummary: Record<string, CallSummary>
  /** The asked calls that are worked, as asked. */
  workedCalls: string[]
  /** 4-char squares, upper-cased, first-seen order. */
  workedGrids: string[]
  gridPoints: QsoGridCount[]
  bandsInLog: string[]
  statistics: LogStats
  lotwBacklog: { unsent: number; timeless: number }
  logSize: number
  /** `null` for a position the log does not have. */
  rowsAt: (LoggedQso | null)[]
  row: LoggedQso | null
  page: LogPage
  locate: LogLocate
}

export type LogKind = LogQuestion['kind']
export type AnswerTo<Q extends LogQuestion> = LogAnswers[Q['kind']]

/** A row's identity in a page: its id, or its log position when it has none. */
export function rowKeyAt(log: readonly LoggedQso[], position: number): string {
  return log[position]?.id ?? `#${position}`
}

/** A stable text key for a question — what caches and in-flight requests are keyed on. */
export function questionKey(q: LogQuestion): string {
  switch (q.kind) {
    case 'callHistory':
      return `callHistory|${q.call}|${q.band}|${q.mode}|${q.matchMode ? 1 : 0}`
    case 'entity':
      return `entity|${q.entity}`
    case 'callsSummary':
    case 'workedCalls':
      return `${q.kind}|${q.calls.join(' ')}`
    case 'gridPoints':
      return `gridPoints|${q.band}`
    case 'rowsAt':
      return `rowsAt|${q.indices.join(',')}`
    case 'row':
      return `row|${q.id}`
    case 'page':
      return `page|${q.offset}|${q.limit}|${orderKey(q.query)}`
    case 'locate':
      return `locate|${q.id}|${orderKey(q.query)}`
    case 'workedGrids':
    case 'bandsInLog':
    case 'statistics':
    case 'lotwBacklog':
    case 'logSize':
      return q.kind
  }
}

function orderKey(query: LogQuery): string {
  return `${query.sort}|${query.asc ? 'asc' : 'desc'}|${query.needsConfirmOnly ? 1 : 0}|${query.search}`
}

/** The answer to `q` from a whole log held in memory, computed by the functions the views used.
 *
 *  `revision` stamps page/locate answers. `order` lets a caller that keeps order vectors hand one
 *  in (building one is the cost of a page); without it the order is built here. */
export function answerFrom<Q extends LogQuestion>(
  log: LoggedQso[],
  q: Q,
  revision: number,
  order: (query: LogQuery) => number[] = (query) => logOrder(log, query),
): AnswerTo<Q> {
  return compute(log, q, revision, order) as AnswerTo<Q>
}

function compute(
  log: LoggedQso[],
  q: LogQuestion,
  revision: number,
  order: (query: LogQuery) => number[],
): LogAnswers[LogKind] {
  switch (q.kind) {
    case 'callHistory':
      return callHistory(log, q.call, q.band, q.mode, q.matchMode)
    case 'entity':
      return { newEntity: isNewEntity(log, q.entity), slots: entitySlots(log, q.entity) }
    case 'callsSummary':
      return callsSummary(log, q.calls)
    case 'workedCalls': {
      // The band map's rule, verbatim: the log's calls upper-cased and NOT trimmed.
      const worked = new Set(log.map((r) => r.call.toUpperCase()))
      return q.calls.filter((c) => worked.has(c.toUpperCase()))
    }
    case 'workedGrids':
      return [...workedGridSet(log)]
    case 'gridPoints':
      return qsoGridCounts(log, q.band)
    case 'bandsInLog': {
      const seen = new Set<string>()
      for (const r of log) if (r.band) seen.add(r.band)
      return [...seen]
    }
    case 'statistics':
      return computeLogStats(log)
    case 'lotwBacklog':
      return lotwBacklog(log)
    case 'logSize':
      return log.length
    case 'rowsAt':
      return q.indices.map((i) => log[i] ?? null)
    case 'row': {
      const at = log.findIndex((r, i) => (r.id ?? `#${i}`) === q.id)
      return at < 0 ? null : log[at]
    }
    case 'page': {
      const view = order(q.query)
      const slice = view.slice(q.offset, q.offset + q.limit)
      return {
        query: q.query,
        revision,
        orderRev: revision,
        contentRev: revision,
        total: view.length,
        logSize: log.length,
        offset: q.offset,
        rows: slice.map((i) => log[i]),
        keys: slice.map((i) => rowKeyAt(log, i)),
      }
    }
    case 'locate': {
      const index = order(q.query).findIndex((i) => rowKeyAt(log, i) === q.id)
      return { query: q.query, orderRev: revision, index: index < 0 ? null : index }
    }
  }
}

const NO_ROWS: LoggedQso[] = []
/** Empty-log answers by question, so a view falling back to one gets the SAME object every render
 *  (its memos key on the answer) — as it did when it computed over the one shared `NO_LOG`. */
const empties = new Map<string, unknown>()
const EMPTIES_KEPT = 64

/** The answer an EMPTY log gives — what every view showed before its first answer arrived, when
 *  it computed over `useSharedLog(...) ?? NO_LOG`. A view that has no answer yet shows this, so
 *  the loading moment looks exactly as it did. (C17a decides per view whether a loading state
 *  should replace it: an empty log's answer is "never worked", which is a claim, not a blank.) */
export function emptyAnswer<Q extends LogQuestion>(q: Q): AnswerTo<Q> {
  const key = questionKey(q)
  if (empties.has(key)) return empties.get(key) as AnswerTo<Q>
  const answer = answerFrom(NO_ROWS, q, 0)
  empties.set(key, answer)
  if (empties.size > EMPTIES_KEPT) {
    const oldest = empties.keys().next()
    if (!oldest.done) empties.delete(oldest.value)
  }
  return answer
}
