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
import { CQWW_RTTY_US_QTH, CQWW_RTTY_VE_QTH } from './cqwwRttyQth'
import { ILQP_COUNTIES, ILQP_MULTS } from './ilqpQth'

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

/** CQ WW RTTY's W/VE QTHs — its QTH multiplier's universe, grouped by call prefix (a token,
 *  never translated). The list is a guarded mirror of the seed (see `cqwwRttyQth.ts`). */
const CQWW_RTTY_QTH: ContestDomain = {
  codes: new Set([...CQWW_RTTY_US_QTH, ...CQWW_RTTY_VE_QTH].map((q) => q.code)),
  groups: [
    { label: 'W', values: CQWW_RTTY_US_QTH },
    { label: 'VE', values: CQWW_RTTY_VE_QTH },
  ],
  total: CQWW_RTTY_US_QTH.length + CQWW_RTTY_VE_QTH.length,
}

/** ⭐ The Illinois QSO Party's 102 counties — the sponsor's own chart, mirrored in
 *  `ilqpQth.ts`. One block: the chart is alphabetical and has no grouping of its own, and
 *  inventing one (by region, by call area) would be this build's grouping shown as the
 *  sponsor's. */
const IL_COUNTIES: ContestDomain = {
  codes: new Set(ILQP_COUNTIES.map((c) => c.code)),
  groups: [{ label: 'IL', values: ILQP_COUNTIES }],
  total: ILQP_COUNTIES.length,
}

/** The Canadian half of `il_mults`, by code — the sponsor's own NF/LAB/PEI spellings
 *  beside the modern NL/PE, since it publishes no exchange code list. Listed rather than
 *  derived: every rule that could separate these from a US state (two letters, a set of
 *  prefixes) is a rule that would silently move one the day a code changes. */
const VE_CODES = new Set([
  'NL', 'NF', 'LAB', 'PE', 'PEI', 'NS', 'NB', 'QC', 'ON', 'MB', 'SK', 'AB', 'BC', 'NT', 'NU', 'YT',
])

/** What a station outside Illinois sends — the states and DC, then the Canadian codes.
 *  The two block labels are call-area TOKENS, never translated, the same convention the
 *  CQ WW RTTY board uses. */
const IL_MULTS: ContestDomain = {
  codes: new Set(ILQP_MULTS.map((c) => c.code)),
  groups: [
    { label: 'W', values: ILQP_MULTS.filter((c) => !VE_CODES.has(c.code)) },
    { label: 'VE', values: ILQP_MULTS.filter((c) => VE_CODES.has(c.code)) },
  ],
  total: ILQP_MULTS.length,
}

const DOMAINS: Record<string, ContestDomain> = {
  fd_sections: FD_SECTIONS,
  // The plain 85-section universe under its own Rust id, same table.
  arrl_sections: FD_SECTIONS,
  cqww_rtty_qth: CQWW_RTTY_QTH,
  il_counties: IL_COUNTIES,
  il_mults: IL_MULTS,
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

/** A value's name, folded for matching: upper-case, and without the punctuation and
 *  spacing an operator will not type ("St. Clair" → "STCLAIR", "Jo Daviess" → "JODAVIESS",
 *  which is also how the ILQP chart itself writes it). */
const nameKey = (s: string): string => s.toUpperCase().replace(/[^A-Z0-9]/g, '')

/**
 * ⭐ **What the operator might mean by what they have typed so far** — the type-ahead
 * behind an exchange box, over every universe the slot draws on.
 *
 * A county exchange is four letters an operator has to know (`JODA`, `SCLA`, `MCDN`), and
 * the name is the part they actually know. So a code prefix and a name prefix both match,
 * in that order: `CO` offers COOK and COLE before Coles-by-name, and `Cook` offers COOK.
 * A name that merely CONTAINS the text is offered last, because `SALINE` is a real answer
 * to "lin" and a poor one to "SA".
 *
 * Empty for an empty box (a list of 102 counties is not a suggestion) and for a domain
 * this build carries no table for — never a guess.
 */
export function domainSuggestions(
  domainIds: string[] | undefined,
  typed: string,
  limit = 6,
): DomainValue[] {
  const q = typed.trim().toUpperCase()
  if (q === '' || !domainIds?.length) return []
  const qk = nameKey(q)
  const seen = new Set<string>()
  const out: DomainValue[] = []
  const take = (pick: (v: DomainValue) => boolean) => {
    for (const id of domainIds) {
      for (const g of contestDomain(id)?.groups ?? []) {
        for (const v of g.values) {
          if (out.length >= limit) return
          if (seen.has(v.code) || !pick(v)) continue
          seen.add(v.code)
          out.push(v)
        }
      }
    }
  }
  take((v) => v.code.startsWith(q))
  take((v) => nameKey(v.name).startsWith(qk))
  take((v) => nameKey(v.name).includes(qk))
  return out
}

/**
 * ⭐ **The CODE the operator means by `typed`, when exactly one value can be meant** —
 * `Cook` → `COOK` — and `undefined` when more than one can, so nothing is ever guessed
 * onto the air.
 *
 * A value that is already a code is returned as itself (upper-cased), so this is safe to
 * run over a box that holds `COOK` as well as one that holds `cook county`. A name has to
 * match in full or be an unambiguous prefix: `Ma` is Macon, Macoupin, Madison, Marion,
 * Marshall, Mason and Massac, so it resolves to nothing at all.
 */
export function resolveDomainValue(
  domainIds: string[] | undefined,
  typed: string,
): string | undefined {
  const q = typed.trim().toUpperCase()
  if (q === '' || !domainIds?.length) return undefined
  const values: DomainValue[] = []
  for (const id of domainIds) {
    for (const g of contestDomain(id)?.groups ?? []) values.push(...g.values)
  }
  if (values.some((v) => v.code === q)) return q
  const qk = nameKey(q)
  const exact = values.filter((v) => nameKey(v.name) === qk)
  if (exact.length === 1) return exact[0].code
  const prefix = values.filter((v) => nameKey(v.name).startsWith(qk))
  return prefix.length === 1 ? prefix[0].code : undefined
}
