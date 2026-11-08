// #483: the caller card's grid square opens that square on OpenStreetMap in the browser. The URL
// is built here from a STRICT locator and handed to `open_external_url`; Nexus itself fetches
// nothing (no tiles, no request: the browser opens openstreetmap.org's own page).
import { describe, it, expect } from 'vitest'
import { gridMapUrl, gridToLatLon } from './grid'

describe('gridMapUrl (#483)', () => {
  it('a 6-character square: a pin at its centre, zoomed to the square', () => {
    const c = gridToLatLon('IN52TK')!
    const u = new URL(gridMapUrl('IN52TK')!)
    expect(u.origin).toBe('https://www.openstreetmap.org')
    expect(Number(u.searchParams.get('mlat'))).toBeCloseTo(c.lat, 3)
    expect(Number(u.searchParams.get('mlon'))).toBeCloseTo(c.lon, 3)
    expect(u.hash.startsWith('#map=13/')).toBe(true)
  })

  it('a 4-character square is zoomed out to show the whole square', () => {
    expect(gridMapUrl('fn31')!).toContain('#map=9/')
  })

  it('an 8-character locator maps at its 6-character square', () => {
    expect(gridMapUrl('IN52TK44')).toBe(gridMapUrl('IN52TK'))
  })

  it('anything that is not a locator gives no URL, so nothing is clickable', () => {
    for (const g of ['', 'IN5', 'ZZ52', 'IN52Z1', 'IN52TK4', 'IN52 TK', 'javascript:1', 'IN52TK&x=1']) {
      expect(gridMapUrl(g), g).toBeNull()
    }
  })
})
