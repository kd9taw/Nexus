// A `LogSource` THAT ASKS — answers fetched from whatever holds the log, one question at a time,
// and only the answers kept (SPEC-2 v3 C17b; v2 §6, R4).
//
// This is the window's side of the engine's log queries (C17a): hand it the transport — for the
// desktop, one IPC call per question — and it keeps the `LogSource` contract the views are built on
// (features/logSource.ts). What it adds over a bare call, each one a rule the views rely on:
//
//   - ONE request per question in flight, however many views show it. A view that shows a question
//     wants it (`want`), and its answer is kept fresh while any view does.
//   - The change feed: a tick the kept answers do not reflect re-asks every wanted question ONCE,
//     however many views report that tick; a burst of ticks while a request is out costs one
//     follow-up, never one each. Until the fresh answer lands the old one is still shown — as a view
//     kept its old copy of the log until the new one arrived.
//   - An answer never goes backwards: one to a request made before the request whose answer is
//     already kept is dropped (v2 R4, "stale answers by identity"). The question is the key, so an
//     answer to another sort or search cannot land in a view either.
//   - A failed request is not retried in a loop: it waits for the next change, a refresh (a view's
//     Retry), or a view opening on the question again. Until then the question reads as failed, in
//     the transport's words, to the views showing it. It used to count as answered at its
//     generation, so a view reopened on it never asked again.
//   - Each answer says where it stands (`status`): asking, current, stale or failed.
//   - Answers no view shows any more are kept a little while (LRU), so scrolling back, or leaving a
//     view and coming back, is not a round trip.
//
// It holds nothing of the log beyond what its answers hold: pages, one call's history, counts.

import { questionKey, type AnswerTo, type LogQuestion } from './logAnswers'
import type { LogAnswerState, LogSource } from './logSource'

/** Ask the log one question. Rejects when it cannot be answered (the view keeps what it had). */
export type LogTransport = <Q extends LogQuestion>(q: Q) => Promise<AnswerTo<Q>>

/** Answers kept for questions no view shows any more. */
const UNWANTED_KEPT = 64

interface Entry {
  question: LogQuestion
  answer?: unknown
  /** The request number the kept answer came from — a lower one never replaces it. */
  answeredBy: number
  /** The change-feed generation the kept answer reflects; -1 for none. */
  generation: number
  /** The generation the latest failed request was asked at; -1 for none. It is the latest word on
   *  the question while it is newer than the answer. */
  failedAt: number
  /** Why that request failed, in the transport's words. */
  failure?: string
  /** Views showing this question. */
  wanted: number
  /** A request is out, asked at generation `askedAt`. */
  inflight: boolean
  askedAt: number
}

export function createAskingLogSource(transport: LogTransport): LogSource {
  const entries = new Map<string, Entry>()
  const listeners = new Set<() => void>()
  let requests = 0
  /** Moves on every change the source learns of: a new tick, a tick-less reader, this window's write. */
  let generation = 0
  let lastTick: number | undefined
  let scheduled = false

  const notify = () => {
    for (const listener of listeners) listener()
  }

  const evictUnwanted = () => {
    const idle = [...entries].filter(([, e]) => e.wanted === 0 && !e.inflight)
    for (const [key] of idle.slice(0, Math.max(0, idle.length - UNWANTED_KEPT))) entries.delete(key)
  }

  /** Whether `entry` has been asked as of the current generation: answered, on its way, or failed
   *  (a failure waits for the next change). */
  const asked = (entry: Entry) => (entry.inflight ? entry.askedAt : Math.max(entry.generation, entry.failedAt)) >= generation
  /** Whether the latest request for `entry` failed: a failure newer than its answer. */
  const failedLast = (entry: Entry) => !entry.inflight && entry.failedAt > entry.generation

  const keep = (entry: Entry, answer: unknown, request: number, asOf: number) => {
    if (request < entry.answeredBy) return
    entry.answer = answer
    entry.answeredBy = request
    entry.generation = Math.max(entry.generation, asOf)
    notify()
  }

  function ask(entry: Entry): void {
    if (entry.inflight || asked(entry)) return
    const request = ++requests
    const asOf = generation
    entry.inflight = true
    entry.askedAt = asOf
    transport(entry.question).then(
      (answer) => {
        entry.inflight = false
        keep(entry, answer, request, asOf)
        askAgainIfMoved(entry)
      },
      (e: unknown) => {
        entry.inflight = false
        // Nothing to apply — the view keeps what it had, and is told the read failed — and nothing
        // retried until the log changes or a view asks for it again.
        entry.failedAt = Math.max(entry.failedAt, asOf)
        entry.failure = e instanceof Error ? e.message : String(e)
        notify()
        askAgainIfMoved(entry)
      },
    )
  }

  /** The log moved while a request was out: ask once more for the views still showing it. */
  function askAgainIfMoved(entry: Entry): void {
    if (entry.wanted > 0 && !asked(entry)) ask(entry)
    else evictUnwanted()
  }

  /** Re-ask the wanted questions the current generation has left stale — once per turn. */
  function schedule(): void {
    if (scheduled) return
    scheduled = true
    void Promise.resolve().then(() => {
      scheduled = false
      for (const entry of entries.values()) if (entry.wanted > 0) ask(entry)
    })
  }

  function entryFor(q: LogQuestion): Entry {
    const key = questionKey(q)
    let entry = entries.get(key)
    if (!entry) {
      entry = { question: q, answeredBy: 0, generation: -1, failedAt: -1, wanted: 0, inflight: false, askedAt: -1 }
      entries.set(key, entry)
    }
    return entry
  }

  /** Where `entry` stands. A failure is said only to the views showing the question: a view
   *  opening on it asks again, so it reads as asking (or stale) from its first render. */
  function stateOf(entry: Entry | undefined): LogAnswerState {
    if (!entry) return 'asking'
    if (entry.wanted > 0 && failedLast(entry) && entry.failedAt >= generation) return 'failed'
    if (entry.answeredBy === 0) return 'asking'
    return entry.generation >= generation ? 'current' : 'stale'
  }

  return {
    peek<Q extends LogQuestion>(q: Q) {
      return entries.get(questionKey(q))?.answer as AnswerTo<Q> | undefined
    },
    status(q) {
      return stateOf(entries.get(questionKey(q)))
    },
    failure(q) {
      const entry = entries.get(questionKey(q))
      return stateOf(entry) === 'failed' ? entry?.failure : undefined
    },
    async ask<Q extends LogQuestion>(q: Q) {
      const request = ++requests
      const asOf = generation
      const answer = await transport(q)
      keep(entryFor(q), answer, request, asOf)
      return answer
    },
    want(q) {
      const entry = entryFor(q)
      entry.wanted++
      // A view opening on a question whose latest request failed asks it again: reopening a view is
      // how an operator asks again, and no view still shows the failure.
      if (entry.wanted === 1 && failedLast(entry)) entry.failedAt = -1
      ask(entry)
      let released = false
      return () => {
        if (released) return
        released = true
        entry.wanted--
        if (entry.wanted === 0) evictUnwanted()
      }
    },
    follow(logTick) {
      // A tick the answers already reflect is not a change; `undefined` is a reader that cannot
      // tell (no snapshot yet), answered with a refresh.
      if (logTick !== undefined && logTick === lastTick) return
      if (logTick !== undefined) lastTick = logTick
      generation++
      // Every kept answer is now from before this change: stale until its fresh one lands.
      notify()
      schedule()
    },
    subscribe(listener) {
      listeners.add(listener)
      return () => {
        listeners.delete(listener)
      }
    },
    refresh() {
      generation++
      notify()
      schedule()
    },
  }
}
