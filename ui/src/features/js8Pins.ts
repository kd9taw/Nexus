// ★ PINNED CALLS in the JS8 stations roster — "keep this one at the top while I work it".
//
// JS8Call's call-activity table opens with an icon column (mainwindow.cpp:10225): a flag for a
// stored message, ★ for a station that has acknowledged something of yours, ☎ for a recent CQ,
// and it sorts the rows with waiting messages to the top. Two of those four are DERIVED from
// state Nexus's `Js8Heard` does not carry (there is no ack timestamp and no relay-through
// field), so a faithful copy of that column would sit blank forever. What an operator actually
// asked for out of it — hold a watched call at the top of a roster that re-sorts under him
// every period — is an operator PIN, and that is what this is. See tasks/js8-activity-parity.md.
//
// Deliberately NOT the watch list (`watchlist.ts`): that is a standing statement about a call
// or an entity, alerts on it, and survives a reinstall. A pin is about who is on the band right
// now — it costs one click to remake and means nothing tomorrow — so it stays in localStorage
// with the other preferences rather than joining DURABLE_KEYS. It is SHARED across windows
// (classified in storage-scope.test.ts) for the same reason the chase star is: which calls this
// operator is watching is a fact about the operator, not about a window.

export const JS8_PINS_KEY = 'nexus.js8.pins'

/** Parse the stored blob (space-separated calls) into normalized, de-duplicated entries. */
export function parseJs8Pins(raw: string | null | undefined): string[] {
  if (!raw) return []
  const seen = new Set<string>()
  return raw
    .split(/[\s,;]+/)
    .map((c) => c.trim().toUpperCase())
    .filter((c) => c.length > 0 && !seen.has(c) && (seen.add(c), true))
}

export function loadJs8Pins(): string[] {
  try {
    return parseJs8Pins(window.localStorage.getItem(JS8_PINS_KEY))
  } catch {
    return []
  }
}

export function saveJs8Pins(calls: readonly string[]): void {
  try {
    window.localStorage.setItem(JS8_PINS_KEY, calls.join(' '))
  } catch {
    /* full / unavailable — the pin still applies for this session, held in component state */
  }
}

/** Add or remove one call, returning the new list. Pure; the caller persists it. */
export function toggleJs8Pin(pins: readonly string[], call: string): string[] {
  const c = call.trim().toUpperCase()
  if (!c) return [...pins]
  return pins.includes(c) ? pins.filter((p) => p !== c) : [...pins, c]
}

/**
 * Pinned calls first, everything else in the order the engine gave — a STABLE partition, so a
 * roster that is not pinned at all renders in exactly the engine's order and a pinned one only
 * lifts the pins out. Sorting the whole list would fight the engine's own most-recent ordering.
 */
export function sortPinnedFirst<T extends { call: string }>(rows: readonly T[], pins: readonly string[]): T[] {
  if (pins.length === 0) return [...rows]
  const set = new Set(pins)
  const pinned = rows.filter((r) => set.has(r.call.toUpperCase()))
  const rest = rows.filter((r) => !set.has(r.call.toUpperCase()))
  return [...pinned, ...rest]
}
