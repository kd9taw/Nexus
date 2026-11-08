// Best times: who asks for which call, and the one callbook lookup an ask may make.
//
// THE ASK. The Conditions pane follows only its own box, a right-click choice and See full day — never
// the selected station, or every decode click would churn it. A call menu's "Best times" item, or the
// card's See full day, hands the call over here; the pane takes it on its next render. The last call is
// remembered per window, as a convenience (`surfaceGet`).
//
// THE LOOKUP. `get_best_times` never touches the network. When its answer rests only on the centre of
// the station's country and the operator asked explicitly (pressed Best times, chose the menu item),
// the UI asks the callbook ONCE for that call — the same `qrzLookup` the station card's Look up uses,
// which keeps its own cache and refusals and hands the location to the shack — and then asks again.
// Never on hover, never twice for one call in a window.
import { qrzLookup } from '../api'
import { surfaceGet, surfaceSet } from './windowScope'

const LAST_KEY = 'nexus.bestTimes.call'

let pending: string | null = null
const listeners = new Set<() => void>()

/** Hand `call` to this window's Best times pane. */
export function askBestTimes(call: string): void {
  pending = call.trim().toUpperCase()
  listeners.forEach((l) => l())
}

/** Take the call handed over, once. */
export function takeAsk(): string | null {
  const c = pending
  pending = null
  return c
}

export function subscribeAsk(l: () => void): () => void {
  listeners.add(l)
  return () => {
    listeners.delete(l)
  }
}

export function lastAsked(): string {
  return surfaceGet(LAST_KEY) ?? ''
}

export function rememberAsked(call: string): void {
  surfaceSet(LAST_KEY, call)
}

const looked = new Map<string, Promise<boolean>>()

/** Ask the callbook for `call` at most once in this window. True when it answered with a place. */
export function lookUpOnce(call: string): Promise<boolean> {
  const key = call.trim().toUpperCase()
  let p = looked.get(key)
  if (!p) {
    p = qrzLookup(key)
      .then((r) => Boolean(r.grid || (r.lat != null && r.lon != null)))
      .catch(() => false)
    looked.set(key, p)
  }
  return p
}

/** Whether `call` has been asked of the callbook in this window. */
export function lookedUp(call: string): boolean {
  return looked.has(call.trim().toUpperCase())
}

/** Tests only: forget every lookup and ask. */
export function resetBestTimesAsk(): void {
  looked.clear()
  pending = null
}
