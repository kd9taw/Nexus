import { describe, it, expect } from 'vitest'
import { pathWhyDetail, pathWhyFacts, pathWhyLine, utcHourLabel } from './pathWhy'
import type { BandOutlook, PathGeoHour, PathHour, PathPrediction } from '../types'

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

function band(name: string, nowScore: number, hop?: PathHour | null): BandOutlook {
  const hourly = Array(24).fill(0.1)
  hourly[12] = nowScore
  return {
    band: name,
    workability: 'Good',
    score: nowScore,
    window: '—',
    grayline: false,
    hourly,
    reliability: 50,
    ...(hop === undefined ? {} : { detail: Array.from({ length: 24 }, (_, h) => (h === 12 ? hop : null)) }),
  }
}

function pred(
  bands: BandOutlook[],
  geo: PathGeoHour[],
  ovalNow?: boolean,
  maxGmlatDeg = 71,
): PathPrediction {
  return {
    engine: 'p533',
    bands,
    mufNow: 21,
    mufHourly: Array(24).fill(21),
    geometry: { distKm: 10800, bearingDeg: 335, maxGmlatDeg, nowHour: 12, hours: geo, ovalNow },
  }
}

const TWO_F2: PathHour = { layer: 'F2', hops: 2, takeoffDeg: 11.24, estimated: false }
/** No SWPC data at all: no reading, no outlook. */
const UNKNOWN = { kp: null, kpSource: 'unknown' as const, capLatDeg: null, polarFrac: null }

