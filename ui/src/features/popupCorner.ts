// #445: THE POP-UP CORNER, as a validated enum. Settings ▸ Spots & Alerts ▸ Alerts ▸ Pop-up corner
// stores one of four corners; the toast column (Toasts.tsx, toast.ts) stands in it. The wire values
// are the settings file's (`PopupCorner` in tempo-app, which loads anything else as bottom right too).

export type PopupCorner = 'bottom-right' | 'bottom-left' | 'top-right' | 'top-left'

const POPUP_CORNERS: readonly PopupCorner[] = ['bottom-right', 'bottom-left', 'top-right', 'top-left']

/** The corner a stored value names; anything this build does not know (absent, a later build's
 *  corner, junk) is bottom right, where the pop-ups always stood. */
export function popupCornerOf(raw: unknown): PopupCorner {
  return POPUP_CORNERS.includes(raw as PopupCorner) ? (raw as PopupCorner) : 'bottom-right'
}

// The corner now, per window. Kept HERE rather than on the toast bus (toast.ts): the column reads
// it, and a test that stands in for the bus must not have to know about it.
let popupCorner: PopupCorner = 'bottom-right'
const listeners = new Set<() => void>()

/** Where the pop-ups stand. Each window sets its own from its settings (App, DetachedPanel), as it
 *  does the pop-up switch. */
export function setPopupCorner(raw: unknown): void {
  const next = popupCornerOf(raw)
  if (next === popupCorner) return
  popupCorner = next
  for (const fn of listeners) fn()
}

/** The corner now, for `useSyncExternalStore` (Toasts.tsx). */
export function getPopupCorner(): PopupCorner {
  return popupCorner
}

export function subscribePopupCorner(fn: () => void): () => void {
  listeners.add(fn)
  return () => {
    listeners.delete(fn)
  }
}
