// THE CALLBOOK ANSWERS THIS WINDOW HAS — every answer a lookup brought back this session, by call.
//
// One store, written by the lookup itself (`qrzLookup` / `callbookRefresh` in api.ts), so every
// surface that shows a callbook answer sees one the window already has, whoever asked for it. The
// station card read only the answers its own Look up got, so a call Phone's log strip had just looked
// up opened on the callsign's initials and a Look up button, with the strip already showing the name
// (reported 2026-10-26: "the picture isn't being shown from QRZ").
//
// Only answers: a failed lookup keeps nothing, so the card's Look up stays there to try again. Nothing
// here asks the network; it only remembers what came back.
import type { QrzLookup } from '../types'

const answers = new Map<string, QrzLookup>()
const listeners = new Set<() => void>()

const key = (call: string) => call.trim().toUpperCase()

/** Keep the answer a lookup of `call` brought back. */
export function noteCallbookAnswer(call: string, answer: QrzLookup): void {
  answers.set(key(call), answer)
  listeners.forEach((l) => l())
}

/** The answer this window has for `call`, if any lookup brought one back this session. */
export function callbookAnswer(call: string): QrzLookup | null {
  return answers.get(key(call)) ?? null
}

/** Told whenever an answer is kept (for `useSyncExternalStore`). */
export function subscribeCallbookAnswers(l: () => void): () => void {
  listeners.add(l)
  return () => {
    listeners.delete(l)
  }
}
