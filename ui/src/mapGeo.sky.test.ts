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
//     · the Moon, COMMAND='301', CENTER='500@399', QUANTITIES='2,10,31' — its apparent right
//       ascension and declination of date, its illuminated percentage, and its ecliptic longitude
//       and latitude of date; the Sun's ecliptic longitude of date comes from QUANTITIES='31' of
//       the Sun query above;
//     · Greenwich, COMMAND='301', CENTER='coord@399', COORD_TYPE='GEODETIC', SITE_COORD='0,0,0',
//       QUANTITIES='7' — the local apparent sidereal time at longitude 0, which is Greenwich
//       apparent sidereal time, in hours.
//   The point on Earth with a body overhead is then (declination, right ascension − Greenwich
//   sidereal time): that subtraction is the only arithmetic done here.
//
//   The U.S. Naval Observatory's Astronomical Applications Department, Phases of the Moon
//   (https://aa.usno.navy.mil/api/moon/phases/date?date=2026-08-01&nump=12), fetched 2026-09-29:
//   the UT of each new and full moon from August to October 2026.
import { describe, it, expect } from 'vitest'
import { moonAt, subsolarPoint } from './mapGeo'

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

/** JPL Horizons DE441: [UT, Moon apparent RA (deg), Moon apparent Dec (deg), Moon illuminated (%),
 *  Moon ecliptic longitude (deg), Moon ecliptic latitude (deg), Sun ecliptic longitude (deg),
 *  Greenwich apparent sidereal time (h)] — ecliptic coordinates of date. */
const MOON: ReadonlyArray<readonly [string, number, number, number, number, number, number, number]> = [
  ['2026-09-01T00:00Z', 21.930704103, 13.883778740, 84.17700, 25.4257235, 4.3551174, 158.5932491, 22.6783640111],
  ['2026-09-02T01:00Z', 35.689123760, 19.428540523, 74.88823, 39.7521949, 4.9537686, 159.6009621, 23.7468104918],
  ['2026-09-03T02:00Z', 50.494909638, 23.918415019, 64.12210, 54.2702117, 5.2464946, 160.6092686, 0.8152581793],
  ['2026-09-04T03:00Z', 66.344829360, 26.929193448, 52.44202, 68.9486378, 5.2033245, 161.6181841, 1.8837070681],
  ['2026-09-05T04:00Z', 82.937434897, 28.102526435, 40.50480, 83.7514229, 4.8174884, 162.6277140, 2.9521568025],
  ['2026-09-06T05:00Z', 99.698772980, 27.255830090, 29.02402, 98.6355668, 4.1079944, 163.6378542, 4.0206067478],
  ['2026-09-07T06:00Z', 115.993568997, 24.458465827, 18.72014, 113.5489838, 3.1203155, 164.6485930, 5.0890561991],
  ['2026-09-08T07:00Z', 131.385566203, 20.012444014, 10.25468, 128.4296794, 1.9242225, 165.6599149, 6.1575045823],
  ['2026-09-09T08:00Z', 145.748559031, 14.355933879, 4.15502, 143.2075334, 0.6078376, 166.6718039, 7.2259516211],
  ['2026-09-10T09:00Z', 159.206427864, 7.960809179, 0.74768, 157.8092865, -0.7320348, 167.6842464, 8.2943974125],
  ['2026-09-11T10:00Z', 172.015560634, 1.268391447, 0.12297, 172.1662118, -2.0016049, 168.6972323, 9.3628423911],
  ['2026-09-12T11:00Z', 184.472915619, -5.336315752, 2.14505, 186.2228581, -3.1208500, 169.7107537, 10.4312871360],
  ['2026-09-13T12:00Z', 196.863209221, -11.527208907, 6.50358, 199.9447224, -4.0304898, 170.7248042, 11.4997321770],
  ['2026-09-14T13:00Z', 209.428284952, -17.029273320, 12.78651, 213.3230593, -4.6939170, 171.7393774, 12.5681779092],
  ['2026-09-15T14:00Z', 222.341429997, -21.608953230, 20.54970, 226.3760493, -5.0947886, 172.7544667, 13.6366245077],
  ['2026-09-16T15:00Z', 235.679664691, -25.069297966, 29.36641, 239.1466777, -5.2322066, 173.7700658, 14.7050719299],
  ['2026-09-17T16:00Z', 249.400855603, -27.254357075, 38.85129, 251.6983776, -5.1155341, 174.7861692, 15.7735200127],
  ['2026-09-18T17:00Z', 263.344621841, -28.062408606, 48.66228, 264.1096017, -4.7602808, 175.8027727, 16.8419684885],
  ['2026-09-19T18:00Z', 277.274130090, -27.460665347, 58.48750, 276.4681572, -4.1857317, 176.8198739, 17.9104170425],
  ['2026-09-20T19:00Z', 290.952001312, -25.490779178, 68.02460, 288.8656631, -3.4144228, 177.8374725, 18.9788653666],
  ['2026-09-21T20:00Z', 304.215948256, -22.260080846, 76.95917, 301.3920862, -2.4732306, 178.8555708, 20.0473131762],
  ['2026-09-22T21:00Z', 317.019960473, -17.924112275, 84.94805, 314.1301030, -1.3956285, 179.8741747, 21.1157602399],
  ['2026-09-23T22:00Z', 329.434653726, -12.671354527, 91.61392, 327.1490706, -0.2244267, 180.8932941, 22.1842064337],
  ['2026-09-24T23:00Z', 341.624234370, -6.717575005, 96.55692, 340.4986947, 0.9859946, 181.9129447, 23.2526517995],
  ['2026-09-26T00:00Z', 353.819521252, -0.310430491, 99.38769, 354.2030384, 2.1692431, 182.9331481, 0.3210965700],
  ['2026-09-27T01:00Z', 6.294927503, 6.259631967, 99.78040, 8.2562024, 3.2495418, 183.9539317, 1.3895411780],
  ['2026-09-28T02:00Z', 19.344445865, 12.647945768, 97.53592, 22.6214692, 4.1480847, 184.9753270, 2.4579861951],
  ['2026-09-29T03:00Z', 33.241713944, 18.450912157, 92.63753, 37.2354524, 4.7924495, 185.9973659, 3.5264321569],
  ['2026-09-30T04:00Z', 48.165483857, 23.218978687, 85.27999, 52.0174619, 5.1268763, 187.0200763, 4.5948793879],
  ['2026-10-01T05:00Z', 64.088837371, 26.502979228, 75.86170, 66.8822223, 5.1204893, 188.0434787, 5.6633278908],
]

