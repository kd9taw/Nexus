// The country-exclusion CATALOG and the pure rules over it.
//
// Deliberately NOT a jsdom test — it reads the vendored cty.dat off disk the same way
// storage-scope.test.ts and wire-consistency.test.ts read the tree, and under jsdom
// `import.meta.url` is an http URL that `fileURLToPath` refuses. The storage and
// cross-window halves live in countryExclude.test.tsx, which does need a DOM.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import {
  EXCLUDABLE_COUNTRIES,
  countryLabel,
  excludedEntities,
  hasOverridingNeed,
  isHiddenByCountry,
} from './countryExclude'

const CTY = readFileSync(
  fileURLToPath(new URL('../../../crates/propagation/data/cty.dat', import.meta.url)),
  'utf8',
)

/** cty.dat entity records: `Name: CQ: ITU: Cont: Lat: Lon: UTC: Prefix:`, one per
 *  unindented line; the alias/prefix body that follows each is indented. Name → primary
 *  prefix. Mirrors the header parse in `propagation::dxcc`. */
function ctyEntities(): Map<string, string> {
  const out = new Map<string, string>()
  for (const line of CTY.split(/\r?\n/)) {
    if (!line || /^\s/.test(line)) continue
    const cols = line.split(':')
    if (cols.length < 8) continue
    out.set(cols[0].trim(), cols[7].trim())
  }
  return out
}

const entities = ctyEntities()

describe('the catalog is the operator vocabulary, and it is short', () => {
  it('ships exactly the curated 18 — the operator asked for under 20', () => {
    expect(EXCLUDABLE_COUNTRIES).toHaveLength(18)
  })

  it('gives every entry a unique persistence key and a unique entity', () => {
    expect(new Set(EXCLUDABLE_COUNTRIES.map((c) => c.key)).size).toBe(18)
    expect(new Set(EXCLUDABLE_COUNTRIES.map((c) => c.entity)).size).toBe(18)
  })

  it('labels every entry "Country (prefix)" — the operator asked for both', () => {
    for (const c of EXCLUDABLE_COUNTRIES) {
      expect(c.prefix, c.key).not.toBe('')
      expect(countryLabel(c)).toBe(`${c.name} (${c.prefix})`)
    }
  })

  it('carries the three the operator named specifically', () => {
    const byPrefix = new Map(EXCLUDABLE_COUNTRIES.map((c) => [c.prefix, c.entity]))
    expect(byPrefix.get('S5')).toBe('Slovenia')
    expect(byPrefix.get('XE')).toBe('Mexico')
    expect(byPrefix.get('VE')).toBe('Canada')
  })
})

describe('every catalog entity is a REAL cty.dat entity name', () => {
  // The test that matters most. The entity name IS the identity — there is no numeric DXCC
  // id in this codebase, `DxccInfo.entity` is a `&'static str` — so a misspelled or
  // upstream-renamed name does not error anywhere: it silently never matches, and the
  // operator ticks a box that does nothing. A cty.dat refresh that respells one fails HERE.
  it.each([...EXCLUDABLE_COUNTRIES])('$key matches cty.dat entity "$entity"', (c) => {
    expect(entities.has(c.entity), `${c.entity} is not a cty.dat entity name`).toBe(true)
  })

  it.each([...EXCLUDABLE_COUNTRIES])("$key advertises cty.dat's own primary prefix", (c) => {
    // The label's prefix is vocabulary, never the matching rule — but vocabulary that
    // disagrees with the entity would mislead the operator about what a box does.
    expect(entities.get(c.entity)).toBe(c.prefix.split('/')[0])
  })
})

describe('the Germany trap', () => {
  // The operator says "Germany (DL)"; the resolver says "Fed. Rep. of Germany". Matching on
  // the label would hide nothing at all, forever, with no error anywhere to notice.
  it('matches on the entity name, which is not the label', () => {
    const de = EXCLUDABLE_COUNTRIES.find((c) => c.key === 'dl')!
    expect(de.name).toBe('Germany')
    expect(de.entity).toBe('Fed. Rep. of Germany')
    expect(entities.has('Germany')).toBe(false)
  })
})

