import { describe, it, expect } from 'vitest'
import {
  makeProjection,
  project,
  destinationPoint,
  subsolarPoint,
  terminator,
  solarElevationDeg,
  nextTerminatorMs,
  sunDay,
  mufMhz,
  flareHafMhz,
  flareHafAt,
  flareRScale,
  flareClass,
  flareRecoveryMin,
  flareField,
  maidenheadLabels,
  viewCentredOn,
} from './mapGeo'
import { gridToLatLon, haversineKm } from './grid'

const W = 800
const H = 800
const EN52 = gridToLatLon('EN52')! // ~ (42.5, -89)

describe('Maidenhead labels on the flat map turned to the operator', () => {
  // The flat map's seam is the operator's antimeridian (91° E for EN52), so it runs through fields
  // and squares. A field there must still be measured as the 20° it is on screen (a probe across the
  // seam once measured EN52's 80–100° E column at 15911 px and wrote 100 square labels in each field
  // of it at world zoom), and each label must be written inside its own cell, on a side of the seam
  // that is on screen.
  const FW = 1600
  const FH = 800
  /** Is (x, y) inside the Maidenhead cell named `text` (a field or a 4-character square)? */
  function inOwnCell(proj: ReturnType<typeof makeProjection>, text: string, x: number, y: number): boolean {
    const ll = proj.invert!([x, y])!
    let west = -180 + (text.charCodeAt(0) - 65) * 20
    let south = -90 + (text.charCodeAt(1) - 65) * 10
    let dw = 20
    let dh = 10
    if (text.length === 4) {
      west += Number(text[2]) * 2
      south += Number(text[3])
      dw = 2
      dh = 1
    }
    const off = (((ll[0] - west) % 360) + 360) % 360
    return off <= dw + 1e-6 && ll[1] >= south - 1e-6 && ll[1] <= south + dh + 1e-6
  }
  // EN52 (seam 91° E, through field NL), and grids by the antimeridian whose seam runs past
  // Greenwich (AA09, RR73), with Greenwich and Sydney besides.
  const GRIDS = ['EN52', 'AA09', 'RR73', 'IO91', 'QF56']

  it.each(GRIDS)("%s, world zoom: one label per field, each the field's true width, in its own field", (g) => {
    const proj = makeProjection('world', gridToLatLon(g)!, FW, FH)
    const k = (proj.scale() * Math.PI) / 180
    const labels = maidenheadLabels('world', proj, FW, FH)
    expect(labels.length).toBeGreaterThan(100)
    expect(labels.filter((l) => l.square)).toEqual([])
    const counts = new Map<string, number>()
    for (const l of labels) {
      counts.set(l.text, (counts.get(l.text) ?? 0) + 1)
      expect(l.fieldW).toBeCloseTo(20 * k, 6)
      expect(inOwnCell(proj, l.text, l.x, l.y)).toBe(true)
    }
    expect(Math.max(...counts.values())).toBe(1)
  })

  it.each([
    ['EN52', 95, 'NL'], // the 91–100° E part of field NL, at the map's left edge
    ['EN52', 86, 'NL'], // its 80–91° E part, at the right edge
    ['AA09', 1.5, 'JM'], // seam at 1° E: the east side of the 0–2° E squares
    ['AA09', 0.5, 'JM'], // and their west side
  ] as const)('%s zoomed in at %s° E: the squares of field %s on screen are written, each in its own square', (g, lon, field) => {
    const me = gridToLatLon(g)!
    const view = viewCentredOn('world', me, { lat: 30, lon }, FW, FH, { zoom: 6, rotate: null, panX: 0, panY: 0 })
    const proj = makeProjection('world', me, FW, FH, view)
    const labels = maidenheadLabels('world', proj, FW, FH)
    expect(labels.length).toBeGreaterThan(0)
    for (const l of labels) {
      expect(l.square).toBe(true)
      expect(l.x).toBeGreaterThanOrEqual(0)
      expect(l.x).toBeLessThanOrEqual(FW)
      expect(inOwnCell(proj, l.text, l.x, l.y)).toBe(true)
    }
    expect(labels.some((l) => l.text.startsWith(field))).toBe(true)
  })

  it.each([['globe', 1], ['aeqd', 3]] as const)('the %s (zoom %s) still labels the operator\'s field', (kind, zoom) => {
    const proj = makeProjection(kind, EN52, W, H, { zoom, rotate: null, panX: 0, panY: 0 })
    expect(maidenheadLabels(kind, proj, W, H).some((l) => l.text === 'EN')).toBe(true)
  })
})

