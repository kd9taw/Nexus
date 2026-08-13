/**
 * The APRS channel list, the grid→channel derivation, and the beaconable symbols — the ONE
 * source of truth for all three, shared by the APRS cockpit and the Settings panel so the
 * picker's options and the derived value can never disagree.
 *
 * ⚠️ THE DERIVED CHANNEL COMMANDS THE RADIO. `AprsCockpit`'s auto-tune effect retunes the rig
 * to the resolved value on view entry, with no act from the operator — so a wrong box here does
 * not merely mislabel a menu, it puts a station on the wrong frequency. Treat the table below
 * as on-air-affecting code: every change gets an assertion in `aprsBeacon.test.ts`.
 */

import { gridToLatLon, isValidGrid } from './grid'

/** The regional 2 m FM APRS channels (MHz) — all AFSK-1200, which is what this decoder
 *  handles. Named rather than repeated as literals: the derivation and the picker must
 *  reference the same number, or a "derived" value can be one the `<select>` cannot show. */
export const NORTH_AMERICA = 144.39
export const EUROPE_AFRICA = 144.8
export const AUSTRALIA = 145.175
export const NEW_ZEALAND = 144.575
export const JAPAN = 144.66
export const ARGENTINA = 144.93
export const BRAZIL = 145.57

/** The seven channels the picker offers, in the order it offers them. */
export const APRS_FREQS: [number, string][] = [
  [NORTH_AMERICA, 'N. America'],
  [EUROPE_AFRICA, 'Europe / Africa'],
  [AUSTRALIA, 'Australia'],
  [NEW_ZEALAND, 'New Zealand'],
  [JAPAN, 'Japan'],
  [ARGENTINA, 'Argentina'],
  [BRAZIL, 'Brazil'],
]

/**
 * The APRS channel for a Maidenhead grid — approximate by construction.
 *
 * ⭐ THE RULE WHEN A BOX IS AMBIGUOUS: RETURN 144.390. The Americas run the R2 standard and
 * Argentina and Brazil are the two exceptions, so the boxes for those two are drawn INSIDE
 * their countries rather than around them. A border region of Argentina getting the
 * continental standard is a small, visible, one-click error; a Chilean or Bolivian station
 * being auto-tuned to an Argentine or Brazilian exception channel is a surprise with no cue.
 * That asymmetry is why the South American boxes look tighter than the country outlines.
 *
 * ⚠️ THE INPUT IS A GRID SQUARE, NOT A POINT. A 4-character locator is 2° of longitude wide,
 * so `gridToLatLon` can move a town up to a degree before these comparisons see it — which is
 * why the bounds sit on square edges and not on national borders, and why two towns either
 * side of a river can be indistinguishable here.
 *
 * Known misses, written down rather than chased: Paraguay and the Uruguayan interior share
 * squares with western/southern Brazil; Argentina's Atlantic strip below 57°W and its
 * Andean fringe fall to 144.390; most of Asia (China 144.640, Korea 144.620, Thailand
 * 145.525) has no channel in the list at all and lands on 144.800. The mitigations are that
 * the resolved number is NAMED on screen, and one pick pins it forever.
 */
export function aprsChannelForGrid(grid: string): number {
  // ⚠️ VALIDATE FIRST. `gridToLatLon` is deliberately permissive — it is a distance-badge
  // helper — so a locator outside the A–R field alphabet parses into coordinates off the
  // planet instead of returning null: `ZZ99` comes back as 169°N, 339°E and lands on the
  // Europe/Africa fallback. A grid this function cannot trust must mean "nothing to derive
  // from", which is 144.390 with a label that says so.
  const ll = isValidGrid(grid) ? gridToLatLon(grid) : null
  if (!ll) return NORTH_AMERICA
  const { lat, lon } = ll

  if (lat >= 30 && lat <= 46 && lon >= 128 && lon <= 146) return JAPAN
  if (lat >= -44 && lat <= -9 && lon >= 112 && lon <= 154) return AUSTRALIA
  if (lat >= -48 && lat <= -33 && (lon >= 166 || lon <= -176)) return NEW_ZEALAND // crosses 180°

  // Argentina — tested BEFORE Brazil, because northern Argentina sits inside any workable
  // Brazil box. Held east of 70°W so Chile's central valley (Santiago is 70.7°W) and north of
  // 55°S; held west of 58°W so Uruguay and Asunción keep the R2 standard; held south of 23°S
  // so the northern Chilean desert — which lies EAST of 70°W — does not come with it.
  if (lat >= -55 && lat <= -23 && lon >= -70 && lon <= -58) return ARGENTINA

  // Brazil, in two bands, because Bolivia occupies the western half of the southern one and
  // would otherwise be swept in with it (La Paz is 68°W, 16°S).
  // North: the Amazon basin, where the only neighbours at these longitudes are Colombia and
  // Peru — held east of 70°W, which is what keeps eastern Colombia on 144.390.
  if (lat >= -9 && lat <= 4 && lon >= -70 && lon <= -35) return BRAZIL
  // South: the populated coast and interior, held east of 56°W to clear Bolivia and Paraguay.
  if (lat >= -33 && lat < -9 && lon >= -56 && lon <= -35) return BRAZIL

  if (lon >= -170 && lon <= -30) return NORTH_AMERICA // the rest of the Americas — the R2 standard
  return EUROPE_AFRICA // ITU R1, and the R1-aligned default for everywhere else
}

/** The channel to use: the operator's pin if they made one, else their region. ONE resolver,
 *  so the cockpit and the Settings label can never name different numbers. */
export const resolveAprsChannel = (pinned: number | null | undefined, grid: string): number =>
  pinned ?? aprsChannelForGrid(grid)

/** `[table, code, label]` — the beaconable symbols. The first eight are the primary table `/`
 *  the cockpit has always offered; the last two are the alternate table `\`, which is where
 *  the two identities a fixed Nexus station is most likely to want actually live. */
export const BEACON_SYMBOLS: [string, string, string][] = [
  ['/', '>', 'Car'],
  ['/', '-', 'House'],
  ['/', '[', 'Person'],
  ['/', 'b', 'Bicycle'],
  ['/', 'j', 'Jeep'],
  ['/', '<', 'Motorcycle'],
  ['/', 'k', 'Truck'],
  ['/', '.', 'Dot'],
  ['\\', '#', 'Digipeater'],
  ['\\', '&', 'iGate'],
]
