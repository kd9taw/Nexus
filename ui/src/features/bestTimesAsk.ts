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

/** The radio's mode in this window as Best times names it (features/bestTimes `radioModeOf`), kept
 *  by the window that has the snapshot (App, a pop-out): the card starts on it. */
let radioMode: string | null = null

export function noteRadioMode(mode: string | null): void {
  radioMode = mode
}

export function radioModeNow(): string | null {
  return radioMode
}

let pending: string | null = null
let asks = 0
const listeners = new Set<() => void>()

/** Hand `call` to this window's Best times pane. */
export function askBestTimes(call: string): void {
  pending = call.trim().toUpperCase()
  asks++
  listeners.forEach((l) => l())
}

let brought = 0

/** Whether a call has been handed over that no Conditions view has brought the box on screen for
 *  yet; true once per hand-off. Kept here, not in the view, so a view that mounts later (a switch
 *  back to Conditions) does not bring the box back for a hand-off already handled. */
export function claimAsk(): boolean {
  if (brought === asks) return false
  brought = asks
  return true
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

/** The hand-off to the Conditions window (See full day): a shared key that window reads when it opens,
 *  and hears when it is already open (another window's write fires its `storage` event). */
const HANDOFF_KEY = 'nexus.bestTimes.handoff'
/** A hand-off older than this is not for a window opening now. */
const HANDOFF_MS = 30_000

export function handOff(call: string): void {
  try {
    localStorage.setItem(HANDOFF_KEY, JSON.stringify({ call: call.trim().toUpperCase(), at: Date.now() }))
  } catch {
    /* storage blocked — the window opens without the call */
  }
}

/** Take a fresh hand-off, once: the call, or null. */
export function takeHandoff(): string | null {
  try {
    const raw = localStorage.getItem(HANDOFF_KEY)
    if (!raw) return null
    localStorage.removeItem(HANDOFF_KEY)
    const h = JSON.parse(raw) as { call?: unknown; at?: unknown }
    return typeof h.call === 'string' && typeof h.at === 'number' && Date.now() - h.at < HANDOFF_MS ? h.call : null
  } catch {
    return null
  }
}

/** In the Conditions window: hand every See full day to this window's Best times box. */
export function listenForHandoff(): () => void {
  const take = () => {
    const c = takeHandoff()
    if (c) askBestTimes(c)
  }
  take()
  const on = (e: StorageEvent) => {
    if (e.key === HANDOFF_KEY && e.newValue) take()
  }
  window.addEventListener('storage', on)
  return () => window.removeEventListener('storage', on)
}

/** The call handed over and not yet taken by a box (the Conditions view puts the box on screen). */
export function pendingAsk(): string | null {
  return pending
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
  asks = 0
  brought = 0
}
