import { describe, it, expect } from 'vitest'
import {
  AURORA_STALE_SECS,
  KP_MEASURED_WITHIN_SECS,
  auroraPicture,
  auroraStorm,
  capPicture,
  capShown,
  edgeLines,
  geomagLat,
  gridCap,
  kpAlert,
  kpLabel,
  kpNow,
  kpWord,
} from './capsNow'
import { t } from './i18n'
import type { AuroraOval, DrapGrid, KpForecast, PcaView, SpaceWxView } from './types'

const NOW = Date.UTC(2026, 9, 31, 1, 30) // 01:30Z
const now = NOW / 1000
const H = 3600

const wx = (o: Partial<SpaceWxView> = {}): SpaceWxView =>
  ({
    sfi: 140,
    kp: 6,
    aIndex: 20,
    xrayClass: 'B2.0-class',
    flare: false,
    solarWind: null,
    fetched: true,
    sfiAt: now - H,
    kpAt: now - 120,
    xrayAt: now - 60,
    aDay: now - 86400,
    ...o,
  }) as SpaceWxView

const fc = (rows: [number, number, string][]): KpForecast =>
  ({ points: rows.map(([timeUnix, kp, kind]) => ({ timeUnix, kp, kind, noaaScale: null })) }) as unknown as KpForecast

// The 21–00Z period ended at 00:00Z, 90 minutes ago.
const P21 = Date.UTC(2026, 9, 30, 21) / 1000

describe('kpNow: NOAA’s measured Kp first, the estimate only without one, nothing without either', () => {
  it('a period that ended within 3 hours is the Kp now, measured', () => {
    const k = kpNow(wx({ kp: 6.33 }), fc([[P21 - 3 * H, 2, 'observed'], [P21, 4.67, 'observed'], [P21 + 3 * H, 6, 'estimated']]), NOW)
    expect(k).toEqual({ kp: 4.67, source: 'measured', at: P21, until: P21 + 3 * H })
    expect(kpLabel(k!)).toBe(t('prop.kpNow.measured', { kp: '4.7', from: '21Z', to: '00Z' }))
    expect(kpWord(k!)).toBe('21Z–00Z')
  })

  it('past 3 hours after its end the measured period is history, and the usable estimate speaks', () => {
    const late = NOW + 2 * H * 1000 // 03:30Z: the period ended 3 h 30 min ago
    const k = kpNow(wx({ kp: 6.33, kpAt: late / 1000 - 60 }), fc([[P21, 4.67, 'observed']]), late)
    expect(k).toMatchObject({ kp: 6.33, source: 'estimated' })
    expect(kpLabel(k!)).toBe(t('prop.kpNow.est', { kp: '6.3', time: '03:29Z' }))
    expect(kpWord(k!)).toBe(t('dash.kp.est'))
    // exactly at the boundary it is still measured
    const edge = (P21 + 3 * H + KP_MEASURED_WITHIN_SECS) * 1000
    expect(kpNow(wx({ kpAt: edge / 1000 - 60 }), fc([[P21, 4.67, 'observed']]), edge)?.source).toBe('measured')
  })

  it('an estimated or predicted row is never "measured", and a period not yet over is not either', () => {
    const k = kpNow(wx({ kp: 5.0 }), fc([[P21, 7, 'estimated'], [P21 + 3 * H, 8, 'predicted'], [now - H, 9, 'observed']]), NOW)
    expect(k).toMatchObject({ kp: 5.0, source: 'estimated' })
  })

  it('fails closed: a stand-in or an old estimate with no measurement is no Kp', () => {
    expect(kpNow(wx({ kpAt: null }), null, NOW), 'stand-in').toBeNull()
    expect(kpNow(wx({ kpAt: now - 20 * 60 }), fc([]), NOW), 'past its cadence').toBeNull()
    expect(kpNow(null, null, NOW)).toBeNull()
    // a garbled forecast answer (the shared mock's `{}`) falls through to the estimate
    expect(kpNow(wx(), {} as KpForecast, NOW)?.source).toBe('estimated')
    // the measurement still speaks when the snapshot's Kp is a stand-in
    expect(kpNow(wx({ kpAt: null }), fc([[P21, 3, 'observed']]), NOW)?.source).toBe('measured')
  })
})

