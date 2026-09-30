// THE SKY ON THE MAP, CHECKED AGAINST REFERENCES THIS CODE DID NOT COMPUTE.
//
// Every expected value below was taken from a published source, never from the functions under
// test:
//
//   JPL Horizons (https://ssd.jpl.nasa.gov/horizons/, ephemeris DE441), queried 2026-09-29 through
//   its API with OBSERVER tables every 1500 minutes from 2026-09-01 00:00 UT to 2026-10-01 06:00 UT
//   (30 rows), ANG_FORMAT=DEG:
//     · the Sun, COMMAND='10', CENTER='500@399' (the geocentre), QUANTITIES='2' — its apparent
//       right ascension and declination of date;
//     · Greenwich, COMMAND='301', CENTER='coord@399', COORD_TYPE='GEODETIC', SITE_COORD='0,0,0',
//       QUANTITIES='7' — the local apparent sidereal time at longitude 0, which is Greenwich
//       apparent sidereal time, in hours.
//   The point on Earth with a body overhead is then (declination, right ascension − Greenwich
//   sidereal time): that subtraction is the only arithmetic done here.
import { describe, it, expect } from 'vitest'
import { subsolarPoint } from './mapGeo'

const D = Math.PI / 180
const norm180 = (deg: number) => ((deg + 540) % 360) - 180
/** Great-circle angle between two points, degrees. */
function separation(a: { lat: number; lon: number }, b: { lat: number; lon: number }): number {
  const c = Math.sin(a.lat * D) * Math.sin(b.lat * D) + Math.cos(a.lat * D) * Math.cos(b.lat * D) * Math.cos((a.lon - b.lon) * D)
  return Math.acos(Math.min(1, Math.max(-1, c))) / D
}
/** Where a body with this apparent RA/Dec is overhead at this Greenwich apparent sidereal time. */
const overhead = (raDeg: number, decDeg: number, gastHours: number) => ({
  lat: decDeg,
  lon: norm180(raDeg - gastHours * 15),
})

/** JPL Horizons DE441: [UT, Sun apparent RA (deg), Sun apparent Dec (deg), Greenwich apparent
 *  sidereal time (h)]. */
const SUN: ReadonlyArray<readonly [string, number, number, number]> = [
  ['2026-09-01T00:00Z', 160.217158964, 8.347657795, 22.6783640111],
  ['2026-09-02T01:00Z', 161.160737075, 7.969398240, 23.7468104918],
  ['2026-09-03T02:00Z', 162.103118471, 7.588775901, 0.8152581793],
  ['2026-09-04T03:00Z', 163.044393274, 7.205888661, 1.8837070681],
  ['2026-09-05T04:00Z', 163.984643790, 6.820838505, 2.9521568025],
  ['2026-09-06T05:00Z', 164.923944839, 6.433731239, 4.0206067478],
  ['2026-09-07T06:00Z', 165.862365751, 6.044675668, 5.0890561991],
  ['2026-09-08T07:00Z', 166.799973553, 5.653782435, 6.1575045823],
  ['2026-09-09T08:00Z', 167.736836422, 5.261162838, 7.2259516211],
  ['2026-09-10T09:00Z', 168.673026201, 4.866927983, 8.2943974125],
  ['2026-09-11T10:00Z', 169.608619176, 4.471188482, 9.3628423911],
  ['2026-09-12T11:00Z', 170.543695251, 4.074054543, 10.4312871360],
  ['2026-09-13T12:00Z', 171.478336449, 3.675636195, 11.4997321770],
  ['2026-09-14T13:00Z', 172.412625724, 3.276043345, 12.5681779092],
  ['2026-09-15T14:00Z', 173.346646544, 2.875385648, 13.6366245077],
  ['2026-09-16T15:00Z', 174.280483101, 2.473772271, 14.7050719299],
  ['2026-09-17T16:00Z', 175.214220791, 2.071311685, 15.7735200127],
  ['2026-09-18T17:00Z', 176.147946668, 1.668111542, 16.8419684885],
  ['2026-09-19T18:00Z', 177.081749800, 1.264278611, 17.9104170425],
  ['2026-09-20T19:00Z', 178.015721595, 0.859918731, 18.9788653666],
  ['2026-09-21T20:00Z', 178.949956228, 0.455136697, 20.0473131762],
  ['2026-09-22T21:00Z', 179.884551285, 0.050036064, 21.1157602399],
  ['2026-09-23T22:00Z', 180.819608650, -0.355281158, 22.1842064337],
  ['2026-09-24T23:00Z', 181.755235452, -0.760714822, 23.2526517995],
  ['2026-09-26T00:00Z', 182.691544722, -1.166166828, 0.3210965700],
  ['2026-09-27T01:00Z', 183.628655086, -1.571540928, 1.3895411780],
  ['2026-09-28T02:00Z', 184.566689000, -1.976741914, 2.4579861951],
  ['2026-09-29T03:00Z', 185.505769494, -2.381674198, 3.5264321569],
  ['2026-09-30T04:00Z', 186.446016152, -2.786240006, 4.5948793879],
  ['2026-10-01T05:00Z', 187.387541674, -3.190337758, 5.6633278908],
]

describe('the subsolar point (the sun marker, the terminator and the flare field all stand on it)', () => {
  // It MIRRORS the Rust engine's solar geometry (crates/propagation/src/geo.rs: Cooper's declination
  // and a day-of-year equation of time), so the map and the engine's solar elevation never disagree.
  // That approximation is a degree off near an equinox: measured against JPL here, the worst of the
  // thirty is 1.08°, on 2026-09-26. Pinned at 1.2°: a sign slip in the equation of time measures
  // 5.4° worst over these rows, and one in the declination 16°.
  it('stays within 1.2° of JPL Horizons across September 2026', () => {
    const errs = SUN.map(([ut, ra, dec, gast]) => ({ ut, err: separation(subsolarPoint(Date.parse(ut)), overhead(ra, dec, gast)) }))
    const worst = errs.reduce((a, b) => (b.err > a.err ? b : a))
    expect(worst.err, `worst at ${worst.ut}`).toBeLessThan(1.2)
    expect(errs.length, 'CONTROL: every reference row was read').toBe(30)
  })
})
