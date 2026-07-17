// Tiny app-wide screen-reader announcement bus (the audio sibling of toast.ts).
//
// `announce(text)` speaks through hidden aria-live regions rendered once by
// <Announcer/> (App). Inaudible without a screen reader running — safe to call
// unconditionally. Two channels:
// - polite (default): queued behind whatever the reader is saying (decode
//   summaries, view changes, QSO progress)
// - assertive: interrupts (TX state, safety events)
//
// Etiquette (the aria-live flooding lesson): callers rate-limit themselves —
// this bus also coalesces bursts within 150 ms into one utterance so a batch
// of announcements reads as a sentence, not a stutter.

type Listener = (polite: string, assertive: string) => void

let listener: Listener | null = null
let politeBuf: string[] = []
let assertiveBuf: string[] = []
let timer: number | null = null
// Alternating terminal space forces re-announcement when the same text repeats
// (readers skip identical consecutive live-region content).
let tick = false

function flush() {
  timer = null
  tick = !tick
  const pad = tick ? ' ' : ''
  const p = politeBuf.join('. ')
  const a = assertiveBuf.join('. ')
  politeBuf = []
  assertiveBuf = []
  listener?.(p ? p + pad : '', a ? a + pad : '')
}

/** Queue a screen-reader announcement (coalesced ~150 ms). */
export function announce(text: string, opts?: { assertive?: boolean }): void {
  const t = text.trim()
  if (!t) return
  if (opts?.assertive) assertiveBuf.push(t)
  else politeBuf.push(t)
  if (timer == null) timer = window.setTimeout(flush, 150)
}

/** <Announcer/> registration — exactly one live subscriber (the App mount). */
export function subscribeAnnouncements(l: Listener): () => void {
  listener = l
  return () => {
    if (listener === l) listener = null
  }
}
