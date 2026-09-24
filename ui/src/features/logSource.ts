// `LogSource` — HOW A VIEW READS THE LOG (SPEC-2 v3 C17b; the interface of v2 §6).
//
// A view does not hold the log. It asks for what it shows — a page of the Logbook, one call's
// history, a roster's summary, a count — and holds the answer. The questions and their answers are
// `logAnswers.ts`; this module is the contract a source of answers keeps, and the hook views use.
//
// Two sources exist or will:
//   - `wholeLogSource` (now): answers from the window's copy of the whole log, with the views' own
//     functions — identical to what they computed, by construction. It is the default.
//   - the engine's (C17a): asks over IPC and holds only answers. It keeps the same five promises.
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
import { questionKey, type AnswerTo, type LogPage, type LogQuestion } from './logAnswers'
import type { LogQuery } from './logQuery'
import { wholeLogSource } from './wholeLogSource'

export interface LogSource {
  peek<Q extends LogQuestion>(q: Q): AnswerTo<Q> | undefined
  ask<Q extends LogQuestion>(q: Q): Promise<AnswerTo<Q>>
  want(q: LogQuestion): () => void
  follow(logTick: number | undefined): void
  subscribe(listener: () => void): () => void
  refresh(): void
}

let current: LogSource = wholeLogSource

/** The window's source of log answers. */
export function logSource(): LogSource {
  return current
}

/** Choose the source — at startup, or in a test before it renders. */
export function setLogSource(source: LogSource): void {
  current = source
}

/** Tests: back to the default, after every test (src/test-setup.ts). */
export function __resetLogSourceForTests(): void {
  current = wholeLogSource
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
 * Several pages of one Logbook order at once — the pages the list's visible rows fall in, besides
 * the first (which the view asks through `useLogAnswer`, for its `total`). `offsets` are page
 * starts; the answer holds a page per offset the source has, and nothing for one on its way (the
 * view draws a placeholder row there).
 *
 * A page is only ever the answer to ITS question — the query is part of the key — so an answer to
 * an older sort or search can never land in this view (v2 R4). Pages cut at different revisions
 * are the view's to reconcile, by `orderRev` against its first page.
 */
export function useLogPages(
  query: LogQuery | null,
  offsets: readonly number[],
  limit: number,
  logTick: number | undefined,
): ReadonlyMap<number, LogPage> {
  const source = current
  const questions: LogQuestion[] = query === null ? [] : offsets.map((offset) => ({ kind: 'page', query, offset, limit }))
  const key = questions.map(questionKey).join('\n')
  const reading = query !== null
  const asked = useRef(questions)
  asked.current = questions
  // The pages held for these questions, as ONE value that stays the same object until one of them
  // changes — `useSyncExternalStore` needs that of whatever it is handed.
  const held = useRef<{ key: string; pages: (LogPage | undefined)[] } | null>(null)
  const read = useCallback(() => {
    const pages = asked.current.map((q) => source.peek(q) as LogPage | undefined)
    const prev = held.current
    if (prev && prev.key === key && prev.pages.every((p, i) => p === pages[i])) return prev.pages
    held.current = { key, pages }
    return pages
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `key` identifies the questions the ref holds
  }, [source, key])
  const pages = useSyncExternalStore(source.subscribe, read)
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
  const byOffset = new Map<number, LogPage>()
  pages.forEach((page, i) => {
    if (page) byOffset.set(offsets[i], page)
  })
  return byOffset
}
