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
//   status(q)  Where the answer to `q` stands, as one word: a primitive, so a view can read it
//              through `useSyncExternalStore` (`useLogStatus`). It changes only when `subscribe`
//              fires.
//                'asking'   no answer yet; one is on its way, or will be once a view wants it.
//                'current'  the held answer reflects every change the source knows of.
//                'stale'    the held answer is from before the latest change; a fresh one is on
//                           its way. A view keeps showing it, and says it is out of date where
//                           that matters (a count).
//                'failed'   the latest ask failed, and nothing has asked since. `peek` still gives
//                           any older answer. A view says so through `failureToShow`, which keeps
//                           the refusal while a change is being saved quiet.
//   failure(q) Why the latest ask of `q` failed, in the transport's words, while it is 'failed'.
//   want(q)    A view shows `q` — keep an answer to it on its way and fresh. Returns the release,
//              called when the view stops showing it (an unmount, a new question). A question
//              whose latest ask failed is asked again when a view opens on it: reopening a view is
//              how an operator asks again. A failure is otherwise not retried: it waits for the
//              next change, or a refresh.
//   follow(t)  The change feed: a view reports its window's snapshot `logTick`, which the engine
//              moves on EVERY change to the log (station.rs `log_tick`). A tick the held answers do
//              not reflect means they are stale; `undefined` is a view with no snapshot, which is
//              answered with a refresh.
//   ask(q)     One answer, as of now — for a view that reads once and keeps what it got, and for
//              an action (a push needs the row).
//   subscribe  Held answers, or where they stand, changed.
//   refresh()  This window just wrote to the log, or the operator pressed Retry: ask every shown
//              question again now, without waiting for the tick.
//
// The source is chosen once, before anything renders (`setLogSource`): the hooks below read it at
// render, and a source swapped under mounted views would leave them subscribed to the old one.

import { useCallback, useEffect, useRef, useSyncExternalStore } from 'react'
import { askLog } from '../api'
import { createAskingLogSource } from './askingLogSource'
import { questionKey, type AnswerTo, type LogPage, type LogQuestion } from './logAnswers'
import type { LogQuery } from './logQuery'
import { askAgainWhileSaving, notAnswered } from './notAnswered'

/** Where the answer to a question stands (`LogSource.status`). */
export type LogAnswerState = 'asking' | 'current' | 'stale' | 'failed'

export interface LogSource {
  peek<Q extends LogQuestion>(q: Q): AnswerTo<Q> | undefined
  status(q: LogQuestion): LogAnswerState
  failure(q: LogQuestion): string | undefined
  ask<Q extends LogQuestion>(q: Q): Promise<AnswerTo<Q>>
  want(q: LogQuestion): () => void
  follow(logTick: number | undefined): void
  subscribe(listener: () => void): () => void
  refresh(): void
}

/** The engine's answers. `async`: a transport that throws before it returns a promise would throw
 *  out of a view's effect (`want`) instead of rejecting. A question the engine refused because a
 *  change made before it is still being saved is asked again a second later, a few times
 *  (features/notAnswered): the tick that change moved has already been followed, so nothing else
 *  would ask it again until the next change. */
const askingTheEngine = (): LogSource => createAskingLogSource(async (q) => askAgainWhileSaving(() => askLog(q)))

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
 * reports no tick.
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
  // The change feed first, on exactly the tick and whether the view reads — so a new question (a
  // keystroke in a call box) asks that question alone, and re-asks nothing else.
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

/** Where an answer stands, and why its latest ask failed when it did (`useLogStatus`). */
export interface LogStatus {
  state: LogAnswerState
  reason: string | undefined
}

/**
 * Where the answer to `q` stands (`LogSource.status`), and why its latest ask failed when it did:
 * what a view says beside, or in place of, the answer `useLogAnswer` gives it. This only reads; the
 * view asks through `useLogAnswer`. `undefined` for `q = null`, a view that is not reading.
 */
export function useLogStatus(q: LogQuestion | null): LogStatus | undefined {
  const source = current
  const key = q === null ? null : questionKey(q)
  const question = useRef(q)
  question.current = q
  const readState = useCallback(
    () => (question.current === null ? undefined : source.status(question.current)),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `key` identifies the question the ref holds
    [source, key],
  )
  const readReason = useCallback(
    () => (question.current === null ? undefined : source.failure(question.current)),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `key` identifies the question the ref holds
    [source, key],
  )
  const state = useSyncExternalStore(source.subscribe, readState)
  const reason = useSyncExternalStore(source.subscribe, readReason)
  return state === undefined ? undefined : { state, reason }
}

/**
 * Why a read failed, for a view to say ("Couldn’t read the logbook: …"), or null when it says
 * nothing. That is a read that has not failed, and the engine's refusal while a change is still
 * being saved (features/notAnswered). A refusal stays quiet even once its asks run out (operator,
 * 2026-10-09: "Keep it silent"): the view keeps saying it is reading the logbook, and the next
 * change, a refresh or a reopened view asks again.
 */
export function failureToShow(status: LogStatus | undefined): string | null {
  return status?.state === 'failed' && !notAnswered(status.reason) ? (status.reason ?? '') : null
}

/**
 * The answer to `q`, or, until `q` has one, the latest answer this view was shown, WITH the
 * question it answered. This is for a question keyed on a set of calls (a roster's heard calls, the
 * calls on a map). Each new call asks a new question, and until its answer lands the calls the
 * latest answer covered keep that answer. They used to drop to `emptyAnswer` for the round trip,
 * so every mark blinked. A call the latest answer's question did not ask about is not answered
 * yet. `undefined` while the view has been shown no answer, and for `q = null`.
 */
export function useLatestLogAnswer<Q extends LogQuestion>(
  q: Q | null,
  logTick: number | undefined,
): { question: Q; answer: AnswerTo<Q> } | undefined {
  const answer = useLogAnswer(q, logTick)
  // A ref, read in the same render: a state update would draw one frame without it, the blink.
  const latest = useRef<{ question: Q; answer: AnswerTo<Q> } | undefined>(undefined)
  if (q !== null && answer !== undefined && latest.current?.answer !== answer) latest.current = { question: q, answer }
  return q === null ? undefined : latest.current
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
