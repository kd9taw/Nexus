// `LogSource` — HOW A VIEW READS THE LOG (SPEC-2 v3 C17b; the interface of v2 §6).
//
// A view does not hold the log. It asks for what it shows — a page of the Logbook, one call's
// history, a roster's summary, a count — and holds the answer. The questions and their answers are
// `logAnswers.ts`; this module is the contract a source of answers keeps, and the hook views use.
//
// The window's source asks the ENGINE (SPEC-2 v3 C17): `createAskingLogSource` over `askLog`, one
// IPC call per question, holding only the answers — a page of the Logbook, one call's history, a
// count. No window holds the log. (Until C17 every window held the whole of it, and read all of
// it again after every upload stamp.)
//
// THE CONTRACT
//   peek(q)    What the source already holds for `q`, synchronously, or undefined. It MUST return
//              the same object for the same question until `subscribe` fires — `useSyncExternalStore`
//              re-renders forever otherwise. An answer may be from before the latest change: a
//              view keeps showing it until the fresh one lands, as it kept its old copy of the log.
//   want(q)    A view shows `q` — keep an answer to it on its way and fresh. Returns the release,
//              called when the view stops showing it (an unmount, a new question).
//   follow(t)  The change feed: a view reports its window's snapshot `logTick`, which the engine
//              moves on EVERY change to the log (station.rs `log_tick`). A tick the held answers do
//              not reflect means they are stale; `undefined` is a view with no snapshot, which is
//              answered with a refresh (the old `useSharedLog(undefined)` rule).
//   ask(q)     One answer, as of now — for a view that reads once and keeps what it got, and for
//              an action (a push needs the row).
//   subscribe  Held answers changed.
//   refresh()  This window just wrote to the log: do not wait for the tick.
//
// The source is chosen once, before anything renders (`setLogSource`): the hooks below read it at
// render, and a source swapped under mounted views would leave them subscribed to the old one.

import { useCallback, useEffect, useRef, useSyncExternalStore } from 'react'
import { askLog } from '../api'
import { createAskingLogSource } from './askingLogSource'
import { questionKey, type AnswerTo, type LogPage, type LogQuestion } from './logAnswers'
import type { LogQuery } from './logQuery'

export interface LogSource {
  peek<Q extends LogQuestion>(q: Q): AnswerTo<Q> | undefined
  ask<Q extends LogQuestion>(q: Q): Promise<AnswerTo<Q>>
  want(q: LogQuestion): () => void
  follow(logTick: number | undefined): void
  subscribe(listener: () => void): () => void
  refresh(): void
}

/** The engine's answers. `async`: a transport that throws before it returns a promise would throw
 *  out of a view's effect (`want`) instead of rejecting. */
const askingTheEngine = (): LogSource => createAskingLogSource(async (q) => askLog(q))

let current: LogSource = askingTheEngine()

/** The window's source of log answers. */
export function logSource(): LogSource {
  return current
}

/** Choose the source — at startup, or in a test before it renders. */
export function setLogSource(source: LogSource): void {
  current = source
}

/** Tests: back to the default — a fresh one, holding no answers — after every test
 *  (src/test-setup.ts). */
export function __resetLogSourceForTests(): void {
  current = askingTheEngine()
}
;(globalThis as { __nexusTestResets?: Set<() => void> }).__nexusTestResets?.add(__resetLogSourceForTests)

/**
 * The answer to `q` for a view, following `logTick` — or `undefined` while the window has none
 * (the view then shows `emptyAnswer(q)`, what it showed while the log loaded before).
 *
 * `q = null` is a view that is not reading (remote mode, a hidden roster): it asks for nothing and
 * reports no tick, as `useSharedLog(tick, false)` did.
 */
export function useLogAnswer<Q extends LogQuestion>(q: Q | null, logTick: number | undefined): AnswerTo<Q> | undefined {
  const source = current
  const key = q === null ? null : questionKey(q)
  const reading = key !== null
  // The question as of this render. `key` names it, so an unchanged question keeps the callbacks
  // and effects below stable however often the view builds a fresh object for it.
  const question = useRef(q)
  question.current = q
  const read = useCallback(
    () => (question.current === null ? undefined : source.peek(question.current)),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `key` identifies the question the ref holds
    [source, key],
  )
  const answer = useSyncExternalStore(source.subscribe, read)
  // The change feed first, on exactly the old dependencies — the tick and whether the view reads —
  // so a new question (a keystroke in a call box) is never a new request on the whole-log path.
  useEffect(() => {
    if (reading) source.follow(logTick)
  }, [source, reading, logTick])
  useEffect(() => {
    const q = question.current
    return q === null ? undefined : source.want(q)
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `key` identifies the question the ref holds
  }, [source, key])
  return answer
}

/**
 * Several questions at once, as ONE value: their answers in question order (`undefined` for one the
 * source does not hold yet), the same array object until one of them changes — which
 * `useSyncExternalStore` needs of whatever it is handed. Each question is wanted while it is asked;
 * `reading` says whether the view follows the tick at all (a view with nothing to ask still does).
 */
export function useLogAnswers(
  questions: readonly LogQuestion[],
  logTick: number | undefined,
  reading = questions.length > 0,
): readonly unknown[] {
  const source = current
  const key = questions.map(questionKey).join('\n')
  const asked = useRef(questions)
  asked.current = questions
  const held = useRef<{ key: string; answers: unknown[] } | null>(null)
  const read = useCallback(() => {
    const answers = asked.current.map((q) => source.peek(q))
    const prev = held.current
    if (prev && prev.key === key && prev.answers.every((a, i) => a === answers[i])) return prev.answers
    held.current = { key, answers }
    return answers
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `key` identifies the questions the ref holds
  }, [source, key])
  const answers = useSyncExternalStore(source.subscribe, read)
  useEffect(() => {
    if (reading) source.follow(logTick)
  }, [source, reading, logTick])
  useEffect(() => {
    const releases = asked.current.map((q) => source.want(q))
    return () => {
      for (const release of releases) release()
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `key` identifies the questions the ref holds
  }, [source, key])
  return answers
}

/**
 * Several pages of one Logbook order at once — the pages the list's visible rows fall in, besides
 * the first (which the view asks through `useLogAnswer`, for its `total`). `offsets` are page
 * starts; the answer holds a page per offset the source has, and nothing for one on its way (the
 * view draws a placeholder row there).
 *
 * A page is only ever the answer to ITS question — the query is part of the key — so an answer to
 * an older sort or search can never land in this view (v2 R4). Pages cut at different revisions
 * are the view's to reconcile, by `orderRev`.
 */
export function useLogPages(
  query: LogQuery | null,
  offsets: readonly number[],
  limit: number,
  logTick: number | undefined,
): ReadonlyMap<number, LogPage> {
  const questions: LogQuestion[] = query === null ? [] : offsets.map((offset) => ({ kind: 'page', query, offset, limit }))
  const pages = useLogAnswers(questions, logTick, query !== null) as readonly (LogPage | undefined)[]
  const byOffset = new Map<number, LogPage>()
  pages.forEach((page, i) => {
    if (page) byOffset.set(offsets[i], page)
  })
  return byOffset
}
