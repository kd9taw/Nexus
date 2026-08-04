// Minimal Maidenhead grid -> lat/lon and great-circle distance helpers,
// used only to show a rough "distance" badge on station cards.

export interface LatLon {
  lat: number
  lon: number
}

export function gridToLatLon(grid: string): LatLon | null {
  const g = grid.trim().toUpperCase()
  if (g.length < 4) return null
  const A = 'A'.charCodeAt(0)
  const lon0 = (g.charCodeAt(0) - A) * 20 - 180
  const lat0 = (g.charCodeAt(1) - A) * 10 - 90
  const lon1 = parseInt(g[2], 10) * 2
  const lat1 = parseInt(g[3], 10) * 1
  if (Number.isNaN(lon1) || Number.isNaN(lat1)) return null
  // center of the 2-char or 4-char square
  let lon = lon0 + lon1 + 1
  let lat = lat0 + lat1 + 0.5
  if (g.length >= 6) {
    const lonSub = (g.charCodeAt(4) - A) * (5 / 60)
    const latSub = (g.charCodeAt(5) - A) * (2.5 / 60)
    lon = lon0 + lon1 + lonSub + 2.5 / 60
    lat = lat0 + lat1 + latSub + 1.25 / 60
  }
  return { lat, lon }
}

/** Lat/lon → 4-char Maidenhead square (e.g. "EN52") — the inverse of
 * gridToLatLon at square precision. Used to ask the path predictor about a
 * map spot that has coordinates but no reported grid. */
/** STRICT Maidenhead check: exactly 4 or 6 chars, fields A–R, squares 0–9,
 * subsquares A–X. `gridToLatLon` deliberately stays permissive (it's a
 * distance-badge helper) — use THIS for the operator's OWN square (the setup
 * wizard, the programming workbench), which Nexus stores at 4/6 precision. */
export function isValidGrid(grid: string): boolean {
  return /^[A-Ra-r]{2}[0-9]{2}([A-Xa-x]{2})?$/.test(grid.trim())
}

/** The same locator alphabet, for a square that goes into a QSO RECORD: 4, 6 or
 * 8 characters. Two rulings, deliberately, and the difference is what the value
 * is FOR — not two parsers, the extended pair is the one optional group added on
 * the end of the pattern above.
 *
 *  · `isValidGrid` gates the operator's own station square, which Nexus stores
 *    and prints at 4/6 and never at 8.
 *  · This one gates a square the operator was PASSED. ADIF's GRIDSQUARE carries
 *    4, 6 or 8 characters, so all three upload cleanly to LoTW/eQSL/ClubLog —
 *    and 8 is exactly what the VHF/microwave and satellite operators who pass
 *    grids at all use. Refusing one meant a callbook that answered `FN31PR99`
 *    filled the log strip's Grid field with a value the strip then refused,
 *    disabling Log over a square the operator never typed. */
export function isValidLoggedGrid(grid: string): boolean {
  return /^[A-Ra-r]{2}[0-9]{2}([A-Xa-x]{2}([0-9]{2})?)?$/.test(grid.trim())
}

export function latLonToGrid(lat: number, lon: number): string {
  const la = Math.min(89.999, Math.max(-90, lat)) + 90
  const lo = (((Math.min(179.999, Math.max(-180, lon)) + 180) % 360) + 360) % 360
  const A = 'A'.charCodeAt(0)
  return (
    String.fromCharCode(A + Math.floor(lo / 20)) +
    String.fromCharCode(A + Math.floor(la / 10)) +
    String(Math.floor((lo % 20) / 2)) +
    String(Math.floor(la % 10))
  )
}

export function haversineKm(a: LatLon, b: LatLon): number {
  const R = 6371
  const dLat = ((b.lat - a.lat) * Math.PI) / 180
  const dLon = ((b.lon - a.lon) * Math.PI) / 180
  const la1 = (a.lat * Math.PI) / 180
  const la2 = (b.lat * Math.PI) / 180
  const h =
    Math.sin(dLat / 2) ** 2 + Math.cos(la1) * Math.cos(la2) * Math.sin(dLon / 2) ** 2
  return 2 * R * Math.asin(Math.sqrt(h))
}

/**
 * The best position we have for a station: the exact coordinates a callbook
 * reported, else the center of its reported grid square, else nothing.
 *
 * A locator is a BOX, not a point, and the box is big: a 4-character square spans
 * 2° of longitude by 1° of latitude, so its center can be ~100 km from the real
 * station. That is a degree or so of bearing on a DX path but up to ~30° on a
 * station 150 miles away — which is why a grid-derived heading disagrees with
 * QRZ's, and why exact coordinates win whenever a callbook actually has them.
 */
export function stationLatLon(
  coords: LatLon | null | undefined,
  grid: string | null | undefined,
): LatLon | null {
  if (coords && Number.isFinite(coords.lat) && Number.isFinite(coords.lon)) return coords
  return grid ? gridToLatLon(grid) : null
}

/** Distance between two RESOLVED points, in statute miles (the unit QRZ shows). */
export function distanceLabelAt(a: LatLon, b: LatLon): string {
  return `${Math.round(haversineKm(a, b) * 0.621371)} mi`
}

export function distanceLabel(myGrid: string, peerGrid: string | null): string | null {
  if (!peerGrid) return null
  const me = gridToLatLon(myGrid)
  const them = gridToLatLon(peerGrid)
  if (!me || !them) return null
  return distanceLabelAt(me, them)
}

/** Initial great-circle bearing (degrees, 0–359) from `a` to `b`. */
export function bearingDeg(a: LatLon, b: LatLon): number {
  const la1 = (a.lat * Math.PI) / 180
  const la2 = (b.lat * Math.PI) / 180
  const dLon = ((b.lon - a.lon) * Math.PI) / 180
  const y = Math.sin(dLon) * Math.cos(la2)
  const x =
    Math.cos(la1) * Math.sin(la2) - Math.sin(la1) * Math.cos(la2) * Math.cos(dLon)
  const deg = (Math.atan2(y, x) * 180) / Math.PI
  return Math.round((deg + 360) % 360)
}

/** Initial true bearing between two RESOLVED points, e.g. "312°". */
export function bearingLabelAt(a: LatLon, b: LatLon): string {
  return `${bearingDeg(a, b)}°`
}

/** Short bearing label from my grid to a peer grid, e.g. "312°", or null. */
export function bearingLabel(myGrid: string, peerGrid: string | null): string | null {
  if (!peerGrid) return null
  const me = gridToLatLon(myGrid)
  const them = gridToLatLon(peerGrid)
  if (!me || !them) return null
  return bearingLabelAt(me, them)
}

/** The magnetic heading for a true bearing given the QTH declination (° east-
 * positive, WMM): magnetic = true − declination. Null declination = unknown. */
export function magneticDeg(trueDeg: number, declination: number | null): number | null {
  if (declination == null || !Number.isFinite(declination)) return null
  return Math.round((trueDeg - declination + 360) % 360)
}