describe('kpAlert: the warning fires on the higher of NOAA’s measurement and the estimate, labelled with which', () => {
  it('measured 4 (a period that ended 90 min ago) and an estimate of 6.3: the estimate warns, as an estimate', () => {
    const k = kpAlert(wx({ kp: 6.3 }), fc([[P21, 4, 'observed']]), NOW)
    expect(k).toMatchObject({ kp: 6.3, source: 'estimated', at: now - 120 })
    // the words for the bar stay NOAA's measurement
    expect(kpNow(wx({ kp: 6.3 }), fc([[P21, 4, 'observed']]), NOW)?.source).toBe('measured')
  })

  it('CONTROL — measured 6 and an estimate of 3: NOAA’s measurement warns, labelled measured', () => {
    expect(kpAlert(wx({ kp: 3 }), fc([[P21, 6, 'observed']]), NOW)).toEqual({ kp: 6, source: 'measured', at: P21, until: P21 + 3 * H })
    // a tie goes to the measurement
    expect(kpAlert(wx({ kp: 6 }), fc([[P21, 6, 'observed']]), NOW)?.source).toBe('measured')
  })

  it('one source alone is that source; neither, or a stand-in and an old period, is nothing', () => {
    expect(kpAlert(wx({ kp: 5.3 }), null, NOW)).toMatchObject({ kp: 5.3, source: 'estimated' })
    expect(kpAlert(wx({ kpAt: null }), fc([[P21, 5, 'observed']]), NOW)).toMatchObject({ kp: 5, source: 'measured' })
    expect(kpAlert(wx({ kpAt: null }), fc([[P21 - 6 * H, 7, 'observed']]), NOW)).toBeNull()
    expect(kpAlert(null, null, NOW)).toBeNull()
  })
})

/** A whole-globe D-RAP grid (2° × 4°), every cell `f(lat, lon)`. */
function grid(f: (lat: number, lon: number) => number, validAt = now - 60): DrapGrid {
  const lats: number[] = []
  const lons: number[] = []
  for (let la = 89; la >= -89; la -= 2) lats.push(la)
  for (let lo = -178; lo <= 178; lo += 4) lons.push(lo)
  const haf: number[] = []
  for (const la of lats) for (const lo of lons) haf.push(f(la, lo))
  return { validAt, recoveryAt: null, lats, lons, haf }
}

const PCA: PcaView = {
  j10: 150,
  a30Day: 1.5,
  a30Night: 0.8,
  cutoffDeg: 60,
  points: [{ lat: 80, lon: 0, db30: 1.2 }],
  protonsAt: now - 300,
  sScale: 2,
  sepRecoveryH: 24.2,
  edge: [
    [[0, 70], [10, 70], [0, 70]],
    [[0, -70], [10, -70], [0, -70]],
  ],
}

