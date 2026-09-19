// THE LOGBOOK, HELD ONCE PER WINDOW (the big-log fix, 2026-09-19).
//
// Every view that reads the log used to fetch all of it for itself: the three hidden log strips
// (RTTY, PSK, JS8) at startup, the Operate callsign card about twice per FT8 contact, the Logbook
// on every change, a band-map pop-out every 15 s. At 150k QSOs one fetch is ~100 MB of JSON, so a
// window held four or more private copies and pulled another on nearly every contact.
//
// Now a window holds ONE copy, here:
//   - the first reader loads it — `get_log_delta(0, 0)` answers with the whole log;
//   - it follows the snapshot's `logTick`, which the engine moves on every change to the log. A
//     reader reporting a tick the copy has not answered costs ONE `get_log_delta`: the rows
//     appended since the copy (a logged contact) or the whole log again (an edit, a delete, an
//     import, a stamp — anything that is not a pure append);
//   - readers share one request. Asks made in the same turn become one request, and ticks that
//     land while a request is out are answered by one follow-up, never one each.
//
// A reader with no tick to report asks for a refresh when it starts reading: nothing guarantees
// a reader that follows the tick is mounted, and a copy with nothing new costs the engine an
// empty answer. The views that read once and keep what they got (Awards, Statistics, the map
// coverage layers) do exactly that through `loadSharedLog`.
//
// The copy is SHARED, so it is read-only to every reader: sort or filter into a new array.
//
// Nothing here fetches on its own. Remote mode keeps its guards at the readers — each passes
// `enabled = false` there, and `refreshSharedLog`/`loadSharedLog` are reached only from native
// paths.

import { useEffect, useSyncExternalStore } from 'react'
import { getLogDelta } from '../api'
import type { LoggedQso } from '../types'

/** The log is not loaded (or this reader is disabled): one stable empty array, so a reader's
 *  memo keyed on the log does not recompute on every render. Shared — never mutate it. */
export const NO_LOG: LoggedQso[] = []

let rows: LoggedQso[] | null = null
/** The engine revision `rows` answers — handed back on the next request. */
let revision = 0
/** The latest `logTick` a reader reported (undefined until one does). */
let seenTick: number | undefined
/** The `seenTick` the last finished request started under. A reported tick that differs asks
 *  for a refresh. A FAILED request records its tick too, so a failure waits for the next change
 *  instead of retrying in a loop. */
let askedTick: number | undefined
/** A request is owed regardless of ticks: the first load, a tick-less reader starting, or a
 *  caller's own write (`refreshSharedLog`). */
let owed = false
let inflight: Promise<void> | null = null
let scheduled = false
let lastError: unknown = null
/** Bumped by the test reset, so a request from before it cannot write into the fresh store. */
let generation = 0
const listeners = new Set<() => void>()

function subscribe(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

const wanted = () => owed || (seenTick !== undefined && seenTick !== askedTick)

/** Start a request once the current turn's asks are all in: readers that mount in one commit
 *  report their ticks before any of them is answered, so they share one request. */
function schedule(): void {
  if (scheduled) return
  scheduled = true
  void Promise.resolve().then(() => {
    scheduled = false
    pump()
  })
}

function pump(): void {
  if (inflight || !wanted()) return
  owed = false
  inflight = request(generation, seenTick)
}

async function request(gen: number, tick: number | undefined): Promise<void> {
  // Suspend before anything can settle this request: a getLogDelta that throws synchronously
  // would otherwise clear `inflight` before pump() has stored it, and the store would wait
  // forever on a request that already finished.
  await null
  if (gen !== generation) return
  try {
    const d = await getLogDelta(revision, rows?.length ?? 0)
    if (gen !== generation) return
    // Every reader in the window gets this answer, so a malformed one is refused here rather
    // than handed to all of them.
    if (!Array.isArray(d?.rows)) throw new Error('get_log_delta answered without rows')
    const next = d.full || rows === null ? d.rows : d.rows.length > 0 ? rows.concat(d.rows) : rows
    revision = d.revision
    lastError = null
    if (next !== rows) {
      rows = next
      for (const listener of listeners) listener()
    }
  } catch (e) {
    if (gen !== generation) return
    lastError = e
  }
  askedTick = tick
  inflight = null
  pump() // ticks and asks that landed while this one was out
}

/** A reader started reading, or its tick moved. */
function report(logTick: number | undefined): void {
  if (logTick === undefined) {
    // No tick to compare: this reader cannot tell a current copy from an old one, so it asks —
    // unless a request is already out, which is as fresh as anything it could ask for.
    if (!inflight) owed = true
  } else {
    seenTick = logTick
    if (rows === null && !inflight) owed = true
  }
  schedule()
}

/**
 * The window's copy of the log, or null until it has loaded (and always null while
 * `enabled` is false — no request is made for a disabled reader).
 *
 * `logTick` is the snapshot's `logTick`: pass it and the copy follows every change to the log.
 * A reader with no snapshot passes undefined and gets a refresh when it starts reading.
 */
export function useSharedLog(logTick: number | undefined, enabled = true): LoggedQso[] | null {
  const log = useSyncExternalStore(subscribe, () => (enabled ? rows : null))
  useEffect(() => {
    if (enabled) report(logTick)
  }, [enabled, logTick])
  return log
}

/** The log for a reader that reads it ONCE — a view's mount, a map layer switched on — and keeps
 *  what it got. It has no tick either, so it is treated like a tick-less reader starting: a
 *  refresh unless a request is already out, then whatever that answered. Rejects only when there
 *  is no copy at all to give. */
export async function loadSharedLog(): Promise<LoggedQso[]> {
  report(undefined)
  await null // let the scheduled pump start its request
  while (inflight) await inflight
  if (rows === null) throw lastError ?? new Error('the log could not be read')
  return rows
}

/** The caller just changed the log (a contact logged, a row edited or deleted, a file imported):
 *  bring the copy up to date now rather than on the next snapshot's tick. */
export function refreshSharedLog(): void {
  owed = true
  schedule()
}

/** Tests: forget the copy and every reader. src/test-setup.ts runs this after every test, so a
 *  log one test loaded can never answer the next one's first read. */
export function __resetSharedLogForTests(): void {
  generation++
  rows = null
  revision = 0
  seenTick = undefined
  askedTick = undefined
  owed = false
  inflight = null
  scheduled = false
  lastError = null
  listeners.clear()
}

// Registered with src/test-setup.ts's after-each reset when that file has installed the set; in
// the app nothing has, and this is a no-op. (The setup file cannot import this module itself:
// Vitest does not mock what a setup file imported, so `../api` here would be the real one.)
;(globalThis as { __nexusTestResets?: Set<() => void> }).__nexusTestResets?.add(__resetSharedLogForTests)
