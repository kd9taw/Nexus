import { describe, it, expect } from 'vitest'
import { nearAntipode, polarChip, polarChipTitle, ribbonRoutes, routeHint, routeHintDetail, routeHintLine } from './pathRoute'
import type { BandOutlook, PathGeoHour, PathGeometry, PathPrediction, RouteCall } from '../types'

// 2026-05-15 12:00 UTC — the request time every fixture is built around.
const T12 = 1_778_846_400

function hours(over: (h: number) => Partial<PathGeoHour> = () => ({})): PathGeoHour[] {
  return Array.from({ length: 24 }, (_, h) => ({
    unix: T12 + ((h + 24 - 12) % 24) * 3600,
    kp: 2,
    kpSource: 'current' as const,
    capLatDeg: 63.6,
    polarFrac: 0,
    greylineFrac: 0,
    sunlitFrac: 1,
    greyMe: false,
    greyDx: false,
    ...over(h),
  }))
}

function geo(bearingDeg: number, distKm: number, hs = hours(), segmentsNow: PathGeometry['segmentsNow'] = []): PathGeometry {
  return { distKm, bearingDeg, maxGmlatDeg: 40, nowHour: 12, hours: hs, segmentsNow }
}

/** The same geometry reaching `lat`° geomagnetic at its highest. */
function reach(g: PathGeometry, lat: number): PathGeometry {
  return { ...g, maxGmlatDeg: lat }
}

const UNKNOWN_KP = { kp: null, kpSource: 'unknown' as const, capLatDeg: null, polarFrac: null }

function band(name: string, nowScore: number): BandOutlook {
  const hourly = Array(24).fill(0)
  hourly[12] = nowScore
  return { band: name, workability: 'Good', score: nowScore, window: '—', grayline: false, hourly, reliability: 50 }
}

function call(b: string, over: Partial<RouteCall>): RouteCall {
  return { band: b, verdict: 'either', shortBcr: 0.6, longBcr: 0.6, reasons: [{ kind: 'withinMargin' }], ...over }
}

/** Boston–Sydney-like: short path 260° 16 000 km, long path 80° 24 000 km. */
function pred(routes: RouteCall[], sp = geo(260, 16_000), lp = geo(80, 24_000)): PathPrediction {
  const bands = routes.map((r) => band(r.band, r.shortBcr))
  return {
    engine: 'p533',
    bands,
    mufNow: 21,
    mufHourly: Array(24).fill(21),
    geometry: sp,
    longPath: { engine: 'p533', bands: routes.map((r) => band(r.band, r.longBcr)), mufNow: 18, mufHourly: Array(24).fill(18), geometry: lp },
    routes,
  }
}

const LONG_ONLY = call('20m', {
  verdict: 'long',
  // The engine holds every long-path call at "possible" until the long path is validated.
  confidence: 'possible',
  marginDb: 32,
  shortBcr: 0,
  longBcr: 0.92,
  reasons: [{ kind: 'oneClosed', open: 'long' }],
})

