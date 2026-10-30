import { describe, it, expect } from 'vitest'
import { ribbonFor, ribbonLines } from './pathRibbon'
import type { PathPrediction } from '../types'
import { haversineKm } from '../grid'

const BOSTON = { lat: 42.36, lon: -71.06 }
const SYDNEY = { lat: -33.87, lon: 151.21 }

describe('the path ribbon lines', () => {
  it('the short path and the long path both end at the station, the long one the other way round', () => {
    const [sp, lp] = ribbonLines(BOSTON, SYDNEY)
    expect(sp.route).toBe('short')
    expect(lp.route).toBe('long')
    for (const l of [sp, lp]) {
      expect(haversineKm(l.points[0], BOSTON)).toBeLessThan(0.01)
      expect(haversineKm(l.points[l.points.length - 1], SYDNEY)).toBeLessThan(5)
    }
    // The two midpoints are antipodes: one great circle, two arcs.
    const mid = (l: typeof sp) => l.points[(l.points.length - 1) / 2]
    expect(haversineKm(mid(sp), mid(lp))).toBeGreaterThan(20_000)
    // Bearing labels at the QTH end, reciprocal.
    const brg = (s: string) => Number(s.replace(/\D+/g, ''))
    expect(sp.label).toMatch(/^SP \d+°$/)
    expect(lp.label).toMatch(/^LP \d+°$/)
    expect(Math.abs(((brg(lp.label) - brg(sp.label) + 360) % 360) - 180)).toBeLessThanOrEqual(1)
    expect(haversineKm(BOSTON, sp.labelAt)).toBeCloseTo(2000, 0)
  })

  it('draws a flagged stretch where the geometry put it, and carries the inks', () => {
    const [sp, lp] = ribbonLines(BOSTON, SYDNEY, {
      short: { ink: 'closed', brgDeg: 0, segments: [{ from: 0.5, to: 1, kind: 'grey' }] },
      long: { ink: 'open', brgDeg: 0, segments: [] },
    })
    expect(sp.ink).toBe('closed')
    expect(lp.ink).toBe('open')
    expect(sp.stretches).toHaveLength(1)
    const s = sp.stretches[0]
    expect(s.kind).toBe('grey')
    // Starts halfway along the short path, ends at the station.
    const half = haversineKm(BOSTON, SYDNEY) / 2
    expect(haversineKm(BOSTON, s.points[0])).toBeCloseTo(half, -1)
    expect(haversineKm(s.points[s.points.length - 1], SYDNEY)).toBeLessThan(5)
    expect(lp.stretches).toEqual([])
  })

  it('no model: both routes neutral; the same spot: nothing to draw', () => {
    const [sp, lp] = ribbonLines(BOSTON, SYDNEY, null)
    expect(sp.ink).toBeNull()
    expect(lp.ink).toBeNull()
    expect(ribbonLines(BOSTON, { lat: 42.36, lon: -71.0601 })).toEqual([])
  })
})

describe('the ribbon from the held outlook', () => {
  const geometry = (distKm: number) => ({ distKm, bearingDeg: 0, maxGmlatDeg: 40, nowHour: 0, hours: [], segmentsNow: [] })
  const outlook = (distKm: number): PathPrediction => ({
    engine: 'p533',
    bands: [],
    mufNow: 0,
    mufHourly: [],
    geometry: geometry(distKm),
    longPath: { engine: 'p533', bands: [], mufNow: 0, mufHourly: [], geometry: geometry(40_030 - distKm) },
    routes: [{ band: '20m', verdict: 'long', confidence: 'possible', shortBcr: 0, longBcr: 0.9, reasons: [{ kind: 'oneClosed', open: 'long' }] }],
  })
  it('colours the routes from an outlook for this pair, and names the favoured one', () => {
    const r = ribbonFor(BOSTON, SYDNEY, outlook(haversineKm(BOSTON, SYDNEY) + 10))
    expect(r.lines.map((l) => l.ink)).toEqual(['closed', 'open'])
    expect(r.favoured).toBe('long')
  })
  it("another station's outlook (still in flight) colours nothing", () => {
    const r = ribbonFor(BOSTON, SYDNEY, outlook(5300))
    expect(r.lines.map((l) => l.ink)).toEqual([null, null])
    expect(r.favoured).toBeNull()
    expect(ribbonFor(BOSTON, SYDNEY, null).lines).toHaveLength(2)
  })
})