/** USNO: every new and full moon from 2026-08-01 to 2026-11-01, UT. */
const USNO_PHASES: ReadonlyArray<readonly ['new' | 'full', string]> = [
  ['new', '2026-08-12T17:37Z'],
  ['full', '2026-08-28T04:18Z'],
  ['new', '2026-09-11T03:27Z'],
  ['full', '2026-09-26T16:49Z'],
  ['new', '2026-10-10T15:50Z'],
  ['full', '2026-10-26T04:12Z'],
]

describe('the moon: the Astronomical Almanac’s low-precision series against JPL and USNO', () => {
  // The series' own accuracy, measured against JPL's DE200 by the source it is taken from (Simpson
  // 1999, cited in mapGeo.ts): about 0.11° rms and 0.35° at worst in position.
  const STATED = 0.35

  it('puts the point with the moon overhead within the series’ stated 0.35° of JPL’s, every row', () => {
    const errs = MOON.map(([ut, ra, dec, , , , , gast]) => ({ ut, err: separation(moonAt(Date.parse(ut)).sublunar, overhead(ra, dec, gast)) }))
    const worst = errs.reduce((a, b) => (b.err > a.err ? b : a))
    expect(worst.err, `worst at ${worst.ut}`).toBeLessThan(STATED)
    expect(errs.length, 'CONTROL: every reference row was read').toBe(30)
    // A longitude like every other on the map: sidereal time runs to millions of degrees, and an
    // unreduced one would still draw in the right place while handing back −351°.
    for (const [ut] of MOON) expect(Math.abs(moonAt(Date.parse(ut)).sublunar.lon), ut).toBeLessThanOrEqual(180)
  })

  it('gives its ecliptic longitude and latitude within the same 0.35°', () => {
    for (const [ut, , , , lon, lat] of MOON) {
      const m = moonAt(Date.parse(ut))
      expect(Math.abs(norm180(m.eclipticLonDeg - lon)), `${ut} longitude`).toBeLessThan(STATED)
      expect(Math.abs(m.eclipticLatDeg - lat), `${ut} latitude`).toBeLessThan(STATED)
    }
  })

  it('lights the same fraction of the disc as JPL, within three percentage points', () => {
    for (const [ut, , , illum] of MOON) {
      expect(Math.abs(moonAt(Date.parse(ut)).illuminated - illum / 100), ut).toBeLessThan(0.03)
    }
  })

  it('says waxing exactly when JPL puts the moon east of the sun', () => {
    for (const [ut, , , , moonLon, , sunLon] of MOON) {
      expect(moonAt(Date.parse(ut)).waxing, ut).toBe(Math.sin(norm180(moonLon - sunLon) * D) > 0)
    }
  })

  it('has each new and full moon on USNO’s day, within 90 minutes, and no other', () => {
    // A new moon is where waning turns to waxing, a full moon where waxing turns to waning: found
    // by walking the three months in ten-minute steps.
    const STEP = 10 * 60_000
    const found: Array<['new' | 'full', number]> = []
    let was = moonAt(Date.UTC(2026, 7, 1)).waxing
    for (let t = Date.UTC(2026, 7, 1) + STEP; t < Date.UTC(2026, 10, 1); t += STEP) {
      const now = moonAt(t).waxing
      if (now !== was) found.push([now ? 'new' : 'full', t])
      was = now
    }
    expect(found.map(([kind]) => kind), 'the phases, in order').toEqual(USNO_PHASES.map(([kind]) => kind))
    found.forEach(([kind, t], i) => {
      const want = Date.parse(USNO_PHASES[i][1])
      const day = (ms: number) => new Date(ms).toISOString().slice(0, 10)
      expect(day(t), `${kind} moon ${USNO_PHASES[i][1]}: the UT day`).toBe(day(want))
      expect(Math.abs(t - want) / 60_000, `${kind} moon ${USNO_PHASES[i][1]}: minutes off`).toBeLessThan(90)
    })
  })
})
