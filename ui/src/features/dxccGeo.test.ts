// The shared cty.dat geography predicate (#174) — the ONE place "is this station somewhere I
// care about" is decided, so the Spots chips, Band Activity's hide-by-continent and the decode
// alerts cannot grow three dialects of the same question.
//
// Every case below uses entities on TWO continents. A fixture with one continent cannot tell
// "filtered correctly" from "not filtered at all" — both readings pass it.
import { describe, it, expect } from 'vitest'
import { CONTINENT_CODES, scopedEntities, scopeAllows } from './dxccGeo'

/** A stand-in for the backend's `dxcc_entity_continents` table: two EU entities, one AS. */
const TABLE: ReadonlyMap<string, string> = new Map([
  ['Fed. Rep. of Germany', 'EU'],
  ['France', 'EU'],
  ['Japan', 'AS'],
  ['United States', 'NA'],
])

describe('scopedEntities — resolving an operator scope to entity names', () => {
  it('an UNCONFIGURED scope resolves to null — the shipped "everything" behaviour', () => {
    expect(scopedEntities([], [], TABLE)).toBeNull()
    expect(scopedEntities(undefined, undefined, TABLE)).toBeNull()
  })

  it('a continent expands through the table to exactly its entities', () => {
    const eu = scopedEntities(['EU'], [], TABLE)
    expect(eu).not.toBeNull()
    expect([...eu!].sort()).toEqual(['Fed. Rep. of Germany', 'France'])
  })

  it('an entity scope needs no table at all', () => {
    const only = scopedEntities([], ['Japan'], null)
    expect([...only!]).toEqual(['Japan'])
  })

  // ⭐ THE RULE THAT DIFFERS FROM THE SPOTS CHIPS. A cluster spot has many voices, so Spots ANDs
  // its two chip sets ("a voice in EU *and* a voice in France"). A decode row has exactly ONE
  // entity, so the same AND would match nothing at all — ticking Europe and Japan would silence
  // the radio. The union is the only reading that can be satisfied here.
  it('continents and entities UNION — Europe plus Japan admits both, not neither', () => {
    const both = scopedEntities(['EU'], ['Japan'], TABLE)
    expect(both!.has('France'), 'the continent half').toBe(true)
    expect(both!.has('Japan'), 'the entity half').toBe(true)
    expect(both!.has('United States'), 'and nothing else').toBe(false)
  })

  it('ignores a stored continent code that is not one — a scope must never guess', () => {
    expect(scopedEntities(['XX'], [], TABLE)).toBeNull()
    const mixed = scopedEntities(['XX', 'AS'], [], TABLE)
    expect([...mixed!]).toEqual(['Japan'])
  })

  // The table is fetched once a continent is actually ticked. Until it lands a continent scope
  // cannot be resolved, and resolving it to "nothing matches" would silence every alert.
  it('a continent scope with no table yet withholds NOTHING', () => {
    expect(scopedEntities(['EU'], [], null)).toBeNull()
  })

  it('a continent scope that resolves to no entity at all withholds nothing', () => {
    expect(scopedEntities(['AF'], [], TABLE)).toBeNull()
  })

  it('the six cty.dat codes, in picker order', () => {
    expect(CONTINENT_CODES).toEqual(['NA', 'SA', 'EU', 'AF', 'AS', 'OC'])
  })
})

describe('scopeAllows — judging one station against a resolved scope', () => {
  const eu = scopedEntities(['EU'], [], TABLE)

  it('admits an entity inside the scope and refuses one outside it', () => {
    expect(scopeAllows('France', eu)).toBe(true)
    expect(scopeAllows('Japan', eu)).toBe(false)
  })

  it('an unconfigured scope admits everyone', () => {
    expect(scopeAllows('Japan', null)).toBe(true)
  })

  // Absence is not a match. A station cty.dat could not place must never be withheld — the same
  // direction `isHiddenByCountry` resolves toward, for the same reason: a filter that removes
  // rows nobody can account for is how "my alerts stopped" becomes unanswerable.
  it('admits a station whose entity never resolved', () => {
    expect(scopeAllows(null, eu)).toBe(true)
    expect(scopeAllows(undefined, eu)).toBe(true)
    expect(scopeAllows('', eu)).toBe(true)
  })
})
