/** Settings ▸ Logging & Connectors ▸ Cloudlog / Wavelog — does the station location the operator
 * picked disagree with who and where Nexus logs as?
 *
 * Wavelog files every QSO it imports under the picked location's callsign and grid, and its
 * import refuses a QSO whose own STATION_CALLSIGN is not the location's ("Differing station
 * callsign … SKIPPED") or whose own MY_GRIDSQUARE does not fit the location's ("Differing locator
 * … SKIPPED"). The API's `qso` call runs both checks (Wavelog `Logbook_model::import`, read
 * 2026-10-04) and answers HTTP 400. Nexus stamps STATION_CALLSIGN on every contact it logs, so a
 * location carrying any other callsign refuses all of them.
 *
 * The backend runs the same rule before an auto-forward (`location_mismatch` in
 * `crates/propagation/src/live/cloudlog.rs`), so that a refusal can name it. Keep the two alike.
 *
 * Pure — no React, no api, no storage.
 */

/** The two things Wavelog checks a QSO against its station location. */
export interface LocationMismatch {
  /** The location's callsign is not the call Nexus logs as. */
  callsign: boolean
  /** The location's grid is not the operator's grid, in its first four characters. */
  grid: boolean
}

/** The first four characters of a grid, uppercased — or `null` when there are fewer. */
function square(grid: string): string | null {
  const g = grid.trim().toUpperCase()
  return g.length >= 4 ? g.slice(0, 4) : null
}

/** Compare a location with the operator's callsign and grid. A side that is empty (or a grid
 *  shorter than four characters) gives no verdict: a warning naming an empty value says nothing
 *  an operator can act on. A location may list several grids separated by commas (a rover or a
 *  grid-line station), and Wavelog accepts a QSO from any of them. */
export function cloudlogLocationMismatch(
  location: { callsign: string; gridsquare: string },
  mycall: string,
  mygrid: string,
): LocationMismatch {
  const call = mycall.trim().toUpperCase()
  const theirs = location.callsign.trim().toUpperCase()
  const mine = square(mygrid)
  const squares = location.gridsquare
    .split(',')
    .map(square)
    .filter((s): s is string => s !== null)
  return {
    callsign: call !== '' && theirs !== '' && call !== theirs,
    grid: mine !== null && squares.length > 0 && !squares.includes(mine),
  }
}
