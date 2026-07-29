import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { APRS_HOME_ZOOM, MAX_ZOOM, aprsMapCenter, makeProjection, project } from './mapGeo'
import type { LatLon } from './grid'

// THE BUG THIS EXISTS FOR (operator, 0.21.0): "I can hear APRS traffic on the radio but nothing
// decodes onto the map." The decode chain was fine — the MAP was showing a whole planet.
//
// The APRS section embeds MapView, which force-locks the orthographic 'globe' and starts at
// `zoom: 1`. At that scale the globe's radius is the Earth's radius, so roughly 23 km lands on
// ONE pixel. APRS is a 2 m local mode: 2 m simplex plus a digipeater or two reaches tens of km,
// so the ENTIRE decodable picture collapsed into a handful of pixels directly underneath the
// operator's own QTH dot. Stations were plotting correctly and were invisible.
//
// These tests pin the projection geometry the APRS map must have, in kilometres and pixels —
// the units the bug was actually expressed in.

const KANSAS: LatLon = { lat: 38.5, lon: -98.0 }
const W = 900
const H = 600

/** Where a point `km` due north of `center` lands, in pixels from the map centre. */
function pixelsFromCentre(center: LatLon, km: number, zoom: number): number {
  const proj = makeProjection('globe', center, W, H, { zoom, rotate: null, panX: 0, panY: 0 })
  const c = project(proj, center)
  const p = project(proj, { lat: center.lat + km / 111.195, lon: center.lon })
  expect(c, 'the centre must project').not.toBeNull()
  expect(p, `a station ${km} km away must project (not be clipped off the globe)`).not.toBeNull()
  return Math.hypot(p![0] - c![0], p![1] - c![1])
}

describe('the APRS map opens on the LOCAL picture, not the planet', () => {
  it('a station 40 km away is plainly separated from the operator, not under their dot', () => {
    // 40 km is an ordinary direct 2 m path. At the old default (zoom 1) this projected ~1.7 px
    // from centre — inside the 3.5 px station dot AND inside the operator's own QTH marker, so
    // every local station drew as one indistinguishable smear.
    const px = pixelsFromCentre(KANSAS, 40, APRS_HOME_ZOOM)
    expect(px).toBeGreaterThan(30)
  })

  it('the default view spans a couple of hundred km — a digipeater picture, not a continent', () => {
    // The half-width of the canvas should land somewhere in "local net" territory. Too far in and
    // a digipeated station 150 km out falls off the edge; too far out and we are back to the bug.
    const proj = makeProjection('globe', KANSAS, W, H, {
      zoom: APRS_HOME_ZOOM,
      rotate: null,
      panX: 0,
      panY: 0,
    })
    const c = project(proj, KANSAS)!
    // Walk north until we leave the canvas; that distance is the visible vertical reach.
    let reachKm = 0
    for (let km = 5; km <= 4000; km += 5) {
      const p = project(proj, { lat: KANSAS.lat + km / 111.195, lon: KANSAS.lon })
      if (!p || Math.abs(p[1] - c[1]) > H / 2) break
      reachKm = km
    }
    expect(reachKm).toBeGreaterThan(80)
    expect(reachKm).toBeLessThan(600)
  })

  it('a 200 km digipeated station is still ON the map at the default zoom', () => {
    const px = pixelsFromCentre(KANSAS, 200, APRS_HOME_ZOOM)
    expect(px).toBeLessThan(Math.min(W, H) / 2)
  })
})

