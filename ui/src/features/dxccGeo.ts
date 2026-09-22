// The shared cty.dat GEOGRAPHY vocabulary — "is this station somewhere the operator cares
// about", asked once.
//
// WHY THIS MODULE EXISTS (#174). Three surfaces already ask a geographic question and each was
// growing its own answer: the Spots panel's "Spotted from" chips, Band Activity's
// hide-by-continent (`countryExclude.ts`, #229), and now the decode alerts. The continent list
// alone had two copies. The operator's ruling on this issue was explicit — one filtering
// vocabulary across the product, not a dialect per surface — so the codes and the predicates
// live here and every surface imports them.
//
// THE VOCABULARY IS cty.dat's OWN, in both halves:
//   - an ENTITY is the entity NAME, exactly the string `DecodeRow.country` / `Station.country` /
//     `SpotRow.entity` carry (there is no numeric DXCC id anywhere in this codebase — see
//     `countryExclude.ts` on why a name is the right and only identity);
//   - a CONTINENT is cty.dat's two-letter code, and a row never carries one, so a continent is
//     expanded to entity names through the backend's `dxcc_entity_continents` table.
//
// ⚠️ PURE, AND IT MUST STAY PURE. No React, no `../api`, no storage. The decode alert path
// (`alerts.ts`) imports it and is unit-tested in a bare node environment with no DOM; a
// transitive import of the transport or of React would break that suite for everyone.

/** cty.dat's six continent codes, in the order every picker shows them. The ONLY values
 *  honoured from storage: a code this list does not know has no checkbox, so honouring it
 *  would filter rows with nothing on screen to account for them. */
export const CONTINENT_CODES: readonly string[] = ['NA', 'SA', 'EU', 'AF', 'AS', 'OC']

/**
 * Resolve an operator's geographic scope to the set of entity NAMES it admits.
 *
 * `null` means NO SCOPE — admit everything. That is the return for an unconfigured operator,
 * and it is what makes this feature invisible until someone opts in.
 *
 * ⭐ THE TWO HALVES UNION, and this is the one place the alert scope deliberately parts company
 * with the Spots chips. A cluster spot has MANY voices, so `SpotsPanel` ANDs its two chip sets —
 * "some voice in EU" and "some voice in France" are both satisfiable at once. A decode row has
 * exactly ONE entity, so the same AND would be unsatisfiable: ticking Europe and Japan would
 * silence the radio entirely. The union is the only reading a single-origin row can satisfy.
 *
 * EVERY AMBIGUITY RESOLVES TOWARD ALERTING. An unknown continent code is dropped; a continent
 * ticked before `table` has arrived resolves to nothing rather than to an empty set; and a scope
 * that ends up admitting no entity at all is treated as no scope. A filter whose failure mode is
 * SILENCE cannot be allowed to fail closed — the operator would hear nothing and have nothing on
 * screen explaining why.
 *
 * @param continents cty.dat continent codes the operator ticked.
 * @param entities   cty.dat entity NAMES the operator picked.
 * @param table      entity name → continent code, from `getDxccEntityContinents()`; `null` until
 *                   it has been fetched (it is only fetched once a continent is ticked).
 */
export function scopedEntities(
  continents: readonly string[] | undefined,
  entities: readonly string[] | undefined,
  table: ReadonlyMap<string, string> | null,
): ReadonlySet<string> | null {
  const codes = (continents ?? []).filter((c) => CONTINENT_CODES.includes(c))
  const names = (entities ?? []).filter((e) => e.length > 0)
  if (codes.length === 0 && names.length === 0) return null

  const out = new Set<string>(names)
  if (codes.length > 0 && table) {
    for (const [entity, cont] of table) if (codes.includes(cont)) out.add(entity)
  }
  // An unresolvable scope (table not in yet, or a continent nothing is on) admits everything
  // rather than nothing — see the doc comment.
  return out.size > 0 ? out : null
}

/**
 * May a station in `entity` pass this scope?
 *
 * A station cty.dat could not place is ALWAYS admitted: absence is not a match, and guessing an
 * entity from the callsign text is exactly the second resolver this codebase must not grow (the
 * same stance `isHiddenByCountry` takes).
 */
export function scopeAllows(
  entity: string | null | undefined,
  scope: ReadonlySet<string> | null | undefined,
): boolean {
  if (!scope) return true
  if (!entity) return true
  return scope.has(entity)
}
