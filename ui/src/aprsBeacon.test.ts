// THE APRS CHANNEL DERIVATION — and this is on-air-affecting code, not a menu label.
//
// `AprsCockpit`'s auto-tune effect commands the rig to the resolved channel on view entry with
// no act from the operator, so a wrong bounding box puts a station on the wrong frequency and
// nothing on screen asked them first. Every box gets a case here, and the three the review
// caught get named cases with real coordinates.
//
// The countries below are checked through `latLonToGrid` rather than by hand-written locators,
// because the input to the derivation is a grid SQUARE: a 4-character locator is 2° of longitude
// wide, so the question that matters is what happens to the square a town falls in, not to the
// town's own coordinates.
import { describe, it, expect } from 'vitest'
import { latLonToGrid } from './grid'
import {
  APRS_FREQS,
  ARGENTINA,
  AUSTRALIA,
  BRAZIL,
  EUROPE_AFRICA,
  JAPAN,
  NEW_ZEALAND,
  NORTH_AMERICA,
  aprsChannelForGrid,
  resolveAprsChannel,
} from './aprsBeacon'

/** The channel derived for a town, by its real coordinates. */
const at = (lat: number, lon: number) => aprsChannelForGrid(latLonToGrid(lat, lon))

describe('aprsChannelForGrid — one case per region box', () => {
  it('resolves each of the seven regions', () => {
    expect(aprsChannelForGrid('EN52')).toBe(NORTH_AMERICA) // Wisconsin
    expect(aprsChannelForGrid('IO91')).toBe(EUROPE_AFRICA) // London
    expect(aprsChannelForGrid('QF56')).toBe(AUSTRALIA) // Sydney
    expect(aprsChannelForGrid('RE78')).toBe(NEW_ZEALAND) // Wellington
    expect(aprsChannelForGrid('PM95')).toBe(JAPAN) // Tokyo
    expect(aprsChannelForGrid('GG66')).toBe(BRAZIL) // São Paulo state
    expect(aprsChannelForGrid('GF05')).toBe(ARGENTINA) // Buenos Aires
  })

  it('falls back to 144.390 with no grid to work from, and says so on screen', () => {
    // The label that renders beside this names the number, so the guess is visible rather
    // than silent — that is the whole mitigation for a table of approximations.
    expect(aprsChannelForGrid('')).toBe(NORTH_AMERICA)
    expect(aprsChannelForGrid('nonsense')).toBe(NORTH_AMERICA)
    expect(aprsChannelForGrid('ZZ99')).toBe(NORTH_AMERICA)
  })

  it('⭐ every value it can return is a channel the picker can show', () => {
    // A derived value absent from APRS_FREQS is a `<select>` with no matching option — it
    // renders blank, or silently snaps to the first entry, and the operator cannot see why.
    const offered = new Set(APRS_FREQS.map(([f]) => f))
    const corners: [number, number][] = []
    for (let lat = -80; lat <= 80; lat += 5) for (let lon = -175; lon <= 175; lon += 5) corners.push([lat, lon])
    for (const [lat, lon] of corners) {
      const f = at(lat, lon)
      expect(offered.has(f), `${lat},${lon} derived ${f}, which the picker cannot show`).toBe(true)
    }
  })
})

describe('⭐ South America — the three the boxes used to get wrong', () => {
  // All three ran the R2 standard 144.390 and were being auto-tuned to an Argentine or
  // Brazilian exception channel instead. These are the regression cases.
  it('Chile is not Argentina', () => {
    expect(at(-33.45, -70.67), 'Santiago').toBe(NORTH_AMERICA)
    expect(at(-23.65, -70.4), 'Antofagasta').toBe(NORTH_AMERICA)
    expect(at(-53.16, -70.91), 'Punta Arenas').toBe(NORTH_AMERICA)
    expect(at(-22.46, -68.93), 'Calama — the northern desert, which lies EAST of 70°W').toBe(
      NORTH_AMERICA,
    )
  })

  it('Bolivia is not Brazil', () => {
    expect(at(-16.5, -68.15), 'La Paz').toBe(NORTH_AMERICA)
    expect(at(-17.78, -63.18), 'Santa Cruz de la Sierra').toBe(NORTH_AMERICA)
  })

  it('eastern Colombia is not Brazil', () => {
    expect(at(4.15, -73.63), 'Villavicencio').toBe(NORTH_AMERICA)
    expect(at(4.71, -74.07), 'Bogotá').toBe(NORTH_AMERICA)
  })

  it('and the two exceptions still resolve to their own channels', () => {
    // The other direction of the same guard: tightening the boxes must not have emptied them.
    expect(at(-34.6, -58.38), 'Buenos Aires').toBe(ARGENTINA)
    expect(at(-31.42, -64.18), 'Córdoba').toBe(ARGENTINA)
    expect(at(-32.89, -68.84), 'Mendoza').toBe(ARGENTINA)
    expect(at(-24.79, -65.41), 'Salta').toBe(ARGENTINA)
    expect(at(-15.79, -47.88), 'Brasília').toBe(BRAZIL)
    expect(at(-23.55, -46.63), 'São Paulo').toBe(BRAZIL)
    expect(at(-22.91, -43.17), 'Rio de Janeiro').toBe(BRAZIL)
    expect(at(-30.03, -51.23), 'Porto Alegre').toBe(BRAZIL)
    expect(at(-3.12, -60.02), 'Manaus').toBe(BRAZIL)
    expect(at(-8.05, -34.9), 'Recife').toBe(BRAZIL)
  })

  it('Uruguay keeps the R2 standard', () => {
    expect(at(-34.9, -56.16), 'Montevideo').toBe(NORTH_AMERICA)
  })

  it('the rest of the Americas is 144.390', () => {
    expect(at(-12.05, -77.04), 'Lima').toBe(NORTH_AMERICA)
    expect(at(19.43, -99.13), 'Mexico City').toBe(NORTH_AMERICA)
    expect(at(45.42, -75.7), 'Ottawa').toBe(NORTH_AMERICA)
    expect(at(-0.18, -78.47), 'Quito').toBe(NORTH_AMERICA)
  })
})

describe('resolveAprsChannel — a pin outranks the region', () => {
  it('uses the operator’s pick when there is one', () => {
    expect(resolveAprsChannel(145.175, 'EN52')).toBe(145.175)
    // Zero is a pick nobody would make, but `??` must not treat it as absent the way `||` would.
    expect(resolveAprsChannel(0, 'EN52')).toBe(0)
  })

  it('follows the grid when there is not', () => {
    expect(resolveAprsChannel(null, 'IO91')).toBe(EUROPE_AFRICA)
    expect(resolveAprsChannel(undefined, 'PM95')).toBe(JAPAN)
    // No pin and no grid — the same last resort, so the cockpit never holds a null channel.
    expect(resolveAprsChannel(null, '')).toBe(NORTH_AMERICA)
  })
})
