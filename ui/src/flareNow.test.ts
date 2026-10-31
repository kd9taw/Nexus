import { describe, expect, it } from 'vitest'
import {
  bandRings,
  drapAt,
  drapFresh,
  DRAP_FRESH_SECS,
  flareFootprint,
  flareHere,
  flareNoaaOf,
  flarePhase,
  flareRecovery,
  flareUnderSun,
  sunWarmth,
  warmSunInk,
} from './flareNow'
import { flareHafAt, flareHafMhz, flareRecoveryMin, subsolarPoint } from './mapGeo'
import type { DrapGrid, FlareEvent } from './types'

const NOW = Date.UTC(2026, 9, 30, 18, 0) // 18:00Z: the sun over the eastern Pacific
const NOW_S = NOW / 1000
const X1 = 1e-4

/** A grid in NOAA's layout whose cells are the analytic field × `gain` at `atMs`. */
function grid(atMs: number, xray: number, gain = 1, recoveryAt: number | null = null): DrapGrid {
  const lats = Array.from({ length: 90 }, (_, i) => 89 - 2 * i)
  const lons = Array.from({ length: 90 }, (_, i) => -178 + 4 * i)
  const haf: number[] = []
  for (const la of lats) for (const lo of lons) haf.push(Math.round(flareHafAt(la, lo, atMs, xray) * gain * 10) / 10)
  return { validAt: atMs / 1000, recoveryAt, lats, lons, haf }
}

/** Great-circle angle (degrees) between two points. */
function angle(a: { lat: number; lon: number }, b: { lat: number; lon: number }): number {
  const r = Math.PI / 180
  const c =
    Math.sin(a.lat * r) * Math.sin(b.lat * r) + Math.cos(a.lat * r) * Math.cos(b.lat * r) * Math.cos((a.lon - b.lon) * r)
  return Math.acos(Math.max(-1, Math.min(1, c))) / r
}

describe('NOAA grid freshness and sampling', () => {
  it('is current for three minutes after its valid time, not before or after', () => {
    const g = grid(NOW, X1)
    expect(drapFresh(g, NOW)).toBe(true)
    expect(drapFresh(g, NOW + (DRAP_FRESH_SECS - 1) * 1000)).toBe(true)
    expect(drapFresh(g, NOW + (DRAP_FRESH_SECS + 1) * 1000)).toBe(false)
    expect(drapFresh(null, NOW)).toBe(false)
    // A valid time well in the future is a broken clock, not a forecast to draw.
    expect(drapFresh(g, NOW - 10 * 60_000)).toBe(false)
  })

  it('samples bilinearly and wraps the dateline (mirrors the Rust sampler)', () => {
    // 22:00Z: the sun is near 150°W, so the dateline is sunlit and its two columns differ.
    const g = grid(Date.UTC(2026, 9, 30, 22, 0), X1)
    const cell = (r: number, c: number) => g.haf[r * 90 + c]
    expect(drapAt(g, 31, -90)).toBeCloseTo(cell(29, 22), 5)
    expect(drapAt(g, 30, -88)).toBeCloseTo((cell(29, 22) + cell(29, 23) + cell(30, 22) + cell(30, 23)) / 4, 4)
    // Across the dateline, on the row nearest the sun's latitude (13°S, row 51).
    expect(Math.abs(cell(51, 89) - cell(51, 0)), 'CONTROL: the dateline columns differ').toBeGreaterThan(0.5)
    expect(drapAt(g, -13, 180)).toBeCloseTo((cell(51, 89) + cell(51, 0)) / 2, 4)
    expect(drapAt(g, -13, -180)).toBeCloseTo(drapAt(g, -13, 180), 5)
    expect(drapAt(g, 90, -90)).toBeCloseTo(cell(0, 22), 5)
  })
})