describe('the heading hint', () => {
  it('long path open and short closed: beam the long-path heading and say why, without a silly dB figure', () => {
    const h = routeHint(pred([LONG_ONLY]))
    expect(h?.route).toBe('long')
    expect(h?.brgDeg).toBe(80)
    expect(routeHintLine(h)).toBe('Beam 80° for 20m: long path (possible) — the short path is modelled closed now.')
    // A narrow card: the reasons go to the hover.
    expect(routeHintLine(h, true)).toBe('Beam 80° for 20m: long path (possible).')
  })

  it('both open with a margin: the dB is the model median difference, rounded', () => {
    const h = routeHint(
      pred([call('20m', { verdict: 'long', confidence: 'possible', marginDb: 9.4, shortBcr: 0.55, longBcr: 0.9, reasons: [{ kind: 'margin', better: 'long' }] })]),
    )
    expect(routeHintLine(h)).toBe('Beam 80° for 20m: long path (possible), modelled about 9 dB better now.')
  })

  it('short path favoured: the short-path heading', () => {
    const h = routeHint(
      pred([call('20m', { verdict: 'short', confidence: 'likely', marginDb: -12, shortBcr: 0.8, longBcr: 0.02, reasons: [{ kind: 'oneClosed', open: 'short' }] })]),
    )
    expect(h?.brgDeg).toBe(260)
    expect(routeHintLine(h)).toBe('Beam 260° for 20m: short path (likely).')
  })

  it('either is a real answer, said as such with both headings', () => {
    const h = routeHint(pred([call('20m', {})]))
    expect(h?.route).toBe('either')
    expect(routeHintLine(h)).toBe('20m works either way: short path 260° or long path 80°, modelled within 6 dB.')
  })

  it('an either the grey line made is never worded as "within 6 dB"', () => {
    // A 7 dB short-path margin, nudged one notch toward the grey long arc.
    const nudged = (toward: 'long' | 'short', better: 'long' | 'short') =>
      routeHintLine(
        routeHint(
          pred([
            call('80m', {
              marginDb: better === 'long' ? 75 : -75,
              shortBcr: 0.3,
              longBcr: 0.3,
              reasons: [{ kind: 'margin', better }, { kind: 'greyLine', route: toward }],
            }),
          ]),
        ),
      )
    expect(nudged('long', 'short')).toBe(
      '80m could go either way: the short path 260° is modelled stronger, the long path 80° lies along the grey line.',
    )
    expect(nudged('short', 'long')).toBe(
      '80m could go either way: the long path 80° is modelled stronger, the short path 260° lies along the grey line.',
    )
    expect(nudged('long', 'short')).not.toMatch(/6 dB/)
  })

  it('a long-path verdict is never worded "likely", whatever confidence came with it', () => {
    const h = routeHint(pred([{ ...LONG_ONLY, confidence: undefined }]))
    expect(routeHintLine(h, true)).toBe('Beam 80° for 20m: long path (possible).')
  })

  it('near the antipode no route is called: every heading is about the same distance', () => {
    const p = pred([LONG_ONLY], geo(90, 19_900), geo(270, 20_130))
    expect(nearAntipode(p)).toBe(true)
    expect(routeHint(p)).toBeNull()
    // Negative control: 8 000 km apart is a real choice.
    expect(nearAntipode(pred([LONG_ONLY]))).toBe(false)
    expect(nearAntipode(null)).toBe(false)
  })

  it('possible, and a grey-line nudge, are worded', () => {
    const h = routeHint(
      pred([
        call('40m', {
          verdict: 'long',
          confidence: 'possible',
          marginDb: 3,
          shortBcr: 0.4,
          longBcr: 0.4,
          reasons: [{ kind: 'withinMargin' }, { kind: 'greyLine', route: 'long' }],
        }),
      ]),
    )
    expect(routeHintLine(h)).toBe('Beam 80° for 40m: long path (possible), the grey line lies along it.')
  })

  it('prefers the band in focus, else the band with the most open route now', () => {
    const p = pred([
      call('40m', { verdict: 'short', confidence: 'likely', shortBcr: 0.7, longBcr: 0, reasons: [{ kind: 'oneClosed', open: 'short' }] }),
      LONG_ONLY,
    ])
    expect(routeHint(p)?.band).toBe('20m')
    expect(routeHint(p, '40m')?.band).toBe('40m')
    // A focus band the outlook does not carry falls back to the best one.
    expect(routeHint(p, '6m')?.band).toBe('20m')
  })

  it('says nothing when the model has no long path, or every band is closed both ways', () => {
    const noLong: PathPrediction = { ...pred([LONG_ONLY]), longPath: undefined, routes: undefined }
    expect(routeHint(noLong)).toBeNull()
    expect(
      routeHint(pred([call('20m', { verdict: 'unknown', shortBcr: 0.01, longBcr: 0.02, reasons: [{ kind: 'bothClosed' }] })])),
    ).toBeNull()
    expect(routeHintLine(null)).toBeNull()
  })

  it('the numbers behind it: both headings and reliabilities, the SNR margin, modelled and the long path unvalidated', () => {
    const rows = routeHintDetail(routeHint(pred([LONG_ONLY])))
    expect(rows).toEqual([
      'Now on 20m: short path 260°, reliability 0% · long path 80°, reliability 92% (P.533 monthly median)',
      'Long path minus short path: +32 dB median SNR',
      'Modelled, unvalidated for long path: the reference model is checked for the short path only.',
    ])
  })
})

