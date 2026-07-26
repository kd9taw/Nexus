import { describe, it, expect } from 'vitest'
import { sectorRing, destinationPoint } from './mapGeo'

const R_KM = 6371

/** Great-circle distance between two [lon, lat] points, km. */
function gcKm(a: [number, number], b: [number, number]): number {
  const toRad = (d: number) => (d * Math.PI) / 180
  const [lon1, lat1] = a
  const [lon2, lat2] = b
  const dLat = toRad(lat2 - lat1)
  const dLon = toRad(lon2 - lon1)
  const s =
    Math.sin(dLat / 2) ** 2 +
    Math.cos(toRad(lat1)) * Math.cos(toRad(lat2)) * Math.sin(dLon / 2) ** 2
  return 2 * R_KM * Math.asin(Math.min(1, Math.sqrt(s)))
}

/** How far below the sphere a straight chord between two surface points dips, in km. */
function chordSagKm(edgeKm: number): number {
  return R_KM * (1 - Math.cos(edgeKm / 2 / R_KM))
}

const QTH = { lat: 41.5, lon: -88.1 } // the operator's approximate QTH

describe('opening-sector ring geometry', () => {
  // REGRESSION (operator, screenshot 2026-07-26): the opening sectors tore through the 3-D globe
  // as green spikes, while the same sectors on the 2-D map were clean.
  //
  // Nothing in the globe layer draws great circles — syncLines maps each vertex to a 3-D point
  // and draws a STRAIGHT chord, and globe.gl's polygon layer triangulates flat between vertices.
  // The wedge used to be [qth, ...16 arc points, qth], so each RADIAL was a single segment
  // spanning the whole opening distance. A 3000 km chord sags ~78 km below the surface while the
  // layer sits ~38 km above it, so the geometry dived through the globe and poked back out.
  //
  // This is the property that actually matters: no edge may sag further than the layer altitude.
  it('keeps every edge short enough to stay above the globe surface', () => {
    // The layer altitudes in Globe3D: fill 0.006 R, outline 0.006 R.
    const layerAltKm = 0.006 * R_KM // ~38 km
    for (const maxKm of [500, 1500, 3000, 6000, 12000]) {
      const ring = sectorRing(QTH, 90, maxKm)
      let worst = 0
      for (let i = 1; i < ring.length; i++) {
        worst = Math.max(worst, chordSagKm(gcKm(ring[i - 1], ring[i])))
      }
      expect(
        worst,
        `maxKm=${maxKm}: worst edge sags ${worst.toFixed(1)} km, layer sits ${layerAltKm.toFixed(1)} km up — the wedge would cut through the globe`,
      ).toBeLessThan(layerAltKm)
    }
  })

  // The pre-fix geometry, reconstructed, so the test proves it is testing something real.
  it('the OLD single-segment radial really did sag through the surface', () => {
    const maxKm = 3000
    const far = destinationPoint(QTH, 90 - 22.5, maxKm)
    const oldRadialSag = chordSagKm(gcKm([QTH.lon, QTH.lat], [far.lon, far.lat]))
    expect(oldRadialSag).toBeGreaterThan(0.006 * R_KM)
  })

  it('is a closed ring that returns to the QTH', () => {
    const ring = sectorRing(QTH, 45, 2000)
    expect(ring[0]).toEqual(ring[ring.length - 1])
    // The QTH is the apex — it appears as the second-to-last vertex, before the closing repeat.
    const apex = ring[ring.length - 2]
    expect(apex[0]).toBeCloseTo(QTH.lon, 6)
    expect(apex[1]).toBeCloseTo(QTH.lat, 6)
  })

  it('spans the full 45 degrees centred on the bearing', () => {
    const maxKm = 1000
    const ring = sectorRing(QTH, 90, maxKm)
    const left = destinationPoint(QTH, 67.5, maxKm)
    const right = destinationPoint(QTH, 112.5, maxKm)
    // Both wedge corners must be present (within a metre or so).
    const has = (p: { lat: number; lon: number }) =>
      ring.some(([lon, lat]) => Math.abs(lon - p.lon) < 1e-6 && Math.abs(lat - p.lat) < 1e-6)
    expect(has(left)).toBe(true)
    expect(has(right)).toBe(true)
  })

  it('stays bounded for a very long opening', () => {
    // The step count is clamped so an antipodal opening cannot explode the vertex buffer.
    const ring = sectorRing(QTH, 0, 20000)
    expect(ring.length).toBeLessThan(150)
  })
})
