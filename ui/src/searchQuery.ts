// What an operator's search box means, in one place, so two lists that both hold callsigns
// don't answer the same typing differently.
//
// The rules, and there are only two:
//   * a term containing `*` or `?` is a PREFIX/PATTERN and must match a whole WORD —
//     `PA*` finds PA0XYZ and PA3EGH, and deliberately does NOT find W1PA. That is the
//     point of the wildcard: an operator hunting a prefix wants the prefix, not the
//     letters loose in the middle of somebody else's call.
//   * a term with no wildcard is a plain SUBSTRING, because that is what everyone expects
//     of a search box: `4FD` finds 4X4FD, `finl` finds Finland in a row that carries it.
//
// `?` is one character, `*` is any run of them — DOS/fldigi/cluster convention, which is
// what hams already have in their fingers. Everything is compared upper-case; callsigns
// have no case and neither should the box.
//
// Combining terms is the CALLER's decision, not this module's, because the right answer
// depends on what the text is. Against a single field (the Stations list is callsigns and
// nothing else) two terms can only sensibly mean "either" — `PA* ON4*` is a list of
// prefixes you want, and AND-ing them would return nothing, ever. Against a row flattened
// from many fields (the Spots list carries call, entity, spotter, mode, band, frequency)
// two terms mean "narrow it down". So this module compiles ONE term and lets the list say
// how to join them.

/** Chars that turn a term from a substring into a pattern. */
const WILDCARD = /[*?]/

function escapeRegex(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

/** Split a query box into terms. Spaces and commas both separate — an operator writing
 *  `PA*, ON4*` and one writing `PA* ON4*` mean the same thing and should not have to know
 *  which one we parse. Uppercased here so every consumer compares the same way. */
export function searchTerms(query: string): string[] {
  return query.toUpperCase().split(/[\s,]+/).filter(Boolean)
}

/** Compile ONE term into a test against already-uppercased text. Compiled once per query
 *  rather than per row: a wildcard term builds a RegExp, and a 500-row list re-parsing that
 *  on every keystroke is the kind of thing that makes typing feel sticky. */
export function compileTerm(term: string): (upperText: string) => boolean {
  const t = term.toUpperCase()
  if (!WILDCARD.test(t)) return (text) => text.includes(t)
  const body = t
    .split(/([*?])/)
    .map((part) => (part === '*' ? '.*' : part === '?' ? '.' : escapeRegex(part)))
    .join('')
  // Anchored, and applied per WORD by the caller-facing helpers below.
  const re = new RegExp(`^${body}$`)
  return (text) => text.split(/\s+/).some((word) => re.test(word))
}

/** "Either of these" — the single-field list (Stations). Null when the box is empty, which
 *  is the caller's signal to skip filtering entirely rather than run a matcher that says
 *  yes to everything. */
export function matchAnyTerm(query: string): ((text: string) => boolean) | null {
  const tests = searchTerms(query).map(compileTerm)
  if (tests.length === 0) return null
  return (text) => {
    const upper = text.toUpperCase()
    return tests.some((test) => test(upper))
  }
}