describe('every map opens centred on the operator', () => {
  // The flat map too: the operator's longitude is the middle column, the world still whole (the
  // seam moves to the antimeridian of the station), and the equator stays the middle row.
  it.each([
    ['EN52', EN52],
    ['QF56 (Sydney)', gridToLatLon('QF56')!],
    ['JO62 (Berlin)', gridToLatLon('JO62')!],
  ])('the flat map puts %s in the middle column', (_g, at) => {
    const proj = makeProjection('world', at, 1600, 800)
    const c = project(proj, at)!
    expect(c[0]).toBeCloseTo(800, 0)
    expect(project(proj, { lat: 0, lon: at.lon })![1]).toBeCloseTo(400, 0)
  })

  it.each([['globe'], ['aeqd']] as const)('the %s puts the operator at the centre', (kind) => {
    const c = project(makeProjection(kind, EN52, W, H), EN52)!
    expect(c[0]).toBeCloseTo(W / 2, 0)
    expect(c[1]).toBeCloseTo(H / 2, 0)
  })
})

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

  it('globe centers the operator and zoom magnifies', () => {
    const proj = makeProjection('globe', EN52, W, H)
    const c = project(proj, EN52)!
    expect(c[0]).toBeCloseTo(W / 2, 0)
    expect(c[1]).toBeCloseTo(H / 2, 0)
    const near = destinationPoint(EN52, 90, 1500)
    const r = (zoom: number) => {
      const p = project(makeProjection('globe', EN52, W, H, { zoom, rotate: null, panX: 0, panY: 0 }), near)!
      return Math.hypot(p[0] - W / 2, p[1] - H / 2)
    }
    expect(r(2)).toBeGreaterThan(r(1)) // zoomed in → further from center
  })

  it('globe rotation moves the operator off-center', () => {
    const centered = makeProjection('globe', EN52, W, H)
    const spun = makeProjection('globe', EN52, W, H, { zoom: 1, rotate: [0, 0], panX: 0, panY: 0 })
    const a = project(centered, EN52)!
    const b = project(spun, EN52)
    // With an explicit (0,0) rotation EN52 is no longer at screen center (and may
    // even rotate to the hidden hemisphere → null).
    if (b) expect(Math.hypot(b[0] - W / 2, b[1] - H / 2)).toBeGreaterThan(20)
    expect(Math.hypot(a[0] - W / 2, a[1] - H / 2)).toBeLessThan(2)
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

describe('mapGeo (greyline / terminator)', () => {
  it('puts the subsolar point near (0,0) at the March equinox ~12:00 UTC', () => {
    // 2024-03-20 12:00 UTC — equinox: declination ~0; subsolar lon ~0 at UTC noon.
    const ms = Date.UTC(2024, 2, 20, 12, 0, 0)
    const ss = subsolarPoint(ms)
    expect(Math.abs(ss.lat)).toBeLessThan(1.5) // declination ~0 at equinox
    expect(Math.abs(ss.lon)).toBeLessThan(5) // ~Greenwich meridian at 12 UTC (eq-of-time)
  })

  it('tracks the sun westward ~15°/hour', () => {
    const noon = subsolarPoint(Date.UTC(2024, 2, 20, 12, 0, 0))
    const oneLater = subsolarPoint(Date.UTC(2024, 2, 20, 13, 0, 0))
    // an hour later the subsolar point is ~15° further west (more negative lon)
    expect(oneLater.lon).toBeLessThan(noon.lon)
    expect(noon.lon - oneLater.lon).toBeCloseTo(15, 0)
  })

  it('puts the subsolar latitude in the northern hemisphere at the June solstice', () => {
    const ss = subsolarPoint(Date.UTC(2024, 5, 21, 12, 0, 0))
    expect(ss.lat).toBeGreaterThan(20) // ~+23.4° at the solstice
  })

  it('builds four nested night caps + a day/night line', () => {
    const t = terminator(Date.UTC(2024, 2, 20, 12, 0, 0))
    expect(t.caps).toHaveLength(4)
    expect(t.line).toBeTruthy()
    expect(t.subsolar).toEqual(subsolarPoint(Date.UTC(2024, 2, 20, 12, 0, 0)))
  })

  it('nextTerminatorMs lands on the horizon (elevation ~0) within 24h', () => {
    const now = Date.UTC(2024, 5, 21, 6, 0, 0)
    const lat = 41.7
    const lon = -87.6 // Chicago
    const n = nextTerminatorMs(lat, lon, now)
    expect(n.atMs).toBeGreaterThan(now)
    expect(n.atMs).toBeLessThanOrEqual(now + 25 * 3_600_000)
    expect(Math.abs(solarElevationDeg(lat, lon, n.atMs))).toBeLessThan(0.5) // on the horizon
    expect(['rise', 'set']).toContain(n.kind)
  })

  it('nextTerminatorMs alternates sunrise/sunset', () => {
    const lat = 41.7
    const lon = -87.6
    const first = nextTerminatorMs(lat, lon, Date.UTC(2024, 5, 21, 6, 0, 0))
    const second = nextTerminatorMs(lat, lon, first.atMs + 60_000)
    expect(second.kind).not.toBe(first.kind)
  })
})

describe('mapGeo (MUF / solar elevation)', () => {
  const ms = Date.UTC(2024, 5, 21, 12, 0, 0)
  const ss = subsolarPoint(ms)
  const antiLat = -ss.lat
  const antiLon = ((ss.lon + 180 + 540) % 360) - 180

  it('sun overhead at the subsolar point, below the horizon at the antipode', () => {
    expect(solarElevationDeg(ss.lat, ss.lon, ms)).toBeGreaterThan(89)
    expect(solarElevationDeg(antiLat, antiLon, ms)).toBeLessThan(-89)
  })

  it('MUF rises with SFI in daylight and floors at ~9 MHz at night', () => {
    const dayLow = mufMhz(ss.lat, ss.lon, ms, 70)
    const dayHigh = mufMhz(ss.lat, ss.lon, ms, 200)
    expect(dayHigh).toBeGreaterThan(dayLow) // SFI raises MUF
    const night = mufMhz(antiLat, antiLon, ms, 200)
    expect(night).toBeCloseTo(9, 0) // foF2 floor 3 MHz × M3000
    expect(dayHigh).toBeGreaterThan(night)
  })
})

describe('mapGeo (D-RAP flare absorption)', () => {
  const ms = Date.UTC(2024, 5, 21, 12, 0, 0)
  const ss = subsolarPoint(ms)
  const antiLat = -ss.lat
  const antiLon = ((ss.lon + 180 + 540) % 360) - 180

  it('hits the D-RAP HAF anchors: M1 → 15 MHz, X1 → 25 MHz at the subsolar point', () => {
    expect(flareHafMhz(1e-5)).toBeCloseTo(15, 5)
    expect(flareHafMhz(1e-4)).toBeCloseTo(25, 5)
    expect(flareHafMhz(5e-5)).toBeCloseTo(10 * Math.log10(5e-5) + 65, 5) // M5 ≈ 22
  })

  it('is zero (nothing to draw) for the quiet sun', () => {
    expect(flareHafMhz(1e-7)).toBe(0) // B1: 10·(−7)+65 < 0 → clamped
    expect(flareHafMhz(0)).toBe(0)
    expect(flareHafMhz(NaN)).toBe(0)
  })

  it('tapers as cos(χ)^0.75: full under the sun, zero on the night side', () => {
    expect(flareHafAt(ss.lat, ss.lon, ms, 1e-4)).toBeCloseTo(25, 1) // χ ≈ 0
    expect(flareHafAt(antiLat, antiLon, ms, 1e-4)).toBe(0) // night
    // A point where the sun sits at 30° elevation: cos χ = sin 30° = 0.5.
    const p30 = destinationPoint(ss, 0, (90 - 30) * 111.195)
    const expected = 25 * Math.pow(0.5, 0.75)
    expect(flareHafAt(p30.lat, p30.lon, ms, 1e-4)).toBeCloseTo(expected, 0)
  })

  it('mirrors the model.rs R-scale thresholds', () => {
    expect(flareRScale(9e-6)).toBe(0)
    expect(flareRScale(1e-5)).toBe(1) // M1
    expect(flareRScale(5e-5)).toBe(2) // M5
    expect(flareRScale(1e-4)).toBe(3) // X1
    expect(flareRScale(1e-3)).toBe(4) // X10
    expect(flareRScale(2e-3)).toBe(5) // X20
  })

  it('labels the flare class like the GOES convention', () => {
    expect(flareClass(1.2e-4)).toBe('X1.2')
    expect(flareClass(5e-5)).toBe('M5.0')
    expect(flareClass(2.3e-6)).toBe('C2.3')
  })

  it("estimates recovery ≈25 min at M1, ≈60 at X1, ≈120 at X5, none below M1", () => {
    expect(flareRecoveryMin(1e-5)!).toBeCloseTo(25, 0)
    expect(flareRecoveryMin(1e-4)!).toBeCloseTo(58, 0)
    expect(flareRecoveryMin(5e-4)!).toBeCloseTo(120, 0)
    expect(flareRecoveryMin(5e-6)).toBeNull()
  })

  // Above X5 NOAA's fit continues as 100·L + 450; it used to be capped at the X5 value, so an
  // X10 or an X20 read as recovering as soon as an X5.
  it('continues above X5 with NOAA\'s 100·L + 450, not a cap', () => {
    expect(flareRecoveryMin(1e-3)!).toBeCloseTo(150, 5) // X10
    expect(flareRecoveryMin(2e-3)!).toBeCloseTo(100 * Math.log10(2e-3) + 450, 5) // X20 ≈ 180
    // The two pieces meet at X5 (L −3.3) within a minute: no jump in the countdown.
    const l = -3.3
    expect(Math.abs(32.19 * l * l + 323.45 * l + 837.2 - (100 * l + 450))).toBeLessThan(1)
    expect(flareRecoveryMin(1e-3)!).toBeGreaterThan(flareRecoveryMin(5e-4)!)
  })

  it('flareField agrees with flareHafAt (the hoisted math is the same physics)', () => {
    const field = flareField(ms, 1e-4)
    expect(field.length).toBeGreaterThan(500) // roughly the dayside half of the grid
    for (const s of [field[0], field[Math.floor(field.length / 2)], field[field.length - 1]]) {
      expect(s.haf).toBeCloseTo(flareHafAt(s.lat, s.lon, ms, 1e-4), 6)
      expect(s.haf).toBeGreaterThanOrEqual(2)
    }
    expect(flareField(ms, 1e-7)).toEqual([]) // quiet sun → nothing
  })
})

// TODAY's sunrise and sunset for the Clock box. "Today" is the location's own solar day (local
// mean midnight to midnight at that longitude), so late in the evening the box still shows the
// sunrise that happened this morning, not tomorrow's — which is the difference between this and
// the Greyline pane's "next terminator" countdown, and the thing most worth pinning.
describe('sunDay (today\'s sunrise and sunset)', () => {
  const lat = 41.7
  const lon = -87.6 // Chicago, UTC−5 in September
  const noon = Date.UTC(2026, 8, 29, 17, 0, 0) // about 12:00 local

  it("finds this morning's sunrise and this evening's sunset, both on the horizon", () => {
    const d = sunDay(lat, lon, noon)
    expect(d.polar).toBeNull()
    expect(d.riseMs).not.toBeNull()
    expect(d.setMs).not.toBeNull()
    expect(d.riseMs!).toBeLessThan(noon)
    expect(d.setMs!).toBeGreaterThan(noon)
    // The map's terminator and the Greyline pane use this elevation model; so must the clock.
    expect(Math.abs(solarElevationDeg(lat, lon, d.riseMs!))).toBeLessThan(0.5)
    expect(Math.abs(solarElevationDeg(lat, lon, d.setMs!))).toBeLessThan(0.5)
    // Chicago on 29 Sep 2026: sunrise ~11:50Z, sunset ~23:35Z (the 0° horizon, a few minutes
    // off the almanac's refracted one). Twenty minutes is slack for the model, and still far
    // tighter than the day-boundary mistake this guards against.
    expect(Math.abs(d.riseMs! - Date.UTC(2026, 8, 29, 11, 50))).toBeLessThan(20 * 60_000)
    expect(Math.abs(d.setMs! - Date.UTC(2026, 8, 29, 23, 35))).toBeLessThan(20 * 60_000)
  })

  it("late in the evening it still shows today's pair, not tomorrow's sunrise", () => {
    const late = Date.UTC(2026, 8, 30, 3, 0, 0) // 22:00 local on the 29th
    const d = sunDay(lat, lon, late)
    expect(d.riseMs!).toBeLessThan(late)
    expect(d.setMs!).toBeLessThan(late)
    expect(d).toEqual(sunDay(lat, lon, noon))
  })

  it("before dawn, today's sunrise is the Greyline pane's next terminator", () => {
    const early = Date.UTC(2026, 8, 29, 9, 0, 0) // 04:00 local
    const next = nextTerminatorMs(lat, lon, early)
    expect(next.kind).toBe('rise')
    expect(Math.abs(sunDay(lat, lon, early).riseMs! - next.atMs)).toBeLessThanOrEqual(60_000)
  })

  it('says so when the sun never sets, or never rises', () => {
    // Tromsø (69.65 N): the midnight sun at the June solstice, the polar night at December's.
    const june = sunDay(69.65, 18.96, Date.UTC(2026, 5, 21, 12, 0, 0))
    expect(june.polar).toBe('up')
    expect(june.riseMs).toBeNull()
    expect(june.setMs).toBeNull()
    expect(sunDay(69.65, 18.96, Date.UTC(2026, 11, 21, 12, 0, 0)).polar).toBe('down')
    // Control: the same place in September has an ordinary day.
    expect(sunDay(69.65, 18.96, Date.UTC(2026, 8, 29, 12, 0, 0)).polar).toBeNull()
  })
})
