import { describe, expect, it } from 'vitest'
import type { PathDay, PathDayCell, PathDayHour, RegionPathDay } from '../types'
import {
  bestBand,
  cellView,
  ceilingRow,
  daySummary,
  kpCell,
  orderRegions,
  ROUTE_GLYPH,
} from './pathDay'

function hour(over: Partial<PathDayHour> = {}): PathDayHour {
  return { unix: 0, kp: 2, kpSource: 'forecast', polarShort: false, polarLong: false, greyMe: false, greyDx: false, ...over }
}

function day(bands: { band: string; cells: PathDayCell[] }[], over: Partial<PathDay> = {}): PathDay {
  return {
    nowHour: 12,
    mufShort: Array(24).fill(15),
    mufLong: Array(24).fill(15),
    bands,
    hours: Array.from({ length: 24 }, (_, h) => hour({ unix: h * 3600 })),
    ...over,
  }
}

const sp = (shortPct: number, longPct?: number, verdict?: PathDayCell['verdict'], confidence?: PathDayCell['confidence']): PathDayCell => ({
  shortPct,
  longPct,
  verdict,
  confidence,
})

describe('cellView', () => {
  it('best follows the route the model favours and marks it', () => {
    expect(cellView(sp(80, 5, 'short', 'likely'), hour(), 'best')).toMatchObject({ pct: 80, route: 'short', glyph: ROUTE_GLYPH.short })
    expect(cellView(sp(5, 70, 'long', 'possible'), hour(), 'best')).toMatchObject({ pct: 70, route: 'long', glyph: ROUTE_GLYPH.long })
    // Either way: the better figure, and no route claimed.
    expect(cellView(sp(40, 55, 'either'), hour(), 'best')).toMatchObject({ pct: 55, route: null, glyph: '' })
    // Both closed: the better (closed) figure, no route.
    expect(cellView(sp(4, 6, 'unknown'), hour(), 'best')).toMatchObject({ pct: 6, route: null })
  })

  it('near the antipode (a long figure, no call) best names no route, as the card and the ribbon do', () => {
    const c = sp(60, 70)
    expect(cellView(c, hour(), 'best')).toMatchObject({ pct: 70, route: null, glyph: '' })
    const d = day([{ band: '20m', cells: Array(24).fill(c) }])
    expect(daySummary(d, 'best')).toEqual([{ band: '20m', from: 0, to: 0, route: null }])
  })

  it('without a long path every mode is the short path, and the long mode has nothing', () => {
    expect(cellView(sp(60), hour(), 'best')).toMatchObject({ pct: 60, route: 'short' })
    expect(cellView(sp(60), hour(), 'short')).toMatchObject({ pct: 60, route: 'short' })
    expect(cellView(sp(60), hour(), 'long')).toMatchObject({ pct: null, route: null })
  })

  it('short and long modes show that route whatever the verdict', () => {
    expect(cellView(sp(10, 70, 'long', 'likely'), hour(), 'short')).toMatchObject({ pct: 10, route: 'short' })
    expect(cellView(sp(80, 5, 'short', 'likely'), hour(), 'long')).toMatchObject({ pct: 5, route: 'long' })
  })

  it('marks polar from the hour of the route shown', () => {
    const h = hour({ polarShort: false, polarLong: true })
    expect(cellView(sp(10, 70, 'long', 'likely'), h, 'best').polar).toBe(true)
    expect(cellView(sp(80, 5, 'short', 'likely'), h, 'best').polar).toBe(false)
    expect(cellView(sp(80, 5, 'short', 'likely'), h, 'long').polar).toBe(true)
    // Either way, the better figure's route decides.
    expect(cellView(sp(40, 55, 'either'), h, 'best').polar).toBe(true)
    expect(cellView(sp(60, 55, 'either'), h, 'best').polar).toBe(false)
  })
})

describe('ceilingRow', () => {
  it('is the highest band at or below the MUF, per hour', () => {
    const d = day(
      ['10m', '20m', '40m'].map((band) => ({ band, cells: Array(24).fill(sp(50)) })),
      { mufShort: [...Array(12).fill(30), ...Array(12).fill(8)], mufLong: Array(24).fill(15) },
    )
    const short = ceilingRow(d, 'short')
    expect(short[0]).toBe(0) // 30 MHz: 10m is under it
    expect(short[13]).toBe(2) // 8 MHz: only 40m
    // Long mode uses the long path's MUF; best the higher of the two.
    expect(ceilingRow(d, 'long')[0]).toBe(1)
    expect(ceilingRow(d, 'best')[13]).toBe(1)
    // A MUF under every band: no ceiling cell that hour.
    const low = day([{ band: '20m', cells: Array(24).fill(sp(0)) }], { mufShort: Array(24).fill(5), mufLong: undefined })
    expect(ceilingRow(low, 'best')[0]).toBeNull()
    expect(ceilingRow(low, 'long')[0]).toBeNull()
  })
})

