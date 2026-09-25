// THE OLD HIDDEN "WANTED" LIST JOINS THE WATCH LIST, ONCE (operator, 2026-09-24: "One list").
//
// `settings.wantedCalls` was the first watch list: calls and trailing-star prefixes that put a
// station at the top of the Needed board. The watch list (Settings ▸ Spots & Alerts, this app's
// `watchlist.ts`) replaced its editor on 2026-07-10 — but the board went on reading it, so an
// operator who had typed entries before then kept a list they could neither see nor change. The
// board now reads the watch list alone, and on the first launch after the upgrade this adds the
// old list's entries to it and retires the old list.
//
// EXACTLY ONCE, and never at the cost of an entry:
//   - the watch list holding them is written to disk (ui-state.json) FIRST, and only then is the
//     old list emptied, through its one writer (`retire_wanted_calls`). A launch that could not
//     write the watch list leaves the old list alone, and the next launch folds it again;
//   - an entry already on the watch list is not added twice, so folding again adds nothing;
//   - once the old list is empty there is nothing to fold, so an entry the operator removes from
//     the watch list afterwards stays removed. (The engine keeps a form save from writing the old
//     list back; a backup RESTORE does bring it back, and the next launch folds it — the same
//     decision loading that backup's settings.json makes.)
//
// The desktop's main window only (App, not on a Remote browser): the watch list lives in this
// computer's ui-state.json, and a Remote browser's list is its own.

import { getSettings, retireWantedCalls } from '../api'
import { t } from '../i18n'
import { pushToast } from '../toast'
import { flushDurable } from './durableStore'
import { loadWatchlist, newWatchFilter, saveWatchlist, WATCHLIST_CHANGED } from '../watchlist'

/** How long the one-time notice stays up: it names entries the operator did not add today. */
const NOTICE_MS = 15_000
/** Attempts at writing the watch list to disk before leaving the old list for the next launch —
 *  one more than one, because a flush already under way when ours starts declines to run. */
const FLUSH_ATTEMPTS = 3
const FLUSH_RETRY_MS = 300

/**
 * What an old entry means on the watch list, or null when it named no station. The old list
 * matched a call EXACTLY, or — with a trailing `*` and nothing else starred — as a PREFIX; a bare
 * `*`, a doubled one or a star anywhere else matched nothing at all. The watch list's star is a
 * glob, so `*` there is EVERY station: such an entry is dropped, never widened into a siren.
 */
export function watchEntryFor(old: string): string | null {
  const v = old.trim().toUpperCase()
  const star = v.indexOf('*')
  if (star < 0) return v === '' ? null : v
  return star === v.length - 1 && star > 0 ? v : null
}

let running: Promise<void> | null = null

/** Fold the old wanted list into the watch list, once per launch however many callers ask (React
 *  runs a mount effect twice in development). Never throws. */
export function foldRetiredWantedList(): Promise<void> {
  running ??= fold().catch(() => {})
  return running
}

async function fold(): Promise<void> {
  let old: string[]
  try {
    old = (await getSettings()).wantedCalls ?? []
  } catch {
    return // no station to ask: a browser preview, a test
  }
  if (!Array.isArray(old) || old.length === 0) return
  const list = loadWatchlist()
  const have = new Set(list.filter((f) => f.kind === 'call').map((f) => f.value.trim().toUpperCase()))
  const added: string[] = []
  for (const entry of old) {
    const value = typeof entry === 'string' ? watchEntryFor(entry) : null
    if (value === null || have.has(value)) continue
    have.add(value)
    added.push(value)
  }
  if (added.length > 0) {
    saveWatchlist([...list, ...added.map((v) => newWatchFilter('call', v))])
    window.dispatchEvent(new Event(WATCHLIST_CHANGED))
  }
  // On disk before the old list goes — see the header.
  let durable = false
  for (let attempt = 0; attempt < FLUSH_ATTEMPTS && !durable; attempt++) {
    if (attempt > 0) await new Promise((r) => setTimeout(r, FLUSH_RETRY_MS))
    durable = await flushDurable()
  }
  if (!durable) return
  await retireWantedCalls()
  if (added.length > 0)
    pushToast(t('watchlist.folded', { count: added.length, entries: added.join(', ') }), 'info', NOTICE_MS)
}

/** Test seam: a fresh launch. */
export function __resetWatchlistFoldForTest(): void {
  running = null
}
