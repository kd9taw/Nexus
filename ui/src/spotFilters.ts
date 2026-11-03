// The filter rules the Spots board and the Needed board SHARE — one definition, so the same chip
// on the two boards can never answer the same row differently (operator: "matching filters between
// Needed and Spots"). Pure: no React, no IO. The chips that drive them are SharedFilterChips.tsx.
//
// What is shared, and why only this much:
//  * My privileges — the row's `licensed` flag, computed backend-side from the SAME licence tables
//    as the TX lockout, for a spot and for a need alike. Hides only an explicit `false`: a row
//    nothing judged (an older station) is kept, never silently dropped. The Open class (outside
//    the US) has every row licensed, so the chip hides nothing for it, on either board.
//  * US state — the row's resolved state. A chosen state hides a row whose state is unknown.
//  * Search — space-separated terms that AND together over the row flattened to one string
//    (`searchQuery.ts` owns what one term means, `PA*` included).
//  * Band — the multi-select both boards always had.
// The rest of each board's bar belongs to that board alone: need types are the Needed board's
// whole subject; Hide worked, the spotted-from chips and a pane's band-follow are the firehose's
// (the Needed board's header comment says why each stays off it).

import { compileTerm, searchTerms } from './searchQuery'

/** "My privileges": with the chip on, a row the operator may not transmit to is hidden. */
export function privilegeOk(licensedOnly: boolean, licensed: boolean | undefined): boolean {
  return !licensedOnly || licensed !== false
}

/** US-state chips: none picked keeps every row; picked, only rows in a picked state. */
export function stateOk(states: readonly string[], state: string | null | undefined): boolean {
  return states.length === 0 || (!!state && states.includes(state))
}

/** Band chips: none picked keeps every row. */
export function bandOk(bands: readonly string[], band: string): boolean {
  return bands.length === 0 || bands.includes(band)
}

/** The search box: `null` for an empty box (keep every row), else a test of a row's fields — every
 *  term must match somewhere in them. Compiled once per query, not per row. */
export function searchMatcher(query: string): ((fields: readonly string[]) => boolean) | null {
  const terms = searchTerms(query).map(compileTerm)
  if (terms.length === 0) return null
  return (fields) => {
    const hay = fields.join(' ').toUpperCase()
    return terms.every((t) => t(hay))
  }
}

/** The states present in a list of rows, sorted — a chip that can never match is a trap. */
export function presentStates(rows: readonly { state?: string | null }[]): string[] {
  const set = new Set<string>()
  for (const r of rows) if (r.state) set.add(r.state)
  return [...set].sort()
}