describe('the APRS map redraws when a packet arrives', () => {
  // THE SHARPEST OF THE THREE. MapView's draw effect READS `aprs` and `selectedAprs` but neither
  // was in its dependency array, so a freshly decoded station did not plot. In a resting APRS
  // view the only dependency that changes on its own is `nowMs` — a 60 SECOND greyline tick —
  // so the map showed the world as it was up to a minute ago, and clicking a row in the station
  // list did not move the map's highlight either. Decode a packet, watch the map: nothing.
  //
  // A dependency array cannot be observed from outside the component, so this guards the source
  // directly — the same technique as hooks-placement.test.ts and aprsLayout.test.ts.
  const src = readFileSync(
    fileURLToPath(new URL('./components/MapView.tsx', import.meta.url)),
    'utf8',
  )

  /** The dependency array of the main draw effect (the one that ends with `void theme`). */
  function drawEffectDeps(): string {
    const marker = src.indexOf('void theme')
    expect(marker, 'the draw effect must still end with `void theme`').toBeGreaterThan(0)
    const deps = /\}, \[([^\]]*)\]\)/.exec(src.slice(marker))
    expect(deps, 'the draw effect must have a dependency array').not.toBeNull()
    return deps![1]
  }

  it('lists the APRS station list as a draw dependency', () => {
    expect(drawEffectDeps().split(',').map((s) => s.trim())).toContain('aprs')
  })

  it('lists the APRS selection as a draw dependency', () => {
    expect(drawEffectDeps().split(',').map((s) => s.trim())).toContain('selectedAprs')
  })
})

describe('the APRS map can be zoomed to a town, not just a continent', () => {
  it('the wheel ceiling clears the local-picture zoom', () => {
    // The ceiling used to be 10 — below the zoom this map needs to OPEN at, so even an
    // operator who found the wheel could not reach a usable local scale.
    expect(MAX_ZOOM).toBeGreaterThanOrEqual(APRS_HOME_ZOOM)
  })
})

describe('the APRS map still has a centre when the operator has no grid', () => {
  // The third way the map came up empty, and the most total: MapView centres on the operator's
  // grid and BAILS OUT OF THE WHOLE DRAW without one (`if (... || !me) return`). With no grid
  // configured the APRS section painted nothing at all — no basemap, no coastline, no stations —
  // while the station list beside it filled up normally.
  const heard = [
    { lat: 38.5, lon: -98.0 },
    { lat: 38.9, lon: -97.4 },
    { lat: 38.2, lon: -98.6 },
  ]

  it('falls back to the middle of the traffic actually heard', () => {
    const c = aprsMapCenter(null, heard)
    expect(c, 'heard traffic must supply a centre').not.toBeNull()
    expect(c!.lat).toBeCloseTo(38.533, 1)
    expect(c!.lon).toBeCloseTo(-98.0, 1)
    // And with that centre the stations land ON the canvas at the opening zoom.
    const proj = makeProjection('globe', c, W, H, {
      zoom: APRS_HOME_ZOOM,
      rotate: null,
      panX: 0,
      panY: 0,
    })
    for (const h of heard) {
      const p = project(proj, h)
      expect(p, `${h.lat},${h.lon} must project`).not.toBeNull()
      expect(p![0], 'on canvas horizontally').toBeGreaterThan(0)
      expect(p![0]).toBeLessThan(W)
      expect(p![1], 'on canvas vertically').toBeGreaterThan(0)
      expect(p![1]).toBeLessThan(H)
    }
  })

  it('prefers the operator grid when there is one', () => {
    const c = aprsMapCenter(KANSAS, heard)
    expect(c).toEqual(KANSAS)
  })

  it('is null when there is neither a grid nor any positioned traffic', () => {
    expect(aprsMapCenter(null, [])).toBeNull()
  })

  it('averages longitudes across the antimeridian instead of landing in Africa', () => {
    // A New Zealand / Fiji net straddles 180°. A naive mean lands at lon 0 — the exact
    // Null-Island failure this fallback exists to avoid.
    const c = aprsMapCenter(null, [
      { lat: -36.8, lon: 174.8 },
      { lat: -18.1, lon: -178.4 },
    ])
    expect(c).not.toBeNull()
    expect(Math.abs(c!.lon)).toBeGreaterThan(150)
  })
})
