// Operator's custom order for the left nav rail's global-section icons (ModeNav's ITEMS —
// Connect, Needed, Spots, Logbook, Awards, Stats, …). The operating group (Phone/CW/Digital)
// and the Settings gear keep their fixed spec order; only the situational/logging sections
// reorder.
//
// SHARED across windows — the rail order is a person/station preference, the SAME in every
// window — so this uses a plain global key, NOT windowScope/surfaceKey (which would make each
// pop-out remember its own rail order).
const KEY = 'nexus.navOrder'

/**
 * Reorder `defaultIds` (the section order shipped in the build) by the operator's `saved` order.
 *
 * Ids present in `saved` lead, in that order. Any id NOT in `saved` — a section added in a later
 * release, or one the operator never dragged — keeps its default relative position AFTER them, so
 * a new section can never vanish just because it isn't in an old saved order. Ids in `saved` that
 * no longer exist are dropped.
 */
export function orderNav(defaultIds: string[], saved: string[]): string[] {
  const known = saved.filter((id) => defaultIds.includes(id))
  const seen = new Set(known)
  const rest = defaultIds.filter((id) => !seen.has(id))
  return [...known, ...rest]
}

/**
 * Move `id` to just before `beforeId` (drop-on-target), or to the end when `beforeId` is null
 * (drop past the last item). Pure — the caller persists and re-renders. A no-op if `id` isn't
 * present or `beforeId` is unknown falls back to appending, so a bad drag never loses an item.
 */
export function moveNav(ids: string[], id: string, beforeId: string | null): string[] {
  if (!ids.includes(id)) return ids
  const without = ids.filter((x) => x !== id)
  if (beforeId == null || beforeId === id) return [...without, id]
  const idx = without.indexOf(beforeId)
  if (idx < 0) return [...without, id]
  return [...without.slice(0, idx), id, ...without.slice(idx)]
}

export function loadNavOrder(): string[] {
  try {
    const raw = localStorage.getItem(KEY)
    const arr = raw ? JSON.parse(raw) : null
    return Array.isArray(arr) ? arr.filter((x): x is string => typeof x === 'string') : []
  } catch {
    return []
  }
}

export function saveNavOrder(order: string[]): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(order))
  } catch {
    /* storage full/unavailable — the order still applies for this session */
  }
}

export function resetNavOrder(): void {
  try {
    localStorage.removeItem(KEY)
  } catch {
    /* ignore */
  }
}
