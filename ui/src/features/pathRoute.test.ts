import { describe, it, expect } from 'vitest'
import { polarChip, ribbonRoutes, routeHint, routeHintDetail, routeHintLine } from './pathRoute'
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
    const p = pred([LONG_ONLY], geo(260, 16_000), geo(80, 24_000, hs))
    expect(polarChip(p, 'long')).toBe('Polar path · Kp 5.3 forecast at 1800Z')
    // The short route of this fixture never touches the cap: no chip for it.
    expect(polarChip(p, 'short')).toBeNull()
  })

  it('in the cap now with no storm forecast: just "Polar path"; a measured Kp is never called a forecast', () => {
    const hs = hours((h) => ({ polarFrac: 0.1, ...(h === 15 ? { kp: 6, kpSource: 'measured' as const } : {}) }))
    expect(polarChip(pred([LONG_ONLY], geo(260, 16_000, hs)), 'short')).toBe('Polar path')
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

  it('scrubbed to another hour, inks and stretches come from that hour of the day', () => {
    const seg3 = [{ from: 0.1, to: 0.4, kind: 'polar' as const }]
    const now = [{ from: 0.5, to: 0.6, kind: 'grey' as const }]
    const p = pred([LONG_ONLY], geo(260, 16_000, hours((h) => (h === 3 ? { segments: seg3 } : {})), now))
    const cells = Array.from({ length: 24 }, (_, h) =>
      h === 3 ? { shortPct: 80, longPct: 4, verdict: 'short' as const, confidence: 'likely' as const } : { shortPct: 0, longPct: 92, verdict: 'long' as const, confidence: 'likely' as const },
    )
    p.day = { nowHour: 12, mufShort: [], bands: [{ band: '20m', cells }], hours: [] }
    const r = ribbonRoutes(p, null, 3)
    expect(r?.short).toEqual({ ink: 'open', segments: seg3, brgDeg: 260 })
    expect(r?.long?.ink).toBe('closed')
    expect(r?.favoured).toBe('short')
    // The current hour (or no hour) is exactly the unscrubbed ribbon.
    expect(ribbonRoutes(p, null, 12)).toEqual(ribbonRoutes(p))
    expect(ribbonRoutes(p, null, null)).toEqual(ribbonRoutes(p))
    // A focused band the day has no row for falls back to the best band that hour.
    expect(ribbonRoutes(p, '40m', 3)?.favoured).toBe('short')
  })
})
