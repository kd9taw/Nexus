import { describe, it, expect } from 'vitest'
import {
  isValidGrid,
  gridToLatLon,
  bearingDeg,
  haversineKm,
  stationLatLon,
  distanceLabel,
  bearingLabel,
  distanceLabelAt,
  bearingLabelAt,
  type LatLon,
} from './grid'

// The STRICT persist-side check (the wizard's gate). gridToLatLon deliberately
// stays permissive for distance badges — these cases document the difference.
describe('isValidGrid', () => {
  it('accepts real 4- and 6-char locators, any case, with surrounding space', () => {
    for (const g of ['EN52', 'en52', 'JJ00', 'RR73', 'EN52xa', 'IO91WM', ' EN52 ']) {
      expect(isValidGrid(g), g).toBe(true)
    }
  })

  it('rejects garbage the lenient parser swallows', () => {
    // '1234' (digit fields), 'ZZ99' (field > R), '3N52' (digit first),
    // 'EN5' (short), 'EN52x' (odd length), 'EN52ya' ok? y > x → reject,
    // 'EN52xa9q' (8 chars — extended precision is not stored).
    for (const g of ['1234', 'ZZ99', '3N52', 'EN5', 'EN52x', 'EN52YA'.replace('X', 'Y'), 'EN52xa9q', '', 'EN 52']) {
      expect(isValidGrid(g), g).toBe(false)
    }
  })

  it('bounds the subsquare letters at X (there is no Y/Z subsquare)', () => {
    expect(isValidGrid('EN52xx')).toBe(true)
    expect(isValidGrid('EN52yz')).toBe(false)
  })
})

// ---------------------------------------------------------------------------
// Reference cases for the caller card's distance/bearing (operator report,
// 2026-08-01: "bearing in the Callsign Block does not match QRZ", Phone + CW).
//
// The expectations below are NOT produced by the functions under test. `refBearing`
// is a second, structurally different derivation — spherical vector algebra on unit
// vectors and cross products, where `bearingDeg` uses the scalar atan2 identity — so
// a sign slip, a swapped argument, or a reciprocal in either one cannot hide behind
// the other. `refKm` is the spherical law of cosines against haversine.
// ---------------------------------------------------------------------------

const D = Math.PI / 180

/** Unit vector on the sphere for a (lat, lon), in an Earth-centred frame. */
function vec(p: LatLon): [number, number, number] {
  const la = p.lat * D
  const lo = p.lon * D
  return [Math.cos(la) * Math.cos(lo), Math.cos(la) * Math.sin(lo), Math.sin(la)]
}
const cross = (a: number[], b: number[]): [number, number, number] => [
  a[1] * b[2] - a[2] * b[1],
  a[2] * b[0] - a[0] * b[2],
  a[0] * b[1] - a[1] * b[0],
]
const dot = (a: number[], b: number[]) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
const norm = (a: number[]) => Math.sqrt(dot(a, a))

/**
 * Initial great-circle bearing, derived independently of `bearingDeg`: the signed
 * angle at `a` between the great circle a→north-pole and the great circle a→b,
 * measured from the normals of those two planes.
 */
function refBearing(a: LatLon, b: LatLon): number {
  const va = vec(a)
  const vb = vec(b)
  const pole: [number, number, number] = [0, 0, 1]
  const toB = cross(va, vb) // normal of the a→b great circle
  const toN = cross(va, pole) // normal of the meridian through a
  // The two planes meet along `va`; the dihedral angle between them is the bearing.
  // cross(toB, toN) is parallel to ±va — its sign says which side of the meridian b lies on.
  const s = dot(cross(toB, toN), va)
  const ang = Math.atan2(Math.sign(s) * norm(cross(toB, toN)), dot(toN, toB))
  return (((ang / D) % 360) + 360) % 360
}

/** Great-circle distance via the spherical law of cosines (not haversine). */
function refKm(a: LatLon, b: LatLon): number {
  const c =
    Math.sin(a.lat * D) * Math.sin(b.lat * D) +
    Math.cos(a.lat * D) * Math.cos(b.lat * D) * Math.cos((b.lon - a.lon) * D)
  return 6371 * Math.acos(Math.min(1, Math.max(-1, c)))
}

const EN52 = gridToLatLon('EN52')!
const angDiff = (x: number, y: number) => Math.abs(((x - y + 540) % 360) - 180)

// A close-in station, a mid-distance one, an EU path and a VK path whose short
// path leaves to the WEST (the case a flat-map intuition gets backwards).
//
// `deg`/`mi` are the ANSWERS, written down so a human can check them against a
// published bearing calculator or the station's QRZ page without running anything.
// They were produced outside this file (spherical vector algebra + law of cosines,
// the same two derivations `refBearing`/`refKm` re-implement above), never by
// calling `bearingDeg`/`haversineKm`.
const paths: [string, string, number, number][] = [
  ['close US ~265 mi (St. Louis)', 'EM48ru', 198.3, 264.5],
  ['mid US ~835 mi (Newington CT)', 'FN31pr', 88.1, 835.4],
  ['EU (Munich)', 'JN58td', 46.0, 4535.1],
  ['VK (Sydney) — short path leaves WEST', 'QF56od', 259.6, 9181.7],
]

