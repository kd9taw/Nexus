// The beam-heading helpers: which of the two possible origins for a DX position gets
// used, when NEITHER may be used, and how a backend-supplied bearing is admitted.
//
// The rule these tests exist to protect is the one that makes the feature safe rather
// than merely present: an azimuth is either measured to the station's own reported
// position, or marked `~` because it is only measured to the middle of its country,
// or NOT SHOWN. There is no fourth case, and in particular there is never a 0°.
import { describe, it, expect } from 'vitest'
import { azimuthTo, azimuthLabel, azimuthTitle, backendAzimuth, type LatLon } from './grid'

/** A stand-in cty.dat table. Names are the real ones — the join is on the exact string
 * `resolve()` puts on a row, punctuation and all, so a test that used "Germany" would
 * pass while the shipping code missed on "Fed. Rep. of Germany". */
const CENTROIDS: ReadonlyMap<string, LatLon> = new Map<string, LatLon>([
  ['Fed. Rep. of Germany', { lat: 51.0, lon: 10.0 }],
  ['Asiatic Russia', { lat: 62.0, lon: 95.0 }],
  ['Sicily', { lat: 37.5, lon: 14.0 }],
])

const HOME = 'EN52' // the operator, somewhere in the US Midwest

describe('azimuthTo picks the best geometry available', () => {
  it('uses the station’s OWN grid when it sent one, and does not mark it approximate', () => {
    const az = azimuthTo(HOME, 'JO31', 'Fed. Rep. of Germany', CENTROIDS)
    expect(az).not.toBeNull()
    expect(az!.approx).toBe(false)
    // North-east-ish from the Midwest to western Germany over the pole side.
    expect(az!.deg).toBeGreaterThan(20)
    expect(az!.deg).toBeLessThan(60)
  })

  it('falls back to the entity centre when no grid was heard, and marks it', () => {
    const az = azimuthTo(HOME, null, 'Fed. Rep. of Germany', CENTROIDS)
    expect(az).not.toBeNull()
    expect(az!.approx).toBe(true)
  })

  it('prefers the grid over the centroid — they are different numbers', () => {
    // Positive control for the precedence itself. If the two agreed, the assertion
    // above that a grid "wins" would hold for a code path that never consulted it.
    const byGrid = azimuthTo(HOME, 'UI99', 'Asiatic Russia', CENTROIDS)!
    const byEntity = azimuthTo(HOME, null, 'Asiatic Russia', CENTROIDS)!
    expect(byGrid.deg).not.toBe(byEntity.deg)
    expect(byGrid.approx).toBe(false)
    expect(byEntity.approx).toBe(true)
  })

  it('joins on the cty.dat name verbatim — punctuation included', () => {
    expect(azimuthTo(HOME, null, 'Fed. Rep. of Germany', CENTROIDS)).not.toBeNull()
    // …and the shorter name a human would type is NOT in the table, so it must miss.
    // This pairing is the point: it shows the lookup is a real exact-string join
    // rather than something fuzzy that would have matched either way.
    expect(azimuthTo(HOME, null, 'Germany', CENTROIDS)).toBeNull()
  })

  it('covers WAE-only entities, which resolve to a country like any other', () => {
    expect(azimuthTo(HOME, null, 'Sicily', CENTROIDS)?.approx).toBe(true)
  })
})

describe('azimuthTo shows NOTHING rather than a wrong bearing', () => {
  it('returns null when the operator has not set a grid', () => {
    expect(azimuthTo('', 'JO31', 'Fed. Rep. of Germany', CENTROIDS)).toBeNull()
    expect(azimuthTo('nonsense', 'JO31', 'Fed. Rep. of Germany', CENTROIDS)).toBeNull()
  })

  it('returns null when the station has no grid and its entity is not in the table', () => {
    expect(azimuthTo(HOME, null, 'Nowhereland', CENTROIDS)).toBeNull()
  })

  it('returns null when there is no entity and no grid at all', () => {
    expect(azimuthTo(HOME, null, null, CENTROIDS)).toBeNull()
  })

  it('returns null before the centroid table has loaded', () => {
    // The table arrives asynchronously; until it does a gridless row simply has no
    // heading. It must not fall through to some other origin in the meantime.
    expect(azimuthTo(HOME, null, 'Fed. Rep. of Germany', null)).toBeNull()
    // Control: the same row WITH the table resolves, so the null above is the missing
    // table and not a mistake in the fixture.
    expect(azimuthTo(HOME, null, 'Fed. Rep. of Germany', CENTROIDS)).not.toBeNull()
  })
})

describe('the printed forms', () => {
  it('marks an approximate heading and leaves an exact one plain', () => {
    expect(azimuthLabel({ deg: 47, approx: false })).toBe('47°')
    expect(azimuthLabel({ deg: 47, approx: true })).toBe('~47°')
    expect(azimuthLabel(null)).toBeNull()
  })

  it('always says short path, and says whose centre an approximate heading is', () => {
    expect(azimuthTitle({ deg: 47, approx: false })).toContain('short path')
    const rough = azimuthTitle({ deg: 47, approx: true }, 'Fed. Rep. of Germany')
    expect(rough).toContain('short path')
    expect(rough).toContain('Fed. Rep. of Germany')
    expect(rough).toContain('not to this station')
  })

  it('adds magnetic only where a surface knows the declination', () => {
    expect(azimuthTitle({ deg: 47, approx: false }, null, 42)).toContain('42° magnetic')
    expect(azimuthTitle({ deg: 47, approx: false }, null, null)).not.toContain('magnetic')
  })
})

describe('backendAzimuth admits a bearing the backend already measured', () => {
  it('passes a real one through, rounded', () => {
    expect(backendAzimuth(46.7, 6800)).toEqual({ deg: 47, approx: false })
  })

  it('rejects the 0/0 pair that means "we never knew where you are"', () => {
    // `propagation::dxped` fills BOTH fields with 0 when it has no operator grid.
    // A bare 0° would render as a confident due-north heading.
    expect(backendAzimuth(0, 0)).toBeNull()
  })

  it('still admits a genuine due-north path, which has a real distance', () => {
    // The control for the rule above: 0° is only rejected together with 0 km.
    expect(backendAzimuth(0, 4200)).toEqual({ deg: 0, approx: false })
  })

  it('rejects non-finite values rather than printing NaN°', () => {
    expect(backendAzimuth(Number.NaN, 100)).toBeNull()
    expect(backendAzimuth(90, Number.NaN)).toBeNull()
  })

  // An absent ORIGIN grid must return null, never throw. `snap.mygrid` is undefined until
  // the operator sets their grid, and the callbook lookup runs async — an unguarded
  // `gridToLatLon(undefined).trim()` there is an UNHANDLED REJECTION that reds the whole
  // suite while every assertion still "passes" (1.10.3 shipped with exactly that).
  it('returns null for an absent origin grid, and never throws', () => {
    expect(azimuthTo(undefined, 'FN31', 'United States')).toBeNull()
    expect(azimuthTo(null, 'FN31', 'United States')).toBeNull()
    expect(azimuthTo('', 'FN31', 'United States')).toBeNull()
    // and a valid origin with an absent peer/entity still yields null, not a throw
    expect(azimuthTo('EN52', undefined, undefined)).toBeNull()
  })
})