describe('flareFootprint', () => {
  it("takes NOAA's grid while it is current, and says so", () => {
    const hot = grid(NOW, X1, 1.2) // NOAA reads 20 % hotter than the GOES flux implies
    const f = flareFootprint(NOW + 60_000, X1, hot)!
    expect(f.source).toBe('noaa')
    expect(f.validAt).toBe(NOW_S)
    expect(f.hafSub).toBeGreaterThan(28) // the grid's ~30, not the analytic 25
    expect(f.gapMhz!).toBeGreaterThan(3)
    const ss = subsolarPoint(NOW + 60_000)
    expect(f.hafAt(ss.lat, ss.lon)).toBeCloseTo(drapAt(hot, ss.lat, ss.lon), 5)
    expect(f.field.length).toBeGreaterThan(100)
    expect(f.field.every((s) => s.haf >= 2)).toBe(true)
  })

  it('falls back to the analytic field, labelled, when the grid is old or absent', () => {
    const old = grid(NOW - (DRAP_FRESH_SECS + 60) * 1000, X1, 1.2)
    for (const g of [old, null, undefined]) {
      const f = flareFootprint(NOW, X1, g)!
      expect(f.source).toBe('goes')
      expect(f.validAt).toBeNull()
      expect(f.gapMhz).toBeNull()
      expect(f.hafSub).toBeCloseTo(flareHafMhz(X1), 5)
    }
    expect(flareFootprint(NOW, null, null)).toBeNull()
    expect(flareFootprint(NOW, 0, null)).toBeNull()
  })

  it('never draws the night side, even from a grid with values there', () => {
    const g = grid(NOW, X1)
    const ss = subsolarPoint(NOW)
    g.haf = g.haf.map(() => 9) // a grid with 9 MHz everywhere (the night side would be a proton cap)
    const f = flareFootprint(NOW, X1, g)!
    expect(f.field.every((s) => angle(s, ss) < 90)).toBe(true)
  })
})

describe('band rings', () => {
  it('are circles round the sub-solar point at the D-RAP radius (analytic)', () => {
    const f = flareFootprint(NOW, X1, null)! // X1: HAF 25 under the sun
    const rings = bandRings(f, NOW)
    const ss = subsolarPoint(NOW)
    const r20 = rings.find((r) => r.band === '20m' && r.db === 1)!
    // cos χ = (14.074 / 25)^(4/3)
    const want = (Math.acos(Math.pow(14.074 / 25, 4 / 3)) * 180) / Math.PI
    for (const [lon, lat] of r20.ring) expect(angle({ lat, lon }, ss)).toBeCloseTo(want, 1)
    // Closed.
    expect(r20.ring[0]).toEqual(r20.ring[r20.ring.length - 1])
    // 10 dB needs HAF ≥ f·10^(2/3) = 4.64 f: at X1 the bands up to 5.39 MHz — 160, 80 and 60 m
    // (60 m's 5.357 needs 24.87, just under X1's 25).
    expect(rings.filter((r) => r.db === 10).map((r) => r.band)).toEqual(['160m', '80m', '60m'])
    // 12 m (24.9) is just inside X1's 25 MHz; 10 m is not reached.
    expect(rings.some((r) => r.band === '12m' && r.db === 1)).toBe(true)
    expect(rings.some((r) => r.band === '10m')).toBe(false)
  })

  it("trace NOAA's grid: the same law gives the same ring within a grid cell", () => {
    const f = flareFootprint(NOW, X1, grid(NOW, X1))!
    expect(f.source).toBe('noaa')
    const ss = subsolarPoint(NOW)
    const r = bandRings(f, NOW).find((x) => x.band === '20m' && x.db === 1)!
    const want = (Math.acos(Math.pow(14.074 / f.hafSub, 4 / 3)) * 180) / Math.PI
    for (const [lon, lat] of r.ring) expect(Math.abs(angle({ lat, lon }, ss) - want)).toBeLessThan(2.5)
  })

  it("follow NOAA's grid where it differs from the GOES estimate", () => {
    const hot = flareFootprint(NOW, X1, grid(NOW, X1, 1.2))!
    const cold = flareFootprint(NOW, X1, null)!
    const ss = subsolarPoint(NOW)
    const radius = (f: typeof hot) => {
      const r = bandRings(f, NOW).find((x) => x.band === '20m' && x.db === 1)!
      return angle({ lon: r.ring[0][0], lat: r.ring[0][1] }, ss)
    }
    expect(radius(hot)).toBeGreaterThan(radius(cold) + 5)
  })

  it("trace a lopsided NOAA grid as it is, not as a circle", () => {
    // NOAA's grid is not bound to the circle law (its proton part, its own model): halve every cell
    // more than 10° east of the sun, and the 20 m ring must come in on the east side and stay out on the west.
    const g = grid(NOW, X1)
    const ss = subsolarPoint(NOW)
    g.haf = g.haf.map((v, i) => {
      const lon = g.lons[i % 90]
      const east = ((lon - ss.lon + 540) % 360) - 180 > 10 // the cells under the sun stay as they are
      return east ? v / 2 : v
    })
    const f = flareFootprint(NOW, X1, g)!
    const r = bandRings(f, NOW).find((x) => x.band === '20m' && x.db === 1)!
    // Bearing 90° (east) is the 23rd point (4° steps), 270° (west) the 68th.
    const east = angle({ lon: r.ring[22][0], lat: r.ring[22][1] }, ss)
    const west = angle({ lon: r.ring[67][0], lat: r.ring[67][1] }, ss)
    expect(west - east).toBeGreaterThan(15)
  })

  it('start at M1, the same onset as the rest of the flare layer', () => {
    // C9.9 still has a ~15 MHz HAF, but the layer's gate is R1: no footprint, so no rings.
    expect(flareFootprint(NOW, 9.9e-6, null)).toBeNull()
    expect(flareFootprint(NOW, 9.9e-6, grid(NOW, 9.9e-6))).toBeNull()
    expect(bandRings(flareFootprint(NOW, 1e-5, null)!, NOW).length).toBeGreaterThan(0)
  })
})

