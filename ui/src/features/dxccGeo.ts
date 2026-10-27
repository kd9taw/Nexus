// The shared cty.dat GEOGRAPHY vocabulary — "is this station somewhere the operator cares
// about", asked once.
//
// WHY THIS MODULE EXISTS (#174). Three surfaces already ask a geographic question and each was
// growing its own answer: the Spots panel's "Spotted from" chips, Band Activity's
// hide-by-continent (`countryExclude.ts`, #229), and the decode alerts. The continent list
// alone had two copies. The operator's ruling on this issue was explicit — one filtering
// vocabulary across the product, not a dialect per surface — so the codes live here and every
// surface imports them. The decode alerts' area filter is resolved at the station, in the same
// vocabulary (`scoped_entities`, crates/tempo-app/src/needed_alert.rs).
//
// THE VOCABULARY IS cty.dat's OWN, in both halves:
//   - an ENTITY is the entity NAME, exactly the string `DecodeRow.country` / `Station.country` /
//     `SpotRow.entity` carry (there is no numeric DXCC id anywhere in this codebase — see
//     `countryExclude.ts` on why a name is the right and only identity);
//   - a CONTINENT is cty.dat's two-letter code, and a row never carries one, so a continent is
//     expanded to entity names through the backend's `dxcc_entity_continents` table.
//
// ⚠️ PURE, AND IT MUST STAY PURE. No React, no `../api`, no storage.

/** cty.dat's six continent codes, in the order every picker shows them. The ONLY values
 *  honoured from storage: a code this list does not know has no checkbox, so honouring it
 *  would filter rows with nothing on screen to account for them. */
export const CONTINENT_CODES: readonly string[] = ['NA', 'SA', 'EU', 'AF', 'AS', 'OC']
