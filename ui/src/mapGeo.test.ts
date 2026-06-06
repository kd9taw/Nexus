import { describe, it, expect } from 'vitest'
import { makeProjection, project, destinationPoint } from './mapGeo'
import { gridToLatLon, haversineKm } from './grid'

const W = 800
const H = 800
const EN52 = gridToLatLon('EN52')! // ~ (42.5, -89)

describe('mapGeo (AEQD beam map)', () => {
  it('centers the operator grid at the screen center', () => {
    const proj = makeProjection('aeqd', EN52, W, H)
    const c = project(proj, EN52)!
    expect(c[0]).toBeCloseTo(W / 2, 0)
    expect(c[1]).toBeCloseTo(H / 2, 0)
  })

  it('renders a due-east point on the +x radial (bearing 90° → screen right, level)', () => {
    const proj = makeProjection('aeqd', EN52, W, H)
    const east = destinationPoint(EN52, 90, 2000)
    const p = project(proj, east)!
    expect(p[0]).toBeGreaterThan(W / 2 + 10) // to the right
    expect(Math.abs(p[1] - H / 2)).toBeLessThan(5) // ~level (straight radial)
  })

  it('makes screen distance from center increase with great-circle km (true range rings)', () => {
    const proj = makeProjection('aeqd', EN52, W, H)
    const r = (km: number) => {
      const p = project(proj, destinationPoint(EN52, 45, km))!
      return Math.hypot(p[0] - W / 2, p[1] - H / 2)
    }
    expect(r(1000)).toBeLessThan(r(3000))
    expect(r(3000)).toBeLessThan(r(5000))
  })

  it('destinationPoint is a real great-circle offset (distance + direction)', () => {
    const d = destinationPoint(EN52, 90, 1000)
    expect(haversineKm(EN52, d)).toBeCloseTo(1000, -1) // ~1000 km
    expect(d.lon).toBeGreaterThan(EN52.lon) // east
  })

  it('recenters when the operator grid changes', () => {
    const here = makeProjection('aeqd', EN52, W, H)
    const jn58 = gridToLatLon('JN58')!
    const there = makeProjection('aeqd', jn58, W, H)
    // EN52 is centered in `here` but off-center in `there`.
    const a = project(here, EN52)!
    const b = project(there, EN52)!
    expect(Math.hypot(a[0] - W / 2, a[1] - H / 2)).toBeLessThan(2)
    expect(Math.hypot(b[0] - W / 2, b[1] - H / 2)).toBeGreaterThan(50)
  })
})