describe('excludedEntities: keys in, cty.dat names out', () => {
  it('resolves ticked keys to the names a decode row actually carries', () => {
    expect([...excludedEntities(new Set(['dl', 'us']))].sort()).toEqual([
      'Fed. Rep. of Germany',
      'United States',
    ])
  })

  it('ignores a key with no catalog entry', () => {
    expect([...excludedEntities(new Set(['atlantis']))]).toEqual([])
  })
})

describe('isHiddenByCountry: what the exclusion may and may not hide', () => {
  const hidden = excludedEntities(new Set(['dl']))

  it('hides a plain decode from an excluded entity', () => {
    expect(isHiddenByCountry({ entity: 'Fed. Rep. of Germany', call: 'DL1ABC' }, hidden)).toBe(true)
  })

  it('leaves every other entity alone', () => {
    expect(isHiddenByCountry({ entity: 'France', call: 'F5ABC' }, hidden)).toBe(false)
  })

  it('hides nothing at all when no country is ticked', () => {
    expect(isHiddenByCountry({ entity: 'Fed. Rep. of Germany' }, new Set())).toBe(false)
  })

  it('keeps a row whose callsign never resolved — absence is not a match', () => {
    expect(isHiddenByCountry({ entity: null, call: '1ABC' }, hidden)).toBe(false)
    expect(isHiddenByCountry({ call: 'DL1ABC' }, hidden)).toBe(false)
  })

  it('NEVER hides the station we are in a QSO with', () => {
    expect(
      isHiddenByCountry(
        { entity: 'Fed. Rep. of Germany', call: 'DL1ABC', qsoPartner: 'DL1ABC' },
        hidden,
      ),
    ).toBe(false)
  })

  it('compares the QSO partner case- and whitespace-insensitively', () => {
    expect(
      isHiddenByCountry(
        { entity: 'Fed. Rep. of Germany', call: 'dl1abc', qsoPartner: ' DL1ABC ' },
        hidden,
      ),
    ).toBe(false)
  })

  it('does not confuse a DIFFERENT station with the QSO partner', () => {
    expect(
      isHiddenByCountry(
        { entity: 'Fed. Rep. of Germany', call: 'DL9ZZZ', qsoPartner: 'DL1ABC' },
        hidden,
      ),
    ).toBe(true)
  })

  it('NEVER hides a message addressed to us, or our own TX', () => {
    // Someone calling US outranks any view filter — hiding that would break the QSO flow
    // WSJT-X compatibility depends on, not merely tidy the list.
    expect(
      isHiddenByCountry(
        { entity: 'Fed. Rep. of Germany', call: 'DL1ABC', addressedToMe: true },
        hidden,
      ),
    ).toBe(false)
  })

  it('NEEDED outranks EXCLUDED', () => {
    expect(
      isHiddenByCountry({ entity: 'Fed. Rep. of Germany', call: 'DL1ABC', needed: true }, hidden),
    ).toBe(false)
  })
})

describe('hasOverridingNeed: only the two claims worth interrupting an exclusion', () => {
  it('lets a new entity and a new band slot through', () => {
    expect(hasOverridingNeed(['NewEntity'])).toBe(true)
    expect(hasOverridingNeed(['NewBand'])).toBe(true)
    expect(hasOverridingNeed(['Confirm', 'NewBand'])).toBe(true)
  })

  it('does not, for the routine ones', () => {
    // In a country dense enough to be on this list an unworked GRID is routine, so
    // exempting it would leave the pane as full as it was — the filter would read broken.
    expect(hasOverridingNeed(['NewGrid'])).toBe(false)
    expect(hasOverridingNeed(['Confirm'])).toBe(false)
    expect(hasOverridingNeed(['Pota', 'Sota', 'Wanted'])).toBe(false)
    expect(hasOverridingNeed([])).toBe(false)
    expect(hasOverridingNeed(undefined)).toBe(false)
  })
})