describe('the polar cap: NOAA’s grid while current, else the model with its approximate edge', () => {
  it('reads NOAA’s proton part poleward of 50° geomagnetic, beyond the X-ray part, at ≥ 0.5 dB', () => {
    // 25 MHz everywhere poleward of 60° geomagnetic: (25/30)^1.5 = 0.76 dB; 10 MHz elsewhere.
    const g = grid((la, lo) => (Math.abs(geomagLat(la, lo)) >= 60 ? 25 : 10))
    const cap = gridCap(g, null, NOW)
    expect(cap.length).toBeGreaterThan(100)
    expect(cap.every((p) => Math.abs(geomagLat(p.lat, p.lon)) >= 60)).toBe(true)
    expect(cap[0].db30).toBeCloseTo(Math.pow(25 / 30, 1.5), 6)
    // a dayside flare's own X-ray part is not counted as protons
    const flare = gridCap(g, 1e-3, NOW) // X10: the dayside cap cells' HAF is mostly the flare's
    expect(flare.length).toBeLessThan(cap.length)
    // and nothing equatorward of 50° geomagnetic, however hot (the flare's footprint)
    const hot = gridCap(grid(() => 40), null, NOW)
    expect(hot.every((p) => Math.abs(geomagLat(p.lat, p.lon)) >= 50)).toBe(true)
    expect(hot.some((p) => Math.abs(geomagLat(p.lat, p.lon)) < 55)).toBe(true)
    // below 0.5 dB at 30 MHz nothing is drawn: 18 MHz is 0.46 dB
    expect(gridCap(grid(() => 18), null, NOW)).toEqual([])
  })

  it('a current grid is the cap, and draws no approximate edge beside it', () => {
    const g = grid((la, lo) => (Math.abs(geomagLat(la, lo)) >= 62 ? 25 : 0))
    const pic = capPicture(PCA, g, null, NOW)!
    expect(pic.source).toBe('noaa')
    expect(pic.validAt).toBe(g.validAt)
    expect(pic.edge).toBeNull()
    expect(pic.maxDb).toBeCloseTo(Math.pow(25 / 30, 1.5), 6)
  })

  it('a stale or absent grid hands the cap to the model, with its edge only at a measured Kp', () => {
    const stale = grid(() => 25, now - 600)
    const pic = capPicture(PCA, stale, null, NOW)!
    expect(pic).toMatchObject({ source: 'model', points: PCA.points, validAt: null, maxDb: 1.2 })
    expect(pic.edge).toEqual(PCA.edge)
    expect(capPicture(PCA, null, null, NOW)!.source).toBe('model')
    expect(capPicture({ ...PCA, cutoffDeg: null }, null, null, NOW)!.edge, 'no measured Kp, no edge').toBeNull()
    expect(capPicture(null, stale, null, NOW)).toBeNull()
  })

  it('NOAA’s grid is the cap only beside a held proton reading: dated and under 30 min old', () => {
    const g = grid((la, lo) => (Math.abs(geomagLat(la, lo)) >= 62 ? 25 : 0))
    expect(capPicture({ ...PCA, protonsAt: now - 29 * 60 }, g, null, NOW)!.source, 'held').toBe('noaa')
    // stale or undated: the grid's polar excess is not read as NOAA's proton part
    expect(capPicture({ ...PCA, protonsAt: now - 30 * 60 }, g, null, NOW)!.source, 'stale').toBe('model')
    expect(capPicture({ ...PCA, protonsAt: null }, g, null, NOW)!.source, 'undated').toBe('model')
    expect(capPicture({ ...PCA, protonsAt: undefined }, g, null, NOW)!.source, 'older station').toBe('model')
  })

  it('a chip for a proton event or any drawn absorption, not for a quiet sky', () => {
    const quiet = { ...PCA, j10: 0.3, sScale: 0, points: [] }
    expect(capShown(quiet, capPicture(quiet, null, null, NOW))).toBe(false)
    const s1 = { ...quiet, j10: 12, sScale: 1 }
    expect(capShown(s1, capPicture(s1, null, null, NOW))).toBe(true)
    expect(capShown(PCA, capPicture(PCA, null, null, NOW))).toBe(true)
    expect(capShown(null, null)).toBe(false)
  })
})

