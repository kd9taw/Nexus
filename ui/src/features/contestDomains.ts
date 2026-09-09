// ---------------------------------------------------------------------------
// THE DOMAIN REGISTRY — legal values and their grouping, by `Domain::id`.
//
// Two surfaces read it and neither may re-derive the other's answer:
//
//   • the ENTRY STRIP's while-typing verdict, which must cost ZERO IPC. It runs on
//     every keystroke, and the snapshot already carries the whole log; shipping 85
//     section codes on every 300 ms poll to answer a question a static table answers
//     is a cost with no buyer. The DTO carries the domain ID; the values are here.
//   • the MULTIPLIER BOARD's cell universe — which values exist to be coloured in,
//     and how they group into blocks.
//
// A domain with no entry here is not an error: the strip falls back to "required means
// non-empty" (never an approximated verdict) and a board renders what has been worked.
// That is the honest behaviour for a county list this build does not carry yet.
//
// ⚠️ EVERY VALUE HERE IS AN INVARIANT TECHNICAL TOKEN — a section code, a county
// abbreviation. Codes are never translated and never locale-formatted. The `name` beside
// a code is prose (it reaches a tooltip) and is the ONE thing here a catalog could ever
// own; it is not one today, exactly as the shipped sections board has always had it.
//
// The section universe itself is `arrlSections.ts`, unchanged — this module names it
// under the id Rust knows it by, so the two surfaces agree about which domain is which.
// ---------------------------------------------------------------------------

import { ARRL_SECTIONS_BY_DIVISION, ARRL_SECTION_TOTAL } from './arrlSections'

/** One cell of a board / one legal value of a slot. */
export interface DomainValue {
  code: string
  /** The full name, for a tooltip. Prose; the CODE is the invariant token. */
  name: string
}

/** A named group of values — one visual block on a board (an ARRL division). */
export interface DomainGroup {
  label: string
  values: DomainValue[]
}

export interface ContestDomain {
  /** Membership, uppercased — the while-typing verdict. */
  codes: Set<string>
  /** The board's blocks, in display order. */
  groups: DomainGroup[]
  /** The board's "N of TOTAL" denominator. */
  total: number
}

/**
 * ⚠️ **`fd_sections` is the ARRL/RAC section list AS THE UI HAS ALWAYS HELD IT** —
 * the 85 codes of `arrlSections.ts`, grouped by division.
 *
 * Rust's `fd_sections` domain is those 85 **plus `MX` and `DX`** (Winter Field Day's
 * "Location Identifier" rule). The divergence predates this module: the shipped strip
 * validated against exactly this set, so a strip that suddenly accepted `DX` would be a
 * behaviour change in a batch whose whole contract is that Field Day behaviour does not
 * move. The two are reconciled where the section universe is, not here.
 */
const FD_SECTIONS: ContestDomain = {
  codes: new Set(ARRL_SECTIONS_BY_DIVISION.flatMap((d) => d.sections).map((s) => s.code)),
  groups: ARRL_SECTIONS_BY_DIVISION.map((d) => ({
    label: d.division,
    values: d.sections.map((s) => ({ code: s.code, name: s.name })),
  })),
  total: ARRL_SECTION_TOTAL,
}

const DOMAINS: Record<string, ContestDomain> = {
  fd_sections: FD_SECTIONS,
  // The plain 85-section universe under its own Rust id, same table.
  arrl_sections: FD_SECTIONS,
}

/** The domain behind an id, or `undefined` when this build carries no value set for it. */
export function contestDomain(id: string | undefined): ContestDomain | undefined {
  return id ? DOMAINS[id] : undefined
}

/**
 * Is `value` a legal member of `domainId`?
 *
 * `true` for an unknown domain — an absent value set is not evidence that a value is
 * wrong, and the hard gate is the engine's, at log time. Refusing what we cannot check
 * would block a legal contact; the strip's job here is to catch a typo, not to be the
 * authority.
 */
export function inDomain(domainId: string | undefined, value: string): boolean {
  const d = contestDomain(domainId)
  return d ? d.codes.has(value.trim().toUpperCase()) : true
}