describe('the path why line', () => {
  it('a polar path with a storm forecast: model clause, overlay clause, and the forecast worded as one', () => {
    // In the cap now (quiet Kp); Kp 5.33 forecast for 18–21Z.
    const geo = hours((h) => ({
      polarFrac: 0.2,
      ...(h >= 18 && h < 21 ? { kp: 5.33, kpSource: 'forecast' as const, polarFrac: 0.35 } : {}),
    }))
    const line = pathWhyLine(pathWhyFacts(pred([band('40m', 0.5), band('20m', 0.8, TWO_F2)], geo)))
    expect(line).toBe(
      '20m short path, 2 F2 hops, crosses the polar cap — an aurora (Kp 5.3 forecast at 1800Z) may weaken it.',
    )
    // Never a prediction stated as fact.
    expect(line).not.toMatch(/\bwill\b/)
  })

  it('a path outside the cap that a forecast storm reaches gets the cap-spreading warning', () => {
    const geo = hours((h) => (h === 19 ? { kp: 7, kpSource: 'forecast' as const, polarFrac: 0.1 } : {}))
    const f = pathWhyFacts(pred([band('20m', 0.8, TWO_F2)], geo))
    expect(f?.polarNow).toBe(false)
    expect(f?.storm).toEqual({ kp: 7, unix: T12 + 7 * 3600 })
    expect(pathWhyLine(f)).toBe(
      '20m short path, 2 F2 hops — a storm (Kp 7 forecast at 1900Z) may push the auroral zone over it.',
    )
  })

  it('no warning when the storm misses the path, or when the Kp is measured rather than forecast', () => {
    // Negative control 1: Kp 8 forecast, but the path stays out of the cap (an equatorial path).
    const miss = hours((h) => (h === 19 ? { kp: 8, kpSource: 'forecast' as const } : {}))
    expect(pathWhyFacts(pred([band('20m', 0.8)], miss))?.storm).toBeNull()
    // Negative control 2: a measured Kp is not a forecast and is not warned about as one.
    const measured = hours((h) => (h === 12 ? { kp: 6, kpSource: 'measured' as const, polarFrac: 0.3 } : {}))
    const line = pathWhyLine(pathWhyFacts(pred([band('20m', 0.8)], measured)))
    expect(line).toBe('20m short path, crosses the polar cap.')
    expect(line).not.toMatch(/forecast/)
    // Below the storm threshold: Kp 4.67 forecast over the path is not a storm.
    const sub = hours((h) => (h === 15 ? { kp: 4.67, kpSource: 'forecast' as const, polarFrac: 0.2 } : {}))
    expect(pathWhyFacts(pred([band('20m', 0.8)], sub))?.storm).toBeNull()
  })

  it('the forecast is searched forward from now, wrapping past midnight', () => {
    // A storm at 03Z (tomorrow) and one at 09Z (tomorrow) — 03Z is the first ahead of 12Z.
    const geo = hours((h) => (h === 3 || h === 9 ? { kp: 6, kpSource: 'forecast' as const, polarFrac: 0.2 } : {}))
    expect(pathWhyFacts(pred([band('20m', 0.8)], geo))?.storm?.unix).toBe(T12 + 15 * 3600)
  })

  it('grey line at either end, and one hop is singular', () => {
    const one: PathHour = { layer: 'E', hops: 1, takeoffDeg: 6, estimated: false }
    const geo = hours((h) => (h === 12 ? { greyDx: true } : {}))
    expect(pathWhyLine(pathWhyFacts(pred([band('40m', 0.9, one)], geo)))).toBe(
      '40m short path, 1 E hop, grey line at their end.',
    )
    const both = hours((h) => (h === 12 ? { greyDx: true, greyMe: true } : {}))
    expect(pathWhyLine(pathWhyFacts(pred([band('40m', 0.9)], both)))).toBe(
      '40m short path, grey line at both ends.',
    )
  })

  it('the band is the best one open NOW, not the best of the day', () => {
    // 15m peaks higher over the day but is closed at 12Z; 20m is open now.
    const b15 = band('15m', 0.1)
    b15.hourly[18] = 1
    b15.score = 1
    const f = pathWhyFacts(pred([b15, band('20m', 0.6)], hours()))
    expect(f?.band).toBe('20m')
  })

  it('the live oval is said only when held and crossed', () => {
    expect(pathWhyLine(pathWhyFacts(pred([band('20m', 0.8)], hours(), true)))).toBe(
      '20m short path, crosses the auroral oval right now.',
    )
    expect(pathWhyLine(pathWhyFacts(pred([band('20m', 0.8)], hours(), false)))).toBe('20m short path.')
    expect(pathWhyLine(pathWhyFacts(pred([band('20m', 0.8)], hours())))).toBe('20m short path.')
  })

  it('says nothing without geometry, or with nothing open and nothing to warn of', () => {
    const p = pred([band('20m', 0.8)], hours())
    expect(pathWhyFacts({ ...p, geometry: undefined })).toBeNull()
    expect(pathWhyLine(null)).toBeNull()
    expect(pathWhyLine(pathWhyFacts(pred([band('20m', 0.1)], hours())))).toBeNull()
    // Positive control: nothing open, but a polar crossing is still worth a sentence.
    const polar = hours(() => ({ polarFrac: 0.2 }))
    expect(pathWhyLine(pathWhyFacts(pred([band('20m', 0.1)], polar)))).toBe(
      'Short path, crosses the polar cap.',
    )
  })

  it('the detail rows carry every number the line stands on', () => {
    const geo = hours((h) =>
      h === 12 ? { polarFrac: 0.23, capLatDeg: 63.6, kp: 2.33, greylineFrac: 0.4, sunlitFrac: 0.6 } : {},
    )
    const rows = pathWhyDetail(pathWhyFacts(pred([band('20m', 0.8, TWO_F2)], geo)), 71.2)
    expect(rows).toEqual([
      'Mode 2F2, take-off 11.2° (P.533 monthly median)',
      'Auroral zone and polar cap: 23% of the path (poleward of 64° geomagnetic at Kp 2.3, latest reading, no forecast for this hour)',
      'Highest geomagnetic latitude on the path: 71°',
      'Grey line: 40% of the path · in daylight: 60%',
      'The model is a monthly median and does not include storms; the auroral zone, the polar cap and the aurora are laid over it from space weather. Kp this hour: latest reading, no forecast for this hour.',
    ])
    // The heuristic carries no mode: the row is absent, not invented.
    const h = pathWhyDetail(pathWhyFacts(pred([band('20m', 0.8)], geo)), 71.2)
    expect(h.some((r) => r.startsWith('Mode'))).toBe(false)
  })

  it('with no space-weather data, Kp is said to be unknown and nothing that depends on it is claimed', () => {
    // A high-latitude path, cold cache: no Kp, no cap edge, no polar share — at every hour.
    const geo = hours(() => UNKNOWN)
    const f = pathWhyFacts(pred([band('20m', 0.8, TWO_F2)], geo, undefined, 64.7))
    expect(f?.polarNow).toBe(false)
    expect(f?.storm).toBeNull()
    const line = pathWhyLine(f)
    expect(line).toBe('20m short path, 2 F2 hops, Kp unknown, no space-weather data.')
    expect(line).not.toMatch(/polar cap|auroral zone|latest reading|current/)
    expect(pathWhyDetail(f, 64.7)).toEqual([
      'Mode 2F2, take-off 11.2° (P.533 monthly median)',
      'Auroral zone and polar cap: unknown, because Kp is unknown',
      'Highest geomagnetic latitude on the path: 65°',
      'Grey line: 0% of the path · in daylight: 100%',
      'The model is a monthly median and does not include storms; the auroral zone, the polar cap and the aurora are laid over it from space weather. Kp this hour: unknown, no space-weather data.',
    ])
    // Negative control: a path that never reaches the edge at any Kp loses nothing by it.
    const low = pathWhyLine(pathWhyFacts(pred([band('20m', 0.8)], geo, undefined, 30)))
    expect(low).toBe('20m short path.')
  })

  it('the note names the real source of this hour\'s Kp', () => {
    const note = (src: PathGeoHour['kpSource']) => {
      const rows = pathWhyDetail(pathWhyFacts(pred([band('20m', 0.8)], hours(() => ({ kpSource: src })))), 71)
      return rows[rows.length - 1]
    }
    expect(note('measured')).toMatch(/Kp this hour: measured\.$/)
    expect(note('forecast')).toMatch(/Kp this hour: forecast\.$/)
    expect(note('current')).toMatch(/Kp this hour: latest reading, no forecast for this hour\.$/)
  })

  it('a path that only grazes the quiet-time edge crosses the auroral zone, not the polar cap', () => {
    // Minneapolis → Tokyo: highest geomagnetic latitude ~64.7°, just past the Kp 2 edge (63.6°).
    const graze = hours(() => ({ polarFrac: 0.17 }))
    expect(pathWhyLine(pathWhyFacts(pred([band('20m', 0.8)], graze, undefined, 64.7)))).toBe(
      '20m short path, crosses the auroral zone.',
    )
    // At Kp 1 (edge 64.8°) the same path is out of it: nothing is said.
    const out = hours(() => ({ kp: 1, capLatDeg: 64.8, polarFrac: 0 }))
    expect(pathWhyLine(pathWhyFacts(pred([band('20m', 0.8)], out, undefined, 64.7)))).toBe(
      '20m short path.',
    )
    // Boston → Tokyo reaches 75°: that is the polar cap proper.
    expect(pathWhyLine(pathWhyFacts(pred([band('20m', 0.8)], graze, undefined, 75)))).toBe(
      '20m short path, crosses the polar cap.',
    )
  })

  it('beyond 7000 km the hops and take-off are worded as an estimate', () => {
    const est: PathHour = { layer: 'F2', hops: 4, takeoffDeg: 6.1, estimated: true }
    const f = pathWhyFacts(pred([band('20m', 0.8, est)], hours(), undefined, 30))
    expect(pathWhyLine(f)).toBe('20m short path, about 4 F2 hops.')
    expect(pathWhyDetail(f, 30)[0]).toBe(
      'Mode 4F2, take-off 6.1° (an estimate: beyond 7000 km P.533 assumes this hop geometry from the distance alone, so it is the same every hour)',
    )
    expect(pathWhyDetail(f, 30)[0]).not.toMatch(/monthly median/)
  })

  it('formats the hour as the outlook windows do', () => {
    expect(utcHourLabel(T12 + 6 * 3600)).toBe('1800Z')
    expect(utcHourLabel(T12 - 12 * 3600)).toBe('0000Z')
  })
})
