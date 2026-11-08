// One floating card at a time: the station card (CallCard) and the Best times card (BestTimesCard)
// stand in the same place beside a call, so opening one closes the other. Each card registers its
// close here once and gets back the function to call when it opens, which closes every other card.
const closers = new Set<() => void>()

/** Register a card's close; returns what that card calls when it opens: close the others. */
export function registerFloatingCard(close: () => void): () => void {
  closers.add(close)
  return () => closers.forEach((c) => c !== close && c())
}