describe('kpCell', () => {
  it('draws a measured hour solid, a forecast hollow, and says which', () => {
    expect(kpCell(hour({ kp: 3, kpSource: 'measured' }))).toMatchObject({ fill: 'measured', tone: 'good' })
    expect(kpCell(hour({ kp: 5.3, kpSource: 'forecast' }))).toMatchObject({ fill: 'forecast', tone: 'bad' })
    expect(kpCell(hour({ kp: 4, kpSource: 'current' }))).toMatchObject({ fill: 'current', tone: 'warn' })
    // No space-weather data: no tone at all, so nothing reads as a quiet Kp.
    expect(kpCell(hour({ kp: null, kpSource: 'unknown' }))).toEqual({ fill: 'unknown', tone: null })
  })
})

describe('daySummary', () => {
  it('names the longest open run on the two best bands, with its route', () => {
    const open = (from: number, to: number, cell: PathDayCell) =>
      Array.from({ length: 24 }, (_, h) => (h >= from && h <= to ? cell : sp(2, 2, 'unknown')))
    const d = day([
      { band: '20m', cells: open(12, 15, sp(10, 80, 'long', 'likely')) },
      { band: '40m', cells: open(20, 23, sp(70, 5, 'short', 'likely')) },
      { band: '80m', cells: open(1, 1, sp(60, 5, 'short', 'likely')) },
    ])
    expect(daySummary(d, 'best')).toEqual([
      { band: '20m', from: 12, to: 16, route: 'long' },
      { band: '40m', from: 20, to: 0, route: 'short' },
    ])
  })

  it('joins a run that wraps past midnight and reports nothing when nothing opens', () => {
    const cells = Array.from({ length: 24 }, (_, h) => (h >= 22 || h <= 1 ? sp(60) : sp(10)))
    expect(daySummary(day([{ band: '40m', cells }]), 'best')).toEqual([{ band: '40m', from: 22, to: 2, route: 'short' }])
    expect(daySummary(day([{ band: '40m', cells: Array(24).fill(sp(49)) }]), 'best')).toEqual([])
  })

  it('splits a band open all day into its routes, longest first', () => {
    const cells = Array.from({ length: 24 }, (_, h) =>
      h >= 6 && h < 20 ? sp(5, 70, 'long', 'likely') : sp(80, 5, 'short', 'likely'),
    )
    expect(daySummary(day([{ band: '20m', cells }]), 'best')).toEqual([
      { band: '20m', from: 6, to: 20, route: 'long' },
      { band: '20m', from: 20, to: 6, route: 'short' },
    ])
    // Shown as the short path alone, the same band is one run.
    expect(daySummary(day([{ band: '20m', cells: Array(24).fill(sp(60, 60, 'either')) }]), 'short')).toEqual([
      { band: '20m', from: 0, to: 0, route: 'short' },
    ])
  })

  it('calls a run either way when the model does not favour one route through it', () => {
    const cells = Array.from({ length: 24 }, (_, h) => (h < 3 ? sp(60, 62, 'either') : sp(1, 1, 'unknown')))
    expect(daySummary(day([{ band: '20m', cells }]), 'best')[0].route).toBeNull()
  })
})

describe('bestBand', () => {
  it('is the band open the most hours by the mode shown', () => {
    const d = day([
      { band: '10m', cells: Array.from({ length: 24 }, (_, h) => sp(h < 4 ? 90 : 0)) },
      { band: '20m', cells: Array.from({ length: 24 }, (_, h) => sp(h < 10 ? 60 : 0)) },
    ])
    expect(bestBand(d, 'best')?.band).toBe('20m')
    expect(bestBand(day([]), 'best')).toBeNull()
  })
})

describe('orderRegions', () => {
  const r = (id: string, zones: number[]): RegionPathDay => ({ id, lat: 0, lon: 0, zones, day: day([]) })
  const regions = [r('eu', [14, 15]), r('ja', [25]), r('vk', [29, 30])]

  it('keeps the fixed order on an empty log with nothing needed', () => {
    expect(orderRegions(regions, [], []).map((x) => x.id)).toEqual(['eu', 'ja', 'vk'])
  })

  it('puts the regions with needed stations first, then the most logged', () => {
    const logged = [
      { zone: 25, qsos: 5 },
      { zone: 29, qsos: 300 },
      { zone: 14, qsos: 10 },
    ]
    expect(orderRegions(regions, [], logged).map((x) => x.id)).toEqual(['vk', 'eu', 'ja'])
    expect(orderRegions(regions, [{ zone: 25 }, { zone: 25 }], logged).map((x) => x.id)).toEqual(['ja', 'vk', 'eu'])
  })
})