describe('the polar chip', () => {
  it('a forecast storm over the favoured route names the Kp and the hour', () => {
    const hs = hours((h) => ({
      polarFrac: 0.2,
      ...(h === 18 ? { kp: 5.33, kpSource: 'forecast' as const } : {}),
    }))
    const p = pred([LONG_ONLY], geo(260, 16_000), reach(geo(80, 24_000, hs), 75))
    expect(polarChip(p, 'long')).toBe('Polar path · Kp 5.3 forecast from 1800Z')
    // The short route of this fixture never touches the cap: no chip for it.
    expect(polarChip(p, 'short')).toBeNull()
  })

  it('in the cap now with no storm forecast: just "Polar path"; a measured Kp is never called a forecast', () => {
    const hs = hours((h) => ({ polarFrac: 0.1, ...(h === 15 ? { kp: 6, kpSource: 'measured' as const } : {}) }))
    expect(polarChip(pred([LONG_ONLY], reach(geo(260, 16_000, hs), 75)), 'short')).toBe('Polar path')
  })

  it('past the edge but short of the cap proper is the auroral zone, as the why line says', () => {
    const hs = hours(() => ({ polarFrac: 0.1 }))
    const p = pred([LONG_ONLY], reach(geo(260, 16_000, hs), 65))
    expect(polarChip(p, 'short')).toBe('Auroral zone')
    expect(polarChipTitle(p, 'short')).toBe(
      'This route crosses the auroral zone, where an aurora brings fading and flutter. The model has no storms; this comes from space weather. Kp this hour: latest reading, no forecast for this hour.',
    )
  })

  it('the tooltip names where this hour\'s Kp came from', () => {
    const at = (src: PathGeoHour['kpSource']) =>
      polarChipTitle(pred([LONG_ONLY], reach(geo(260, 16_000, hours(() => ({ polarFrac: 0.1, kpSource: src }))), 75)), 'short')
    expect(at('measured')).toBe(
      'This route crosses the polar cap, where an aurora brings fading and flutter. The model has no storms; this comes from space weather. Kp this hour: measured.',
    )
    expect(at('forecast')).toMatch(/Kp this hour: forecast\.$/)
    expect(at('current')).toMatch(/Kp this hour: latest reading, no forecast for this hour\.$/)
  })

  it('a storm that would put the auroral zone over a route clear of it now is an if, not a fact', () => {
    // Clear now; the 1800Z forecast Kp 5 pushes the edge over the route.
    const hs = hours((h) => (h === 18 ? { kp: 5, kpSource: 'forecast' as const, polarFrac: 0.15 } : {}))
    const p = pred([LONG_ONLY], reach(geo(260, 16_000, hs), 62))
    expect(polarChip(p, 'short')).toBe('Auroral zone if Kp 5 (forecast from 1800Z)')
    expect(polarChipTitle(p, 'short')).toBe(
      'The space-weather forecast puts the auroral zone over this route from 1800Z (Kp 5, forecast). The model has no storms.',
    )
  })

  it('with Kp unknown there is no chip on either route', () => {
    const hs = hours(() => UNKNOWN_KP)
    const p = pred([LONG_ONLY], reach(geo(260, 16_000, hs), 80), reach(geo(80, 24_000, hs), 80))
    expect(polarChip(p, 'short')).toBeNull()
    expect(polarChip(p, 'long')).toBeNull()
    expect(polarChipTitle(p, 'long')).toBeNull()
  })
})

describe('the map ribbon', () => {
  it('colours each route by its reliability on the hinted band and carries its stretches', () => {
    const seg = [{ from: 0.4, to: 0.6, kind: 'polar' as const }]
    const r = ribbonRoutes(pred([LONG_ONLY], geo(260, 16_000, hours(), seg)))
    expect(r?.short).toEqual({ ink: 'closed', segments: seg, brgDeg: 260 })
    expect(r?.long).toEqual({ ink: 'open', segments: [], brgDeg: 80 })
    expect(r?.favoured).toBe('long')
  })

  it('marginal between 10 % and 50 %; no long path from the engine leaves the long route uncoloured', () => {
    const r = ribbonRoutes(pred([call('20m', { shortBcr: 0.3, longBcr: 0.05 })]))
    expect(r?.short.ink).toBe('marginal')
    expect(r?.long?.ink).toBe('closed')
    const heur: PathPrediction = { engine: 'heuristic', bands: [band('20m', 0.6)], mufNow: 0, mufHourly: [], geometry: geo(260, 16_000) }
    const h = ribbonRoutes(heur)
    expect(h?.short.ink).toBeNull()
    expect(h?.long).toBeNull()
    expect(h?.favoured).toBeNull()
    expect(ribbonRoutes(null)).toBeNull()
  })
})
