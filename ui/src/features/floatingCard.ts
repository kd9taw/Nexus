// One floating card at a time: the station card (CallCard) and the Best times card (BestTimesCard)
// stand in the same place beside a call, so opening one closes the other. Each card registers its
// close here once and gets back the function to call when it opens, which closes every other card.
const closers = new Set<() => void>()

/** Register a card's close; returns what that card calls when it opens: close the others. */
export function registerFloatingCard(close: () => void): () => void {
  closers.add(close)
  return () => closers.forEach((c) => c !== close && c())
}

/** The Best times card's opener, registered by that card (BestTimesCard), so the station card can offer
 *  it without importing it (the Best times card is placed by the station card's own rule). */
let bestTimesOpener: ((call: string, anchor: HTMLElement) => void) | null = null

export function setBestTimesOpener(open: (call: string, anchor: HTMLElement) => void): void {
  bestTimesOpener = open
}

/** Open the Best times card for `call` beside `anchor`; false where none is registered. */
export function openBestTimesFrom(call: string, anchor: HTMLElement): boolean {
  if (!bestTimesOpener) return false
  bestTimesOpener(call, anchor)
  return true
}