describe('what the flare does here', () => {
  it('names the highest band out and weakened, and the night side', () => {
    const f = flareFootprint(NOW, 1e-3, null)! // X10: HAF 35 under the sun
    const ss = subsolarPoint(NOW)
    const under = flareHere(f, ss.lat, ss.lon, NOW)
    // 35 / 4.64 = 7.5 → 40 m out; 35 → 10 m weakened.
    expect(under).toEqual({ night: false, out: '40m', weak: '10m' })
    expect(flareUnderSun(f)).toEqual({ out: '40m', weak: '10m' })
    const anti = { lat: -ss.lat, lon: ss.lon + 180 }
    expect(flareHere(f, anti.lat, anti.lon, NOW)).toEqual({ night: true, out: null, weak: null })
    // An M1 (HAF 15): 160 m out under the sun (it needs 8.5), 20 m weakened (14.074 ≤ 15).
    expect(flareUnderSun(flareFootprint(NOW, 1e-5, null)!)).toEqual({ out: '160m', weak: '20m' })
    // A C-class sun is not a flare footprint at all (the M1 gate), so nothing to name.
  })
})

describe('flarePhase', () => {
  const ev = (o: Partial<FlareEvent>): FlareEvent => ({
    at: NOW_S - 60,
    beginAt: NOW_S - 20 * 60,
    beginClass: 'C5.0',
    maxAt: null,
    maxClass: null,
    maxFlux: null,
    endAt: null,
    currentClass: 'M2.0',
    ...o,
  })

  it("speaks NOAA's words with NOAA's times", () => {
    expect(flarePhase(ev({}), 'falling', NOW, null)).toMatchObject({ kind: 'rising', noaa: true, beginAt: NOW_S - 1200 })
    expect(flarePhase(ev({ maxAt: NOW_S - 600, maxClass: 'M4.2' }), 'rising', NOW, null)).toMatchObject({
      kind: 'peaked',
      noaa: true,
      peakAt: NOW_S - 600,
      peakClass: 'M4.2',
    })
    expect(flarePhase(ev({ maxAt: NOW_S - 600, endAt: NOW_S - 120 }), null, NOW, null)).toMatchObject({
      kind: 'ended',
      noaa: true,
      endAt: NOW_S - 120,
    })
  })

  it('falls back to the trend word, not NOAA, when the list is stale, old or absent', () => {
    const fallbacks: Array<FlareEvent | null> = [
      null,
      ev({ at: NOW_S - 11 * 60 }), // the list stopped updating
      ev({ beginAt: NOW_S - 7 * 3600 }), // a flare from this morning
      ev({ maxAt: NOW_S - 3 * 3600, endAt: NOW_S - 2 * 3600 }), // over for two hours
    ]
    for (const e of fallbacks) {
      expect(flarePhase(e, 'falling', NOW, null)).toEqual({
        kind: 'falling',
        noaa: false,
        beginAt: null,
        peakAt: null,
        peakClass: null,
        endAt: null,
      })
      expect(flarePhase(e, 'steady', NOW, null).kind).toBeNull()
    }
  })

  // NOAA lists a flare only after several minutes of rise; an impulsive one crosses M1 sooner. Until
  // NOAA lists it, the newest event on the list is the PREVIOUS flare, and its words are not this one's.
  const C8_ENDED = ev({
    beginAt: NOW_S - 90 * 60,
    beginClass: 'C2.0',
    maxAt: NOW_S - 60 * 60,
    maxClass: 'C8.0',
    maxFlux: 8e-6,
    endAt: NOW_S - 30 * 60,
    currentClass: 'C3.0',
  })

  it("never quotes the previous flare's words for a new one", () => {
    // A C8 ended 30 min ago; an M2 is rising now and NOAA has not listed it yet.
    expect(flarePhase(C8_ENDED, 'rising', NOW, 2e-5)).toMatchObject({ kind: 'rising', noaa: false, endAt: null })
    // The flux is well above the event's peak, whatever the trend says.
    expect(flarePhase(C8_ENDED, 'steady', NOW, 2e-5)).toMatchObject({ kind: null, noaa: false })
    // Peaked (not ended) at C8, and the flux now an M2: not that event either.
    const peaked = { ...C8_ENDED, endAt: null }
    expect(flarePhase(peaked, 'falling', NOW, 2e-5)).toMatchObject({ kind: 'falling', noaa: false })
    // Ended, and the flux is rising again: a new rise, even with no peak flux to compare.
    expect(flarePhase({ ...C8_ENDED, maxFlux: null }, 'rising', NOW, 6e-6)).toMatchObject({ kind: 'rising', noaa: false })
  })

  it("POSITIVE CONTROL: the same event at its own flux keeps NOAA's words", () => {
    expect(flarePhase(C8_ENDED, 'falling', NOW, 5e-6)).toMatchObject({ kind: 'ended', noaa: true, endAt: NOW_S - 1800 })
    // Within 10 % of NOAA's peak is the same flare (the GOES sample and NOAA's peak differ a little).
    expect(flarePhase({ ...C8_ENDED, endAt: null }, 'falling', NOW, 8.5e-6)).toMatchObject({ kind: 'peaked', noaa: true })
    // A rising event is this flare's however high the flux goes.
    expect(flarePhase(ev({}), 'rising', NOW, 2e-4)).toMatchObject({ kind: 'rising', noaa: true })
  })
})

