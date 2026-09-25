// THE WHOLE-LOG ADAPTER — `LogSource` answered from the window's one copy of the log.
//
// SPEC-2 v3 C17b runs the UI against a `LogSource` before the engine can answer its questions
// (C17a). Until then this adapter answers them, from the copy `features/logStore.ts` keeps, with
// the functions the views themselves ran (`answerFrom`). So a view switched onto `LogSource` shows
// exactly what it showed, at the moment it showed it:
//
//   - `peek` answers SYNCHRONOUSLY whenever the copy is loaded — where a view used to compute its
//     `useMemo` over the shared array, it now reads the same answer during the same render;
//   - `follow` is the store's own tick report (`useSharedLog`'s effect), so the requests the
//     window makes are unchanged: one `get_log_delta` per tick, shared by every reader;
//   - `ask` is `loadSharedLog` plus the answer — the read-once views' old path.
//
// ⚠️ THIS MODULE AND logStore.ts ARE THE ONLY PLACES THE WHOLE LOG MAY BE READ IN THE UI.
// `noWholeLog.test.ts` enforces it. C17a replaces this adapter with one that asks the engine, and
// deletes both modules; nothing else in the UI should notice.

import type { LoggedQso } from '../types'
import { answerFrom, questionKey, type AnswerTo, type LogQuestion } from './logAnswers'
import { logOrder, logQueryKey, type LogQuery } from './logQuery'
import type { LogSource } from './logSource'
import {
  followSharedLog,
  loadSharedLog,
  refreshSharedLog,
  sharedLogRevision,
  sharedLogRows,
  subscribeSharedLog,
} from './logStore'

/** Answers kept per question. Validated against the copy they came from, so a stale one is never
 *  served — the bound only caps memory. Far above the number of questions on screen at once, which
 *  `useSyncExternalStore` needs: it must get the SAME answer back for an unchanged question. */
const ANSWERS_KEPT = 256
/** Order vectors kept (v2 §2: an LRU of four). One is a log-sized array of positions. */
const ORDERS_KEPT = 4

let answers = new Map<string, { rows: LoggedQso[]; answer: unknown }>()
let orders = new Map<string, { rows: LoggedQso[]; order: number[] }>()
/** The revision each copy was first seen at. The store can move its revision without replacing
 *  the rows (a change that appended nothing); an answer must carry the revision its ROWS came
 *  from, or two pages cut from the same rows could name different revisions. */
let seenAt = new WeakMap<LoggedQso[], number>()

function revisionOf(rows: LoggedQso[]): number {
  let r = seenAt.get(rows)
  if (r === undefined) {
    r = sharedLogRevision()
    seenAt.set(rows, r)
  }
  return r
}

/** Put `key` at the young end of an insertion-ordered map, evicting the oldest beyond `cap`. */
function keep<V>(map: Map<string, V>, key: string, value: V, cap: number): void {
  map.delete(key)
  map.set(key, value)
  while (map.size > cap) {
    const oldest = map.keys().next()
    if (oldest.done) break
    map.delete(oldest.value)
  }
}

function orderOf(rows: LoggedQso[], query: LogQuery): number[] {
  const key = logQueryKey(query)
  const hit = orders.get(key)
  if (hit && hit.rows === rows) {
    keep(orders, key, hit, ORDERS_KEPT)
    return hit.order
  }
  const order = logOrder(rows, query)
  keep(orders, key, { rows, order }, ORDERS_KEPT)
  return order
}

function answer<Q extends LogQuestion>(rows: LoggedQso[], q: Q): AnswerTo<Q> {
  const key = questionKey(q)
  const hit = answers.get(key)
  if (hit && hit.rows === rows) {
    keep(answers, key, hit, ANSWERS_KEPT)
    return hit.answer as AnswerTo<Q>
  }
  const computed = answerFrom(rows, q, revisionOf(rows), (query) => orderOf(rows, query))
  keep(answers, key, { rows, answer: computed }, ANSWERS_KEPT)
  return computed
}

/** Nothing to fetch: the copy answers every question the moment it is loaded. */
const RELEASE = () => {}

export const wholeLogSource: LogSource = {
  peek(q) {
    const rows = sharedLogRows()
    return rows === null ? undefined : answer(rows, q)
  },
  async ask(q) {
    return answer(await loadSharedLog(), q)
  },
  follow: followSharedLog,
  want: () => RELEASE,
  subscribe: subscribeSharedLog,
  refresh: refreshSharedLog,
}

/** Tests: forget every kept answer, like the store's own reset. */
export function __resetWholeLogSourceForTests(): void {
  answers = new Map()
  orders = new Map()
  seenAt = new WeakMap()
}

;(globalThis as { __nexusTestResets?: Set<() => void> }).__nexusTestResets?.add(__resetWholeLogSourceForTests)