describe('the aurora oval: OVATION while current, and nothing at all when it is stale or missing', () => {
  const lats = Array.from({ length: 24 }, (_, i) => (i === 3 || i === 4 ? null : 60 + i * 0.1))
  const OVAL: AuroraOval = {
    points: [{ lat: 66, lon: 10, prob: 40 }],
    observedAt: now - 15 * 60,
    forecastAt: now + 40 * 60,
    edge: { north: lats, south: lats.map((l) => (l == null ? null : -l)) },
  }

  it('a current forecast draws OVATION’s own edge', () => {
    const p = auroraPicture(OVAL, NOW)
    expect(p.mode).toBe('ovation')
    if (p.mode !== 'ovation') return
    expect(p.forecastAt).toBe(OVAL.forecastAt)
    expect(p.edge).toEqual([...edgeLines(OVAL.edge!.north), ...edgeLines(OVAL.edge!.south)])
    // up to two cycles past its forecast time it is still the picture
    expect(auroraPicture({ ...OVAL, forecastAt: now - AURORA_STALE_SECS }, NOW).mode).toBe('ovation')
  })

  it('a stale forecast draws no oval and no edge, and says for when it was', () => {
    const f = now - AURORA_STALE_SECS - 1
    expect(auroraPicture({ ...OVAL, forecastAt: f }, NOW)).toEqual({ mode: 'none', why: 'stale', forecastAt: f })
    // an undated oval cannot be proven current: nothing, and it says it is undated
    expect(auroraPicture({ ...OVAL, forecastAt: null }, NOW)).toEqual({ mode: 'none', why: 'undated' })
  })

  it('no feed at all draws nothing and says so', () => {
    expect(auroraPicture({ points: [], observedAt: null, forecastAt: null }, NOW)).toEqual({ mode: 'none', why: 'nofeed' })
    expect(auroraPicture(null, NOW)).toEqual({ mode: 'none', why: 'nofeed' })
  })

  it('an older station’s DATED oval an hour past its forecast time is stale like any other: nothing drawn', () => {
    const old = { points: OVAL.points, observedAt: now - 4000, forecastAt: now - 3600 }
    expect(auroraPicture(old, NOW)).toEqual({ mode: 'none', why: 'stale', forecastAt: now - 3600 })
    // control: the same older-station oval while its forecast time is current is drawn
    expect(auroraPicture({ ...old, forecastAt: now + 600 }, NOW).mode).toBe('ovation')
  })

  it('an older station’s UNDATED oval (no edge, no forecast time) fails closed: nothing drawn, said undated', () => {
    const old = { points: OVAL.points, observedAt: null, forecastAt: null }
    expect(auroraPicture(old, NOW)).toEqual({ mode: 'none', why: 'undated' })
    // control: the same older-station oval with a current forecast time is drawn
    expect(auroraPicture({ ...old, forecastAt: now + 600 }, NOW)).toEqual({ mode: 'ovation', forecastAt: now + 600, edge: [] })
  })

  it('OVATION’s edge is joined bin to bin, broken at a gap, joined across the dateline', () => {
    const L = (i: number) => -180 + 15 * (i + 0.5)
    const lines = edgeLines(lats)
    // the gap at bins 3–4 leaves one line from bin 5 round through 23 and 0..2
    expect(lines).toHaveLength(1)
    expect(lines[0][0]).toEqual([L(5), lats[5]])
    expect(lines[0][lines[0].length - 1]).toEqual([L(2), lats[2]])
    expect(lines[0]).toHaveLength(22)
    // a whole ring closes
    const whole = edgeLines(Array.from({ length: 24 }, () => 62))
    expect(whole[0]).toHaveLength(25)
    expect(whole[0][0]).toEqual(whole[0][24])
    // more gaps make more lines; the lone bin 11 between two gaps is no line
    const two = edgeLines(lats.map((l, i) => (i === 10 || i === 12 ? null : l)))
    expect(two.map((l) => l.length).sort((a, b) => a - b)).toEqual([5, 14])
    expect(edgeLines([null, 60, null])).toEqual([])
    expect(edgeLines(undefined)).toEqual([])
  })

  it('turns itself on for Kp ≥ 5 or a G watch in force, and not below', () => {
    expect(auroraStorm({ kp: 5, source: 'measured', at: P21, until: P21 + 3 * H }, null)).toBe(true)
    expect(auroraStorm({ kp: 4.67, source: 'estimated', at: now }, null)).toBe(false)
    expect(auroraStorm(null, null)).toBe(false)
    const watch = { watch: { serial: 1, code: 'WATA20', issued: now, days: [], comment: null, window: null, maxG: 1 }, model: null, l1: null, rationale: null }
    expect(auroraStorm(null, watch)).toBe(true)
    expect(auroraStorm(null, { ...watch, watch: null })).toBe(false)
  })
})