describe('flareNoaaOf: a simulation never carries NOAA', () => {
  const lane = { asOf: NOW_S - 60, series: [[NOW_S - 60, 5e-5]] as [number, number][], event: null, drap: grid(NOW, X1) }
  it("drops NOAA's grid, event and trace beside simulated flux", () => {
    expect(flareNoaaOf(lane, true)).toBeNull()
  })
  it('POSITIVE CONTROL: the real lane carries them', () => {
    expect(flareNoaaOf(lane, false)).toMatchObject({ xrayAt: NOW_S - 60, drap: lane.drap })
    expect(flareNoaaOf(null, false)).toBeNull()
  })
})

describe('flareRecovery', () => {
  it("prefers NOAA's recovery time from a current grid, else NOAA's empirical fit", () => {
    const g = grid(NOW, X1, 1, NOW_S + 40 * 60)
    expect(flareRecovery(g, X1, NOW)).toEqual({ mins: 40, noaa: true })
    // The grid is old: the fit on the current flux.
    expect(flareRecovery(g, X1, NOW + (DRAP_FRESH_SECS + 1) * 1000)).toEqual({
      mins: Math.round(flareRecoveryMin(X1)!),
      noaa: false,
    })
    // NOAA's time has passed: not a negative countdown — the fit.
    const past = grid(NOW, X1, 1, NOW_S - 60)
    expect(flareRecovery(past, X1, NOW)!.noaa).toBe(false)
    // Below M1 with no NOAA time: nothing.
    expect(flareRecovery(null, 5e-6, NOW)).toBeNull()
  })
})

describe('the quiet-sun warmth below the flare gate', () => {
  it('warms the sun disc from C1 to M1 and leaves a quiet sun alone', () => {
    expect(sunWarmth(null)).toBe(0)
    expect(sunWarmth(5e-7)).toBe(0) // B5
    expect(sunWarmth(1e-6)).toBe(0) // C1: the start
    expect(sunWarmth(Math.sqrt(1e-6 * 1e-5))).toBeCloseTo(0.5, 5) // ~C3.2, half way on the log scale
    expect(sunWarmth(1e-5)).toBe(1) // M1: the flare layer takes over
    expect(sunWarmth(1e-3)).toBe(1)
  })

  it('mixes the theme sun toward orange, and keeps a colour it cannot read', () => {
    expect(warmSunInk('#ffd166', 0)).toBe('#ffd166')
    expect(warmSunInk('#ffd166', 1)).toBe('#ff8c3a')
    expect(warmSunInk('#ffd166', 0.5)).toBe('#ffaf50')
    expect(warmSunInk('rgb(1, 2, 3)', 1)).toBe('rgb(1, 2, 3)')
  })
})
