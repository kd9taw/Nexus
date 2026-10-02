// Program's map fits the searched area to its box (`programProjection`): the radius ring at the
// same fraction of the short side whatever the box, and a route's corridor the way it binds, so a
// long route in a wide box spans the box's width, not only its height. And a site's machines are
// spread, so each is a dot of its own (`spreadSites`).
import { describe, expect, it } from 'vitest'
import { programProjection, spreadSites } from './RepeaterMap'
import { destinationPoint, project } from '../mapGeo'

const IO83 = { lat: 53.5, lon: -3.0 }
const IO93 = { lat: 53.5, lon: -1.0 }
/** The default corridor, 25 mi. */
const CORRIDOR_KM = 25 * 1.609344

describe('programProjection', () => {
  it('puts the radius ring at the same fraction of the short side, whatever the box', () => {
    for (const [w, h] of [
      [603, 221],
      [968, 219],
      [603, 533],
    ]) {
      const proj = programProjection(IO83, null, 80.5, w, h)
      const top = project(proj, destinationPoint(IO83, 0, 80.5))!
      expect(h / 2 - top[1]).toBeCloseTo((0.84 * Math.min(w, h)) / 2, 0)
    }
  })

  it('fits a route the way it binds: a long route in a wide box spans its width', () => {
    const [w, h] = [603, 169]
    const proj = programProjection(IO83, IO93, CORRIDOR_KM, w, h)
    // CONTROL: every edge of the corridor is on the map.
    for (const end of [IO83, IO93]) {
      for (let brg = 0; brg < 360; brg += 30) {
        const [x, y] = project(proj, destinationPoint(end, brg, CORRIDOR_KM))!
        expect(x).toBeGreaterThanOrEqual(0)
        expect(x).toBeLessThanOrEqual(w)
        expect(y).toBeGreaterThanOrEqual(0)
        expect(y).toBeLessThanOrEqual(h)
      }
    }
    // Fitted to the box's short side alone, this 132 km route was 88 px long: 0.15 of the width.
    const a = project(proj, IO83)!
    const b = project(proj, IO93)!
    expect(Math.abs(b[0] - a[0])).toBeGreaterThan(w / 3)
  })
})

describe('spreadSites', () => {
  it('spreads the machines of one site on a ring around it and leaves a lone one where it is', () => {
    const out = spreadSites(
      [
        { id: 'a', x: 100, y: 100 },
        { id: 'b', x: 100, y: 100 },
        { id: 'c', x: 140, y: 100 },
      ],
      6,
    )
    expect(out.map((p) => [p.id, Math.round(p.x), Math.round(p.y)])).toEqual([
      ['a', 100, 94],
      ['b', 100, 106],
      ['c', 140, 100],
    ])
  })
})