describe('bearingDeg / haversineKm against an independent derivation', () => {
  // `bearingDeg` rounds to whole degrees, so 0.5° of the budget is rounding alone.
  it.each(paths)('%s — bearing matches the reference and the vector derivation', (_l, grid, deg) => {
    const them = gridToLatLon(grid)!
    expect(angDiff(bearingDeg(EN52, them), deg)).toBeLessThan(0.51)
    expect(angDiff(bearingDeg(EN52, them), refBearing(EN52, them))).toBeLessThan(0.51)
  })

  it.each(paths)('%s — distance matches the reference and the law of cosines', (_l, grid, _d, mi) => {
    const them = gridToLatLon(grid)!
    expect(haversineKm(EN52, them) * 0.621371).toBeCloseTo(mi, 0)
    expect(Math.abs(haversineKm(EN52, them) - refKm(EN52, them))).toBeLessThan(1)
  })

  it('is not the reciprocal — the return path differs from the outbound', () => {
    // The classic swapped-argument signature. Great-circle bearings are NOT exact
    // reciprocals off the equator, so this also pins that we compute the INITIAL one.
    const them = gridToLatLon('JN58td')!
    expect(angDiff(bearingDeg(EN52, them), bearingDeg(them, EN52))).toBeGreaterThan(90)
    expect(bearingDeg(EN52, them)).toBeCloseTo(46, 0) // EN52 -> Munich leaves NE
    expect(bearingDeg(EN52, gridToLatLon('QF56od')!)).toBeCloseTo(260, -1) // -> Sydney, W
  })
})

describe('stationLatLon — the caller card computes from the best position it has', () => {
  // W1AW: QRZ lists 41.7147, -72.7272 and grid FN31pr. The 6-character square's
  // center is 2.2 km from that pin; the 4-character FN31's is 33 km from it.
  const exact: LatLon = { lat: 41.7147, lon: -72.7272 }

  it('prefers exact callbook coordinates over the grid square center', () => {
    expect(stationLatLon(exact, 'FN31pr')).toEqual(exact)
    expect(stationLatLon(exact, 'FN31')).toEqual(exact)
    // …and the two disagree, which is the whole point of preferring one.
    expect(stationLatLon(null, 'FN31')).not.toEqual(exact)
  })

  it('falls back to the locator when the callbook vouched for no position', () => {
    expect(stationLatLon(null, 'FN31pr')).toEqual(gridToLatLon('FN31pr'))
    expect(stationLatLon(undefined, 'FN31pr')).toEqual(gridToLatLon('FN31pr'))
  })

  it('renders absence as absence — never a point at 0,0', () => {
    expect(stationLatLon(null, null)).toBeNull()
    expect(stationLatLon(null, '')).toBeNull()
    expect(stationLatLon({ lat: NaN, lon: 0 }, null)).toBeNull()
  })

  it('quantifies what the exact position buys, at 835 mi and at 125', () => {
    // Unrounded, so the measurement is the geometry and not the display rounding.
    // At 835 mi the PEER's 4-character square is worth ~1.2° (and 11 mi); their
    // 6-character one is worth 0.08°, which is why the peer side is the small half.
    expect(
      angDiff(refBearing(EN52, gridToLatLon('FN31')!), refBearing(EN52, exact)),
    ).toBeGreaterThan(1)
    expect(
      angDiff(refBearing(EN52, gridToLatLon('FN31pr')!), refBearing(EN52, exact)),
    ).toBeLessThan(0.2)
    // …and on a station ~125 mi out (EN61 center), the OPERATOR'S own 4-character
    // square dominates everything else: the same target seen from opposite corners of
    // EN52 swings ~50°, so assuming the center is up to ~29° wrong on its own. That is
    // the scale of disagreement with QRZ the operator reported, and no callbook field
    // can close it — only a 6-character locator on their side can.
    const target = gridToLatLon('EN61')!
    expect(
      angDiff(refBearing(gridToLatLon('EN52aa')!, target), refBearing(gridToLatLon('EN52xx')!, target)),
    ).toBeGreaterThan(45)
    // The exact answer is the one that agrees with the independent derivation.
    expect(angDiff(bearingDeg(EN52, exact), refBearing(EN52, exact))).toBeLessThan(0.51)
  })
})

describe('the labels the operator actually reads', () => {
  it('format miles and degrees the way the caller card shows them', () => {
    const them = gridToLatLon('FN31pr')!
    expect(distanceLabelAt(EN52, them)).toMatch(/^\d+ mi$/)
    expect(bearingLabelAt(EN52, them)).toMatch(/^\d+°$/)
    expect(distanceLabelAt(EN52, them)).toBe(`${Math.round(haversineKm(EN52, them) * 0.621371)} mi`)
    expect(bearingLabelAt(EN52, them)).toBe(`${bearingDeg(EN52, them)}°`)
  })

  // The two label pairs pinned to the SAME externally-derived answers as the maths
  // above, rounded the way the display rounds. `distanceLabel`/`bearingLabel` are the
  // grid-in form still used by the roster, the needed board and the station cards;
  // `*At` is the resolved-point form the caller card moved to. Both must land on the
  // same string for a station whose only known position IS its square — otherwise the
  // fix would have quietly changed every other readout that shares these helpers.
  it.each(paths)('%s — both label forms print the reference answer', (_l, grid, deg, mi) => {
    const them = gridToLatLon(grid)!
    expect(distanceLabel('EN52', grid)).toBe(`${Math.round(mi)} mi`)
    expect(bearingLabel('EN52', grid)).toBe(`${Math.round(deg)}°`)
    expect(distanceLabelAt(EN52, them)).toBe(distanceLabel('EN52', grid))
    expect(bearingLabelAt(EN52, them)).toBe(bearingLabel('EN52', grid))
  })
})
